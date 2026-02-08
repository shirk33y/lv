mod cli;
pub mod data;
mod db;
mod debug;
mod ipc;
mod preload;
mod protocol;
mod scanner;
mod thumbs;
mod watcher;
mod worker;

use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "lv", about = "Little Viewer — stupid media tracker")]
struct Cli {
    /// Enable debug logging
    #[arg(short = 'd', long, global = true)]
    debug: bool,

    #[command(subcommand)]
    command: Option<Commands>,

    /// Paths to open in GUI (recursive)
    #[arg(trailing_var_arg = true)]
    paths: Vec<PathBuf>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Add directory to library
    Add { path: PathBuf },
    /// Scan watched dirs (or PATH if given)
    #[command(short_flag = 's')]
    Scan {
        path: Option<PathBuf>,
        /// Re-scan everything
        #[arg(short = 'a', long)]
        all: bool,
    },
    /// Watch directory for changes
    #[command(short_flag = 'w')]
    Watch { path: PathBuf },
    /// Unwatch directory
    #[command(short_flag = 'u')]
    Unwatch { path: PathBuf },
    /// Run headless worker (hash + thumbnail jobs)
    Worker {
        /// Drain all pending jobs then exit (default: loop forever)
        #[arg(long)]
        once: bool,
    },
    /// Show library and job status
    Status,
    /// Reset all thumbnails and re-enqueue generation jobs
    ResetThumbs,
    /// Diagnose and fix stalled/failed jobs, then run worker to completion
    Doctor,
}

#[cfg(all(not(debug_assertions), windows))]
fn detach_console() {
    unsafe {
        windows_sys::Win32::System::Console::FreeConsole();
    }
}

pub fn run() {
    let cli_args = Cli::parse();

    if cli_args.debug {
        debug::enable();
    }

    let db_path = db::default_db_path();
    debug::dbg_log!("db path: {}", db_path.display());
    let conn = db::open(&db_path).expect("failed to open database");
    let db = data::Db::new(conn);

    db.jobs_recover_stale();

    let thumb_db = db.clone();

    tauri::Builder::default()
        .manage(ipc::AppState { db: db.clone() })
        .register_asynchronous_uri_scheme_protocol("lv-file", |_ctx, request, responder| {
            std::thread::spawn(move || {
                responder.respond(protocol::handle_file_request(&request));
            });
        })
        .register_asynchronous_uri_scheme_protocol("thumb", move |_ctx, request, responder| {
            let db = thumb_db.clone();
            std::thread::spawn(move || {
                responder.respond(protocol::handle_thumb_request(&db, &request));
            });
        })
        .invoke_handler(tauri::generate_handler![
            ipc::get_files,
            ipc::navigate_dir,
            ipc::toggle_like,
            ipc::record_view,
            ipc::random_file,
            ipc::newest_file,
            ipc::random_fav,
            ipc::latest_fav,
            ipc::toggle_fullscreen,
            ipc::get_file_metadata,
            ipc::get_status,
            ipc::rescan,
            ipc::boost_jobs,
            ipc::get_first_dir,
            ipc::get_cwd,
            ipc::report_broken_thumb,
        ])
        .setup(move |app| {
            match cli_args.command {
                Some(Commands::Add { path }) => {
                    cli::add(&db, &path);
                    app.handle().exit(0);
                }
                Some(Commands::Scan { path, all }) => {
                    cli::scan(&db, path.as_deref(), all);
                    app.handle().exit(0);
                }
                Some(Commands::Watch { path }) => {
                    cli::watch(&db, &path);
                    app.handle().exit(0);
                }
                Some(Commands::Unwatch { path }) => {
                    cli::unwatch(&db, &path);
                    app.handle().exit(0);
                }
                Some(Commands::Status) => {
                    cli::status(&db);
                    app.handle().exit(0);
                }
                Some(Commands::ResetThumbs) => {
                    cli::reset_thumbs(&db);
                    app.handle().exit(0);
                }
                Some(Commands::Doctor) => {
                    cli::doctor(&db);
                    app.handle().exit(0);
                }
                Some(Commands::Worker { once }) => {
                    let db = db.clone();
                    let handle = app.handle().clone();
                    std::thread::spawn(move || {
                        worker::run_headless(&db, once);
                        if once {
                            handle.exit(0);
                        }
                    });
                }
                None => {
                    // GUI mode
                    #[cfg(all(not(debug_assertions), windows))]
                    detach_console();

                    // Scan CLI paths into database
                    for p in &cli_args.paths {
                        let abs = p.canonicalize().unwrap_or_else(|_| p.clone());
                        println!("Scanning {}...", abs.display());
                        scanner::discover(&db, &abs);
                    }

                    // Auto-start background worker (hash + thumbnail jobs)
                    let worker_db = db.clone();
                    std::thread::spawn(move || {
                        worker::run_headless(&worker_db, false);
                    });

                    // Create the main window
                    tauri::WebviewWindowBuilder::new(
                        app,
                        "main",
                        tauri::WebviewUrl::App("index.html".into()),
                    )
                    .title("lv")
                    .inner_size(1280.0, 800.0)
                    .build()?;
                }
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
