use serde::{Deserialize, Serialize};
use sha2::Digest;
use std::fs::File;
use std::io::Read;
use std::path::Path;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum HashAlgorithm {
    Blake3,
    XxHash3,
    Crc32,
    Md5,
    Sha256,
}

fn hash_file_internal(file: &File, algorithm: HashAlgorithm) -> Result<String, String> {
    // Try zero-copy memory mapping for files > 64 KB
    if let Ok(metadata) = file.metadata() {
        if metadata.len() > 64 * 1024 {
            if let Ok(mmap) = unsafe { memmap2::MmapOptions::new().map(file) } {
                return match algorithm {
                    HashAlgorithm::Blake3 => {
                        let hash = blake3::hash(&mmap);
                        Ok(hash.to_hex().to_string())
                    }
                    HashAlgorithm::XxHash3 => {
                        let hash = xxhash_rust::xxh3::xxh3_64(&mmap);
                        Ok(format!("{:016x}", hash))
                    }
                    HashAlgorithm::Crc32 => {
                        let hash = crc32fast::hash(&mmap);
                        Ok(format!("{:08x}", hash))
                    }
                    HashAlgorithm::Md5 => {
                        let hash = md5::compute(&mmap);
                        Ok(format!("{:x}", hash))
                    }
                    HashAlgorithm::Sha256 => {
                        let mut hasher = sha2::Sha256::new();
                        hasher.update(&mmap);
                        Ok(format!("{:x}", hasher.finalize()))
                    }
                };
            }
        }
    }

    // Streaming buffer fallback
    let mut file_ref = file;
    let mut buffer = vec![0u8; 1024 * 1024];
    match algorithm {
        HashAlgorithm::Blake3 => {
            let mut hasher = blake3::Hasher::new();
            loop {
                let bytes_read = file_ref
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
                let bytes_read = file_ref
                    .read(&mut buffer)
                    .map_err(|e| format!("Read error during hashing: {}", e))?;
                if bytes_read == 0 {
                    break;
                }
                hasher.update(&buffer[..bytes_read]);
            }
            Ok(format!("{:016x}", hasher.digest()))
        }
        HashAlgorithm::Crc32 => {
            let mut hasher = crc32fast::Hasher::new();
            loop {
                let bytes_read = file_ref
                    .read(&mut buffer)
                    .map_err(|e| format!("Read error during hashing: {}", e))?;
                if bytes_read == 0 {
                    break;
                }
                hasher.update(&buffer[..bytes_read]);
            }
            Ok(format!("{:08x}", hasher.finalize()))
        }
        HashAlgorithm::Md5 => {
            let mut hasher = md5::Context::new();
            loop {
                let bytes_read = file_ref
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
                let bytes_read = file_ref
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
}

pub async fn hash_file_async(path: String, algorithm: HashAlgorithm) -> Result<String, String> {
    tokio::task::spawn_blocking(move || {
        let path_obj = Path::new(&path);
        let file =
            File::open(path_obj).map_err(|e| format!("Failed to open file for hashing: {}", e))?;
        hash_file_internal(&file, algorithm)
    })
    .await
    .map_err(|e| format!("Blocking task failed: {}", e))?
}

pub fn hash_files_batch_parallel(
    items: Vec<(String, HashAlgorithm)>,
) -> Vec<Result<String, String>> {
    use rayon::prelude::*;
    items
        .into_par_iter()
        .map(|(path, algo)| {
            let path_obj = Path::new(&path);
            let file = File::open(path_obj)
                .map_err(|e| format!("Failed to open file for hashing: {}", e))?;
            hash_file_internal(&file, algo)
        })
        .collect()
}
