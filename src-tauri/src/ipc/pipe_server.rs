#![cfg(windows)]

use crate::engine::queue::QueueManager;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::io::AsyncReadExt;
use tokio::net::windows::named_pipe::ServerOptions;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipeJobSpec {
    pub src_paths: Vec<String>,
    pub dest_dir: String,
    pub is_move: bool,
}

pub fn start_pipe_server(queue_manager: Arc<QueueManager>) {
    tauri::async_runtime::spawn(async move {
        let pipe_name = r"\\.\pipe\CopyTej";
        let mut first = true;

        loop {
            let server_result = ServerOptions::new()
                .first_pipe_instance(first)
                .create(pipe_name);

            let mut server = match server_result {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("Failed to create named pipe server: {}", e);
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                    continue;
                }
            };

            first = false;

            // Wait for client to connect
            if server.connect().await.is_ok() {
                let q_manager = Arc::clone(&queue_manager);
                tokio::spawn(async move {
                    let mut buffer = Vec::new();
                    let mut temp_buf = [0u8; 4096];

                    loop {
                        match server.read(&mut temp_buf).await {
                            Ok(0) => break,
                            Ok(n) => buffer.extend_from_slice(&temp_buf[..n]),
                            Err(_) => break,
                        }
                    }

                    if let Ok(job_spec) = serde_json::from_slice::<PipeJobSpec>(&buffer) {
                        println!("Received job from pipe: {:?}", job_spec);
                        if job_spec.dest_dir.is_empty() {
                            use tauri::Emitter;
                            let _ = q_manager.app_handle.emit(
                                "transfer://configure-new",
                                (job_spec.src_paths, job_spec.is_move),
                            );
                        } else {
                            let _ = q_manager
                                .add_job(job_spec.src_paths, job_spec.dest_dir, job_spec.is_move)
                                .await;
                        }
                    }
                });
            }
        }
    });
}

pub async fn try_send_to_pipe(spec: PipeJobSpec) -> bool {
    use tokio::io::AsyncWriteExt;
    use tokio::net::windows::named_pipe::ClientOptions;

    let pipe_name = r"\\.\pipe\CopyTej";
    if let Ok(mut client) = ClientOptions::new().open(pipe_name) {
        if let Ok(payload) = serde_json::to_vec(&spec) {
            if client.write_all(&payload).await.is_ok() {
                let _ = client.flush().await;
                return true;
            }
        }
    }
    false
}
