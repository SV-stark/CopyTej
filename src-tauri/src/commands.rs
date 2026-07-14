use crate::engine::TransferEngine;
use crate::engine::conflict::{ConflictManager, ConflictResolution};
use crate::engine::queue::QueueManager;
use crate::engine::{JobStatus, TransferJob};
use crate::store::db::DbManager;
use std::sync::Arc;
use tauri::State;
use uuid::Uuid;

#[tauri::command]
pub async fn add_transfer_job(
    queue_manager: State<'_, Arc<QueueManager>>,
    src_paths: Vec<String>,
    dest_dir: String,
    is_move: bool,
) -> Result<String, String> {
    let job_id = queue_manager.add_job(src_paths, dest_dir, is_move).await?;
    Ok(job_id.to_string())
}

#[tauri::command]
pub async fn pause_transfer_job(
    engine: State<'_, Arc<TransferEngine>>,
    db: State<'_, Arc<DbManager>>,
    job_id: String,
) -> Result<(), String> {
    let uuid = Uuid::parse_str(&job_id).map_err(|e| e.to_string())?;
    engine.pause_job(uuid);
    db.update_job_status(uuid, JobStatus::Paused, None)
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn resume_transfer_job(
    engine: State<'_, Arc<TransferEngine>>,
    queue_manager: State<'_, Arc<QueueManager>>,
    db: State<'_, Arc<DbManager>>,
    job_id: String,
) -> Result<(), String> {
    let uuid = Uuid::parse_str(&job_id).map_err(|e| e.to_string())?;
    engine.resume_job(uuid);
    db.update_job_status(uuid, JobStatus::Queued, None)
        .map_err(|e| e.to_string())?;
    queue_manager.trigger_worker();
    Ok(())
}

#[tauri::command]
pub async fn cancel_transfer_job(
    engine: State<'_, Arc<TransferEngine>>,
    db: State<'_, Arc<DbManager>>,
    job_id: String,
) -> Result<(), String> {
    let uuid = Uuid::parse_str(&job_id).map_err(|e| e.to_string())?;
    engine.cancel_job(uuid);
    db.update_job_status(
        uuid,
        JobStatus::Canceled,
        Some(chrono::Utc::now().timestamp()),
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn resolve_conflict(
    conflict_manager: State<'_, Arc<ConflictManager>>,
    conflict_id: String,
    resolution: ConflictResolution,
) -> Result<bool, String> {
    Ok(conflict_manager.resolve(&conflict_id, resolution))
}

#[tauri::command]
pub async fn get_active_jobs(db: State<'_, Arc<DbManager>>) -> Result<Vec<TransferJob>, String> {
    db.get_active_jobs().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_job_details(
    db: State<'_, Arc<DbManager>>,
    job_id: String,
) -> Result<Option<TransferJob>, String> {
    let uuid = Uuid::parse_str(&job_id).map_err(|e| e.to_string())?;
    db.get_job(uuid).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_history(
    db: State<'_, Arc<DbManager>>,
    limit: u32,
    offset: u32,
) -> Result<Vec<TransferJob>, String> {
    // Collect jobs by their history IDs to avoid nested locks
    let ids = db
        .get_history_ids(limit, offset)
        .map_err(|e| e.to_string())?;
    let mut jobs = Vec::new();
    for id in ids {
        if let Some(job) = db.get_job(id).map_err(|e| e.to_string())? {
            jobs.push(job);
        }
    }
    Ok(jobs)
}

#[tauri::command]
pub async fn get_setting(
    db: State<'_, Arc<DbManager>>,
    key: String,
) -> Result<Option<String>, String> {
    db.get_setting(&key).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn set_setting(
    engine: State<'_, Arc<TransferEngine>>,
    db: State<'_, Arc<DbManager>>,
    key: String,
    value: String,
) -> Result<(), String> {
    db.set_setting(&key, &value).map_err(|e| e.to_string())?;
    if key == "speed_limit_kbps" {
        let val = value.parse::<u64>().unwrap_or(0);
        engine
            .global_speed_limit_kbps
            .store(val, std::sync::atomic::Ordering::SeqCst);
    }
    Ok(())
}

fn file_path_to_string(file_path: tauri_plugin_dialog::FilePath) -> String {
    match file_path {
        tauri_plugin_dialog::FilePath::Path(path_buf) => path_buf.to_string_lossy().to_string(),
        tauri_plugin_dialog::FilePath::Url(url) => {
            if let Ok(p) = url.to_file_path() {
                p.to_string_lossy().to_string()
            } else {
                url.path().to_string()
            }
        }
    }
}

#[tauri::command]
pub async fn select_directory(app: tauri::AppHandle) -> Result<String, String> {
    use tauri_plugin_dialog::DialogExt;
    let folder_path = app.dialog().file().blocking_pick_folder();

    match folder_path {
        Some(path) => Ok(file_path_to_string(path)),
        None => Err("Cancelled".to_string()),
    }
}

#[tauri::command]
pub async fn select_files(app: tauri::AppHandle) -> Result<Vec<String>, String> {
    use tauri_plugin_dialog::DialogExt;
    let file_paths = app.dialog().file().blocking_pick_files();

    match file_paths {
        Some(paths) => {
            let list: Vec<String> = paths.into_iter().map(file_path_to_string).collect();
            Ok(list)
        }
        None => Err("Cancelled".to_string()),
    }
}

#[tauri::command]
pub async fn delete_job(db: State<'_, Arc<DbManager>>, job_id: String) -> Result<(), String> {
    let uuid = Uuid::parse_str(&job_id).map_err(|e| e.to_string())?;
    db.delete_job(uuid).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn clear_history(db: State<'_, Arc<DbManager>>) -> Result<(), String> {
    db.clear_history().map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn get_cli_args(
    state: tauri::State<'_, crate::InitialArgs>,
) -> Result<(Vec<String>, bool), String> {
    let src = state.src_paths.lock().unwrap().clone();
    let is_move = *state.is_move.lock().unwrap();
    state.src_paths.lock().unwrap().clear();
    Ok((src, is_move))
}

#[tauri::command]
pub async fn register_explorer_context_menu() -> Result<(), String> {
    #[cfg(windows)]
    {
        let exe_path = std::env::current_exe()
            .map_err(|e| format!("Failed to get current executable path: {}", e))?;
        let exe_str = exe_path.to_string_lossy();

        // Register for files (*)
        let reg_shell_file = r"HKCU\Software\Classes\*\shell\CopyTej";
        let reg_cmd_file = r"HKCU\Software\Classes\*\shell\CopyTej\command";

        let file_menu_cmd = std::process::Command::new("reg")
            .args(&[
                "add",
                reg_shell_file,
                "/ve",
                "/t",
                "REG_SZ",
                "/d",
                "Copy with CopyTej",
                "/f",
            ])
            .output()
            .map_err(|e| format!("Failed to run reg: {}", e))?;
        if !file_menu_cmd.status.success() {
            return Err("Failed to add file context menu registry key".to_string());
        }

        let file_exe_cmd = std::process::Command::new("reg")
            .args(&[
                "add",
                reg_cmd_file,
                "/ve",
                "/t",
                "REG_SZ",
                "/d",
                &format!("\"{}\" \"%1\"", exe_str),
                "/f",
            ])
            .output()
            .map_err(|e| format!("Failed to run reg: {}", e))?;
        if !file_exe_cmd.status.success() {
            return Err("Failed to add file command registry key".to_string());
        }

        // Register for directories (Directory)
        let reg_shell_dir = r"HKCU\Software\Classes\Directory\shell\CopyTej";
        let reg_cmd_dir = r"HKCU\Software\Classes\Directory\shell\CopyTej\command";

        let dir_menu_cmd = std::process::Command::new("reg")
            .args(&[
                "add",
                reg_shell_dir,
                "/ve",
                "/t",
                "REG_SZ",
                "/d",
                "Copy with CopyTej",
                "/f",
            ])
            .output()
            .map_err(|e| format!("Failed to run reg: {}", e))?;
        if !dir_menu_cmd.status.success() {
            return Err("Failed to add directory context menu registry key".to_string());
        }

        let dir_exe_cmd = std::process::Command::new("reg")
            .args(&[
                "add",
                reg_cmd_dir,
                "/ve",
                "/t",
                "REG_SZ",
                "/d",
                &format!("\"{}\" \"%1\"", exe_str),
                "/f",
            ])
            .output()
            .map_err(|e| format!("Failed to run reg: {}", e))?;
        if !dir_exe_cmd.status.success() {
            return Err("Failed to add directory command registry key".to_string());
        }

        Ok(())
    }
    #[cfg(not(windows))]
    {
        Err("Registry operations are only supported on Windows".to_string())
    }
}

#[tauri::command]
pub async fn unregister_explorer_context_menu() -> Result<(), String> {
    #[cfg(windows)]
    {
        let reg_shell_file = r"HKCU\Software\Classes\*\shell\CopyTej";
        let reg_shell_dir = r"HKCU\Software\Classes\Directory\shell\CopyTej";

        let _ = std::process::Command::new("reg")
            .args(&["delete", reg_shell_file, "/f"])
            .output();

        let _ = std::process::Command::new("reg")
            .args(&["delete", reg_shell_dir, "/f"])
            .output();

        Ok(())
    }
    #[cfg(not(windows))]
    {
        Err("Registry operations are only supported on Windows".to_string())
    }
}
