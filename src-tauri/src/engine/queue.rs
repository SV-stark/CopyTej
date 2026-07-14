use crate::engine::conflict::{ConflictManager, ConflictResolution};
use crate::engine::hasher::{HashAlgorithm, hash_file_async};
use crate::engine::{
    FileStatus, JobStatus, TransferEngine, TransferFile, TransferJob, copy_file_chunked,
};
use crate::store::db::DbManager;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, atomic::Ordering};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};
use tokio::sync::Notify;
use uuid::Uuid;

pub struct QueueManager {
    db: Arc<DbManager>,
    engine: Arc<TransferEngine>,
    conflict_manager: Arc<ConflictManager>,
    pub app_handle: AppHandle,
    pub running_jobs: Mutex<HashSet<Uuid>>,
    notify_run: Arc<Notify>,
}

impl QueueManager {
    pub fn new(
        db: Arc<DbManager>,
        engine: Arc<TransferEngine>,
        conflict_manager: Arc<ConflictManager>,
        app_handle: AppHandle,
    ) -> Self {
        Self {
            db,
            engine,
            conflict_manager,
            app_handle,
            running_jobs: Mutex::new(HashSet::new()),
            notify_run: Arc::new(Notify::new()),
        }
    }

    pub fn start_worker(self: Arc<Self>) {
        let manager = Arc::clone(&self);
        tauri::async_runtime::spawn(async move {
            loop {
                // Wait until notified or search for a queued job
                let queued_jobs = match manager.db.get_active_jobs() {
                    Ok(jobs) => jobs,
                    Err(_) => Vec::new(),
                };

                let running = manager.running_jobs.lock().unwrap().clone();
                let next_job = queued_jobs
                    .into_iter()
                    .find(|j| j.status == JobStatus::Queued && !running.contains(&j.id));

                if let Some(mut job) = next_job {
                    manager.running_jobs.lock().unwrap().insert(job.id);

                    // Clean up/init flags for this job
                    manager
                        .engine
                        .get_or_create_pause_flag(job.id)
                        .store(false, std::sync::atomic::Ordering::SeqCst);
                    manager
                        .engine
                        .get_or_create_cancel_flag(job.id)
                        .store(false, std::sync::atomic::Ordering::SeqCst);

                    let manager_clone = Arc::clone(&manager);
                    tauri::async_runtime::spawn(async move {
                        let _ =
                            manager_clone
                                .db
                                .update_job_status(job.id, JobStatus::Running, None);
                        let _ = manager_clone
                            .app_handle
                            .emit("transfer://status-changed", (job.id.to_string(), "Running"));

                        let job_res = manager_clone.run_job(&mut job).await;

                        let final_status = match job_res {
                            Ok(_) => JobStatus::Done,
                            Err(e) if e == "Operation cancelled" => {
                                if manager_clone
                                    .engine
                                    .get_or_create_pause_flag(job.id)
                                    .load(std::sync::atomic::Ordering::SeqCst)
                                {
                                    JobStatus::Paused
                                } else {
                                    JobStatus::Canceled
                                }
                            }
                            Err(e) => JobStatus::Error(e),
                        };

                        let now = chrono::Utc::now().timestamp();
                        let _ = manager_clone.db.update_job_status(
                            job.id,
                            final_status.clone(),
                            Some(now),
                        );
                        let _ = manager_clone.app_handle.emit(
                            "transfer://status-changed",
                            (job.id.to_string(), format!("{:?}", final_status)),
                        );

                        // Trigger completion notification if the job succeeded
                        if let JobStatus::Done = final_status {
                            if let Ok(Some(loaded_job)) = manager_clone.db.get_job(job.id) {
                                use tauri_plugin_notification::NotificationExt;
                                let title = "Transfer Completed".to_string();
                                let body = format!(
                                    "Successfully copied {} items to {}",
                                    loaded_job.files.len(),
                                    loaded_job.dest_dir
                                );
                                let _ = manager_clone
                                    .app_handle
                                    .notification()
                                    .builder()
                                    .title(title)
                                    .body(body)
                                    .show();
                            }
                        }

                        manager_clone.running_jobs.lock().unwrap().remove(&job.id);
                        manager_clone.engine.remove_job_flags(job.id);
                        manager_clone
                            .conflict_manager
                            .clear_job(&job.id.to_string());

                        // Wake up worker to check for more jobs
                        manager_clone.trigger_worker();
                    });
                } else {
                    manager.notify_run.notified().await;
                }
            }
        });
    }

