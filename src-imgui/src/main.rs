//! lv-imgui POC: full viewer with database, dual-path rendering, preloading.
//!
//! - Images: `image` crate decode → GL texture (feh-speed), LRU preload cache
//! - Videos: mpv render API
//! - Navigation: j/k h/l u n m b y f c q — same as Tauri version
//! - Reads from existing lv.db
//!
//! Usage: cargo run --release [-- <dir_override>]

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
    let (_fs_watcher, fs_rx) = watcher::FsWatcher::start(lv_db.clone());

    // Load initial file list
    let mut collection_mode: Option<u8> = None;
    let (mut files, mut current_dir, cursor_init) = if let Some(p) = args.paths.first() {
        let path = std::fs::canonicalize(p).unwrap_or_else(|_| p.clone());
        if path.is_file() {
            let parent = path.parent().unwrap_or(&path);
            let parent_str = parent.to_string_lossy();
            let already_tracked =
                lv_db.dir_is_tracked(&parent_str) || lv_db.dir_is_covered(&parent_str);

            if already_tracked {
                // File is in an already-tracked dir → open in dir mode, no temporary flag
                scanner::discover(&lv_db, parent);
                let f = lv_db.files_by_dir(&parent_str);
                let idx = f
                    .iter()
                    .position(|e| e.path == path.to_string_lossy().as_ref())
                    .unwrap_or(0);
                eprintln!("open (tracked): {}", path.display());
                (f, parent_str.to_string(), idx)
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
                let all = lv_db.files_by_collection(1);
                let idx = all
                    .iter()
                    .position(|f| f.path == path.to_string_lossy().as_ref())
                    .unwrap_or(0);
                (all, parent_str.to_string(), idx)
            }
        } else if path.is_dir() {
            let dir_str = path.to_string_lossy().to_string();
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
    let mut nav_forward: bool = true;
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
                    if let Some(c) = collection_mode {
                        let new_files = lv_db.files_by_collection(c);
                        let cur_id = files.get(cursor).map(|f| f.id);
                        files = new_files;
                        cursor = cur_id
                            .and_then(|id| files.iter().position(|f| f.id == id))
                            .unwrap_or(cursor.min(files.len().saturating_sub(1)));
                    } else if dir == current_dir {
                        // In dir mode, refresh if the changed dir is the current one
                        let new_files = lv_db.files_by_dir(&current_dir);
                        let cur_id = files.get(cursor).map(|f| f.id);
                        files = new_files;
                        cursor = cur_id
                            .and_then(|id| files.iter().position(|f| f.id == id))
                            .unwrap_or(cursor.min(files.len().saturating_sub(1)));
                    }
                    needs_display = true;
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
                            nav_forward = true;
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
                            nav_forward = false;
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
                // Decode failed — skip in navigation direction
                eprintln!("SKIP (async decode fail): {}", cold_path);
                pending_cold_load = None;
                if nav_forward && cursor + 1 < files.len() {
                    cursor += 1;
                    needs_display = true;
                } else if !nav_forward && cursor > 0 {
                    cursor -= 1;
                    needs_display = true;
                }
            }
        }

        // ── Display current file ────────────────────────────────────────
        if needs_display {
            needs_display = false;

            if let Some(file) = files.get(cursor) {
                let t0 = Instant::now();
                let path = &file.path;

                if is_image(path) {
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
                        let total = t0.elapsed().as_secs_f64() * 1000.0;
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
                    // Unknown extension — skip in navigation direction
                    eprintln!("SKIP (unknown ext): {}", file.filename);
                    if nav_forward && cursor + 1 < files.len() {
                        cursor += 1;
                        needs_display = true;
                    } else if !nav_forward && cursor > 0 {
                        cursor -= 1;
                        needs_display = true;
                    }
                }

                update_title(&window, &files, cursor, &current_dir);

                // Deferred: record view after display work is done
                lv_db.record_view(file.id);
            }
        }

        let _t_display = _t2.elapsed();
        let _t3 = Instant::now();

        // ── Fire deferred video load after debounce period ──────────────
        if let Some((ref vpath, ref stamp)) = pending_video {
            if stamp.elapsed().as_millis() >= VIDEO_DEBOUNCE_MS {
                let vpath = vpath.clone();
                let t0 = Instant::now();
                unsafe {
                    mpv_loadfile_async(mpv_handle, &vpath);
                }
                let total = t0.elapsed().as_secs_f64() * 1000.0;
                let fname = vpath.rsplit('/').next().unwrap_or(&vpath);
                #[cfg(debug_assertions)]
                {
                    eprintln!(
                        "[{:>4}/{}] {:<14} {:>7.2}ms  {}",
                        cursor + 1,
                        files.len(),
                        "mpv",
                        total,
                        fname,
                    );
                    timings.push(TimingEntry {
                        filename: fname.to_string(),
                        method: "mpv",
                        total_ms: total,
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
            quad_renderer.draw(mpv_display_tex, w, h, w, h);
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

        if (using_mpv && !video_has_frame) || pending_cold_load.is_some() {
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
        if _frame_total.as_millis() > 8 {
            eprintln!(
                "SLOW FRAME {:.1}ms (delta={:.1}ms) | pump={:.1} events={:.1} display={:.1} debounce={:.1} drain={:.1} query={:.1} render={:.1} imgui={:.1} swap={:.1}",
                _frame_total.as_secs_f64() * 1000.0,
                _frame_delta.as_secs_f64() * 1000.0,
                _t_pump.as_secs_f64() * 1000.0,
                _t_events.as_secs_f64() * 1000.0,
                _t_display.as_secs_f64() * 1000.0,
                _t_debounce.as_secs_f64() * 1000.0,
                _t_drain.as_secs_f64() * 1000.0,
                _t_query.as_secs_f64() * 1000.0,
                _t_render.as_secs_f64() * 1000.0,
                _t_imgui.as_secs_f64() * 1000.0,
                _t_swap.as_secs_f64() * 1000.0,
            );
        }

        std::thread::sleep(std::time::Duration::from_millis(2));
    }

    // ── Shutdown ──────────────────────────────────────────────────────
    job_engine.stop();
    mpv_shared.quit.store(true, Ordering::Release);
    render_thread.join().ok();

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
        let dir_short = dir.rsplit('/').next().unwrap_or(dir);
        let title = format!(
            "[{}/{}] {}{} — {} — lv",
            cursor + 1,
            files.len(),
            file.filename,
            like,
            dir_short,
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
