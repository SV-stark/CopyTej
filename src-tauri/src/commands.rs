use crate::engine::conflict::{ConflictManager, ConflictResolution};
use crate::engine::queue::QueueManager;
use crate::engine::TransferEngine;
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
    engine.pause();
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
    engine.resume();
    db.update_job_status(uuid, JobStatus::Running, None)
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
    engine.cancel();
    db.update_job_status(uuid, JobStatus::Done, Some(chrono::Utc::now().timestamp()))
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
    db: State<'_, Arc<DbManager>>,
    key: String,
    value: String,
) -> Result<(), String> {
    db.set_setting(&key, &value).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn select_directory() -> Result<String, String> {
    let output = tokio::process::Command::new("powershell")
        .args(&[
            "-NoProfile",
            "-Command",
            "[System.Reflection.Assembly]::LoadWithPartialName('System.Windows.Forms') | Out-Null; $dialog = New-Object System.Windows.Forms.FolderBrowserDialog; $dialog.Description = 'Select Destination Folder'; if ($dialog.ShowDialog() -eq 'OK') { $dialog.SelectedPath }",
        ])
        .output()
        .await
        .map_err(|e| e.to_string())?;

    if output.status.success() {
        let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if path.is_empty() {
            Err("Cancelled".to_string())
        } else {
            Ok(path)
        }
    } else {
        Err("Failed to open folder picker".to_string())
    }
}

#[tauri::command]
pub async fn select_files() -> Result<Vec<String>, String> {
    let output = tokio::process::Command::new("powershell")
        .args(&[
            "-NoProfile",
            "-Command",
            "[System.Reflection.Assembly]::LoadWithPartialName('System.Windows.Forms') | Out-Null; $dialog = New-Object System.Windows.Forms.OpenFileDialog; $dialog.Multiselect = $true; $dialog.Title = 'Select Files/Folders'; if ($dialog.ShowDialog() -eq 'OK') { $dialog.FileNames -join '|' }",
        ])
        .output()
        .await
        .map_err(|e| e.to_string())?;

    if output.status.success() {
        let path_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if path_str.is_empty() {
            Err("Cancelled".to_string())
        } else {
            Ok(path_str
                .split('|')
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
                .collect())
        }
    } else {
        Err("Failed to open file picker".to_string())
    }
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