    pub fn trigger_worker(&self) {
        self.notify_run.notify_one();
    }

    pub async fn add_job(
        &self,
        src_paths: Vec<String>,
        dest_dir: String,
        is_move: bool,
    ) -> Result<Uuid, String> {
        let job_id = Uuid::new_v4();

        // Check if sources exist
        let mut files = Vec::new();
        let mut bytes_total = 0u64;

        for src_str in &src_paths {
            let src_path = Path::new(src_str);
            if !src_path.exists() {
                return Err(format!("Source path does not exist: {}", src_str));
            }

            let filename = src_path.file_name().unwrap().to_string_lossy().to_string();
            let dest_path = Path::new(&dest_dir).join(&filename);

            let mut renamed = false;
            if is_move {
                let p1_abs =
                    std::fs::canonicalize(src_path).unwrap_or_else(|_| src_path.to_path_buf());
                let dest_path_parent = Path::new(&dest_dir);
                let p2_abs = std::fs::canonicalize(dest_path_parent)
                    .unwrap_or_else(|_| dest_path_parent.to_path_buf());
                let p1_root = p1_abs.components().next();
                let p2_root = p2_abs.components().next();
                let same_volume = p1_root.is_some() && p1_root == p2_root;

                if same_volume {
                    if let Some(parent) = dest_path.parent() {
                        let _ = std::fs::create_dir_all(parent);
                    }
                    if std::fs::rename(src_path, &dest_path).is_ok() {
                        renamed = true;
                        files.push(TransferFile {
                            src: src_str.clone(),
                            dest: dest_path.to_string_lossy().to_string(),
                            bytes_total: 0,
                            bytes_done: 0,
                            status: FileStatus::Done,
                            hash_src: None,
                            hash_dest: None,
                            error: None,
                        });
                    }
                }
            }

            if renamed {
                continue;
            }

            if src_path.is_file() {
                let metadata = std::fs::metadata(src_path)
                    .map_err(|e| format!("Failed to read metadata for {}: {}", src_str, e))?;
                let file_size = metadata.len();
                let filename = src_path.file_name().unwrap().to_string_lossy().to_string();
                let dest_path = Path::new(&dest_dir).join(filename);

                files.push(TransferFile {
                    src: src_str.clone(),
                    dest: dest_path.to_string_lossy().to_string(),
                    bytes_total: file_size,
                    bytes_done: 0,
                    status: FileStatus::Queued,
                    hash_src: None,
                    hash_dest: None,
                    error: None,
                });
                bytes_total += file_size;
            } else if src_path.is_dir() {
                let dirname = src_path.file_name().unwrap().to_string_lossy().to_string();
                let root_dest = Path::new(&dest_dir).join(dirname);

                for entry in walkdir::WalkDir::new(src_path) {
                    let entry = entry.map_err(|e| format!("Walkdir error: {}", e))?;
                    let path = entry.path();
                    let relative_path = path.strip_prefix(src_path).unwrap();
                    let dest_path = root_dest.join(relative_path);

                    if path.is_file() {
                        let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
                        files.push(TransferFile {
                            src: path.to_string_lossy().to_string(),
                            dest: dest_path.to_string_lossy().to_string(),
                            bytes_total: size,
                            bytes_done: 0,
                            status: FileStatus::Queued,
                            hash_src: None,
                            hash_dest: None,
                            error: None,
                        });
                        bytes_total += size;
                    } else if path.is_dir() {
                        // Recreate directory as 0-byte transfer task
                        files.push(TransferFile {
                            src: path.to_string_lossy().to_string(),
                            dest: dest_path.to_string_lossy().to_string(),
                            bytes_total: 0,
                            bytes_done: 0,
                            status: FileStatus::Queued,
                            hash_src: None,
                            hash_dest: None,
                            error: None,
                        });
                    }
                }
            }
        }

        let now = chrono::Utc::now().timestamp();
        let job = TransferJob {
            id: job_id,
            src_paths,
            dest_dir,
            is_move,
            status: JobStatus::Queued,
            files,
            bytes_total,
            bytes_done: 0,
            speed_bps: 0,
            started_at: Some(now),
            finished_at: None,
        };

        self.db
            .insert_job(&job)
            .map_err(|e| format!("Database error: {}", e))?;

        let _ = self
            .app_handle
            .emit("transfer://new-job", job_id.to_string());

        self.trigger_worker();
        Ok(job_id)
    }

