use serde::{Deserialize, Serialize};
use sha2::Digest;
use std::fs::File;
use std::io::Read;
use std::path::Path;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum HashAlgorithm {
    Blake3,
    XxHash3,
    Md5,
    Sha256,
}

pub async fn hash_file_async(path: String, algorithm: HashAlgorithm) -> Result<String, String> {
    tokio::task::spawn_blocking(move || {
        let path = Path::new(&path);
        let mut file =
            File::open(path).map_err(|e| format!("Failed to open file for hashing: {}", e))?;

        let mut buffer = vec![0u8; 1024 * 1024]; // 1MB buffer

        match algorithm {
            HashAlgorithm::Blake3 => {
                let mut hasher = blake3::Hasher::new();
                loop {
                    let bytes_read = file
                        .read(&mut buffer)
                        .map_err(|e| format!("Read error during hashing: {}", e))?;
                    if bytes_read == 0 {
                        break;
                    }
                    hasher.update(&buffer[..bytes_read]);
                }
                Ok(hasher.finalize().to_hex().to_string())
            }
            HashAlgorithm::XxHash3 => {
                let mut hasher = xxhash_rust::xxh3::Xxh3::new();
                loop {
                    let bytes_read = file
                        .read(&mut buffer)
                        .map_err(|e| format!("Read error during hashing: {}", e))?;
                    if bytes_read == 0 {
                        break;
                    }
                    hasher.update(&buffer[..bytes_read]);
                }
                Ok(format!("{:016x}", hasher.digest()))
            }
            HashAlgorithm::Md5 => {
                let mut hasher = md5::Context::new();
                loop {
                    let bytes_read = file
                        .read(&mut buffer)
                        .map_err(|e| format!("Read error during hashing: {}", e))?;
                    if bytes_read == 0 {
                        break;
                    }
                    hasher.consume(&buffer[..bytes_read]);
                }
                Ok(format!("{:x}", hasher.compute()))
            }
            HashAlgorithm::Sha256 => {
                let mut hasher = sha2::Sha256::new();
                loop {
                    let bytes_read = file
                        .read(&mut buffer)
                        .map_err(|e| format!("Read error during hashing: {}", e))?;
                    if bytes_read == 0 {
                        break;
                    }
                    hasher.update(&buffer[..bytes_read]);
                }
                Ok(format!("{:x}", hasher.finalize()))
            }
        }
    })
    .await
    .map_err(|e| format!("Blocking task failed: {}", e))?
}
