use anyhow::Result;

pub trait HiveKeyStore: Send + Sync {
    fn save(&self, device_id: &str, signing_key_hex: &str) -> Result<()>;
    fn load(&self, device_id: &str) -> Result<Option<String>>;
}

pub struct KeyringHiveKeyStore;

impl HiveKeyStore for KeyringHiveKeyStore {
    fn save(&self, device_id: &str, signing_key_hex: &str) -> Result<()> {
        let entry = keyring::Entry::new("hivemind-hive", device_id)?;
        entry.set_password(signing_key_hex)?;
        Ok(())
    }

    fn load(&self, device_id: &str) -> Result<Option<String>> {
        let entry = keyring::Entry::new("hivemind-hive", device_id)?;
        match entry.get_password() {
            Ok(pw) => Ok(Some(pw)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }
}

#[cfg(test)]
pub struct FakeHiveKeyStore(std::sync::Mutex<std::collections::HashMap<String, String>>);

#[cfg(test)]
impl Default for FakeHiveKeyStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
impl FakeHiveKeyStore {
    pub fn new() -> Self {
        Self(std::sync::Mutex::new(std::collections::HashMap::new()))
    }
}

#[cfg(test)]
impl HiveKeyStore for FakeHiveKeyStore {
    fn save(&self, device_id: &str, signing_key_hex: &str) -> Result<()> {
        self.0
            .lock()
            .unwrap()
            .insert(device_id.to_string(), signing_key_hex.to_string());
        Ok(())
    }

    fn load(&self, device_id: &str) -> Result<Option<String>> {
        Ok(self.0.lock().unwrap().get(device_id).cloned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fake_store_round_trips() {
        let store = FakeHiveKeyStore::new();
        store.save("hive_abc", "deadbeef").unwrap();
        assert_eq!(
            store.load("hive_abc").unwrap(),
            Some("deadbeef".to_string())
        );
    }

    #[test]
    fn fake_store_missing_device_returns_none() {
        let store = FakeHiveKeyStore::new();
        assert_eq!(store.load("hive_missing").unwrap(), None);
    }
}
