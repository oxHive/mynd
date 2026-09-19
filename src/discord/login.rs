use crate::config::write_discord_login;
use crate::discord::token_store::TokenStore;
use anyhow::Result;
use std::path::Path;

pub fn persist_login(
    application_id: &str,
    bot_token: &str,
    store: &dyn TokenStore,
    global_config_path: &Path,
) -> Result<()> {
    store.save(application_id, bot_token)?;
    write_discord_login(global_config_path, application_id)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discord::token_store::FakeTokenStore;

    #[test]
    fn persists_token_and_writes_config() {
        let tmp = tempfile::tempdir().unwrap();
        let config_path = tmp.path().join("config.toml");
        let store = FakeTokenStore::new();
        persist_login("123456789012345678", "bot-token-abc", &store, &config_path).unwrap();
        assert_eq!(
            store.load("123456789012345678").unwrap(),
            Some("bot-token-abc".to_string())
        );
        let settings = crate::config::load_discord_settings(&config_path)
            .unwrap()
            .unwrap();
        assert_eq!(settings.application_id, "123456789012345678");
    }
}
