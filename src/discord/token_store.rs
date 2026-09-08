use anyhow::Result;

pub trait TokenStore: Send + Sync {
    fn save(&self, application_id: &str, token: &str) -> Result<()>;
    fn load(&self, application_id: &str) -> Result<Option<String>>;
    fn delete(&self, application_id: &str) -> Result<()>;
}

pub struct KeyringTokenStore;

impl TokenStore for KeyringTokenStore {
    fn save(&self, application_id: &str, token: &str) -> Result<()> {
        let entry = keyring::Entry::new("hivemind-discord", application_id)?;
        entry.set_password(token)?;
        Ok(())
    }

    fn load(&self, application_id: &str) -> Result<Option<String>> {
        let entry = keyring::Entry::new("hivemind-discord", application_id)?;
        match entry.get_password() {
            Ok(pw) => Ok(Some(pw)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    fn delete(&self, application_id: &str) -> Result<()> {
        let entry = keyring::Entry::new("hivemind-discord", application_id)?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(e.into()),
        }
    }
}

#[cfg(test)]
pub struct FakeTokenStore(std::sync::Mutex<std::collections::HashMap<String, String>>);

#[cfg(test)]
impl Default for FakeTokenStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
impl FakeTokenStore {
    pub fn new() -> Self {
        FakeTokenStore(std::sync::Mutex::new(std::collections::HashMap::new()))
    }
}

#[cfg(test)]
impl TokenStore for FakeTokenStore {
    fn save(&self, application_id: &str, token: &str) -> Result<()> {
        self.0
            .lock()
            .unwrap()
            .insert(application_id.to_string(), token.to_string());
        Ok(())
    }

    fn load(&self, application_id: &str) -> Result<Option<String>> {
        Ok(self.0.lock().unwrap().get(application_id).cloned())
    }

    fn delete(&self, application_id: &str) -> Result<()> {
        self.0.lock().unwrap().remove(application_id);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_on_empty_store_returns_none() {
        let store = FakeTokenStore::new();
        assert_eq!(store.load("123456789012345678").unwrap(), None);
    }

    #[test]
    fn save_then_load_round_trips() {
        let store = FakeTokenStore::new();
        store.save("123456789012345678", "bot-token-abc").unwrap();
        assert_eq!(
            store.load("123456789012345678").unwrap(),
            Some("bot-token-abc".to_string())
        );
    }

    #[test]
    fn delete_removes_the_entry() {
        let store = FakeTokenStore::new();
        store.save("123456789012345678", "tok").unwrap();
        store.delete("123456789012345678").unwrap();
        assert_eq!(store.load("123456789012345678").unwrap(), None);
    }

    #[test]
    fn delete_on_missing_entry_does_not_error() {
        let store = FakeTokenStore::new();
        store.delete("000000000000000000").unwrap();
    }

    #[test]
    fn entries_are_keyed_per_application_id() {
        let store = FakeTokenStore::new();
        store.save("111111111111111111", "tok-a").unwrap();
        store.save("222222222222222222", "tok-b").unwrap();
        assert_eq!(
            store.load("111111111111111111").unwrap(),
            Some("tok-a".to_string())
        );
        assert_eq!(
            store.load("222222222222222222").unwrap(),
            Some("tok-b".to_string())
        );
    }
}
