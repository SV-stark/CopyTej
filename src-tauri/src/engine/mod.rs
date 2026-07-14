pub mod conflict;
pub mod hasher;
pub mod queue;

use serde::{Deserialize, Serialize};
use sha2::Digest;
use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::fs::{File, OpenOptions};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt, SeekFrom};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum FileStatus {
    Queued,
    Copying,
    Verifying,
    Done,
    Skipped,
    Error(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum JobStatus {
    Queued,
    Running,
    Paused,
    Canceled,
    Done,
    Error(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferFile {
    pub src: String,
    pub dest: String,
    pub bytes_total: u64,
    pub bytes_done: u64,
    pub status: FileStatus,
    pub hash_src: Option<String>,
    pub hash_dest: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferJob {
    pub id: Uuid,
    pub src_paths: Vec<String>,
    pub dest_dir: String,
    pub is_move: bool,
    pub status: JobStatus,
    pub files: Vec<TransferFile>,
    pub bytes_total: u64,
    pub bytes_done: u64,
    pub speed_bps: u64,
    pub started_at: Option<i64>,
    pub finished_at: Option<i64>,
}

pub struct GlobalRateLimiter {
    pub limit_kbps: Arc<std::sync::atomic::AtomicU64>,
    state: Mutex<LimiterState>,
}

struct LimiterState {
    tokens: f64,
    last_refill: Instant,
}

impl GlobalRateLimiter {
    pub fn new(limit_kbps: Arc<std::sync::atomic::AtomicU64>) -> Self {
        Self {
            limit_kbps,
            state: Mutex::new(LimiterState {
                tokens: 0.0,
                last_refill: Instant::now(),
            }),
        }
    }

    pub async fn consume(&self, amount: usize) {
        let limit = self.limit_kbps.load(Ordering::Relaxed);
        if limit == 0 {
            return;
        }

        let limit_bytes_per_sec = (limit * 1024) as f64;
        let max_tokens = limit_bytes_per_sec * 0.5;

        loop {
            let sleep_dur = {
                let mut state = self.state.lock().unwrap();
                let now = Instant::now();
                let elapsed = now.duration_since(state.last_refill).as_secs_f64();
                state.last_refill = now;

                state.tokens = (state.tokens + elapsed * limit_bytes_per_sec).min(max_tokens);

                if state.tokens >= amount as f64 {
                    state.tokens -= amount as f64;
                    return;
                }

                let needed = amount as f64 - state.tokens;
                Duration::from_secs_f64(needed / limit_bytes_per_sec)
            };

            tokio::time::sleep(sleep_dur).await;
        }
    }
}

pub struct TransferEngine {
    pub pause_flags: Arc<Mutex<HashMap<Uuid, Arc<AtomicBool>>>>,
    pub cancel_flags: Arc<Mutex<HashMap<Uuid, Arc<AtomicBool>>>>,
    pub global_speed_limit_kbps: Arc<std::sync::atomic::AtomicU64>,
    pub rate_limiter: Arc<GlobalRateLimiter>,
}

impl TransferEngine {
    pub fn new() -> Self {
        let limit_kbps = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let rate_limiter = Arc::new(GlobalRateLimiter::new(Arc::clone(&limit_kbps)));
        Self {
            pause_flags: Arc::new(Mutex::new(HashMap::new())),
            cancel_flags: Arc::new(Mutex::new(HashMap::new())),
            global_speed_limit_kbps: limit_kbps,
            rate_limiter,
        }
    }

    pub fn get_or_create_pause_flag(&self, job_id: Uuid) -> Arc<AtomicBool> {
        let mut flags = self.pause_flags.lock().unwrap();
        flags
            .entry(job_id)
            .or_insert_with(|| Arc::new(AtomicBool::new(false)))
            .clone()
    }

    pub fn get_or_create_cancel_flag(&self, job_id: Uuid) -> Arc<AtomicBool> {
        let mut flags = self.cancel_flags.lock().unwrap();
        flags
            .entry(job_id)
            .or_insert_with(|| Arc::new(AtomicBool::new(false)))
            .clone()
    }

    pub fn pause_job(&self, job_id: Uuid) {
        self.get_or_create_pause_flag(job_id)
            .store(true, Ordering::SeqCst);
    }

    pub fn resume_job(&self, job_id: Uuid) {
        self.get_or_create_pause_flag(job_id)
            .store(false, Ordering::SeqCst);
    }

    pub fn cancel_job(&self, job_id: Uuid) {
        self.get_or_create_cancel_flag(job_id)
            .store(true, Ordering::SeqCst);
    }

    pub fn remove_job_flags(&self, job_id: Uuid) {
        self.pause_flags.lock().unwrap().remove(&job_id);
        self.cancel_flags.lock().unwrap().remove(&job_id);
    }
}

pub fn get_adaptive_buffer_size(file_size: u64) -> usize {
    if file_size < 1024 * 1024 {
        64 * 1024 // 64 KB for < 1 MB
    } else if file_size < 100 * 1024 * 1024 {
        256 * 1024 // 256 KB for < 100 MB
    } else if file_size < 1024 * 1024 * 1024 {
        1024 * 1024 // 1 MB for < 1 GB
    } else {
        4 * 1024 * 1024 // 4 MB for >= 1 GB
    }
}

#[cfg(windows)]
unsafe fn try_block_clone(src_file: &std::fs::File, dest_file: &std::fs::File, len: u64) -> bool {
    use std::os::windows::io::AsRawHandle;

    #[repr(C)]
    struct DUPLICATE_EXTENTS_DATA {
        file_handle: *mut std::ffi::c_void,
        source_file_offset: i64,
        target_file_offset: i64,
        byte_count: i64,
    }

    let data = DUPLICATE_EXTENTS_DATA {
        file_handle: src_file.as_raw_handle(),
        source_file_offset: 0,
        target_file_offset: 0,
        byte_count: len as i64,
    };

    unsafe extern "system" {
        fn DeviceIoControl(
            hdevice: *mut std::ffi::c_void,
            dwiocontrolcode: u32,
            lpinbuffer: *const std::ffi::c_void,
            ninbuffersize: u32,
            lpoutbuffer: *mut std::ffi::c_void,
            noutbuffersize: u32,
            lpbytesreturned: *mut u32,
            lpoverlapped: *mut std::ffi::c_void,
        ) -> i32;
    }

    let mut bytes_returned = 0u32;
    let success = unsafe {
        DeviceIoControl(
            dest_file.as_raw_handle(),
            0x00098344, // FSCTL_DUPLICATE_EXTENTS_TO_FILE
            &data as *const _ as *const std::ffi::c_void,
            std::mem::size_of::<DUPLICATE_EXTENTS_DATA>() as u32,
            std::ptr::null_mut(),
            0,
            &mut bytes_returned,
            std::ptr::null_mut(),
        )
    };

    success != 0
}

#[allow(clippy::too_many_arguments, clippy::collapsible_if)]
pub async fn copy_file_chunked<F>(
    src_path: &Path,
    dest_path: &Path,
    start_offset: u64,
    expected_size: Option<u64>,
    pause_flag: Arc<AtomicBool>,
    cancel_flag: Arc<AtomicBool>,
    rate_limiter: Arc<GlobalRateLimiter>,
    hash_algorithm: Option<hasher::HashAlgorithm>,
    enable_block_cloning: bool,
    progress_callback: F,
) -> Result<(u64, Option<String>), String>
where
    F: Fn(u64) + Send + Sync + 'static,
{
    // Create destination parent directory if it does not exist
    if let Some(parent) = dest_path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| format!("Failed to create destination directories: {}", e))?;
    }

    #[cfg(windows)]
    if enable_block_cloning && start_offset == 0 {
        if let Ok(src_meta) = std::fs::metadata(src_path) {
            let file_len = src_meta.len();
            if let Some(expected) = expected_size {
                if expected != file_len {
                    return Err(format!(
                        "Source file size changed from {} to {} bytes since the transfer was enqueued. Cannot resume.",
                        expected, file_len
                    ));
                }
            }
            if let (Ok(src_std_file), Ok(dest_std_file)) = (
                std::fs::File::open(src_path),
                std::fs::OpenOptions::new()
                    .write(true)
                    .create(true)
                    .truncate(true)
                    .open(dest_path),
            ) {
                unsafe {
                    if try_block_clone(&src_std_file, &dest_std_file, file_len) {
                        progress_callback(file_len);

                        let src_hash = match hash_algorithm {
                            Some(hasher::HashAlgorithm::Blake3) => hasher::hash_file_async(
                                src_path.to_string_lossy().to_string(),
                                hasher::HashAlgorithm::Blake3,
                            )
                            .await
                            .ok(),
                            Some(hasher::HashAlgorithm::XxHash3) => hasher::hash_file_async(
                                src_path.to_string_lossy().to_string(),
                                hasher::HashAlgorithm::XxHash3,
                            )
                            .await
                            .ok(),
                            Some(hasher::HashAlgorithm::Md5) => hasher::hash_file_async(
                                src_path.to_string_lossy().to_string(),
                                hasher::HashAlgorithm::Md5,
                            )
                            .await
                            .ok(),
                            Some(hasher::HashAlgorithm::Sha256) => hasher::hash_file_async(
                                src_path.to_string_lossy().to_string(),
                                hasher::HashAlgorithm::Sha256,
                            )
                            .await
                            .ok(),
                            None => None,
                        };

                        let mut times = std::fs::FileTimes::new();
                        let mut has_times = false;
                        if let Ok(modified) = src_meta.modified() {
                            times = times.set_modified(modified);
                            has_times = true;
                        }
                        if let Ok(accessed) = src_meta.accessed() {
                            times = times.set_accessed(accessed);
                            has_times = true;
                        }
                        if let Ok(created) = src_meta.created() {
                            use std::os::windows::fs::FileTimesExt;
                            times = times.set_created(created);
                            has_times = true;
                        }
                        if has_times {
                            let _ = dest_std_file.set_times(times);
                        }

                        let attrs = std::os::windows::fs::MetadataExt::file_attributes(&src_meta);
                        let wide_path: Vec<u16> = dest_path
                            .to_string_lossy()
                            .encode_utf16()
                            .chain(std::iter::once(0))
                            .collect();
                        unsafe extern "system" {
                            fn SetFileAttributesW(
                                lpfilename: *const u16,
                                dwfileattributes: u32,
                            ) -> i32;
                        }
                        SetFileAttributesW(wide_path.as_ptr(), attrs);

                        return Ok((file_len, src_hash));
                    }
                }
            }
        }
    }

    let mut src_file = File::open(src_path)
        .await
        .map_err(|e| format!("Failed to open source file: {}", e))?;

    let file_metadata = src_file
        .metadata()
        .await
        .map_err(|e| format!("Failed to read source file metadata: {}", e))?;
    let file_len = file_metadata.len();

    // Verify source file size has not changed since the job was enqueued (especially on resume)
    if let Some(expected) = expected_size {
        if expected != file_len {
            return Err(format!(
                "Source file size changed from {} to {} bytes since the transfer was enqueued. Cannot resume.",
                expected, file_len
            ));
        }
    }

    let mut dest_file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(start_offset == 0)
        .open(dest_path)
        .await
        .map_err(|e| format!("Failed to open/create destination file: {}", e))?;

    // Seek if we are resuming
    if start_offset > 0 && start_offset < file_len {
        src_file
            .seek(SeekFrom::Start(start_offset))
            .await
            .map_err(|e| format!("Failed to seek source: {}", e))?;
        dest_file
            .seek(SeekFrom::Start(start_offset))
            .await
            .map_err(|e| format!("Failed to seek destination: {}", e))?;
    }

    let buffer_size = get_adaptive_buffer_size(file_len);
    let mut bytes_copied = start_offset;

    // Initialize streaming hasher if start_offset is 0
    let mut blake3_hasher = if start_offset == 0 {
        match hash_algorithm {
            Some(hasher::HashAlgorithm::Blake3) => Some(blake3::Hasher::new()),
            _ => None,
        }
    } else {
        None
    };

    let mut xxhash_hasher = if start_offset == 0 {
        match hash_algorithm {
            Some(hasher::HashAlgorithm::XxHash3) => Some(xxhash_rust::xxh3::Xxh3::new()),
            _ => None,
        }
    } else {
        None
    };

    let mut md5_hasher = if start_offset == 0 {
        match hash_algorithm {
            Some(hasher::HashAlgorithm::Md5) => Some(md5::Context::new()),
            _ => None,
        }
    } else {
        None
    };

    let mut sha256_hasher = if start_offset == 0 {
        match hash_algorithm {
            Some(hasher::HashAlgorithm::Sha256) => Some(sha2::Sha256::new()),
            _ => None,
        }
    } else {
        None
    };

    // Tokio Channel Double-Buffering
    let (tx, mut rx) = tokio::sync::mpsc::channel::<Vec<u8>>(4);
    let mut reader_src_file = src_file;
    let reader_cancel_flag = Arc::clone(&cancel_flag);
    let reader_pause_flag = Arc::clone(&pause_flag);

    let read_task = tokio::spawn(async move {
        let mut read_bytes_copied = start_offset;
        while read_bytes_copied < file_len {
            if reader_cancel_flag.load(Ordering::SeqCst) {
                return Err("Operation cancelled".to_string());
            }
            while reader_pause_flag.load(Ordering::SeqCst) {
                if reader_cancel_flag.load(Ordering::SeqCst) {
                    return Err("Operation cancelled".to_string());
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }

            let to_read = std::cmp::min(buffer_size as u64, file_len - read_bytes_copied) as usize;
            let mut read_buf = vec![0u8; to_read];

            match reader_src_file.read_exact(&mut read_buf).await {
                Ok(_) => {
                    read_bytes_copied += to_read as u64;
                    if tx.send(read_buf).await.is_err() {
                        break;
                    }
                }
                Err(e) => return Err(format!("Read error: {}", e)),
            }
        }
        Ok(())
    });

    while let Some(chunk) = rx.recv().await {
        if cancel_flag.load(Ordering::SeqCst) {
            return Err("Operation cancelled".to_string());
        }

        dest_file
            .write_all(&chunk)
            .await
            .map_err(|e| format!("Write error: {}", e))?;

        let read_bytes = chunk.len();
        if read_bytes > 0 {
            if let Some(ref mut hasher) = blake3_hasher {
                hasher.update(&chunk);
            }
            if let Some(ref mut hasher) = xxhash_hasher {
                hasher.update(&chunk);
            }
            if let Some(ref mut hasher) = md5_hasher {
                hasher.consume(&chunk);
            }
            if let Some(ref mut hasher) = sha256_hasher {
                use sha2::Digest;
                hasher.update(&chunk);
            }
        }

        bytes_copied += read_bytes as u64;
        progress_callback(read_bytes as u64);

        // Dynamic Global Rate Limiting
        rate_limiter.consume(read_bytes).await;
    }

    match read_task.await {
        Ok(Ok(())) => {}
        Ok(Err(e)) => return Err(e),
        Err(e) => return Err(format!("Reader task panicked: {}", e)),
    }

    // Finalize hash
    let src_hash = if let Some(hasher) = blake3_hasher {
        Some(hasher.finalize().to_hex().to_string())
    } else if let Some(hasher) = xxhash_hasher {
        Some(format!("{:016x}", hasher.digest()))
    } else if let Some(hasher) = md5_hasher {
        Some(format!("{:x}", hasher.compute()))
    } else if let Some(hasher) = sha256_hasher {
        use sha2::Digest;
        Some(format!("{:x}", hasher.finalize()))
    } else {
        None
    };

    // Flush destination file
    dest_file
        .flush()
        .await
        .map_err(|e| format!("Failed to flush destination file: {}", e))?;

    // Try copy timestamps (Creation, Last Access, Last Modified)
    let mut times = std::fs::FileTimes::new();
    let mut has_times = false;
    if let Ok(modified) = file_metadata.modified() {
        times = times.set_modified(modified);
        has_times = true;
    }
    if let Ok(accessed) = file_metadata.accessed() {
        times = times.set_accessed(accessed);
        has_times = true;
    }
    #[cfg(windows)]
    if let Ok(created) = file_metadata.created() {
        use std::os::windows::fs::FileTimesExt;
        times = times.set_created(created);
        has_times = true;
    }
    if has_times {
        if let Ok(dest_std_file) = std::fs::File::open(dest_path) {
            let _ = dest_std_file.set_times(times);
        }
    }

    // Try copy Windows File Attributes (Hidden, Read-Only, System, Archive)
    #[cfg(windows)]
    {
        if let Ok(meta) = std::fs::metadata(src_path) {
            let attrs = std::os::windows::fs::MetadataExt::file_attributes(&meta);
            let wide_path: Vec<u16> = dest_path
                .to_string_lossy()
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect();
            unsafe extern "system" {
                fn SetFileAttributesW(lpfilename: *const u16, dwfileattributes: u32) -> i32;
            }
            unsafe {
                SetFileAttributesW(wide_path.as_ptr(), attrs);
            }
        }
    }

    Ok((bytes_copied, src_hash))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_buffer_sizing() {
        assert_eq!(get_adaptive_buffer_size(500 * 1024), 64 * 1024);
        assert_eq!(get_adaptive_buffer_size(50 * 1024 * 1024), 256 * 1024);
        assert_eq!(get_adaptive_buffer_size(500 * 1024 * 1024), 1024 * 1024);
        assert_eq!(
            get_adaptive_buffer_size(2 * 1024 * 1024 * 1024),
            4 * 1024 * 1024
        );
    }

    #[tokio::test]
    async fn test_copy_file_chunked_success() {
        let temp_dir = std::env::temp_dir();
        let src_path = temp_dir.join(format!("copytej_src_{}.txt", uuid::Uuid::new_v4()));
        let dest_path = temp_dir.join(format!("copytej_dest_{}.txt", uuid::Uuid::new_v4()));

        {
            let mut file = std::fs::File::create(&src_path).unwrap();
            file.write_all(
                b"Hello World from CopyTej! This is a test file for streaming hashing and copying.",
            )
            .unwrap();
        }

        let pause = Arc::new(AtomicBool::new(false));
        let cancel = Arc::new(AtomicBool::new(false));
        let limit = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let rate_limiter = Arc::new(GlobalRateLimiter::new(limit));

        let res = copy_file_chunked(
            &src_path,
            &dest_path,
            0,
            None,
            pause.clone(),
            cancel.clone(),
            rate_limiter,
            Some(hasher::HashAlgorithm::Blake3),
            true,
            |_| {},
        )
        .await;

        assert!(res.is_ok());
        let (bytes, hash_opt) = res.unwrap();
        assert_eq!(bytes, 80);
        assert!(hash_opt.is_some());

        let dest_content = std::fs::read_to_string(&dest_path).unwrap();
        assert_eq!(
            dest_content,
            "Hello World from CopyTej! This is a test file for streaming hashing and copying."
        );

        let _ = std::fs::remove_file(&src_path);
        let _ = std::fs::remove_file(&dest_path);
    }
}
