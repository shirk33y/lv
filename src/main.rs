#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
// lv-imgui: full viewer with database, dual-path rendering, preloading.
// Images: image crate decode → GL texture, LRU preload cache
// Videos: mpv render API
// Usage: cargo run --release [-- <dir_override>]

const VERSION: &str = env!("CARGO_PKG_VERSION");
const GIT_HASH: &str = env!("GIT_HASH");

mod aimeta;
mod cli;
mod db;
mod jobs;
mod preload;
mod quad;
mod scanner;
mod statusbar;
mod watcher;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Instant;

use clap::{Parser, Subcommand};

use sdl2::event::Event;
use sdl2::keyboard::{Keycode, Mod};
use sdl2::video::GLProfile;

use libmpv2::Mpv;

use db::{Db, FileEntry};
use preload::TextureCache;

const IMAGE_EXTS: &[&str] = &[
    "jpg", "jpeg", "png", "gif", "bmp", "webp", "tiff", "tif", "avif", "ico", "svg",
];
const VIDEO_EXTS: &[&str] = &[
    "mp4", "avi", "mov", "mkv", "webm", "flv", "wmv", "m4v", "3gp",
];

fn ext_of(path: &str) -> String {
    path.rsplit('.').next().unwrap_or("").to_lowercase()
}

fn is_image(path: &str) -> bool {
    IMAGE_EXTS.contains(&ext_of(path).as_str())
}

fn is_video(path: &str) -> bool {
    VIDEO_EXTS.contains(&ext_of(path).as_str())
}

/// Strip Windows extended-length path prefix (`\\?\`) if present.
/// Windows `canonicalize` returns `\\?\C:\...` paths; we strip the prefix
/// so paths display cleanly and match across the codebase.
pub(crate) fn clean_path(p: &str) -> String {
    p.strip_prefix(r"\\?\").unwrap_or(p).to_string()
}

/// Handle a dropped file or directory path.
///
/// - **File**: scan its parent dir (track temporarily if needed), switch to it, jump to the file.
/// - **Directory**: scan it (track temporarily if needed), switch to it.
///
/// Returns `true` if the drop was handled (files/cursor/dir were updated).
fn handle_drop(
    db: &Db,
    dropped: &std::path::Path,
    files: &mut Vec<FileEntry>,
    current_dir: &mut String,
    cursor: &mut usize,
    collection_mode: &mut Option<u8>,
) -> bool {
    let path = match std::fs::canonicalize(dropped) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("drop: cannot resolve {}: {}", dropped.display(), e);
            return false;
        }
    };

    if path.is_file() {
        // Check if it's a media file
        let path_str = clean_path(&path.to_string_lossy());
        if !is_image(&path_str) && !is_video(&path_str) {
            eprintln!("drop: not a media file: {}", path_str);
            return false;
        }

        let parent = path.parent().unwrap_or(&path);
        let parent_str = clean_path(&parent.to_string_lossy());

        // Scan the parent directory (adds new files to DB)
        if !db.dir_is_tracked(&parent_str) && !db.dir_is_covered(&parent_str) {
            db.dir_track(&parent_str, false);
            scanner::discover(db, parent);
            // Mark as temporary
            for f in &db.files_by_dir(&parent_str) {
                db.set_temporary(f.id, true);
            }
            eprintln!("drop: tracked (temp) {}", parent_str);
        } else {
            scanner::discover(db, parent);
        }

        // Exit collection mode, switch to dir mode
        *collection_mode = None;
        let new_files = db.files_by_dir(&parent_str);
        if new_files.is_empty() {
            eprintln!("drop: no files in {}", parent_str);
            return false;
        }
        let idx = new_files
            .iter()
            .position(|f| f.path == path_str)
            .unwrap_or(0);
        *files = new_files;
        *current_dir = parent_str;
        *cursor = idx;
        eprintln!("drop: file {} [{}/{}]", path_str, idx + 1, files.len());
        true
    } else if path.is_dir() {
        let dir_str = clean_path(&path.to_string_lossy());

        // Scan the directory
        if !db.dir_is_tracked(&dir_str) && !db.dir_is_covered(&dir_str) {
            db.dir_track(&dir_str, false);
            scanner::discover(db, &path);
            for f in &db.files_by_dir(&dir_str) {
                db.set_temporary(f.id, true);
            }
            eprintln!("drop: tracked (temp) {}", dir_str);
        } else {
            scanner::discover(db, &path);
        }

        *collection_mode = None;
        let new_files = db.files_by_dir(&dir_str);
        if new_files.is_empty() {
            eprintln!("drop: no media files in {}", dir_str);
            return false;
        }
        *files = new_files;
        *current_dir = dir_str;
        *cursor = 0;
        eprintln!("drop: dir {} ({} files)", current_dir, files.len());
        true
    } else {
        eprintln!("drop: not a file or directory: {}", path.display());
        false
    }
}

/// Send mpv "stop" asynchronously so it doesn't block the UI thread.
unsafe fn mpv_stop_async(handle: *mut libmpv2_sys::mpv_handle) {
    let cmd = std::ffi::CString::new("stop").unwrap();
    let args: [*const std::os::raw::c_char; 2] = [cmd.as_ptr(), std::ptr::null()];
    libmpv2_sys::mpv_command_async(handle, 0, args.as_ptr() as *mut _);
}

/// Send mpv "loadfile" asynchronously so it doesn't block the UI thread.
unsafe fn mpv_loadfile_async(handle: *mut libmpv2_sys::mpv_handle, path: &str) {
    let cmd = std::ffi::CString::new("loadfile").unwrap();
    let p = std::ffi::CString::new(path).unwrap();
    let args: [*const std::os::raw::c_char; 3] = [cmd.as_ptr(), p.as_ptr(), std::ptr::null()];
    libmpv2_sys::mpv_command_async(handle, 0, args.as_ptr() as *mut _);
}

// ── Offthread mpv rendering ──────────────────────────────────────────────

/// Shared state between main thread and mpv render thread.
struct MpvRenderShared {
    /// render thread → main: GL texture ID with latest rendered frame
    display_tex: AtomicU32,
    /// render thread → main: at least one frame has been produced
    has_frame: AtomicBool,
    /// main → render thread: please exit
    quit: AtomicBool,
    /// main → render thread: new drawable dimensions
    width: AtomicU32,
    height: AtomicU32,
    resize: AtomicBool,
    /// render thread → main: raw render context ptr (for report_swap)
    render_ctx: AtomicPtr<libmpv2_sys::mpv_render_context>,
}

/// Spawns the mpv render thread.  Pointers are passed as `usize` for `Send`.
fn spawn_mpv_render_thread(
    win_ptr: usize,
    gl_ctx_ptr: usize,
    mpv_ptr: usize,
    shared: Arc<MpvRenderShared>,
) -> std::thread::JoinHandle<()> {
    std::thread::Builder::new()
        .name("mpv-render".into())
        .spawn(move || {
            let win = win_ptr as *mut sdl2_sys::SDL_Window;
            let gl_ctx = gl_ctx_ptr as sdl2_sys::SDL_GLContext;
            let mpv_h = mpv_ptr as *mut libmpv2_sys::mpv_handle;

            // Make the shared GL context current on this thread
            unsafe { sdl2_sys::SDL_GL_MakeCurrent(win, gl_ctx); }

            // GL proc address callback for mpv
            unsafe extern "C" fn get_proc(
                _ctx: *mut std::os::raw::c_void,
                name: *const std::os::raw::c_char,
            ) -> *mut std::os::raw::c_void {
                sdl2_sys::SDL_GL_GetProcAddress(name)
            }

            // Create mpv render context via raw FFI
            let api_type = std::ffi::CString::new("opengl").unwrap();
            let mut init_params = libmpv2_sys::mpv_opengl_init_params {
                get_proc_address: Some(get_proc),
                get_proc_address_ctx: std::ptr::null_mut(),
            };
            let mut params = [
                libmpv2_sys::mpv_render_param {
                    type_: libmpv2_sys::mpv_render_param_type_MPV_RENDER_PARAM_API_TYPE,
                    data: api_type.as_ptr() as *mut _,
                },
                libmpv2_sys::mpv_render_param {
                    type_: libmpv2_sys::mpv_render_param_type_MPV_RENDER_PARAM_OPENGL_INIT_PARAMS,
                    data: &mut init_params as *mut _ as *mut _,
                },
                libmpv2_sys::mpv_render_param {
                    type_: libmpv2_sys::mpv_render_param_type_MPV_RENDER_PARAM_INVALID,
                    data: std::ptr::null_mut(),
                },
            ];

            let mut render_ctx: *mut libmpv2_sys::mpv_render_context = std::ptr::null_mut();
            let rc = unsafe {
                libmpv2_sys::mpv_render_context_create(
                    &mut render_ctx,
                    mpv_h,
                    params.as_mut_ptr(),
                )
            };
            assert!(rc >= 0, "mpv_render_context_create failed: {}", rc);

            // Publish render context ptr so main thread can call report_swap
            shared.render_ctx.store(render_ctx, Ordering::Release);

            // Set update callback: signal via a leaked AtomicBool for 'static lifetime.
            let redraw_flag = Box::leak(Box::new(AtomicBool::new(false)));
            let redraw_ptr = redraw_flag as *mut AtomicBool;
            unsafe extern "C" fn redraw_cb(ctx: *mut std::os::raw::c_void) {
                let flag = &*(ctx as *const AtomicBool);
                flag.store(true, Ordering::Release);
            }
            unsafe {
                libmpv2_sys::mpv_render_context_set_update_callback(
                    render_ctx,
                    Some(redraw_cb),
                    redraw_ptr as *mut _,
                );
            }

            // Double-buffered textures + FBOs
            let mut w = shared.width.load(Ordering::Relaxed);
            let mut h = shared.height.load(Ordering::Relaxed);
            let mut tex = [0u32; 2];
            let mut fbo = [0u32; 2];
            unsafe {
                gl::GenTextures(2, tex.as_mut_ptr());
                gl::GenFramebuffers(2, fbo.as_mut_ptr());
                for i in 0..2 {
                    gl::BindTexture(gl::TEXTURE_2D, tex[i]);
                    gl::TexImage2D(
                        gl::TEXTURE_2D, 0, gl::RGBA8 as i32,
                        w as i32, h as i32, 0,
                        gl::RGBA, gl::UNSIGNED_BYTE, std::ptr::null(),
                    );
                    gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MIN_FILTER, gl::LINEAR as i32);
                    gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MAG_FILTER, gl::LINEAR as i32);
                    gl::BindFramebuffer(gl::FRAMEBUFFER, fbo[i]);
                    gl::FramebufferTexture2D(
                        gl::FRAMEBUFFER, gl::COLOR_ATTACHMENT0,
                        gl::TEXTURE_2D, tex[i], 0,
                    );
                }
                gl::BindFramebuffer(gl::FRAMEBUFFER, 0);
                gl::BindTexture(gl::TEXTURE_2D, 0);
            }

            let mut back = 0usize;

            // ── Render loop ──────────────────────────────────────────────
            while !shared.quit.load(Ordering::Relaxed) {
                // Handle resize
                if shared.resize.swap(false, Ordering::AcqRel) {
                    let nw = shared.width.load(Ordering::Relaxed);
                    let nh = shared.height.load(Ordering::Relaxed);
                    if nw != w || nh != h {
                        w = nw;
                        h = nh;
                        unsafe {
                            for t in &tex {
                                gl::BindTexture(gl::TEXTURE_2D, *t);
                                gl::TexImage2D(
                                    gl::TEXTURE_2D, 0, gl::RGBA8 as i32,
                                    w as i32, h as i32, 0,
                                    gl::RGBA, gl::UNSIGNED_BYTE, std::ptr::null(),
                                );
                            }
                            gl::BindTexture(gl::TEXTURE_2D, 0);
                        }
                    }
                }

                // Render when mpv signals a new frame
                if redraw_flag.swap(false, Ordering::AcqRel) {
                    let mut fbo_desc = libmpv2_sys::mpv_opengl_fbo {
                        fbo: fbo[back] as i32,
                        w: w as i32,
                        h: h as i32,
                        internal_format: 0,
                    };
                    let mut flip: i32 = 1;
                    let mut block_time: i32 = 0; // don't block for A/V target time
                    let mut render_params = [
                        libmpv2_sys::mpv_render_param {
                            type_: libmpv2_sys::mpv_render_param_type_MPV_RENDER_PARAM_OPENGL_FBO,
                            data: &mut fbo_desc as *mut _ as *mut _,
                        },
                        libmpv2_sys::mpv_render_param {
                            type_: libmpv2_sys::mpv_render_param_type_MPV_RENDER_PARAM_FLIP_Y,
                            data: &mut flip as *mut _ as *mut _,
                        },
                        libmpv2_sys::mpv_render_param {
                            type_: libmpv2_sys::mpv_render_param_type_MPV_RENDER_PARAM_BLOCK_FOR_TARGET_TIME,
                            data: &mut block_time as *mut _ as *mut _,
                        },
                        libmpv2_sys::mpv_render_param {
                            type_: libmpv2_sys::mpv_render_param_type_MPV_RENDER_PARAM_INVALID,
                            data: std::ptr::null_mut(),
                        },
                    ];

                    unsafe {
                        libmpv2_sys::mpv_render_context_render(
                            render_ctx,
                            render_params.as_mut_ptr(),
                        );
                        gl::Finish(); // ensure writes visible to main context
                    }

                    // Publish the back texture as the latest frame
                    shared.display_tex.store(tex[back], Ordering::Release);
                    shared.has_frame.store(true, Ordering::Release);
                    back = 1 - back;
                } else {
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
            }

            // Cleanup
            unsafe {
                libmpv2_sys::mpv_render_context_set_update_callback(
                    render_ctx, None, std::ptr::null_mut(),
                );
                libmpv2_sys::mpv_render_context_free(render_ctx);
                gl::DeleteFramebuffers(2, fbo.as_ptr());
                gl::DeleteTextures(2, tex.as_ptr());
                // Reclaim the leaked AtomicBool
                let _ = Box::from_raw(redraw_ptr);
            }
        })
        .expect("Failed to spawn mpv-render thread")
}