    async fn run_job(&self, job: &mut TransferJob) -> Result<(), String> {
        let app_handle = self.app_handle.clone();

        // Reload settings
        let auto_verify = self
            .db
            .get_setting("auto_verify")
            .unwrap_or(Some("true".to_string()))
            .unwrap_or("true".to_string())
            == "true";

        let hash_algo_str = self
            .db
            .get_setting("hash_algorithm")
            .unwrap_or(Some("Blake3".to_string()))
            .unwrap_or("Blake3".to_string());
        let hash_algo = match hash_algo_str.as_str() {
            "XxHash3" => HashAlgorithm::XxHash3,
            "Md5" => HashAlgorithm::Md5,
            "Sha256" => HashAlgorithm::Sha256,
            _ => HashAlgorithm::Blake3,
        };

        let enable_block_cloning = self
            .db
            .get_setting("enable_block_cloning")
            .unwrap_or(Some("true".to_string()))
            .unwrap_or("true".to_string())
            == "true";

        let speed_limit_kbps = self
            .db
            .get_setting("speed_limit_kbps")
            .unwrap_or(Some("0".to_string()))
            .unwrap_or("0".to_string())
            .parse::<u64>()
            .unwrap_or(0);
        self.engine
            .global_speed_limit_kbps
            .store(speed_limit_kbps, std::sync::atomic::Ordering::SeqCst);

        let mut overall_bytes_done = job.bytes_done;

        // Get fresh files list
        let db_job = self
            .db
            .get_job(job.id)
            .map_err(|e| format!("DB Error: {}", e))?
            .ok_or_else(|| "Job not found in database".to_string())?;

        for file_idx in 0..db_job.files.len() {
            let mut file = db_job.files[file_idx].clone();

            if file.status == FileStatus::Done || file.status == FileStatus::Skipped {
                continue;
            }

            // Check cancellation or pause
            if self
                .engine
                .get_or_create_cancel_flag(job.id)
                .load(std::sync::atomic::Ordering::SeqCst)
            {
                return Err("Operation cancelled".to_string());
            }

            let src_path = Path::new(&file.src);
            let mut dest_path = PathBuf::from(&file.dest);

            // Handle directory creation separately
            if src_path.is_dir() {
                match tokio::fs::create_dir_all(&dest_path).await {
                    Ok(_) => {
                        file.status = FileStatus::Done;
                        let _ = self.db.update_file_result(
                            job.id,
                            &file.src,
                            FileStatus::Done,
                            None,
                            None,
                            None,
                        );
                        let _ = app_handle.emit(
                            "transfer://file-status",
                            (job.id.to_string(), file.src.clone(), "Done"),
                        );
                        continue;
                    }
                    Err(e) => {
                        let err_msg = format!("Failed to create directory: {}", e);
                        file.status = FileStatus::Error(err_msg.clone());
                        let _ = self.db.update_file_result(
                            job.id,
                            &file.src,
                            FileStatus::Error(err_msg.clone()),
                            None,
                            None,
                            Some(err_msg),
                        );
                        continue;
                    }
                }
            }

            // Optimize same-drive moves (rename)
            let is_rename = job.is_move && {
                let p1_abs =
                    std::fs::canonicalize(src_path).unwrap_or_else(|_| src_path.to_path_buf());
                let p2_parent = dest_path.parent().unwrap_or(&dest_path);
                let p2_abs =
                    std::fs::canonicalize(p2_parent).unwrap_or_else(|_| p2_parent.to_path_buf());
                let p1_root = p1_abs.components().next();
                let p2_root = p2_abs.components().next();
                p1_root.is_some() && p1_root == p2_root
            };

            if is_rename {
                if let Some(parent) = dest_path.parent() {
                    let _ = tokio::fs::create_dir_all(parent).await;
                }

                match std::fs::rename(src_path, &dest_path) {
                    Ok(_) => {
                        file.bytes_done = file.bytes_total;
                        file.status = FileStatus::Done;
                        let _ = self.db.update_file_progress(
                            job.id,
                            &file.src,
                            file.bytes_total,
                            FileStatus::Done,
                        );
                        let _ = self.db.update_file_result(
                            job.id,
                            &file.src,
                            FileStatus::Done,
                            None,
                            None,
                            None,
                        );
                        let _ = app_handle.emit(
                            "transfer://file-progress",
                            (job.id.to_string(), file.src.clone(), file.bytes_total),
                        );
                        let _ = app_handle.emit(
                            "transfer://file-status",
                            (job.id.to_string(), file.src.clone(), "Done"),
                        );
                        overall_bytes_done += file.bytes_total - file.bytes_done;
                        continue;
                    }
                    Err(_) => {
                        // Fall back to copy-then-delete if rename fails
                    }
                }
            }

            // Resolve name conflict
            if dest_path.exists() {
                let src_meta = std::fs::metadata(src_path).map_err(|e| e.to_string())?;
                let dest_meta = std::fs::metadata(&dest_path).map_err(|e| e.to_string())?;

                let get_modified_time = |meta: &std::fs::Metadata| -> u64 {
                    meta.modified()
                        .map(|t| {
                            t.duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_secs()
                        })
                        .unwrap_or(0)
                };

                let mut resolution = None;

                // Check active job resolutions first
                if let Some(job_res) = self
                    .conflict_manager
                    .get_job_resolution(&job.id.to_string())
                {
                    resolution = match job_res {
                        ConflictResolution::SkipAll => Some(ConflictResolution::Skip),
                        ConflictResolution::OverwriteAll => Some(ConflictResolution::Overwrite),
                        ConflictResolution::RenameAll => Some(ConflictResolution::Rename),
                        ConflictResolution::OverwriteOlderAll => {
                            let src_mod = get_modified_time(&src_meta);
                            let dest_mod = get_modified_time(&dest_meta);
                            if src_mod > dest_mod {
                                Some(ConflictResolution::Overwrite)
                            } else {
                                Some(ConflictResolution::Skip)
                            }
                        }
                        ConflictResolution::SkipSameSizeDateAll => {
                            let src_mod = get_modified_time(&src_meta);
                            let dest_mod = get_modified_time(&dest_meta);
                            if src_meta.len() == dest_meta.len() && src_mod == dest_mod {
                                Some(ConflictResolution::Skip)
                            } else {
                                Some(ConflictResolution::Overwrite)
                            }
                        }
                        _ => None,
                    };
                }

                let resolution = match resolution {
                    Some(res) => res,
                    None => {
                        let res = self
                            .conflict_manager
                            .ask_user(
                                job.id.to_string(),
                                file.src.clone(),
                                src_meta.len(),
                                get_modified_time(&src_meta),
                                dest_meta.len(),
                                get_modified_time(&dest_meta),
                                {
                                    let app = app_handle.clone();
                                    move |info| {
                                        let _ = app.emit("transfer://conflict", info);
                                    }
                                },
                            )
                            .await?;

                        match res {
                            ConflictResolution::OverwriteOlder => {
                                let src_mod = get_modified_time(&src_meta);
                                let dest_mod = get_modified_time(&dest_meta);
                                if src_mod > dest_mod {
                                    ConflictResolution::Overwrite
                                } else {
                                    ConflictResolution::Skip
                                }
                            }
                            ConflictResolution::SkipSameSizeDate => {
                                let src_mod = get_modified_time(&src_meta);
                                let dest_mod = get_modified_time(&dest_meta);
                                if src_meta.len() == dest_meta.len() && src_mod == dest_mod {
                                    ConflictResolution::Skip
                                } else {
                                    ConflictResolution::Overwrite
                                }
                            }
                            ConflictResolution::OverwriteOlderAll => {
                                let src_mod = get_modified_time(&src_meta);
                                let dest_mod = get_modified_time(&dest_meta);
                                if src_mod > dest_mod {
                                    ConflictResolution::Overwrite
                                } else {
                                    ConflictResolution::Skip
                                }
                            }
                            ConflictResolution::SkipSameSizeDateAll => {
                                let src_mod = get_modified_time(&src_meta);
                                let dest_mod = get_modified_time(&dest_meta);
                                if src_meta.len() == dest_meta.len() && src_mod == dest_mod {
                                    ConflictResolution::Skip
                                } else {
                                    ConflictResolution::Overwrite
                                }
                            }
                            other => other,
                        }
                    }
                };

                match resolution {
                    ConflictResolution::Skip | ConflictResolution::SkipAll => {
                        file.status = FileStatus::Skipped;
                        let _ = self.db.update_file_result(
                            job.id,
                            &file.src,
                            FileStatus::Skipped,
                            None,
                            None,
                            None,
                        );
                        overall_bytes_done += file.bytes_total;
                        let _ = self.db.update_job_progress(job.id, overall_bytes_done, 0);
                        continue;
                    }
                    ConflictResolution::Rename | ConflictResolution::RenameAll => {
                        // Find unique name
                        let mut counter = 1;
                        let stem = dest_path.file_stem().unwrap().to_string_lossy().to_string();
                        let ext = dest_path
                            .extension()
                            .map(|e| e.to_string_lossy().to_string())
                            .unwrap_or_default();
                        let parent = dest_path.parent().unwrap();

                        loop {
                            let new_name = if ext.is_empty() {
                                format!("{} ({})", stem, counter)
                            } else {
                                format!("{} ({}).{}", stem, counter, ext)
                            };
                            let check_path = parent.join(new_name);
                            if !check_path.exists() {
                                dest_path = check_path;
                                file.dest = dest_path.to_string_lossy().to_string();
                                break;
                            }
                            counter += 1;
                        }
                    }
                    ConflictResolution::Overwrite | ConflictResolution::OverwriteAll => {
                        // Do nothing, standard write will truncate/overwrite
                    }
                    _ => {}
                }
            }

            // Perform Copy
            file.status = FileStatus::Copying;
            let _ = self.db.update_file_progress(
                job.id,
                &file.src,
                file.bytes_done,
                FileStatus::Copying,
            );

            let job_id = job.id;

            struct ProgressState {
                overall_bytes_done: u64,
                last_bytes_done: u64,
                last_speed_calc: Instant,
                last_emit: Instant,
                speed_bps: u64,
            }

            let progress_state = Arc::new(Mutex::new(ProgressState {
                overall_bytes_done,
                last_bytes_done: overall_bytes_done,
                last_speed_calc: Instant::now(),
                last_emit: Instant::now(),
                speed_bps: job.speed_bps,
            }));

            let last_file_bytes_done = Arc::new(Mutex::new(file.bytes_done));

            let mut copy_res = Err("Initial".to_string());
            let mut retries = 0;
            const MAX_RETRIES: usize = 3;

            while retries <= MAX_RETRIES {
                if self
                    .engine
                    .get_or_create_cancel_flag(job.id)
                    .load(Ordering::SeqCst)
                {
                    copy_res = Err("Operation cancelled".to_string());
                    break;
                }

                while self
                    .engine
                    .get_or_create_pause_flag(job.id)
                    .load(Ordering::SeqCst)
                {
                    if self
                        .engine
                        .get_or_create_cancel_flag(job.id)
                        .load(Ordering::SeqCst)
                    {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }

                let last_done_val = *last_file_bytes_done.lock().unwrap();

                copy_res = copy_file_chunked(
                    src_path,
                    &dest_path,
                    last_done_val,
                    Some(file.bytes_total),
                    self.engine.get_or_create_pause_flag(job.id),
                    self.engine.get_or_create_cancel_flag(job.id),
                    Arc::clone(&self.engine.rate_limiter),
                    if auto_verify { Some(hash_algo) } else { None },
                    enable_block_cloning,
                    {
                        let last_file_bytes_done_cb = Arc::clone(&last_file_bytes_done);
                        let progress_state_cb = Arc::clone(&progress_state);
                        let db_ref = Arc::clone(&self.db);
                        let app_emitter = app_handle.clone();
                        let file_src = file.src.clone();
                        move |chunk_size| {
                            let mut file_bytes = last_file_bytes_done_cb.lock().unwrap();
                            *file_bytes += chunk_size;
                            let _ = db_ref.update_file_progress(
                                job_id,
                                &file_src,
                                *file_bytes,
                                FileStatus::Copying,
                            );

                            let _ = app_emitter.emit(
                                "transfer://file-progress",
                                (job_id.to_string(), file_src.clone(), *file_bytes),
                            );

                            let mut state = progress_state_cb.lock().unwrap();
                            state.overall_bytes_done += chunk_size;

                            let now = Instant::now();
                            let elapsed_speed = state.last_speed_calc.elapsed().as_secs_f64();
                            if elapsed_speed > 0.5 {
                                let copied_diff = state
                                    .overall_bytes_done
                                    .saturating_sub(state.last_bytes_done);
                                let inst_speed = (copied_diff as f64 / elapsed_speed) as u64;
                                if state.speed_bps == 0 {
                                    state.speed_bps = inst_speed;
                                } else {
                                    let alpha = 0.3;
                                    state.speed_bps = ((alpha * inst_speed as f64)
                                        + ((1.0 - alpha) * state.speed_bps as f64))
                                        as u64;
                                }
                                state.last_bytes_done = state.overall_bytes_done;
                                state.last_speed_calc = now;

                                let _ = db_ref.update_job_progress(
                                    job_id,
                                    state.overall_bytes_done,
                                    state.speed_bps,
                                );
                            }

                            if now.duration_since(state.last_emit) > Duration::from_millis(200) {
                                let _ = app_emitter.emit(
                                    "transfer://job-progress",
                                    (
                                        job_id.to_string(),
                                        state.overall_bytes_done,
                                        state.speed_bps,
                                    ),
                                );
                                state.last_emit = now;
                            }
                        }
                    },
                )
                .await;

                if copy_res.is_ok() {
                    break;
                }

                if let Err(ref err_msg) = copy_res {
                    if err_msg.contains("cancelled") || err_msg.contains("size changed") {
                        break;
                    }
                }

                retries += 1;
                if retries <= MAX_RETRIES {
                    tokio::time::sleep(Duration::from_secs(5)).await;
                }
            }

            match copy_res {
                Ok((bytes_written, src_hash_opt)) => {
                    let old_done = file.bytes_done;
                    file.bytes_done = bytes_written;

                    {
                        let mut state = progress_state.lock().unwrap();
                        let actual_written_diff = bytes_written.saturating_sub(old_done);
                        let expected_overall = overall_bytes_done + actual_written_diff;
                        if state.overall_bytes_done < expected_overall {
                            state.overall_bytes_done = expected_overall;
                        }
                    }

                    // Verify if requested
                    if auto_verify {
                        file.status = FileStatus::Verifying;
                        let _ = self.db.update_file_result(
                            job.id,
                            &file.src,
                            FileStatus::Verifying,
                            None,
                            None,
                            None,
                        );
                        let _ = app_handle.emit(
                            "transfer://file-status",
                            (job.id.to_string(), file.src.clone(), "Verifying"),
                        );

                        let src_hash = match src_hash_opt {
                            Some(h) => Ok(h),
                            None => hash_file_async(file.src.clone(), hash_algo).await,
                        };
                        let dest_hash = hash_file_async(file.dest.clone(), hash_algo).await;

                        match (src_hash, dest_hash) {
                            (Ok(sh), Ok(dh)) => {
                                file.hash_src = Some(sh.clone());
                                file.hash_dest = Some(dh.clone());

                                if sh == dh {
                                    file.status = FileStatus::Done;
                                    let _ = self.db.update_file_result(
                                        job.id,
                                        &file.src,
                                        FileStatus::Done,
                                        Some(sh),
                                        Some(dh),
                                        None,
                                    );
                                } else {
                                    file.status =
                                        FileStatus::Error("Checksum mismatch".to_string());
                                    let _ = self.db.update_file_result(
                                        job.id,
                                        &file.src,
                                        FileStatus::Error("Checksum mismatch".to_string()),
                                        Some(sh),
                                        Some(dh),
                                        Some("Checksum mismatch".to_string()),
                                    );
                                }
                            }
                            (e1, e2) => {
                                let err_msg = format!(
                                    "Verification failed: Src: {:?}, Dest: {:?}",
                                    e1.err(),
                                    e2.err()
                                );
                                file.status = FileStatus::Error(err_msg.clone());
                                let _ = self.db.update_file_result(
                                    job.id,
                                    &file.src,
                                    FileStatus::Error(err_msg.clone()),
                                    None,
                                    None,
                                    Some(err_msg),
                                );
                            }
                        }
                    } else {
                        file.status = FileStatus::Done;
                        let _ = self.db.update_file_result(
                            job.id,
                            &file.src,
                            FileStatus::Done,
                            None,
                            None,
                            None,
                        );
                    }

                    // Delete source if it is a move operation
                    if job.is_move && matches!(file.status, FileStatus::Done) {
                        let _ = std::fs::remove_file(src_path);
                    }
                }
                Err(e) => {
                    file.status = FileStatus::Error(e.clone());
                    let _ = self.db.update_file_result(
                        job.id,
                        &file.src,
                        FileStatus::Error(e.clone()),
                        None,
                        None,
                        Some(e),
                    );
                }
            }

            let (final_overall, speed_bps) = {
                let state = progress_state.lock().unwrap();
                (state.overall_bytes_done, state.speed_bps)
            };
            overall_bytes_done = final_overall;

            job.bytes_done = overall_bytes_done;
            job.speed_bps = speed_bps;
            let _ = self
                .db
                .update_job_progress(job.id, overall_bytes_done, speed_bps);

            let _ = app_handle.emit(
                "transfer://job-progress",
                (job.id.to_string(), overall_bytes_done, speed_bps),
            );
        }

        // Clean up empty directories for move jobs
        if job.is_move {
            for file in db_job.files.iter().rev() {
                let src_path = Path::new(&file.src);
                if src_path.is_dir() && src_path.exists() {
                    let _ = std::fs::remove_dir(src_path); // only removes if empty
                }
            }
            for src_str in &job.src_paths {
                let src_path = Path::new(src_str);
                if src_path.is_dir() && src_path.exists() {
                    let _ = std::fs::remove_dir(src_path); // only removes if empty
                }
            }
        }

        // Final UI progress update
        let _ = app_handle.emit(
            "transfer://job-progress",
            (job.id.to_string(), overall_bytes_done, 0u64),
        );

        Ok(())
    }
}
