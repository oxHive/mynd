use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

struct Entry {
    session_id: String,
    last_active: Instant,
}

/// How long a channel's session stays resumable after its last activity, in
/// the absence of a configured `[discord] session_ttl_seconds`. Past this,
/// `get` treats it as detached and the next message starts a fresh agent
/// session instead of resuming a stale one.
const DEFAULT_SESSION_TTL: Duration =
    Duration::from_secs(crate::config::DEFAULT_SESSION_TTL_SECONDS);

#[derive(Clone)]
pub struct SessionMap {
    entries: Arc<Mutex<HashMap<String, Entry>>>,
    ttl: Duration,
}

impl Default for SessionMap {
    fn default() -> Self {
        Self::new(DEFAULT_SESSION_TTL)
    }
}

impl SessionMap {
    pub fn new(ttl: Duration) -> Self {
        SessionMap {
            entries: Arc::new(Mutex::new(HashMap::new())),
            ttl,
        }
    }

    pub async fn get(&self, channel_id: &str) -> Option<String> {
        let mut map = self.entries.lock().await;
        let entry = map.get(channel_id)?;
        if entry.last_active.elapsed() > self.ttl {
            map.remove(channel_id);
            return None;
        }
        Some(entry.session_id.clone())
    }

    pub async fn set(&self, channel_id: &str, session_id: String) {
        self.entries.lock().await.insert(
            channel_id.to_string(),
            Entry {
                session_id,
                last_active: Instant::now(),
            },
        );
    }

    pub async fn reset(&self, channel_id: &str) {
        self.entries.lock().await.remove(channel_id);
    }

    #[cfg(test)]
    async fn set_aged(&self, channel_id: &str, session_id: String, age: Duration) {
        self.entries.lock().await.insert(
            channel_id.to_string(),
            Entry {
                session_id,
                last_active: Instant::now() - age,
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn get_on_empty_map_returns_none() {
        let map = SessionMap::default();
        assert_eq!(map.get("111111111111111111").await, None);
    }

    #[tokio::test]
    async fn set_then_get_returns_the_stored_session_id() {
        let map = SessionMap::default();
        map.set("111111111111111111", "sess-1".to_string()).await;
        assert_eq!(
            map.get("111111111111111111").await,
            Some("sess-1".to_string())
        );
    }

    #[tokio::test]
    async fn set_overwrites_previous_session_id_for_the_same_channel() {
        let map = SessionMap::default();
        map.set("111111111111111111", "sess-1".to_string()).await;
        map.set("111111111111111111", "sess-2".to_string()).await;
        assert_eq!(
            map.get("111111111111111111").await,
            Some("sess-2".to_string())
        );
    }

    #[tokio::test]
    async fn session_within_ttl_is_resumable() {
        let map = SessionMap::default();
        map.set_aged(
            "111111111111111111",
            "sess-1".to_string(),
            Duration::from_secs(60),
        )
        .await;
        assert_eq!(
            map.get("111111111111111111").await,
            Some("sess-1".to_string())
        );
    }

    #[tokio::test]
    async fn session_past_ttl_is_detached_and_removed() {
        let map = SessionMap::default();
        map.set_aged(
            "111111111111111111",
            "sess-1".to_string(),
            Duration::from_secs(121),
        )
        .await;
        assert_eq!(map.get("111111111111111111").await, None);
        map.set_aged(
            "111111111111111111",
            "sess-1".to_string(),
            Duration::from_secs(121),
        )
        .await;
        assert_eq!(map.get("111111111111111111").await, None);
    }

    #[tokio::test]
    async fn reset_clears_only_that_channel() {
        let map = SessionMap::default();
        map.set("111111111111111111", "sess-a".to_string()).await;
        map.set("222222222222222222", "sess-b".to_string()).await;
        map.reset("111111111111111111").await;
        assert_eq!(map.get("111111111111111111").await, None);
        assert_eq!(
            map.get("222222222222222222").await,
            Some("sess-b".to_string())
        );
    }

    #[tokio::test]
    async fn cloned_map_shares_state() {
        let map = SessionMap::default();
        let cloned = map.clone();
        cloned.set("111111111111111111", "sess-1".to_string()).await;
        assert_eq!(
            map.get("111111111111111111").await,
            Some("sess-1".to_string())
        );
    }
}
