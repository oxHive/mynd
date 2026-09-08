use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::Mutex;

#[derive(Debug, Clone)]
pub enum QueryError {
    NotRunning,
    Protocol(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChannelStatus {
    pub channel_id: String,
    pub alias: Option<String>,
    pub active_session: bool,
    pub last_active_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StatusReply {
    pub logged_in: bool,
    pub application_id: String,
    pub sync_state: String,
    pub last_sync_at: Option<String>,
    pub channels: Vec<ChannelStatus>,
}

pub fn socket_path() -> PathBuf {
    crate::db::xdg_data_dir().join("hivemind-discord.sock")
}

pub async fn serve_status(socket_path: &Path, reply_source: Arc<Mutex<StatusReply>>) -> Result<()> {
    if socket_path.exists() {
        std::fs::remove_file(socket_path)?;
    }
    if let Some(dir) = socket_path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let listener = UnixListener::bind(socket_path)?;
    loop {
        let (mut stream, _) = listener.accept().await?;
        let reply = reply_source.lock().await.clone();
        let line = serde_json::to_string(&reply)? + "\n";
        let _ = stream.write_all(line.as_bytes()).await;
    }
}

pub async fn query_status(socket_path: &Path) -> Result<StatusReply, QueryError> {
    let mut stream = UnixStream::connect(socket_path)
        .await
        .map_err(|_| QueryError::NotRunning)?;
    let mut buf = String::new();
    stream
        .read_to_string(&mut buf)
        .await
        .map_err(|e| QueryError::Protocol(format!("failed reading from daemon: {e}")))?;
    serde_json::from_str(buf.trim())
        .map_err(|e| QueryError::Protocol(format!("daemon sent malformed status: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_reply() -> StatusReply {
        StatusReply {
            logged_in: true,
            application_id: "123456789012345678".to_string(),
            sync_state: "connected".to_string(),
            last_sync_at: Some("2026-08-22T10:03:00Z".to_string()),
            channels: vec![ChannelStatus {
                channel_id: "222222222222222222".to_string(),
                alias: Some("hivemind-project".to_string()),
                active_session: true,
                last_active_at: Some("2026-08-22T10:02:40Z".to_string()),
            }],
        }
    }

    #[tokio::test]
    async fn client_receives_the_servers_current_reply() {
        let dir = tempfile::tempdir().unwrap();
        let socket_path = dir.path().join("test.sock");
        let reply = Arc::new(Mutex::new(sample_reply()));
        let server_socket = socket_path.clone();
        let server_reply = reply.clone();
        tokio::spawn(async move {
            serve_status(&server_socket, server_reply).await.unwrap();
        });
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let got = query_status(&socket_path).await.unwrap();
        assert_eq!(got, sample_reply());
    }

    #[tokio::test]
    async fn client_sees_updates_to_the_shared_reply() {
        let dir = tempfile::tempdir().unwrap();
        let socket_path = dir.path().join("test.sock");
        let reply = Arc::new(Mutex::new(sample_reply()));
        let server_socket = socket_path.clone();
        let server_reply = reply.clone();
        tokio::spawn(async move {
            serve_status(&server_socket, server_reply).await.unwrap();
        });
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        {
            let mut r = reply.lock().await;
            r.sync_state = "reconnecting".to_string();
        }
        let got = query_status(&socket_path).await.unwrap();
        assert_eq!(got.sync_state, "reconnecting");
    }

    #[tokio::test]
    async fn query_against_nonexistent_socket_errors() {
        let dir = tempfile::tempdir().unwrap();
        let socket_path = dir.path().join("does-not-exist.sock");
        assert!(matches!(
            query_status(&socket_path).await,
            Err(QueryError::NotRunning)
        ));
    }

    #[tokio::test]
    async fn query_against_a_socket_serving_garbage_reports_a_protocol_error_not_not_running() {
        let dir = tempfile::tempdir().unwrap();
        let socket_path = dir.path().join("garbage.sock");
        let listener = tokio::net::UnixListener::bind(&socket_path).unwrap();
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let _ = stream.write_all(b"not valid json\n").await;
        });
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let result = query_status(&socket_path).await;
        assert!(
            matches!(result, Err(QueryError::Protocol(_))),
            "expected a Protocol error (daemon reachable but sent bad data), got: {result:?}"
        );
    }
}
