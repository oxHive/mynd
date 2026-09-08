use anyhow::Result;

pub trait SessionStore: Send + Sync {
    fn save(&self, user_id: &str, session_json: &str) -> Result<()>;
    fn load(&self, user_id: &str) -> Result<Option<String>>;
    fn delete(&self, user_id: &str) -> Result<()>;
}

/// Current keyring service name. `LEGACY_SERVICE` is the pre-rename name, still
/// read (and then migrated forward) so an existing Matrix login survives an
/// upgrade without a re-login.
const SERVICE: &str = "mynd-matrix";
const LEGACY_SERVICE: &str = "hivemind-matrix";

pub struct KeyringSessionStore;

impl SessionStore for KeyringSessionStore {
    fn save(&self, user_id: &str, session_json: &str) -> Result<()> {
        let entry = keyring::Entry::new(SERVICE, user_id)?;
        entry.set_password(session_json)?;
        Ok(())
    }

    fn load(&self, user_id: &str) -> Result<Option<String>> {
        let entry = keyring::Entry::new(SERVICE, user_id)?;
        match entry.get_password() {
            Ok(pw) => return Ok(Some(pw)),
            Err(keyring::Error::NoEntry) => {}
            Err(e) => return Err(e.into()),
        }
        // Fall back to the pre-rename service name; if found, migrate it forward
        // so subsequent loads hit the new name directly.
        let legacy = keyring::Entry::new(LEGACY_SERVICE, user_id)?;
        match legacy.get_password() {
            Ok(pw) => {
                let _ = self.save(user_id, &pw);
                let _ = legacy.delete_credential();
                Ok(Some(pw))
            }
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    fn delete(&self, user_id: &str) -> Result<()> {
        for service in [SERVICE, LEGACY_SERVICE] {
            let entry = keyring::Entry::new(service, user_id)?;
            match entry.delete_credential() {
                Ok(()) | Err(keyring::Error::NoEntry) => {}
                Err(e) => return Err(e.into()),
            }
        }
        Ok(())
    }
}

#[cfg(test)]
pub struct FakeSessionStore(std::sync::Mutex<std::collections::HashMap<String, String>>);

#[cfg(test)]
impl Default for FakeSessionStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
impl FakeSessionStore {
    pub fn new() -> Self {
        FakeSessionStore(std::sync::Mutex::new(std::collections::HashMap::new()))
    }
}

#[cfg(test)]
impl SessionStore for FakeSessionStore {
    fn save(&self, user_id: &str, session_json: &str) -> Result<()> {
        self.0
            .lock()
            .unwrap()
            .insert(user_id.to_string(), session_json.to_string());
        Ok(())
    }

    fn load(&self, user_id: &str) -> Result<Option<String>> {
        Ok(self.0.lock().unwrap().get(user_id).cloned())
    }

    fn delete(&self, user_id: &str) -> Result<()> {
        self.0.lock().unwrap().remove(user_id);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_on_empty_store_returns_none() {
        let store = FakeSessionStore::new();
        assert_eq!(store.load("@bot:matrix.org").unwrap(), None);
    }

    #[test]
    fn save_then_load_round_trips() {
        let store = FakeSessionStore::new();
        store
            .save("@bot:matrix.org", "{\"token\":\"abc\"}")
            .unwrap();
        assert_eq!(
            store.load("@bot:matrix.org").unwrap(),
            Some("{\"token\":\"abc\"}".to_string())
        );
    }

    #[test]
    fn delete_removes_the_entry() {
        let store = FakeSessionStore::new();
        store.save("@bot:matrix.org", "{}").unwrap();
        store.delete("@bot:matrix.org").unwrap();
        assert_eq!(store.load("@bot:matrix.org").unwrap(), None);
    }

    #[test]
    fn delete_on_missing_entry_does_not_error() {
        let store = FakeSessionStore::new();
        store.delete("@nobody:matrix.org").unwrap();
    }

    #[test]
    fn entries_are_keyed_per_user_id() {
        let store = FakeSessionStore::new();
        store.save("@a:matrix.org", "session-a").unwrap();
        store.save("@b:matrix.org", "session-b").unwrap();
        assert_eq!(
            store.load("@a:matrix.org").unwrap(),
            Some("session-a".to_string())
        );
        assert_eq!(
            store.load("@b:matrix.org").unwrap(),
            Some("session-b".to_string())
        );
    }
}