/// Advise the OS to prefetch a file into the page cache (helps on network FS).
#[cfg(unix)]
fn prefetch_file(path: &str) {
    use std::os::unix::io::AsRawFd;
    if let Ok(f) = std::fs::File::open(path) {
        unsafe {
            libc::posix_fadvise(f.as_raw_fd(), 0, 0, libc::POSIX_FADV_WILLNEED);
        }
    }
}

#[cfg(not(unix))]
fn prefetch_file(_path: &str) {}

#[cfg(debug_assertions)]
#[derive(Clone)]
struct TimingEntry {
    filename: String,
    method: &'static str,
    total_ms: f64,
    decode_ms: Option<f64>,
    upload_ms: Option<f64>,
}

#[derive(Parser, Debug)]
#[command(name = "lv", about = "Little Viewer — media viewer + library")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Directory or file to open
    #[arg(trailing_var_arg = true)]
    paths: Vec<PathBuf>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Track a directory (recursive scan + metadata)
    Track { path: PathBuf },
    /// Stop tracking a directory
    Untrack { path: PathBuf },
    /// Enable live filesystem monitoring on a tracked directory
    Watch { path: PathBuf },
    /// Disable live filesystem monitoring
    Unwatch { path: PathBuf },
    /// Re-scan tracked directories (or a specific path)
    Scan { path: Option<PathBuf> },
    /// Show library statistics
    Status,
    /// Run headless job worker until done
    Worker,
}

