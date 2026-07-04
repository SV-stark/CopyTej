use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;
use tokio::sync::oneshot;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ConflictResolution {
    Overwrite,
    Skip,
    Rename,
    OverwriteAll,
    SkipAll,
    RenameAll,
    OverwriteOlder,
    OverwriteOlderAll,
    SkipSameSizeDate,
    SkipSameSizeDateAll,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConflictInfo {
    pub conflict_id: String,
    pub job_id: String,
    pub file_path: String,
    pub src_size: u64,
    pub src_modified: u64,
    pub dest_size: u64,
    pub dest_modified: u64,
}

pub struct ConflictManager {
    channels: Mutex<HashMap<String, oneshot::Sender<ConflictResolution>>>,
    job_resolutions: Mutex<HashMap<String, ConflictResolution>>,
}

impl ConflictManager {
    pub fn new() -> Self {
        Self {
            channels: Mutex::new(HashMap::new()),
            job_resolutions: Mutex::new(HashMap::new()),
        }
    }

    pub fn get_job_resolution(&self, job_id: &str) -> Option<ConflictResolution> {
        let resolutions = self.job_resolutions.lock().unwrap();
        resolutions.get(job_id).copied()
    }

    pub fn set_job_resolution(&self, job_id: String, resolution: ConflictResolution) {
        let mut resolutions = self.job_resolutions.lock().unwrap();
        resolutions.insert(job_id, resolution);
    }

    pub fn clear_job(&self, job_id: &str) {
        let mut resolutions = self.job_resolutions.lock().unwrap();
        resolutions.remove(job_id);
    }

    pub async fn ask_user<F>(
        &self,
        job_id: String,
        file_path: String,
        src_size: u64,
        src_modified: u64,
        dest_size: u64,
        dest_modified: u64,
        emit_event: F,
    ) -> Result<ConflictResolution, String>
    where
        F: Fn(ConflictInfo),
    {
        // First check if there is an active job-wide resolution
        if let Some(res) = self.get_job_resolution(&job_id) {
            return Ok(res);
        }

        let conflict_id = Uuid::new_v4().to_string();
        let (tx, rx) = oneshot::channel();

        {
            let mut chans = self.channels.lock().unwrap();
            chans.insert(conflict_id.clone(), tx);
        }

        // Emit conflict event to frontend
        emit_event(ConflictInfo {
            conflict_id: conflict_id.clone(),
            job_id: job_id.clone(),
            file_path,
            src_size,
            src_modified,
            dest_size,
            dest_modified,
        });

        // Await frontend response
        let resolution = rx
            .await
            .map_err(|e| format!("Failed to receive conflict resolution: {}", e))?;

        // Cleanup
        {
            let mut chans = self.channels.lock().unwrap();
            chans.remove(&conflict_id);
        }

        // If it's an "All" resolution, save it for future files in this job
        if matches!(
            resolution,
            ConflictResolution::OverwriteAll
                | ConflictResolution::SkipAll
                | ConflictResolution::RenameAll
                | ConflictResolution::OverwriteOlderAll
                | ConflictResolution::SkipSameSizeDateAll
        ) {
            self.set_job_resolution(job_id, resolution);
        }

        Ok(resolution)
    }

    pub fn resolve(&self, conflict_id: &str, resolution: ConflictResolution) -> bool {
        let mut chans = self.channels.lock().unwrap();
        if let Some(tx) = chans.remove(conflict_id) {
            let _ = tx.send(resolution);
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[tokio::test]
    async fn test_conflict_job_resolutions() {
        let cm = ConflictManager::new();
        let job_id = "job-123".to_string();

        assert_eq!(cm.get_job_resolution(&job_id), None);

        cm.set_job_resolution(job_id.clone(), ConflictResolution::OverwriteAll);
        assert_eq!(
            cm.get_job_resolution(&job_id),
            Some(ConflictResolution::OverwriteAll)
        );

        cm.clear_job(&job_id);
        assert_eq!(cm.get_job_resolution(&job_id), None);
    }

    #[tokio::test]
    async fn test_conflict_interactive_resolution() {
        let cm = Arc::new(ConflictManager::new());
        let job_id = "job-456".to_string();
        let file_path = "collision.txt".to_string();

        let cm_clone = Arc::clone(&cm);
        let job_id_clone = job_id.clone();

        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;

            let conflict_id = {
                let chans = cm_clone.channels.lock().unwrap();
                chans.keys().next().cloned().unwrap()
            };

            let resolved = cm_clone.resolve(&conflict_id, ConflictResolution::Rename);
            assert!(resolved);
        });

        let res = cm
            .ask_user(job_id_clone, file_path, 100, 0, 200, 0, |_| {})
            .await;

        assert_eq!(res.unwrap(), ConflictResolution::Rename);
    }
}
