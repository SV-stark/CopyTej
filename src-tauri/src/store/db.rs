use crate::engine::{FileStatus, JobStatus, TransferFile, TransferJob};
use rusqlite::{Connection, Result, params};
use std::path::PathBuf;
use std::sync::Mutex;
use uuid::Uuid;

pub struct DbManager {
    conn: Mutex<Connection>,
}

impl DbManager {
    pub fn new(db_path: PathBuf) -> Result<Self> {
        // Ensure parent directory exists
        if let Some(parent) = db_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        let conn = Connection::open(db_path)?;
        let db = Self {
            conn: Mutex::new(conn),
        };
        db.init_tables()?;
        Ok(db)
    }

    fn init_tables(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();

        conn.execute(
            "CREATE TABLE IF NOT EXISTS transfers (
                id TEXT PRIMARY KEY,
                src_paths TEXT NOT NULL,
                dest_dir TEXT NOT NULL,
                is_move INTEGER NOT NULL,
                status TEXT NOT NULL,
                bytes_total INTEGER NOT NULL,
                bytes_done INTEGER NOT NULL,
                speed_bps INTEGER NOT NULL,
                started_at INTEGER,
                finished_at INTEGER
            )",
            [],
        )?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS transfer_files (
                job_id TEXT NOT NULL,
                src TEXT NOT NULL,
                dest TEXT NOT NULL,
                bytes_total INTEGER NOT NULL,
                bytes_done INTEGER NOT NULL,
                status TEXT NOT NULL,
                hash_src TEXT,
                hash_dest TEXT,
                error TEXT,
                PRIMARY KEY (job_id, src)
            )",
            [],
        )?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            )",
            [],
        )?;

        Ok(())
    }

    pub fn insert_job(&self, job: &TransferJob) -> Result<()> {
        let conn = self.conn.lock().unwrap();

        let src_paths_json = serde_json::to_string(&job.src_paths).unwrap_or_default();

        conn.execute(
            "INSERT OR REPLACE INTO transfers (
                id, src_paths, dest_dir, is_move, status, bytes_total, bytes_done, speed_bps, started_at, finished_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                job.id.to_string(),
                src_paths_json,
                job.dest_dir,
                if job.is_move { 1 } else { 0 },
                serde_json::to_string(&job.status).unwrap(),
                job.bytes_total as i64,
                job.bytes_done as i64,
                job.speed_bps as i64,
                job.started_at,
                job.finished_at,
            ],
        )?;

        for file in &job.files {
            conn.execute(
                "INSERT OR REPLACE INTO transfer_files (
                    job_id, src, dest, bytes_total, bytes_done, status, hash_src, hash_dest, error
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    job.id.to_string(),
                    file.src,
                    file.dest,
                    file.bytes_total as i64,
                    file.bytes_done as i64,
                    serde_json::to_string(&file.status).unwrap(),
                    file.hash_src,
                    file.hash_dest,
                    file.error,
                ],
            )?;
        }

        Ok(())
    }

    pub fn update_job_progress(&self, id: Uuid, bytes_done: u64, speed_bps: u64) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE transfers SET bytes_done = ?1, speed_bps = ?2 WHERE id = ?3",
            params![bytes_done as i64, speed_bps as i64, id.to_string()],
        )?;
        Ok(())
    }

    pub fn update_job_status(
        &self,
        id: Uuid,
        status: JobStatus,
        finished_at: Option<i64>,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let status_str = serde_json::to_string(&status).unwrap();
        conn.execute(
            "UPDATE transfers SET status = ?1, finished_at = ?2 WHERE id = ?3",
            params![status_str, finished_at, id.to_string()],
        )?;
        Ok(())
    }

    pub fn update_file_progress(
        &self,
        job_id: Uuid,
        src: &str,
        bytes_done: u64,
        status: FileStatus,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let status_str = serde_json::to_string(&status).unwrap();
        conn.execute(
            "UPDATE transfer_files SET bytes_done = ?1, status = ?2 WHERE job_id = ?3 AND src = ?4",
            params![bytes_done as i64, status_str, job_id.to_string(), src],
        )?;
        Ok(())
    }

    pub fn update_file_result(
        &self,
        job_id: Uuid,
        src: &str,
        status: FileStatus,
        hash_src: Option<String>,
        hash_dest: Option<String>,
        error: Option<String>,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let status_str = serde_json::to_string(&status).unwrap();
        conn.execute(
            "UPDATE transfer_files SET status = ?1, hash_src = ?2, hash_dest = ?3, error = ?4 WHERE job_id = ?5 AND src = ?6",
            params![status_str, hash_src, hash_dest, error, job_id.to_string(), src],
        )?;
        Ok(())
    }

    pub fn get_job(&self, id: Uuid) -> Result<Option<TransferJob>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT src_paths, dest_dir, is_move, status, bytes_total, bytes_done, speed_bps, started_at, finished_at FROM transfers WHERE id = ?1")?;

        let job_opt = stmt.query_row(params![id.to_string()], |row| {
            let src_paths_json: String = row.get(0)?;
            let src_paths: Vec<String> = serde_json::from_str(&src_paths_json).unwrap_or_default();
            let status_json: String = row.get(3)?;
            let status: JobStatus = serde_json::from_str(&status_json)
                .unwrap_or(JobStatus::Error("Invalid status".into()));

            Ok(TransferJob {
                id,
                src_paths,
                dest_dir: row.get(1)?,
                is_move: row.get::<_, i32>(2)? != 0,
                status,
                files: Vec::new(),
                bytes_total: row.get::<_, i64>(4)? as u64,
                bytes_done: row.get::<_, i64>(5)? as u64,
                speed_bps: row.get::<_, i64>(6)? as u64,
                started_at: row.get(7)?,
                finished_at: row.get(8)?,
            })
        });

        match job_opt {
            Ok(mut job) => {
                let mut stmt_files = conn.prepare("SELECT src, dest, bytes_total, bytes_done, status, hash_src, hash_dest, error FROM transfer_files WHERE job_id = ?1")?;
                let files_iter = stmt_files.query_map(params![id.to_string()], |row| {
                    let status_json: String = row.get(4)?;
                    let status: FileStatus =
                        serde_json::from_str(&status_json).unwrap_or(FileStatus::Queued);
                    Ok(TransferFile {
                        src: row.get(0)?,
                        dest: row.get(1)?,
                        bytes_total: row.get::<_, i64>(2)? as u64,
                        bytes_done: row.get::<_, i64>(3)? as u64,
                        status,
                        hash_src: row.get(5)?,
                        hash_dest: row.get(6)?,
                        error: row.get(7)?,
                    })
                })?;

                for file_res in files_iter {
                    if let Ok(file) = file_res {
                        job.files.push(file);
                    }
                }
                Ok(Some(job))
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }

    pub fn get_active_jobs(&self) -> Result<Vec<TransferJob>> {
        let ids = {
            let conn = self.conn.lock().unwrap();
            let mut stmt = conn.prepare("SELECT id FROM transfers WHERE status IN ('\"Queued\"', '\"Running\"', '\"Paused\"')")?;
            let id_iter = stmt.query_map([], |row| {
                let id_str: String = row.get(0)?;
                Ok(Uuid::parse_str(&id_str).unwrap())
            })?;

            let mut ids = Vec::new();
            for id_res in id_iter {
                if let Ok(id) = id_res {
                    ids.push(id);
                }
            }
            ids
        };

        let mut jobs = Vec::new();
        for id in ids {
            if let Ok(Some(job)) = self.get_job(id) {
                jobs.push(job);
            }
        }
        Ok(jobs)
    }

    pub fn get_history(&self, limit: u32, offset: u32) -> Result<Vec<TransferJob>> {
        let ids = self.get_history_ids(limit, offset)?;
        let mut jobs = Vec::new();
        for id in ids {
            if let Ok(Some(job)) = self.get_job(id) {
                jobs.push(job);
            }
        }
        Ok(jobs)
    }

    pub fn get_history_ids(&self, limit: u32, offset: u32) -> Result<Vec<Uuid>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt =
            conn.prepare("SELECT id FROM transfers ORDER BY started_at DESC LIMIT ?1 OFFSET ?2")?;
        let id_iter = stmt.query_map(params![limit, offset], |row| {
            let id_str: String = row.get(0)?;
            Ok(Uuid::parse_str(&id_str).unwrap())
        })?;

        let mut ids = Vec::new();
        for id_res in id_iter {
            if let Ok(id) = id_res {
                ids.push(id);
            }
        }
        Ok(ids)
    }

    pub fn get_setting(&self, key: &str) -> Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT value FROM settings WHERE key = ?1")?;
        let val_opt = stmt.query_row(params![key], |row| row.get::<_, String>(0));
        match val_opt {
            Ok(val) => Ok(Some(val)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO settings (key, value) VALUES (?1, ?2)",
            params![key, value],
        )?;
        Ok(())
    }

    pub fn delete_job(&self, id: Uuid) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM transfers WHERE id = ?1",
            params![id.to_string()],
        )?;
        conn.execute(
            "DELETE FROM transfer_files WHERE job_id = ?1",
            params![id.to_string()],
        )?;
        Ok(())
    }

    pub fn clear_history(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM transfers WHERE status NOT IN ('\"Queued\"', '\"Running\"', '\"Paused\"')",
            [],
        )?;
        conn.execute(
            "DELETE FROM transfer_files WHERE job_id NOT IN (SELECT id FROM transfers)",
            [],
        )?;
        Ok(())
    }

    pub fn reset_running_jobs(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("UPDATE transfers SET status = '\"Paused\"' WHERE status IN ('\"Running\"', '\"Paused\"')", [])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{FileStatus, JobStatus, TransferFile, TransferJob};

    #[test]
    fn test_db_settings() {
        let temp_dir = std::env::temp_dir();
        let db_path = temp_dir.join(format!("copytej_test_settings_{}.db", Uuid::new_v4()));
        let db = DbManager::new(db_path.clone()).unwrap();

        assert_eq!(db.get_setting("test_key").unwrap(), None);

        db.set_setting("test_key", "test_value").unwrap();
        assert_eq!(
            db.get_setting("test_key").unwrap(),
            Some("test_value".to_string())
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[test]
    fn test_db_job_operations() {
        let temp_dir = std::env::temp_dir();
        let db_path = temp_dir.join(format!("copytej_test_jobs_{}.db", Uuid::new_v4()));
        let db = DbManager::new(db_path.clone()).unwrap();

        let job_id = Uuid::new_v4();
        let file = TransferFile {
            src: "src/path/file.txt".to_string(),
            dest: "dest/path/file.txt".to_string(),
            bytes_total: 1000,
            bytes_done: 0,
            status: FileStatus::Queued,
            hash_src: None,
            hash_dest: None,
            error: None,
        };

        let job = TransferJob {
            id: job_id,
            src_paths: vec!["src/path/file.txt".to_string()],
            dest_dir: "dest/path".to_string(),
            is_move: false,
            status: JobStatus::Queued,
            files: vec![file],
            bytes_total: 1000,
            bytes_done: 0,
            speed_bps: 0,
            started_at: Some(chrono::Utc::now().timestamp()),
            finished_at: None,
        };

        db.insert_job(&job).unwrap();

        let retrieved = db.get_job(job_id).unwrap().unwrap();
        assert_eq!(retrieved.id, job_id);
        assert_eq!(retrieved.dest_dir, "dest/path");
        assert_eq!(retrieved.files.len(), 1);
        assert_eq!(retrieved.files[0].src, "src/path/file.txt");

        let active = db.get_active_jobs().unwrap();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].id, job_id);

        db.update_file_progress(job_id, "src/path/file.txt", 500, FileStatus::Copying)
            .unwrap();
        db.update_job_progress(job_id, 500, 100).unwrap();

        let retrieved2 = db.get_job(job_id).unwrap().unwrap();
        assert_eq!(retrieved2.bytes_done, 500);
        assert_eq!(retrieved2.speed_bps, 100);
        assert_eq!(retrieved2.files[0].bytes_done, 500);
        assert_eq!(
            matches!(retrieved2.files[0].status, FileStatus::Copying),
            true
        );

        db.update_file_result(
            job_id,
            "src/path/file.txt",
            FileStatus::Done,
            Some("hash1".to_string()),
            Some("hash1".to_string()),
            None,
        )
        .unwrap();
        db.update_job_status(
            job_id,
            JobStatus::Done,
            Some(chrono::Utc::now().timestamp()),
        )
        .unwrap();

        let active_after = db.get_active_jobs().unwrap();
        assert_eq!(active_after.len(), 0);

        let history = db.get_history(10, 0).unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].id, job_id);

        let _ = std::fs::remove_file(db_path);
    }
}