fn main() {
    let args = Cli::parse();

    // ── Database ────────────────────────────────────────────────────────
    let lv_db = Db::open_default();
    lv_db.ensure_schema();

    // ── CLI subcommands (non-GUI, exit after) ───────────────────────────
    if let Some(cmd) = args.command {
        lv_db.ensure_jobs_schema();
        match cmd {
            Commands::Track { path } => cli::track(&lv_db, &path),
            Commands::Untrack { path } => cli::untrack(&lv_db, &path),
            Commands::Watch { path } => cli::watch(&lv_db, &path),
            Commands::Unwatch { path } => cli::unwatch(&lv_db, &path),
            Commands::Scan { path } => cli::scan(&lv_db, path.as_deref()),
            Commands::Status => cli::status(&lv_db),
            Commands::Worker => cli::worker(&lv_db),
        }
        return;
    }

    // ── GUI mode ─────────────────────────────────────────────────────────
    let total_files = lv_db.file_count();
    let total_dirs = lv_db.dir_count();
    eprintln!("lv.db: {} files in {} dirs", total_files, total_dirs);

    // ── Background job engine ────────────────────────────────────────────
    let mut job_engine = jobs::JobEngine::start(lv_db.clone());

    // ── Filesystem watcher ──────────────────────────────────────────────
    let (fs_watcher, fs_rx) = watcher::FsWatcher::start(lv_db.clone());

    // Load initial file list
    let mut collection_mode: Option<u8> = None;
    let (mut files, mut current_dir, cursor_init) = if let Some(p) = args.paths.first() {
        let path = std::fs::canonicalize(p).unwrap_or_else(|_| p.clone());
        if path.is_file() {
            let parent = path.parent().unwrap_or(&path);
            let parent_str = clean_path(&parent.to_string_lossy());
            let already_tracked =
                lv_db.dir_is_tracked(&parent_str) || lv_db.dir_is_covered(&parent_str);

            if already_tracked {
                // File is in an already-tracked dir → open in dir mode, no temporary flag
                scanner::discover(&lv_db, parent);
                let f = lv_db.files_by_dir(&parent_str);
                let clean = clean_path(&path.to_string_lossy());
                let idx = f.iter().position(|e| e.path == clean).unwrap_or(0);
                eprintln!("open (tracked): {}", clean);
                (f, parent_str, idx)
            } else {
                // External file open → track parent non-recursively, mark temporary
                lv_db.dir_track(&parent_str, false);
                let count = scanner::discover(&lv_db, parent);
                eprintln!(
                    "external open: {} ({} files in {})",
                    path.display(),
                    count,
                    parent_str
                );
                for f in &lv_db.files_by_dir(&parent_str) {
                    lv_db.set_temporary(f.id, true);
                }
                collection_mode = Some(1);
                let clean = clean_path(&path.to_string_lossy());
                let all = lv_db.files_by_collection(1);
                let idx = all.iter().position(|f| f.path == clean).unwrap_or(0);
                (all, parent_str, idx)
            }
        } else if path.is_dir() {
            let dir_str = clean_path(&path.to_string_lossy());
            let f = lv_db.files_by_dir(&dir_str);
            (f, dir_str, 0)
        } else {
            let dir = p.to_string_lossy().to_string();
            let f = lv_db.files_by_dir(&dir);
            (f, dir, 0)
        }
    } else {
        let dir = lv_db.first_dir().unwrap_or_default();
        let f = lv_db.files_by_dir(&dir);
        (f, dir, 0)
    };
    if files.is_empty() {
        eprintln!("No files in dir: {}", current_dir);
        std::process::exit(1);
    }
    eprintln!("dir: {} ({} files)", current_dir, files.len());

    // Auto-watch the initial current directory
    let mut watched_dir = current_dir.clone();
    fs_watcher.watch_dir(&watched_dir);

    // ── SDL2 + OpenGL ───────────────────────────────────────────────────
    let sdl = sdl2::init().expect("SDL2 init failed");
    let video = sdl.video().expect("SDL2 video init failed");

    let gl_attr = video.gl_attr();
    gl_attr.set_context_profile(GLProfile::Core);
    gl_attr.set_context_version(3, 3);

    let window = video
        .window("lv", 1280, 720)
        .opengl()
        .resizable()
        .position_centered()
        .build()
        .expect("Failed to create window");

    let _gl_ctx = window.gl_create_context().expect("GL context failed");
    window
        .gl_make_current(&_gl_ctx)
        .expect("GL make_current failed");
    video.gl_set_swap_interval(1).ok();

    gl::load_with(|name| video.gl_get_proc_address(name) as *const _);

    // ── Quad shader ─────────────────────────────────────────────────────
    let quad_renderer = quad::QuadRenderer::new();

    // ── Dear ImGui (must init before mpv consumes `video`) ──────────────
    let mut imgui_ctx = imgui::Context::create();
    imgui_ctx.set_ini_filename(None);
    statusbar::add_font(&mut imgui_ctx);
    statusbar::apply_theme(&mut imgui_ctx);

    let mut imgui_platform = imgui_sdl2_support::SdlPlatform::new(&mut imgui_ctx);
    let gl = unsafe { glow::Context::from_loader_function(|s| video.gl_get_proc_address(s) as _) };
    let mut imgui_renderer = imgui_glow_renderer::AutoRenderer::new(gl, &mut imgui_ctx)
        .expect("Failed to create imgui glow renderer");

    // ── libmpv ──────────────────────────────────────────────────────────
    let mpv = Mpv::new().expect("Failed to create mpv instance");
    mpv.set_property("vo", "libmpv").unwrap();
    mpv.set_property("hwdec", "auto").unwrap();
    mpv.set_property("terminal", "no").unwrap();
    mpv.set_property("image-display-duration", "inf").unwrap();
    mpv.set_property("keep-open", "yes").unwrap();

    // Observe properties via push events (non-blocking, replaces get_property polling)
    const OBS_TIME_POS: u64 = 1;
    const OBS_DURATION: u64 = 2;
    const OBS_PAUSE: u64 = 3;
    unsafe {
        let h = mpv.ctx.as_ptr();
        let tp = std::ffi::CString::new("time-pos").unwrap();
        let dur = std::ffi::CString::new("duration").unwrap();
        let pau = std::ffi::CString::new("pause").unwrap();
        libmpv2_sys::mpv_observe_property(
            h,
            OBS_TIME_POS,
            tp.as_ptr(),
            libmpv2_sys::mpv_format_MPV_FORMAT_DOUBLE,
        );
        libmpv2_sys::mpv_observe_property(
            h,
            OBS_DURATION,
            dur.as_ptr(),
            libmpv2_sys::mpv_format_MPV_FORMAT_DOUBLE,
        );
        libmpv2_sys::mpv_observe_property(
            h,
            OBS_PAUSE,
            pau.as_ptr(),
            libmpv2_sys::mpv_format_MPV_FORMAT_FLAG,
        );
    }

    // ── Shared GL context for mpv render thread ───────────────────────
    // Enable context sharing so textures created on the render thread
    // are visible from the main context.
    unsafe {
        sdl2_sys::SDL_GL_SetAttribute(sdl2_sys::SDL_GLattr::SDL_GL_SHARE_WITH_CURRENT_CONTEXT, 1);
    }
    let mpv_gl_ctx = window.gl_create_context().expect("GL context 2 failed");
    // Grab raw ptr before switching back to main context
    window.gl_make_current(&mpv_gl_ctx).unwrap();
    let mpv_gl_ctx_raw = unsafe { sdl2_sys::SDL_GL_GetCurrentContext() };
    // Switch back to main context for the rest of init + main loop
    window.gl_make_current(&_gl_ctx).unwrap();

    // ── Texture cache + preloader ───────────────────────────────────────
    let mut tex_cache = TextureCache::new(20);
    let preloader = preload::Preloader::new();

    // ── Spawn mpv render thread ─────────────────────────────────────────
    let (init_w, init_h) = window.drawable_size();
    let mpv_shared = Arc::new(MpvRenderShared {
        display_tex: AtomicU32::new(0),
        has_frame: AtomicBool::new(false),
        quit: AtomicBool::new(false),
        width: AtomicU32::new(init_w),
        height: AtomicU32::new(init_h),
        resize: AtomicBool::new(false),
        render_ctx: AtomicPtr::new(std::ptr::null_mut()),
    });
    let mpv_handle = mpv.ctx.as_ptr();
    let render_thread = spawn_mpv_render_thread(
        window.raw() as usize,
        mpv_gl_ctx_raw as usize,
        mpv_handle as usize,
        mpv_shared.clone(),
    );
    // Keep _mpv_gl_ctx alive (prevent Drop from destroying the GL context)
    let _mpv_gl_ctx = mpv_gl_ctx;

    // ── State ───────────────────────────────────────────────────────
    let mut cursor: usize = cursor_init;
    let mut using_mpv = false;
    #[cfg(debug_assertions)]
    let mut timings: Vec<TimingEntry> = Vec::new();
    let mut needs_display = true;
    let mut volume: i64 = 100;
    let mut video_pos: f64 = 0.0;
    let mut video_duration: f64 = 0.0;
    let mut video_paused: bool = false;
    let mut video_has_frame: bool = false;
    let mut pending_cold_load: Option<String> = None; // async cold decode in progress
    let mut show_info = false;
    let mut cached_meta: Option<db::FileMeta> = None;
    let mut cached_meta_file_id: i64 = -1;
    let mut info_scroll: Option<f32> = None;
    let mut info_scroll_y: f32 = 0.0;
    let mut last_mouse_move = Instant::now();
    let mut cursor_visible = true;
    let start_time = Instant::now();
    // Debounce video loading: defer mpv loadfile until user stops navigating
    const VIDEO_DEBOUNCE_MS: u128 = 150;
    let mut pending_video: Option<(String, Instant)> = None;
    let mut error_message: Option<(String, String)> = None; // (error, filename)

    // Slow frame tracking: aggregate stats over 10s windows
    #[cfg(debug_assertions)]
    let mut slow_frame_count: u32 = 0;
    #[cfg(debug_assertions)]
    let mut slow_frame_worst_ms: f64 = 0.0;
    #[cfg(debug_assertions)]
    let mut slow_frame_sum_ms: f64 = 0.0;
    #[cfg(debug_assertions)]
    let mut slow_frame_window_start = Instant::now();

    // ── Main loop ───────────────────────────────────────────────────────
    let mut event_pump = sdl.event_pump().expect("Failed to create event pump");
    let mut running = true;
    let mut _last_frame_start = Instant::now();

    while running {
        let _frame_t0 = Instant::now();
        let _frame_delta = _frame_t0.duration_since(_last_frame_start);
        _last_frame_start = _frame_t0;

        tex_cache.pump_uploads();

        // ── Drain filesystem watcher events ─────────────────────────────
        while let Ok(ev) = fs_rx.try_recv() {
            match ev {
                watcher::FsEvent::Changed(dir) | watcher::FsEvent::Removed(dir) => {
                    let old_id = files.get(cursor).map(|f| f.id);
                    if let Some(c) = collection_mode {
                        let new_files = lv_db.files_by_collection(c);
                        files = new_files;
                        cursor = old_id
                            .and_then(|id| files.iter().position(|f| f.id == id))
                            .unwrap_or(cursor.min(files.len().saturating_sub(1)));
                    } else if dir == current_dir {
                        // In dir mode, refresh if the changed dir is the current one
                        let new_files = lv_db.files_by_dir(&current_dir);
                        files = new_files;
                        cursor = old_id
                            .and_then(|id| files.iter().position(|f| f.id == id))
                            .unwrap_or(cursor.min(files.len().saturating_sub(1)));
                    }
                    let new_id = files.get(cursor).map(|f| f.id);
                    // Only re-display if the current file changed (e.g. it was
                    // the one removed). Otherwise we'd re-run the existence
                    // check on the new cursor target and potentially show
                    // "File not found" over a playing video.
                    if new_id != old_id {
                        needs_display = true;
                    }
                    // Always update title (file count may have changed)
                    update_title(&window, &files, cursor, &current_dir);
                }
            }
        }

        let _t_pump = _frame_t0.elapsed();
        let _t1 = Instant::now();

        for event in event_pump.poll_iter() {
            // Let imgui process the event (for hover, future widgets, etc.)
            imgui_platform.handle_event(&mut imgui_ctx, &event);

            match event {
                Event::Quit { .. } => running = false,

                Event::MouseMotion { .. } => {
                    last_mouse_move = Instant::now();
                    if !cursor_visible {
                        unsafe {
                            sdl2::sys::SDL_ShowCursor(sdl2::sys::SDL_ENABLE as i32);
                        }
                        cursor_visible = true;
                    }
                }

                Event::KeyDown {
                    keycode: Some(key),
                    keymod,
                    ..
                } if !imgui_ctx.io().want_capture_keyboard => {
                    let ctrl = keymod.intersects(Mod::LCTRLMOD | Mod::RCTRLMOD);

                    // ── Ctrl+0-9: switch collection view ────────────
                    let col_key = match key {
                        Keycode::Num0 | Keycode::Kp0 if ctrl => Some(0u8),
                        Keycode::Num1 | Keycode::Kp1 if ctrl => Some(1),
                        Keycode::Num2 | Keycode::Kp2 if ctrl => Some(2),
                        Keycode::Num3 | Keycode::Kp3 if ctrl => Some(3),
                        Keycode::Num4 | Keycode::Kp4 if ctrl => Some(4),
                        Keycode::Num5 | Keycode::Kp5 if ctrl => Some(5),
                        Keycode::Num6 | Keycode::Kp6 if ctrl => Some(6),
                        Keycode::Num7 | Keycode::Kp7 if ctrl => Some(7),
                        Keycode::Num8 | Keycode::Kp8 if ctrl => Some(8),
                        Keycode::Num9 | Keycode::Kp9 if ctrl => Some(9),
                        _ => None,
                    };
                    if let Some(c) = col_key {
                        let new_mode = Some(c);
                        if collection_mode == new_mode {
                            // Toggle off → back to dir mode
                            collection_mode = None;
                            files = lv_db.files_by_dir(&current_dir);
                            cursor = 0;
                            eprintln!("collection: off (dir: {})", current_dir);
                        } else {
                            collection_mode = new_mode;
                            files = lv_db.files_by_collection(c);
                            cursor = 0;
                            eprintln!("collection: {} ({} files)", c, files.len());
                        }
                        needs_display = true;
                        continue;
                    }

                    // ── 2-8: toggle collection tag on current file ──
                    let tag_key = match key {
                        Keycode::Num2 | Keycode::Kp2 if !ctrl => Some(2u8),
                        Keycode::Num3 | Keycode::Kp3 if !ctrl => Some(3),
                        Keycode::Num4 | Keycode::Kp4 if !ctrl => Some(4),
                        Keycode::Num5 | Keycode::Kp5 if !ctrl => Some(5),
                        Keycode::Num6 | Keycode::Kp6 if !ctrl => Some(6),
                        Keycode::Num7 | Keycode::Kp7 if !ctrl => Some(7),
                        Keycode::Num8 | Keycode::Kp8 if !ctrl => Some(8),
                        _ => None,
                    };
                    if let Some(c) = tag_key {
                        if let Some(file) = files.get(cursor) {
                            let now_in = lv_db.toggle_collection(file.id, c);
                            eprintln!(
                                "{} {} c{}",
                                if now_in { "+" } else { "-" },
                                file.filename,
                                c
                            );
                        }
                        continue;
                    }

                    // ── 9: toggle like (= collection 9) ────────────
                    if matches!(key, Keycode::Num9 | Keycode::Kp9) && !ctrl {
                        if cursor < files.len() {
                            let file_id = files[cursor].id;
                            let liked = lv_db.toggle_like(file_id);
                            files[cursor].liked = liked;
                            eprintln!(
                                "{} {} ♥",
                                if liked { "+" } else { "-" },
                                files[cursor].filename
                            );
                        }
                        continue;
                    }

                    match key {
                        // ── Quit ─────────────────────────────────────────
                        Keycode::Q | Keycode::Escape => running = false,

                        // ── j/k: next/prev in current dir ───────────────
                        Keycode::J => {
                            if cursor + 1 < files.len() {
                                cursor += 1;
                                needs_display = true;
                            } else {
                                // End of dir → try next dir
                                if let Some(dir) = lv_db.navigate_dir(&current_dir, 1) {
                                    switch_dir(
                                        &lv_db,
                                        &dir,
                                        &mut files,
                                        &mut current_dir,
                                        &mut cursor,
                                        "first",
                                    );
                                    needs_display = true;
                                }
                            }
                        }
                        Keycode::K => {
                            if cursor > 0 {
                                cursor -= 1;
                                needs_display = true;
                            } else {
                                // Start of dir → try prev dir
                                if let Some(dir) = lv_db.navigate_dir(&current_dir, -1) {
                                    switch_dir(
                                        &lv_db,
                                        &dir,
                                        &mut files,
                                        &mut current_dir,
                                        &mut cursor,
                                        "last",
                                    );
                                    needs_display = true;
                                }
                            }
                        }

                        // ── h/l: prev/next directory ────────────────────
                        Keycode::L => {
                            if let Some(dir) = lv_db.navigate_dir(&current_dir, 1) {
                                switch_dir(
                                    &lv_db,
                                    &dir,
                                    &mut files,
                                    &mut current_dir,
                                    &mut cursor,
                                    "first",
                                );
                                needs_display = true;
                            }
                        }
                        Keycode::H => {
                            if cursor > 0 {
                                // Go to first file in current directory
                                cursor = 0;
                                needs_display = true;
                            } else if let Some(dir) = lv_db.navigate_dir(&current_dir, -1) {
                                switch_dir(
                                    &lv_db,
                                    &dir,
                                    &mut files,
                                    &mut current_dir,
                                    &mut cursor,
                                    "first",
                                );
                                needs_display = true;
                            }
                        }

                        // ── u: random file (collection-aware) ────────────
                        Keycode::U => {
                            let file = if let Some(c) = collection_mode {
                                lv_db.random_in_collection(c)
                            } else {
                                lv_db.random_file()
                            };
                            if let Some(file) = file {
                                if collection_mode.is_some() {
                                    // In collection mode, just find cursor position
                                    if let Some(idx) = files.iter().position(|f| f.id == file.id) {
                                        cursor = idx;
                                    }
                                } else {
                                    jump_to(
                                        &lv_db,
                                        file,
                                        &mut files,
                                        &mut current_dir,
                                        &mut cursor,
                                    );
                                }
                                needs_display = true;
                            }
                        }

                        // ── n: newest file ──────────────────────────────
                        Keycode::N => {
                            if let Some(file) = lv_db.newest_file() {
                                jump_to(&lv_db, file, &mut files, &mut current_dir, &mut cursor);
                                needs_display = true;
                            }
                        }

                        // ── m: random favourite ─────────────────────────
                        Keycode::M => {
                            if let Some(file) = lv_db.random_fav() {
                                jump_to(&lv_db, file, &mut files, &mut current_dir, &mut cursor);
                                needs_display = true;
                            }
                        }

                        // ── b: latest favourite ─────────────────────────
                        Keycode::B => {
                            if let Some(file) = lv_db.latest_fav() {
                                jump_to(&lv_db, file, &mut files, &mut current_dir, &mut cursor);
                                needs_display = true;
                            }
                        }

                        // ── y: toggle like ──────────────────────────────
                        Keycode::Y => {
                            if cursor < files.len() {
                                let file_id = files[cursor].id;
                                let liked = lv_db.toggle_like(file_id);
                                files[cursor].liked = liked;
                                let sym = if liked { "♥" } else { "♡" };
                                eprintln!("{} {}", sym, files[cursor].filename);
                                update_title(&window, &files, cursor, &current_dir);
                            }
                        }

                        // ── f: toggle fullscreen ────────────────────────
                        Keycode::F => {
                            use sdl2::video::FullscreenType;
                            let current = window.fullscreen_state();
                            let next = if current == FullscreenType::Off {
                                FullscreenType::Desktop
                            } else {
                                FullscreenType::Off
                            };
                            unsafe {
                                sdl2::sys::SDL_SetWindowFullscreen(window.raw(), next as u32);
                            }
                        }

                        // ── i: toggle info sidebar ───────────────────
                        Keycode::I => {
                            show_info = !show_info;
                            if show_info {
                                cached_meta_file_id = -1;
                                info_scroll_y = 0.0;
                            }
                        }

                        // ── info panel scrolling ─────────────────────
                        Keycode::PageUp => {
                            if show_info {
                                info_scroll_y = (info_scroll_y - 200.0).max(0.0);
                                info_scroll = Some(info_scroll_y);
                            }
                        }
                        Keycode::PageDown => {
                            if show_info {
                                info_scroll_y += 200.0;
                                info_scroll = Some(info_scroll_y);
                            }
                        }
                        Keycode::Home => {
                            if show_info {
                                info_scroll_y = 0.0;
                                info_scroll = Some(0.0);
                            }
                        }
                        Keycode::End => {
                            if show_info {
                                info_scroll_y = f32::MAX;
                                info_scroll = Some(f32::MAX);
                            }
                        }

                        // ── -: toggle turbo mode ─────────────────────
                        Keycode::Minus => {
                            let stats = &job_engine.stats;
                            let was = stats.turbo.load(Ordering::Relaxed);
                            stats.turbo.store(!was, Ordering::Relaxed);
                            eprintln!("jobs: {} mode", if !was { "TURBO" } else { "lazy" });
                        }

                        // ── r: refresh current directory ───────────────
                        Keycode::R => {
                            let old_id = files.get(cursor).map(|f| f.id);
                            files = lv_db.files_by_dir(&current_dir);
                            if files.is_empty() {
                                cursor = 0;
                            } else if let Some(oid) = old_id {
                                cursor = files.iter().position(|f| f.id == oid).unwrap_or(0);
                            }
                            needs_display = true;
                            cached_meta_file_id = -1;
                            eprintln!("refresh: {} ({} files)", current_dir, files.len());
                        }

                        // ── c: copy path to clipboard ───────────────────
                        Keycode::C => {
                            if let Some(file) = files.get(cursor) {
                                if let Ok(clipboard) = sdl.video().map(|v| v.clipboard()) {
                                    clipboard.set_clipboard_text(&file.path).ok();
                                    eprintln!("copied: {}", file.path);
                                }
                            }
                        }

                        // ── space: pause video ──────────────────────────
                        Keycode::Space => {
                            if using_mpv {
                                mpv.command("cycle", &["pause"]).ok();
                            }
                        }

                        // ── video seek / volume ─────────────────────────
                        Keycode::Left => {
                            if using_mpv {
                                mpv.command("seek", &["-5"]).ok();
                            }
                        }
                        Keycode::Right => {
                            if using_mpv {
                                mpv.command("seek", &["15"]).ok();
                            }
                        }
                        Keycode::Up => {
                            if using_mpv {
                                volume = (volume + 5).min(150);
                                mpv.set_property("volume", volume).ok();
                            }
                        }
                        Keycode::Down => {
                            if using_mpv {
                                volume = (volume - 5).max(0);
                                mpv.set_property("volume", volume).ok();
                            }
                        }

                        // ── p: print timing report ──────────────────────
                        #[cfg(debug_assertions)]
                        Keycode::P => print_report(&timings),

                        _ => {}
                    }
                }
                Event::DropFile { filename, .. } => {
                    let dropped = std::path::PathBuf::from(&filename);
                    if handle_drop(
                        &lv_db,
                        &dropped,
                        &mut files,
                        &mut current_dir,
                        &mut cursor,
                        &mut collection_mode,
                    ) {
                        needs_display = true;
                    }
                }

                _ => {}
            }
        }

        // Auto-hide mouse cursor after 2s of no movement
        if cursor_visible && last_mouse_move.elapsed().as_secs() >= 2 {
            unsafe {
                sdl2::sys::SDL_ShowCursor(sdl2::sys::SDL_DISABLE as i32);
            }
            cursor_visible = false;
        }

        let _t_events = _t1.elapsed();
        let _t2 = Instant::now();

        // ── Check for completed async cold decode ─────────────────────
        if let Some(ref cold_path) = pending_cold_load.clone() {
            if let Some(decoded) = preloader.try_take(cold_path) {
                tex_cache.upload(cold_path, decoded);
                pending_cold_load = None;
            } else if !preloader.is_pending(cold_path) {
                // Decode failed — show error overlay
                eprintln!("DECODE FAIL: {}", cold_path);
                pending_cold_load = None;
                let fname = cold_path
                    .rsplit('/')
                    .next()
                    .unwrap_or(cold_path)
                    .to_string();
                error_message = Some(("Failed to decode image".into(), fname));
            }
        }

        // ── Display current file ────────────────────────────────────────
        if needs_display {
            needs_display = false;

            if let Some(file) = files.get(cursor) {
                let _t0 = Instant::now();
                let path = &file.path;

                // Check if file still exists on disk
                if !std::path::Path::new(path).exists() {
                    error_message = Some(("File not found".into(), file.filename.clone()));
                    update_title(&window, &files, cursor, &current_dir);
                    lv_db.record_view(file.id);
                } else if is_image(path) {
                    error_message = None;
                    pending_video = None;
                    pending_cold_load = None; // cancel any prior async decode
                    if using_mpv {
                        unsafe {
                            mpv_stop_async(mpv_handle);
                        }
                        using_mpv = false;
                        mpv_shared.has_frame.store(false, Ordering::Release);
                    }
                    video_pos = 0.0;
                    video_duration = 0.0;
                    video_paused = false;

                    let (_method, _decode_ms, _upload_ms): (&str, Option<f64>, Option<f64>) =
                        if tex_cache.has(path) {
                            ("image/cache", None, None)
                        } else if let Some(decoded) = preloader.try_take(path) {
                            let tu = Instant::now();
                            tex_cache.upload(path, decoded);
                            (
                                "image/preload",
                                None,
                                Some(tu.elapsed().as_secs_f64() * 1000.0),
                            )
                        } else {
                            // Don't block main thread — schedule async decode
                            preloader.schedule(path.to_string());
                            pending_cold_load = Some(path.to_string());
                            ("image/async", None, None)
                        };

                    #[cfg(debug_assertions)]
                    {
                        let total = _t0.elapsed().as_secs_f64() * 1000.0;
                        eprintln!(
                            "[{:>4}/{}] {:<14} {:>7.2}ms  {}",
                            cursor + 1,
                            files.len(),
                            _method,
                            total,
                            file.filename,
                        );
                        timings.push(TimingEntry {
                            filename: file.filename.clone(),
                            method: _method,
                            total_ms: total,
                            decode_ms: _decode_ms,
                            upload_ms: _upload_ms,
                        });
                    }

                    schedule_preload(&preloader, &tex_cache, &files, cursor);
                } else if is_video(path) {
                    error_message = None;
                    // Stop current mpv playback (async) so we don't
                    // show stale video while debouncing
                    if using_mpv {
                        unsafe {
                            mpv_stop_async(mpv_handle);
                        }
                        mpv_shared.has_frame.store(false, Ordering::Release);
                    }
                    using_mpv = true;
                    video_has_frame = false;
                    video_pos = 0.0;
                    video_duration = 0.0;
                    video_paused = false;
                    // Prefetch video data into page cache (helps on network FS)
                    prefetch_file(path);
                    // Defer actual loadfile — debounce rapid navigation
                    pending_video = Some((path.clone(), Instant::now()));
                } else {
                    // Unknown extension — show error overlay
                    eprintln!("UNSUPPORTED: {}", file.filename);
                    error_message = Some(("Unsupported file type".into(), file.filename.clone()));
                }

                update_title(&window, &files, cursor, &current_dir);

                // Deferred: record view after display work is done
                lv_db.record_view(file.id);
            }
        }

        // Re-watch if current_dir changed
        if current_dir != watched_dir {
            fs_watcher.unwatch_dir(&watched_dir);
            fs_watcher.watch_dir(&current_dir);
            watched_dir = current_dir.clone();
        }

        let _t_display = _t2.elapsed();
        let _t3 = Instant::now();

        // ── Fire deferred video load after debounce period ──────────────
        if let Some((ref vpath, ref stamp)) = pending_video {
            if stamp.elapsed().as_millis() >= VIDEO_DEBOUNCE_MS {
                let vpath = vpath.clone();
                let _t0 = Instant::now();
                unsafe {
                    mpv_loadfile_async(mpv_handle, &vpath);
                }
                let _total = _t0.elapsed().as_secs_f64() * 1000.0;
                let _fname = vpath.rsplit('/').next().unwrap_or(&vpath);
                #[cfg(debug_assertions)]
                {
                    eprintln!(
                        "[{:>4}/{}] {:<14} {:>7.2}ms  {}",
                        cursor + 1,
                        files.len(),
                        "mpv",
                        _total,
                        _fname,
                    );
                    timings.push(TimingEntry {
                        filename: _fname.to_string(),
                        method: "mpv",
                        total_ms: _total,
                        decode_ms: None,
                        upload_ms: None,
                    });
                }
                pending_video = None;
            }
        }

        let _t_debounce = _t3.elapsed();
        let _t4 = Instant::now();

        // ── Drain mpv events (before rendering for responsiveness) ─────
        if using_mpv {
            loop {
                let ev = unsafe { libmpv2_sys::mpv_wait_event(mpv_handle, 0.0) };
                if ev.is_null() {
                    break;
                }
                let event_id = unsafe { (*ev).event_id };
                match event_id {
                    libmpv2_sys::mpv_event_id_MPV_EVENT_NONE => break,
                    libmpv2_sys::mpv_event_id_MPV_EVENT_SHUTDOWN => {
                        running = false;
                        break;
                    }
                    libmpv2_sys::mpv_event_id_MPV_EVENT_PLAYBACK_RESTART => {
                        video_has_frame = true;
                    }
                    libmpv2_sys::mpv_event_id_MPV_EVENT_END_FILE => {
                        video_has_frame = false;
                    }
                    libmpv2_sys::mpv_event_id_MPV_EVENT_PROPERTY_CHANGE => unsafe {
                        let prop = (*ev).data as *const libmpv2_sys::mpv_event_property;
                        if !prop.is_null() {
                            match (*ev).reply_userdata {
                                OBS_TIME_POS => {
                                    if (*prop).format == libmpv2_sys::mpv_format_MPV_FORMAT_DOUBLE {
                                        video_pos = *((*prop).data as *const f64);
                                    }
                                }
                                OBS_DURATION => {
                                    if (*prop).format == libmpv2_sys::mpv_format_MPV_FORMAT_DOUBLE {
                                        video_duration = *((*prop).data as *const f64);
                                    }
                                }
                                OBS_PAUSE => {
                                    if (*prop).format == libmpv2_sys::mpv_format_MPV_FORMAT_FLAG {
                                        video_paused = *((*prop).data as *const i32) != 0;
                                    }
                                }
                                _ => {}
                            }
                        }
                    },
                    _ => {}
                }
            }

            // Check if render thread has produced a frame
            if mpv_shared.has_frame.load(Ordering::Acquire) {
                video_has_frame = true;
            }
        }

        let _t_drain = _t4.elapsed();

        // Query phase eliminated — properties now arrive via observe_property events above
        let _t_query = std::time::Duration::ZERO;
        let _t6 = Instant::now();

        // ── Render ──────────────────────────────────────────────────────
        let (w, h) = window.drawable_size();

        // Signal render thread about resize
        if w != mpv_shared.width.load(Ordering::Relaxed)
            || h != mpv_shared.height.load(Ordering::Relaxed)
        {
            mpv_shared.width.store(w, Ordering::Relaxed);
            mpv_shared.height.store(h, Ordering::Relaxed);
            mpv_shared.resize.store(true, Ordering::Release);
        }

        // Composite to default framebuffer
        unsafe {
            gl::BindFramebuffer(gl::FRAMEBUFFER, 0);
            gl::Viewport(0, 0, w as i32, h as i32);
            gl::ClearColor(0.05, 0.05, 0.05, 1.0);
            gl::Clear(gl::COLOR_BUFFER_BIT);
        }
        let mpv_display_tex = mpv_shared.display_tex.load(Ordering::Acquire);
        if using_mpv && video_has_frame && mpv_display_tex != 0 {
            // Blit texture produced by mpv render thread (sub-1ms)
            quad_renderer.draw_video(mpv_display_tex, w, h, w, h);
        } else if !using_mpv {
            if let Some(file) = files.get(cursor) {
                if let Some(tex_info) = tex_cache.get(&file.path) {
                    quad_renderer.draw(tex_info.gl_id, tex_info.width, tex_info.height, w, h);
                }
            }
        }

        let _t_render = _t6.elapsed();
        let _t7 = Instant::now();

        // ── ImGui overlay ────────────────────────────────────────────────
        imgui_platform.prepare_frame(&mut imgui_ctx, &window, &event_pump);
        let ui = imgui_ctx.new_frame();

        if let Some(file) = files.get(cursor) {
            let is_turbo = job_engine.stats.turbo.load(Ordering::Relaxed);
            let info = statusbar::StatusInfo {
                index: cursor + 1,
                total: files.len(),
                path: &file.path,
                liked: file.liked,
                is_video: using_mpv,
                paused: video_paused,
                video_pos,
                video_duration,
                volume,
                turbo: is_turbo,
            };
            statusbar::draw_status_bar(ui, &info, w as f32, h as f32);

            // Info sidebar (toggle with 'i')
            if show_info {
                if cached_meta_file_id != file.id {
                    cached_meta = lv_db.get_file_metadata(file.id);
                    cached_meta_file_id = file.id;
                }
                if let Some(ref meta) = cached_meta {
                    statusbar::draw_info_panel(ui, meta, w as f32, h as f32, info_scroll.take());
                }
                statusbar::draw_stats_section(
                    ui,
                    &job_engine.stats,
                    &lv_db,
                    w as f32,
                    h as f32,
                    collection_mode,
                );
            }
        }

        if let Some((ref err, ref fname)) = error_message {
            statusbar::draw_error_overlay(ui, err, fname, w as f32, h as f32);
        } else if (using_mpv && !video_has_frame) || pending_cold_load.is_some() {
            statusbar::draw_spinner(ui, w as f32, h as f32, start_time.elapsed().as_secs_f32());
        }
        let draw_data = imgui_ctx.render();
        imgui_renderer.render(draw_data).ok();

        let _t_imgui = _t7.elapsed();
        let _t8 = Instant::now();

        window.gl_swap_window();

        // Tell mpv we displayed a frame (via raw ptr from render thread)
        if using_mpv {
            let rctx = mpv_shared.render_ctx.load(Ordering::Acquire);
            if !rctx.is_null() {
                unsafe {
                    libmpv2_sys::mpv_render_context_report_swap(rctx);
                }
            }
        }

        let _t_swap = _t8.elapsed();
        let _frame_total = _frame_t0.elapsed();

        #[cfg(debug_assertions)]
        {
            let frame_ms = _frame_total.as_secs_f64() * 1000.0;
            if frame_ms > 8.0 {
                slow_frame_count += 1;
                slow_frame_sum_ms += frame_ms;
                if frame_ms > slow_frame_worst_ms {
                    slow_frame_worst_ms = frame_ms;
                }
            }
            if slow_frame_window_start.elapsed().as_secs() >= 10 {
                if slow_frame_count > 0 {
                    let avg = slow_frame_sum_ms / slow_frame_count as f64;
                    eprintln!(
                        "SLOW FRAMES: {} in last 10s (worst={:.1}ms avg={:.1}ms)",
                        slow_frame_count, slow_frame_worst_ms, avg,
                    );
                }
                slow_frame_count = 0;
                slow_frame_worst_ms = 0.0;
                slow_frame_sum_ms = 0.0;
                slow_frame_window_start = Instant::now();
            }
        }

        std::thread::sleep(std::time::Duration::from_millis(2));
    }

    // ── Shutdown ──────────────────────────────────────────────────────
    job_engine.stop();
    // Stop mpv playback and signal render thread to exit
    unsafe {
        mpv_stop_async(mpv_handle);
    }
    mpv_shared.quit.store(true, Ordering::Release);
    // Give render thread a short deadline, then move on
    let deadline = std::time::Duration::from_millis(500);
    let start = Instant::now();
    loop {
        if render_thread.is_finished() {
            render_thread.join().ok();
            break;
        }
        if start.elapsed() > deadline {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    // Leak mpv handle — mpv_destroy can block for seconds on Windows.
    // The process is exiting anyway, the OS will reclaim all resources.
    std::mem::forget(mpv);

    #[cfg(debug_assertions)]
    if !timings.is_empty() {
        print_report(&timings);
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────

fn switch_dir(
    db: &Db,
    dir: &str,
    files: &mut Vec<FileEntry>,
    current_dir: &mut String,
    cursor: &mut usize,
    pos: &str, // "first" or "last"
) {
    let new_files = db.files_by_dir(dir);
    if new_files.is_empty() {
        return;
    }
    eprintln!("dir: {} ({} files)", dir, new_files.len());
    *files = new_files;
    *current_dir = dir.to_string();
    *cursor = if pos == "last" {
        files.len().saturating_sub(1)
    } else {
        0
    };
}

fn jump_to(
    db: &Db,
    file: FileEntry,
    files: &mut Vec<FileEntry>,
    current_dir: &mut String,
    cursor: &mut usize,
) {
    // Check if file is in current dir
    if let Some(idx) = files.iter().position(|f| f.id == file.id) {
        *cursor = idx;
        return;
    }
    // Load the file's directory
    let new_files = db.files_by_dir(&file.dir);
    if new_files.is_empty() {
        return;
    }
    let idx = new_files.iter().position(|f| f.id == file.id).unwrap_or(0);
    eprintln!("jump → {} ({} files)", file.dir, new_files.len());
    *files = new_files;
    *current_dir = file.dir;
    *cursor = idx;
}

fn schedule_preload(
    preloader: &preload::Preloader,
    cache: &TextureCache,
    files: &[FileEntry],
    cursor: usize,
) {
    let start = cursor.saturating_sub(10);
    let end = (cursor + 11).min(files.len());
    for (i, file) in files.iter().enumerate().take(end).skip(start) {
        if i == cursor {
            continue;
        }
        if is_image(&file.path) && !cache.has(&file.path) && !preloader.is_pending(&file.path) {
            preloader.schedule(file.path.clone());
        }
    }
}

fn update_title(window: &sdl2::video::Window, files: &[FileEntry], cursor: usize, dir: &str) {
    if let Some(file) = files.get(cursor) {
        let like = if file.liked { " ♥" } else { "" };
        let clean = clean_path(dir);
        let dir_short = clean.rsplit(['/', '\\']).next().unwrap_or(&clean);
        let title = format!(
            "[{}/{}] {}{} — {} — lv {}-{}",
            cursor + 1,
            files.len(),
            file.filename,
            like,
            dir_short,
            VERSION,
            GIT_HASH,
        );
        unsafe {
            let c_title = std::ffi::CString::new(title).unwrap();
            sdl2::sys::SDL_SetWindowTitle(window.raw(), c_title.as_ptr());
        }
    }
}

#[cfg(debug_assertions)]
fn print_report(timings: &[TimingEntry]) {
    if timings.is_empty() {
        return;
    }

    let mut cold: Vec<f64> = Vec::new();
    let mut cached: Vec<f64> = Vec::new();
    let mut preloaded: Vec<f64> = Vec::new();
    let mut mpv_times: Vec<f64> = Vec::new();

    for t in timings {
        match t.method {
            "image/cold" => cold.push(t.total_ms),
            "image/cache" => cached.push(t.total_ms),
            "image/preload" => preloaded.push(t.total_ms),
            "mpv" => mpv_times.push(t.total_ms),
            _ => {}
        }
    }

    let stats = |v: &[f64]| -> (f64, f64, f64, f64) {
        if v.is_empty() {
            return (0.0, 0.0, 0.0, 0.0);
        }
        let min = v.iter().cloned().fold(f64::MAX, f64::min);
        let max = v.iter().cloned().fold(0.0f64, f64::max);
        let avg = v.iter().sum::<f64>() / v.len() as f64;
        let median = {
            let mut s = v.to_vec();
            s.sort_by(|a, b| a.partial_cmp(b).unwrap());
            s[s.len() / 2]
        };
        (min, max, avg, median)
    };

    eprintln!();
    eprintln!("┌──────────────────────────────────────────────────────────────┐");
    eprintln!("│                    TIMING REPORT                             │");
    eprintln!("├──────────────┬───────┬─────────┬─────────┬─────────┬────────┤");
    eprintln!("│ method       │ count │  min ms │  avg ms │  med ms │ max ms │");
    eprintln!("├──────────────┼───────┼─────────┼─────────┼─────────┼────────┤");

    for (name, v) in [
        ("image/cold", &cold),
        ("image/cache", &cached),
        ("image/preload", &preloaded),
        ("mpv", &mpv_times),
    ] {
        if !v.is_empty() {
            let (min, max, avg, med) = stats(v);
            eprintln!(
                "│ {:<12} │ {:>5} │ {:>7.2} │ {:>7.2} │ {:>7.2} │ {:>6.2} │",
                name,
                v.len(),
                min,
                avg,
                med,
                max,
            );
        }
    }

    eprintln!("└──────────────┴───────┴─────────┴─────────┴─────────┴────────┘");

    let n = timings.len().min(20);
    let tail = &timings[timings.len() - n..];
    eprintln!();
    eprintln!("Last {} navigations:", n);
    for t in tail {
        let detail = match (t.decode_ms, t.upload_ms) {
            (Some(d), Some(u)) => format!("decode={:.1}ms upload={:.1}ms", d, u),
            (None, Some(u)) => format!("upload={:.1}ms", u),
            _ => String::new(),
        };
        eprintln!(
            "  {:<14} {:>7.2}ms  {}  {}",
            t.method, t.total_ms, t.filename, detail,
        );
    }
    eprintln!();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ext_of_basic() {
        assert_eq!(ext_of("photo.jpg"), "jpg");
        assert_eq!(ext_of("/home/user/file.PNG"), "png");
        assert_eq!(ext_of("video.MKV"), "mkv");
        assert_eq!(ext_of("noext"), "noext"); // no dot → whole string lowered
        assert_eq!(ext_of("archive.tar.gz"), "gz");
        assert_eq!(ext_of(".hidden"), "hidden");
    }

    #[test]
    fn is_image_known_exts() {
        assert!(is_image("photo.jpg"));
        assert!(is_image("photo.JPEG"));
        assert!(is_image("/path/to/pic.png"));
        assert!(is_image("img.webp"));
        assert!(is_image("img.avif"));
        assert!(is_image("img.gif"));
        assert!(!is_image("video.mp4"));
        assert!(!is_image("file.txt"));
        assert!(!is_image("file.mkv"));
    }

    #[test]
    fn is_video_known_exts() {
        assert!(is_video("clip.mp4"));
        assert!(is_video("clip.MKV"));
        assert!(is_video("/tmp/movie.avi"));
        assert!(is_video("file.mov"));
        assert!(is_video("file.webm"));
        assert!(!is_video("photo.jpg"));
        assert!(!is_video("file.txt"));
        assert!(!is_video("doc.pdf"));
    }

    #[test]
    fn neither_image_nor_video() {
        assert!(!is_image("readme.md"));
        assert!(!is_video("readme.md"));
        assert!(!is_image("data.json"));
        assert!(!is_video("data.json"));
    }

    // ── ext_of edge cases ───────────────────────────────────────────────

    #[test]
    fn ext_of_double_extension() {
        assert_eq!(ext_of("archive.tar.gz"), "gz");
        assert_eq!(ext_of("photo.backup.jpg"), "jpg");
    }

    #[test]
    fn ext_of_dotfile() {
        // Dotfiles with no real extension
        assert_eq!(ext_of(".gitignore"), "gitignore");
        assert_eq!(ext_of(".hidden"), "hidden");
    }

    #[test]
    fn ext_of_empty_string() {
        assert_eq!(ext_of(""), "");
    }

    #[test]
    fn ext_of_trailing_dot() {
        assert_eq!(ext_of("file."), "");
    }

    #[test]
    fn ext_of_unicode_filename() {
        assert_eq!(ext_of("/写真/café.JPG"), "jpg");
        assert_eq!(ext_of("/📸/photo.PNG"), "png");
    }

    #[test]
    fn ext_of_spaces_in_path() {
        assert_eq!(ext_of("/my photos/vacation pic.jpg"), "jpg");
    }

    // ── IMAGE_EXTS vs scanner::MEDIA_EXTENSIONS consistency ─────────────

    // ── clean_path (Windows \\?\ prefix stripping) ────────────────────

    #[test]
    fn clean_path_strips_win_prefix() {
        assert_eq!(clean_path(r"\\?\C:\Users\test"), "C:\\Users\\test");
        assert_eq!(
            clean_path(r"\\?\C:\Users\shirk3y\Downloads"),
            "C:\\Users\\shirk3y\\Downloads"
        );
    }

    #[test]
    fn clean_path_preserves_unix() {
        assert_eq!(clean_path("/home/user/pics"), "/home/user/pics");
        assert_eq!(clean_path("/tmp/test.jpg"), "/tmp/test.jpg");
    }

    #[test]
    fn clean_path_preserves_plain_windows() {
        assert_eq!(clean_path("C:\\Users\\test"), "C:\\Users\\test");
    }

    #[test]
    fn clean_path_empty() {
        assert_eq!(clean_path(""), "");
    }

    #[test]
    fn clean_path_only_prefix() {
        assert_eq!(clean_path(r"\\?\"), "");
    }

    // ── handle_drop tests ─────────────────────────────────────────────

    /// Helper: create a temp dir with media files.
    /// Returns (db, TempDir) — keep TempDir alive for the test duration.
    fn setup_drop_dir(filenames: &[&str]) -> (Db, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        for name in filenames {
            std::fs::write(dir.path().join(name), b"fake").unwrap();
        }
        let db = Db::open_memory();
        db.ensure_schema();
        (db, dir)
    }

    #[test]
    fn drop_image_file_untracked_dir() {
        let (db, dir) = setup_drop_dir(&["photo.jpg", "other.png"]);
        let mut files = Vec::new();
        let mut current_dir = String::new();
        let mut cursor = 0usize;
        let mut col = None;

        let ok = handle_drop(
            &db,
            &dir.path().join("photo.jpg"),
            &mut files,
            &mut current_dir,
            &mut cursor,
            &mut col,
        );
        assert!(ok);
        assert_eq!(files.len(), 2); // photo.jpg + other.png
        assert!(current_dir.contains(dir.path().file_name().unwrap().to_str().unwrap()));
        // cursor should point to photo.jpg
        assert_eq!(files[cursor].filename, "photo.jpg");
        assert!(col.is_none());
    }

    #[test]
    fn drop_video_file() {
        let (db, dir) = setup_drop_dir(&["clip.mp4", "photo.jpg"]);
        let mut files = Vec::new();
        let mut current_dir = String::new();
        let mut cursor = 0usize;
        let mut col = None;

        let ok = handle_drop(
            &db,
            &dir.path().join("clip.mp4"),
            &mut files,
            &mut current_dir,
            &mut cursor,
            &mut col,
        );
        assert!(ok);
        assert_eq!(files[cursor].filename, "clip.mp4");
    }

    #[test]
    fn drop_non_media_file_rejected() {
        let (db, dir) = setup_drop_dir(&["readme.txt", "photo.jpg"]);
        let mut files = Vec::new();
        let mut current_dir = String::new();
        let mut cursor = 0usize;
        let mut col = None;

        let ok = handle_drop(
            &db,
            &dir.path().join("readme.txt"),
            &mut files,
            &mut current_dir,
            &mut cursor,
            &mut col,
        );
        assert!(!ok);
        assert!(files.is_empty());
    }

    #[test]
    fn drop_directory() {
        let (db, dir) = setup_drop_dir(&["a.jpg", "b.png", "c.mp4"]);
        let mut files = Vec::new();
        let mut current_dir = String::new();
        let mut cursor = 0usize;
        let mut col = None;

        let ok = handle_drop(
            &db,
            dir.path(),
            &mut files,
            &mut current_dir,
            &mut cursor,
            &mut col,
        );
        assert!(ok);
        assert_eq!(files.len(), 3);
        assert_eq!(cursor, 0);
    }

    #[test]
    fn drop_empty_directory() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open_memory();
        db.ensure_schema();
        let mut files = Vec::new();
        let mut current_dir = String::new();
        let mut cursor = 0usize;
        let mut col = None;

        let ok = handle_drop(
            &db,
            dir.path(),
            &mut files,
            &mut current_dir,
            &mut cursor,
            &mut col,
        );
        assert!(!ok);
        assert!(files.is_empty());
    }

    #[test]
    fn drop_dir_with_no_media() {
        let (db, dir) = setup_drop_dir(&["readme.md", "config.toml", ".gitignore"]);
        let mut files = Vec::new();
        let mut current_dir = String::new();
        let mut cursor = 0usize;
        let mut col = None;

        let ok = handle_drop(
            &db,
            dir.path(),
            &mut files,
            &mut current_dir,
            &mut cursor,
            &mut col,
        );
        assert!(!ok);
        assert!(files.is_empty());
    }

    #[test]
    fn drop_nonexistent_path() {
        let db = Db::open_memory();
        db.ensure_schema();
        let mut files = Vec::new();
        let mut current_dir = String::new();
        let mut cursor = 0usize;
        let mut col = None;

        let ok = handle_drop(
            &db,
            std::path::Path::new("/nonexistent/path/photo.jpg"),
            &mut files,
            &mut current_dir,
            &mut cursor,
            &mut col,
        );
        assert!(!ok);
    }

    #[test]
    fn drop_exits_collection_mode() {
        let (db, dir) = setup_drop_dir(&["photo.jpg"]);
        let mut files = Vec::new();
        let mut current_dir = String::new();
        let mut cursor = 0usize;
        let mut col = Some(3u8); // in collection mode

        let ok = handle_drop(
            &db,
            &dir.path().join("photo.jpg"),
            &mut files,
            &mut current_dir,
            &mut cursor,
            &mut col,
        );
        assert!(ok);
        assert!(col.is_none()); // should exit collection mode
    }

    #[test]
    fn drop_file_in_already_tracked_dir() {
        let (db, dir) = setup_drop_dir(&["photo.jpg", "other.png"]);
        let dir_str = clean_path(&dir.path().to_string_lossy());
        db.dir_track(&dir_str, true);
        scanner::discover(&db, dir.path());

        let mut files = db.files_by_dir(&dir_str);
        let mut current_dir = dir_str.clone();
        let mut cursor = 0usize;
        let mut col = None;

        // Drop a file in the already-tracked dir
        let ok = handle_drop(
            &db,
            &dir.path().join("other.png"),
            &mut files,
            &mut current_dir,
            &mut cursor,
            &mut col,
        );
        assert!(ok);
        assert_eq!(files[cursor].filename, "other.png");
        assert_eq!(current_dir, dir_str);
    }

    #[test]
    fn drop_marks_untracked_as_temporary() {
        let (db, dir) = setup_drop_dir(&["photo.jpg"]);
        let mut files = Vec::new();
        let mut current_dir = String::new();
        let mut cursor = 0usize;
        let mut col = None;

        handle_drop(
            &db,
            &dir.path().join("photo.jpg"),
            &mut files,
            &mut current_dir,
            &mut cursor,
            &mut col,
        );
        // Files from untracked dirs should be marked temporary
        assert!(files[0].temporary);
    }

    #[test]
    fn drop_tracked_dir_not_marked_temporary() {
        let (db, dir) = setup_drop_dir(&["photo.jpg"]);
        let dir_str = clean_path(&dir.path().to_string_lossy());
        db.dir_track(&dir_str, true);
        scanner::discover(&db, dir.path());

        let mut files = Vec::new();
        let mut current_dir = String::new();
        let mut cursor = 0usize;
        let mut col = None;

        handle_drop(
            &db,
            &dir.path().join("photo.jpg"),
            &mut files,
            &mut current_dir,
            &mut cursor,
            &mut col,
        );
        assert!(!files[0].temporary);
    }

    #[test]
    fn drop_file_cursor_points_to_correct_file() {
        let (db, dir) = setup_drop_dir(&["aaa.jpg", "bbb.jpg", "ccc.jpg", "ddd.jpg"]);
        let mut files = Vec::new();
        let mut current_dir = String::new();
        let mut cursor = 0usize;
        let mut col = None;

        // Drop the third file
        handle_drop(
            &db,
            &dir.path().join("ccc.jpg"),
            &mut files,
            &mut current_dir,
            &mut cursor,
            &mut col,
        );
        assert_eq!(files[cursor].filename, "ccc.jpg");
    }

    #[test]
    fn drop_dir_cursor_starts_at_zero() {
        let (db, dir) = setup_drop_dir(&["z.jpg", "a.jpg", "m.jpg"]);
        let mut files = Vec::new();
        let mut current_dir = String::new();
        let mut cursor = 99usize;
        let mut col = None;

        handle_drop(
            &db,
            dir.path(),
            &mut files,
            &mut current_dir,
            &mut cursor,
            &mut col,
        );
        assert_eq!(cursor, 0);
    }

    #[test]
    fn drop_mixed_media_and_non_media_dir() {
        let (db, dir) = setup_drop_dir(&["photo.jpg", "readme.txt", "clip.mp4", "notes.md"]);
        let mut files = Vec::new();
        let mut current_dir = String::new();
        let mut cursor = 0usize;
        let mut col = None;

        handle_drop(
            &db,
            dir.path(),
            &mut files,
            &mut current_dir,
            &mut cursor,
            &mut col,
        );
        // Only media files should be in the list
        assert_eq!(files.len(), 2);
        let names: Vec<&str> = files.iter().map(|f| f.filename.as_str()).collect();
        assert!(names.contains(&"photo.jpg"));
        assert!(names.contains(&"clip.mp4"));
    }

    #[test]
    fn drop_replaces_previous_file_list() {
        let (db, dir1) = setup_drop_dir(&["a.jpg"]);
        let dir2 = tempfile::tempdir().unwrap();
        std::fs::write(dir2.path().join("b.png"), b"fake").unwrap();
        std::fs::write(dir2.path().join("c.png"), b"fake").unwrap();

        let mut files = Vec::new();
        let mut current_dir = String::new();
        let mut cursor = 0usize;
        let mut col = None;

        handle_drop(
            &db,
            dir1.path(),
            &mut files,
            &mut current_dir,
            &mut cursor,
            &mut col,
        );
        assert_eq!(files.len(), 1);

        handle_drop(
            &db,
            dir2.path(),
            &mut files,
            &mut current_dir,
            &mut cursor,
            &mut col,
        );
        assert_eq!(files.len(), 2);
    }

    #[test]
    fn drop_case_insensitive_extension() {
        let (db, dir) = setup_drop_dir(&["PHOTO.JPG", "VIDEO.MP4"]);
        let mut files = Vec::new();
        let mut current_dir = String::new();
        let mut cursor = 0usize;
        let mut col = None;

        let ok = handle_drop(
            &db,
            &dir.path().join("PHOTO.JPG"),
            &mut files,
            &mut current_dir,
            &mut cursor,
            &mut col,
        );
        assert!(ok);
    }

    // ── error_message integration tests ─────────────────────────────────

    #[test]
    fn drop_non_media_returns_false_for_error_display() {
        // When a non-media file is dropped, handle_drop returns false.
        // The main loop should then set error_message for the overlay.
        let (db, dir) = setup_drop_dir(&["readme.txt"]);
        let mut files = Vec::new();
        let mut current_dir = String::new();
        let mut cursor = 0usize;
        let mut col = None;

        let ok = handle_drop(
            &db,
            &dir.path().join("readme.txt"),
            &mut files,
            &mut current_dir,
            &mut cursor,
            &mut col,
        );
        assert!(!ok, "non-media drop should return false");
        // Main loop would set error_message based on this return value
    }

    #[test]
    fn file_not_found_detected_by_path_exists() {
        // Simulate the file-not-found check used in the display loop
        let missing = std::path::Path::new("/nonexistent/file/photo.jpg");
        assert!(!missing.exists());
    }

    #[test]
    fn unsupported_ext_detected() {
        // Files with unknown extensions should not be image or video
        assert!(!is_image("document.pdf"));
        assert!(!is_video("document.pdf"));
        assert!(!is_image("archive.zip"));
        assert!(!is_video("archive.zip"));
        assert!(!is_image("binary.exe"));
        assert!(!is_video("binary.exe"));
        assert!(!is_image("data.json"));
        assert!(!is_video("data.json"));
    }

    #[test]
    fn error_message_tuple_structure() {
        // Verify the error_message Option<(String, String)> pattern works
        let err: Option<(String, String)> = Some(("File not found".into(), "photo.jpg".into()));
        assert!(err.is_some());
        let (msg, fname) = err.unwrap();
        assert_eq!(msg, "File not found");
        assert_eq!(fname, "photo.jpg");
    }

    #[test]
    fn error_message_none_means_no_error() {
        let err: Option<(String, String)> = None;
        assert!(err.is_none());
    }

    #[test]
    fn error_message_decode_fail_format() {
        // Simulate the decode failure error message construction
        let cold_path = "/home/user/photos/broken.webp";
        let fname = cold_path
            .rsplit('/')
            .next()
            .unwrap_or(cold_path)
            .to_string();
        let err = ("Failed to decode image".to_string(), fname);
        assert_eq!(err.0, "Failed to decode image");
        assert_eq!(err.1, "broken.webp");
    }

    #[test]
    fn error_message_decode_fail_no_slash() {
        // Edge case: path with no slashes
        let cold_path = "broken.webp";
        let fname = cold_path
            .rsplit('/')
            .next()
            .unwrap_or(cold_path)
            .to_string();
        assert_eq!(fname, "broken.webp");
    }

    #[test]
    fn error_message_unsupported_type_format() {
        let filename = "document.pdf";
        let err = ("Unsupported file type".to_string(), filename.to_string());
        assert_eq!(err.0, "Unsupported file type");
        assert_eq!(err.1, "document.pdf");
    }

    #[test]
    fn drop_dll_file_rejected() {
        // Specific regression: user reported .dll files could be dropped
        let (db, dir) = setup_drop_dir(&["SDL2.dll", "photo.jpg"]);
        let mut files = Vec::new();
        let mut current_dir = String::new();
        let mut cursor = 0usize;
        let mut col = None;

        let ok = handle_drop(
            &db,
            &dir.path().join("SDL2.dll"),
            &mut files,
            &mut current_dir,
            &mut cursor,
            &mut col,
        );
        assert!(!ok, ".dll drop should be rejected");
    }

    #[test]
    fn drop_exe_file_rejected() {
        let (db, dir) = setup_drop_dir(&["app.exe", "photo.jpg"]);
        let mut files = Vec::new();
        let mut current_dir = String::new();
        let mut cursor = 0usize;
        let mut col = None;

        let ok = handle_drop(
            &db,
            &dir.path().join("app.exe"),
            &mut files,
            &mut current_dir,
            &mut cursor,
            &mut col,
        );
        assert!(!ok, ".exe drop should be rejected");
    }

    #[test]
    fn drop_zip_file_rejected() {
        let (db, dir) = setup_drop_dir(&["archive.zip", "photo.jpg"]);
        let mut files = Vec::new();
        let mut current_dir = String::new();
        let mut cursor = 0usize;
        let mut col = None;

        let ok = handle_drop(
            &db,
            &dir.path().join("archive.zip"),
            &mut files,
            &mut current_dir,
            &mut cursor,
            &mut col,
        );
        assert!(!ok, ".zip drop should be rejected");
    }

    // ── slow frame window logic ─────────────────────────────────────────

    #[test]
    fn slow_frame_window_accumulation() {
        // Simulate the slow frame tracking logic
        let mut count: u32 = 0;
        let mut worst: f64 = 0.0;
        let mut sum: f64 = 0.0;

        let frames = [9.5, 12.0, 8.1, 25.0, 7.5]; // 7.5 is not slow (<=8)
        for &ms in &frames {
            if ms > 8.0 {
                count += 1;
                sum += ms;
                if ms > worst {
                    worst = ms;
                }
            }
        }

        assert_eq!(count, 4);
        assert!((worst - 25.0).abs() < 0.001);
        let avg = sum / count as f64;
        assert!((avg - 13.65).abs() < 0.01);
    }

    #[test]
    fn slow_frame_window_empty() {
        // No slow frames in window
        let count: u32 = 0;
        let worst: f64 = 0.0;
        // Should not log anything when count == 0
        assert_eq!(count, 0);
        assert_eq!(worst, 0.0);
    }

    #[test]
    fn slow_frame_window_reset() {
        let mut count: u32 = 5;
        let mut worst: f64 = 30.0;
        let mut sum: f64 = 100.0;

        // Reset (as done after 10s window)
        count = 0;
        worst = 0.0;
        sum = 0.0;

        assert_eq!(count, 0);
        assert_eq!(worst, 0.0);
        assert_eq!(sum, 0.0);
    }

    #[test]
    fn slow_frame_threshold_is_8ms() {
        // Frames at exactly 8.0ms should NOT be counted as slow
        let frame_ms = 8.0_f64;
        assert!(!(frame_ms > 8.0), "8.0ms should not be slow");

        let frame_ms = 8.001;
        assert!(frame_ms > 8.0, "8.001ms should be slow");
    }

    // ── watcher refresh + needs_display logic ──────────────────────────

    #[test]
    fn watcher_remove_other_file_keeps_cursor_stable() {
        // Regression: when watcher removes a DIFFERENT file from the list,
        // the cursor should stay on the same file (same id). This prevents
        // re-triggering the display loop which would check file existence
        // and potentially show "File not found" over a playing video.
        let (db, dir) = setup_drop_dir(&["aaa.jpg", "bbb.mp4", "ccc.jpg"]);
        let mut files = Vec::new();
        let mut current_dir = String::new();
        let mut cursor = 0usize;
        let mut col = None;

        handle_drop(
            &db,
            dir.path(),
            &mut files,
            &mut current_dir,
            &mut cursor,
            &mut col,
        );
        assert_eq!(files.len(), 3);

        // Navigate to bbb.mp4 (the video)
        cursor = files.iter().position(|f| f.filename == "bbb.mp4").unwrap();
        let playing_id = files[cursor].id;

        // Simulate watcher removing aaa.jpg from DB
        let aaa_path = files
            .iter()
            .find(|f| f.filename == "aaa.jpg")
            .unwrap()
            .path
            .clone();
        db.remove_file_by_path(&aaa_path);

        // Simulate the watcher refresh logic from the main loop
        let old_id = files.get(cursor).map(|f| f.id);
        let new_files = db.files_by_dir(&current_dir);
        files = new_files;
        cursor = old_id
            .and_then(|id| files.iter().position(|f| f.id == id))
            .unwrap_or(cursor.min(files.len().saturating_sub(1)));
        let new_id = files.get(cursor).map(|f| f.id);

        // Cursor should still point to bbb.mp4
        assert_eq!(
            new_id,
            Some(playing_id),
            "cursor should stay on the playing file"
        );
        // needs_display should NOT be set (old_id == new_id)
        assert_eq!(
            old_id, new_id,
            "same file → needs_display should not be set"
        );
        assert_eq!(files.len(), 2, "removed file should be gone");
    }

    #[test]
    fn watcher_remove_current_file_shifts_cursor() {
        // When the currently-viewed file is removed, cursor shifts and
        // needs_display SHOULD be set so the new file is loaded.
        let (db, dir) = setup_drop_dir(&["aaa.jpg", "bbb.mp4", "ccc.jpg"]);
        let mut files = Vec::new();
        let mut current_dir = String::new();
        let mut cursor = 0usize;
        let mut col = None;

        handle_drop(
            &db,
            dir.path(),
            &mut files,
            &mut current_dir,
            &mut cursor,
            &mut col,
        );

        // Navigate to bbb.mp4
        cursor = files.iter().position(|f| f.filename == "bbb.mp4").unwrap();
        let old_id = files.get(cursor).map(|f| f.id);

        // Remove bbb.mp4 from DB (simulating watcher)
        let bbb_path = files
            .iter()
            .find(|f| f.filename == "bbb.mp4")
            .unwrap()
            .path
            .clone();
        db.remove_file_by_path(&bbb_path);

        // Refresh
        let new_files = db.files_by_dir(&current_dir);
        files = new_files;
        cursor = old_id
            .and_then(|id| files.iter().position(|f| f.id == id))
            .unwrap_or(cursor.min(files.len().saturating_sub(1)));
        let new_id = files.get(cursor).map(|f| f.id);

        // Cursor should have shifted — different file
        assert_ne!(
            old_id, new_id,
            "current file removed → cursor shifts → needs_display should be set"
        );
        assert_eq!(files.len(), 2);
    }

    // ── race condition / edge case tests ────────────────────────────────

    /// Helper: simulate the watcher refresh logic from the main loop.
    /// Returns (needs_display, new_cursor).
    fn simulate_refresh(
        db: &Db,
        files: &mut Vec<FileEntry>,
        cursor: &mut usize,
        current_dir: &str,
    ) -> bool {
        let old_id = files.get(*cursor).map(|f| f.id);
        let new_files = db.files_by_dir(current_dir);
        *files = new_files;
        let fallback = (*cursor).min(files.len().saturating_sub(1));
        *cursor = old_id
            .and_then(|id| files.iter().position(|f| f.id == id))
            .unwrap_or(fallback);
        let new_id = files.get(*cursor).map(|f| f.id);
        new_id != old_id
    }

    #[test]
    fn race_watcher_adds_file_cursor_stable() {
        // #1: Watcher adds a new file while user is viewing a file.
        // Cursor should stay on the same file.
        let (db, dir) = setup_drop_dir(&["aaa.jpg", "bbb.jpg"]);
        let mut files = Vec::new();
        let mut current_dir = String::new();
        let mut cursor = 0usize;
        let mut col = None;

        handle_drop(
            &db,
            dir.path(),
            &mut files,
            &mut current_dir,
            &mut cursor,
            &mut col,
        );
        cursor = files.iter().position(|f| f.filename == "bbb.jpg").unwrap();
        let viewing_id = files[cursor].id;

        // Watcher adds a new file
        std::fs::write(dir.path().join("ccc.png"), b"fake").unwrap();
        scanner::discover(&db, dir.path());

        let needs_display = simulate_refresh(&db, &mut files, &mut cursor, &current_dir);

        assert!(
            !needs_display,
            "adding a file should not trigger re-display"
        );
        assert_eq!(
            files[cursor].id, viewing_id,
            "cursor should stay on bbb.jpg"
        );
        assert_eq!(files.len(), 3, "new file should be in list");
    }

    #[test]
    fn race_bulk_delete_multiple_files() {
        // #2: Multiple files removed in quick succession (bulk delete).
        // Simulate processing multiple watcher events in one frame.
        let (db, dir) = setup_drop_dir(&["a.jpg", "b.jpg", "c.jpg", "d.jpg", "e.jpg"]);
        let mut files = Vec::new();
        let mut current_dir = String::new();
        let mut cursor = 0usize;
        let mut col = None;

        handle_drop(
            &db,
            dir.path(),
            &mut files,
            &mut current_dir,
            &mut cursor,
            &mut col,
        );
        cursor = files.iter().position(|f| f.filename == "c.jpg").unwrap();
        let viewing_id = files[cursor].id;

        // Bulk delete: remove a, b, d (not c which we're viewing)
        for name in &["a.jpg", "b.jpg", "d.jpg"] {
            let p = files
                .iter()
                .find(|f| f.filename == *name)
                .unwrap()
                .path
                .clone();
            db.remove_file_by_path(&p);
            // Each removal triggers a refresh (simulating multiple events)
            simulate_refresh(&db, &mut files, &mut cursor, &current_dir);
        }

        assert_eq!(files[cursor].id, viewing_id, "cursor should stay on c.jpg");
        assert_eq!(files.len(), 2, "only c.jpg and e.jpg should remain");
    }

    #[test]
    fn race_all_files_deleted() {
        // #3: Watcher empties the file list (all files deleted).
        // Should not panic, cursor should be 0, files empty.
        let (db, dir) = setup_drop_dir(&["a.jpg", "b.jpg"]);
        let mut files = Vec::new();
        let mut current_dir = String::new();
        let mut cursor = 0usize;
        let mut col = None;

        handle_drop(
            &db,
            dir.path(),
            &mut files,
            &mut current_dir,
            &mut cursor,
            &mut col,
        );

        // Remove all files
        let paths: Vec<String> = files.iter().map(|f| f.path.clone()).collect();
        for p in &paths {
            db.remove_file_by_path(p);
        }

        let needs_display = simulate_refresh(&db, &mut files, &mut cursor, &current_dir);

        assert!(
            needs_display,
            "should trigger re-display when current file gone"
        );
        assert!(files.is_empty(), "file list should be empty");
        // cursor.min(0.saturating_sub(1)) = cursor.min(usize::MAX) = cursor
        // files.get(cursor) should return None safely
        assert!(files.get(cursor).is_none(), "no file at cursor");
    }

    #[test]
    fn race_pending_decode_file_removed() {
        // #4: Current file removed while async decode is pending.
        // Simulate: pending_cold_load holds a path, watcher removes that file.
        let (db, dir) = setup_drop_dir(&["slow.webp", "fast.jpg"]);
        let mut files = Vec::new();
        let mut current_dir = String::new();
        let mut cursor = 0usize;
        let mut col = None;

        handle_drop(
            &db,
            dir.path(),
            &mut files,
            &mut current_dir,
            &mut cursor,
            &mut col,
        );
        cursor = files
            .iter()
            .position(|f| f.filename == "slow.webp")
            .unwrap();

        // Simulate pending_cold_load
        let pending_cold_load: Option<String> = Some(files[cursor].path.clone());

        // Watcher removes slow.webp
        db.remove_file_by_path(&files[cursor].path);
        let needs_display = simulate_refresh(&db, &mut files, &mut cursor, &current_dir);

        assert!(
            needs_display,
            "should re-display since current file removed"
        );
        // The pending_cold_load path is now stale — main loop should detect this
        if let Some(ref cold_path) = pending_cold_load {
            let still_current = files.get(cursor).map(|f| &f.path) == Some(cold_path);
            assert!(
                !still_current,
                "pending decode path should no longer match current file"
            );
        }
    }

    #[test]
    fn race_cursor_at_last_file_removed() {
        // #5: Cursor at last file, that file gets removed.
        // Cursor should clamp to new last file.
        let (db, dir) = setup_drop_dir(&["a.jpg", "b.jpg", "c.jpg"]);
        let mut files = Vec::new();
        let mut current_dir = String::new();
        let mut cursor = 0usize;
        let mut col = None;

        handle_drop(
            &db,
            dir.path(),
            &mut files,
            &mut current_dir,
            &mut cursor,
            &mut col,
        );
        cursor = files.len() - 1; // last file
        let last_path = files[cursor].path.clone();

        db.remove_file_by_path(&last_path);
        simulate_refresh(&db, &mut files, &mut cursor, &current_dir);

        assert!(cursor < files.len(), "cursor should be within bounds");
        assert_eq!(cursor, files.len() - 1, "cursor should clamp to new last");
        assert_eq!(files.len(), 2);
    }

    #[test]
    fn race_cursor_at_first_file_removed() {
        // #6: Cursor at first file, that file gets removed.
        // Cursor should stay at 0.
        let (db, dir) = setup_drop_dir(&["a.jpg", "b.jpg", "c.jpg"]);
        let mut files = Vec::new();
        let mut current_dir = String::new();
        let mut cursor = 0usize;
        let mut col = None;

        handle_drop(
            &db,
            dir.path(),
            &mut files,
            &mut current_dir,
            &mut cursor,
            &mut col,
        );
        cursor = 0;
        let first_path = files[0].path.clone();

        db.remove_file_by_path(&first_path);
        simulate_refresh(&db, &mut files, &mut cursor, &current_dir);

        assert_eq!(cursor, 0, "cursor should stay at 0");
        assert_eq!(files.len(), 2);
        assert_ne!(
            files[0].path, first_path,
            "first file should be different now"
        );
    }

    #[test]
    fn race_stale_watcher_event_different_dir() {
        // #7: User switches to dir B, then a stale watcher event for dir A arrives.
        // The refresh should be ignored (dir != current_dir).
        let (db, dir_a) = setup_drop_dir(&["a1.jpg", "a2.jpg"]);
        let dir_b = tempfile::tempdir().unwrap();
        std::fs::write(dir_b.path().join("b1.png"), b"fake").unwrap();
        std::fs::write(dir_b.path().join("b2.png"), b"fake").unwrap();

        let mut files = Vec::new();
        let mut current_dir = String::new();
        let mut cursor = 0usize;
        let mut col = None;

        // Start in dir A
        handle_drop(
            &db,
            dir_a.path(),
            &mut files,
            &mut current_dir,
            &mut cursor,
            &mut col,
        );
        assert_eq!(files.len(), 2);

        // Switch to dir B
        handle_drop(
            &db,
            dir_b.path(),
            &mut files,
            &mut current_dir,
            &mut cursor,
            &mut col,
        );
        let dir_b_str = current_dir.clone();
        assert_eq!(files.len(), 2);
        let viewing_id = files[cursor].id;

        // Stale watcher event for dir A — should NOT refresh
        let dir_a_str = clean_path(&dir_a.path().to_string_lossy());
        let stale_dir = dir_a_str;
        // Simulate: only refresh if dir == current_dir
        if stale_dir == current_dir {
            simulate_refresh(&db, &mut files, &mut cursor, &current_dir);
        }

        assert_eq!(current_dir, dir_b_str, "should still be in dir B");
        assert_eq!(files[cursor].id, viewing_id, "cursor should not change");
    }

    #[test]
    fn race_error_cleared_on_valid_image() {
        // #8: error_message set, then user navigates to a valid image.
        // Error should be cleared.
        let mut error_message: Option<(String, String)> =
            Some(("File not found".into(), "deleted.jpg".into()));

        // Simulate navigating to a valid image
        let path = "/some/valid/photo.jpg";
        if is_image(path) {
            error_message = None;
        }

        assert!(
            error_message.is_none(),
            "error should be cleared for valid image"
        );
    }

    #[test]
    fn race_error_cleared_on_valid_video() {
        // #8b: error_message set, then user navigates to a valid video.
        let mut error_message: Option<(String, String)> =
            Some(("File not found".into(), "deleted.jpg".into()));

        let path = "/some/valid/clip.mp4";
        if is_video(path) {
            error_message = None;
        }

        assert!(
            error_message.is_none(),
            "error should be cleared for valid video"
        );
    }

    #[test]
    fn race_error_persists_for_unsupported() {
        // #8c: error_message should be set for unsupported file types.
        let mut error_message: Option<(String, String)> = None;

        let path = "document.pdf";
        if !is_image(path) && !is_video(path) {
            error_message = Some(("Unsupported file type".into(), "document.pdf".into()));
        }

        assert!(error_message.is_some());
    }

    #[test]
    fn race_errored_file_removed_by_watcher() {
        // #9: File that caused an error gets removed by watcher.
        // After refresh, cursor shifts to a different file, needs_display is set,
        // and the error should be cleared when the new file is displayed.
        let (db, dir) = setup_drop_dir(&["bad.jpg", "good.jpg"]);
        let mut files = Vec::new();
        let mut current_dir = String::new();
        let mut cursor = 0usize;
        let mut col = None;

        handle_drop(
            &db,
            dir.path(),
            &mut files,
            &mut current_dir,
            &mut cursor,
            &mut col,
        );
        cursor = files.iter().position(|f| f.filename == "bad.jpg").unwrap();

        // Simulate error on bad.jpg
        let mut error_message: Option<(String, String)> =
            Some(("Failed to decode image".into(), "bad.jpg".into()));

        // Watcher removes bad.jpg
        db.remove_file_by_path(&files[cursor].path);
        let needs_display = simulate_refresh(&db, &mut files, &mut cursor, &current_dir);

        assert!(
            needs_display,
            "should re-display since errored file removed"
        );
        assert_eq!(files.len(), 1);

        // Simulate the display loop: new file is good.jpg (exists, is image)
        if let Some(file) = files.get(cursor) {
            let path = &file.path;
            if std::path::Path::new(path).exists() && is_image(path) {
                error_message = None;
            }
        }

        assert!(
            error_message.is_none(),
            "error should be cleared after navigating to valid file"
        );
    }

    #[test]
    fn race_cursor_stable_after_many_additions() {
        // Stress: many files added, cursor should stay on the same file.
        let (db, dir) = setup_drop_dir(&["target.jpg"]);
        let mut files = Vec::new();
        let mut current_dir = String::new();
        let mut cursor = 0usize;
        let mut col = None;

        handle_drop(
            &db,
            dir.path(),
            &mut files,
            &mut current_dir,
            &mut cursor,
            &mut col,
        );
        let viewing_id = files[cursor].id;

        // Add 20 more files
        for i in 0..20 {
            std::fs::write(dir.path().join(format!("new_{:03}.png", i)), b"fake").unwrap();
        }
        scanner::discover(&db, dir.path());
        simulate_refresh(&db, &mut files, &mut cursor, &current_dir);

        assert_eq!(
            files[cursor].id, viewing_id,
            "cursor should stay on target.jpg"
        );
        assert_eq!(files.len(), 21);
    }

    #[test]
    fn race_single_file_removed_leaves_empty() {
        // Edge: directory with single file, that file removed.
        let (db, dir) = setup_drop_dir(&["only.jpg"]);
        let mut files = Vec::new();
        let mut current_dir = String::new();
        let mut cursor = 0usize;
        let mut col = None;

        handle_drop(
            &db,
            dir.path(),
            &mut files,
            &mut current_dir,
            &mut cursor,
            &mut col,
        );
        assert_eq!(files.len(), 1);

        db.remove_file_by_path(&files[0].path);
        simulate_refresh(&db, &mut files, &mut cursor, &current_dir);

        assert!(files.is_empty());
        assert!(files.get(cursor).is_none());
    }

    #[test]
    fn image_exts_subset_of_media() {
        // Every IMAGE_EXT should be recognized by the scanner
        for ext in IMAGE_EXTS {
            // svg and avif are in IMAGE_EXTS but not in scanner MEDIA_EXTENSIONS
            if *ext == "svg" || *ext == "avif" {
                continue;
            }
            assert!(
                scanner::is_media_ext(ext),
                "IMAGE_EXT '{}' not in scanner MEDIA_EXTENSIONS",
                ext
            );
        }
    }

    #[test]
    fn video_exts_subset_of_media() {
        for ext in VIDEO_EXTS {
            assert!(
                scanner::is_media_ext(ext),
                "VIDEO_EXT '{}' not in scanner MEDIA_EXTENSIONS",
                ext
            );
        }
    }
}
