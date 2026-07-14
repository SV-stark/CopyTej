#![allow(clippy::collapsible_if, clippy::manual_flatten, clippy::new_without_default, clippy::too_many_arguments, clippy::manual_unwrap_or_default)]

pub mod commands;
pub mod engine;
pub mod ipc;
pub mod store;

use crate::engine::TransferEngine;
use crate::engine::conflict::ConflictManager;
use crate::engine::queue::QueueManager;
use crate::store::db::DbManager;
use std::sync::Arc;
use std::sync::Mutex;
use tauri::Manager;

pub struct InitialArgs {
    pub src_paths: Mutex<Vec<String>>,
    pub is_move: Mutex<bool>,
}

struct CliArgs {
    src_paths: Vec<String>,
    dest_dir: Option<String>,
    is_move: bool,
}

fn parse_cli_args() -> Option<CliArgs> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() <= 1 {
        return None;
    }

    let mut src_paths = Vec::new();
    let mut dest_dir = None;
    let mut is_move = false;
    let mut i = 1;

    while i < args.len() {
        match args[i].as_str() {
            "-d" | "--dest" => {
                if i + 1 < args.len() {
                    dest_dir = Some(args[i + 1].clone());
                    i += 2;
                } else {
                    i += 1;
                }
            }
            "-m" | "--move" => {
                is_move = true;
                i += 1;
            }
            "-c" | "--copy" => {
                is_move = false;
                i += 1;
            }
            s => {
                if !s.starts_with('-') {
                    src_paths.push(s.to_string());
                }
                i += 1;
            }
        }
    }

    if src_paths.is_empty() {
        None
    } else {
        Some(CliArgs {
            src_paths,
            dest_dir,
            is_move,
        })
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let cli_args = parse_cli_args();

    #[cfg(windows)]
    if let Some(ref args) = cli_args {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        let spec = ipc::pipe_server::PipeJobSpec {
            src_paths: args.src_paths.clone(),
            dest_dir: args.dest_dir.clone().unwrap_or_default(),
            is_move: args.is_move,
        };

        if rt.block_on(ipc::pipe_server::try_send_to_pipe(spec)) {
            // Forwarded successfully to the running instance, exit now
            return;
        }
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .setup(move |app| {
            // Get app data directory for DB storage
            let app_data_dir = app.path().app_data_dir().map_err(|e| {
                tauri::Error::Io(std::io::Error::other(e.to_string()))
            })?;
            let db_path = app_data_dir.join("copytej.db");

            let db = Arc::new(DbManager::new(db_path).map_err(|e| {
                tauri::Error::Io(std::io::Error::other(e.to_string()))
            })?);
            let _ = db.reset_running_jobs();
            let engine = Arc::new(TransferEngine::new());
            let conflict_manager = Arc::new(ConflictManager::new());

            let queue_manager = Arc::new(QueueManager::new(
                Arc::clone(&db),
                Arc::clone(&engine),
                Arc::clone(&conflict_manager),
                app.handle().clone(),
            ));

            // Start queue worker thread
            Arc::clone(&queue_manager).start_worker();

            // Start Named Pipe server on Windows
            #[cfg(windows)]
            {
                crate::ipc::pipe_server::start_pipe_server(Arc::clone(&queue_manager));
            }

            // Register state managers
            let initial_src = cli_args
                .as_ref()
                .map(|a| a.src_paths.clone())
                .unwrap_or_default();
            let initial_is_move = cli_args.as_ref().map(|a| a.is_move).unwrap_or(false);

            app.manage(InitialArgs {
                src_paths: Mutex::new(initial_src),
                is_move: Mutex::new(initial_is_move),
            });
            app.manage(db);
            app.manage(engine);
            app.manage(conflict_manager);
            app.manage(queue_manager);

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_cli_args,
            commands::add_transfer_job,
            commands::pause_transfer_job,
            commands::resume_transfer_job,
            commands::cancel_transfer_job,
            commands::resolve_conflict,
            commands::get_active_jobs,
            commands::get_job_details,
            commands::get_history,
            commands::get_setting,
            commands::set_setting,
            commands::select_directory,
            commands::select_files,
            commands::delete_job,
            commands::clear_history
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
