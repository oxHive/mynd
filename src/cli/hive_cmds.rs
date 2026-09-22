use anyhow::{Context, Result, bail};
use serde::Deserialize;
use serde_json::{Value, json};

// ── hive ─────────────────────────────────────────────────────────────────
// CLI-native pairing: wraps the same loopback-gated endpoints the dashboard
// uses (`POST /api/v1/hive/pairing-code`, `POST /api/v1/hive/join`), so a
// headless box (no browser, e.g. a Raspberry Pi reached over SSH) can pair
// without a browser or an SSH tunnel.

fn require_hive_ready(settings: &crate::config::ServerSettings) -> Result<()> {
    if !settings.hive.enabled {
        bail!(
            "Hive Mode is not enabled.\n\
             Set [hive] enabled = true in ~/.config/mynd/config.toml first, \
             then run `mynd up` and try again."
        );
    }
    if !crate::cli::probe_server_up(settings) {
        bail!(
            "mynd up is not running on this device.\n\
             Start it first (`mynd up` or `mynd up --headless`), then try again."
        );
    }
    Ok(())
}

/// Extracts `{"error": "..."}` from a failed API response, falling back to a
/// generic message if the body isn't the shape `ApiError` sends.
fn error_message(body: &Value) -> &str {
    body.get("error")
        .and_then(|v| v.as_str())
        .unwrap_or("request failed")
}

pub fn cmd_hive_pair() -> Result<()> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(cmd_hive_pair_async())
}

#[derive(Deserialize)]
struct PairingCodeResponse {
    code: String,
    expires_at: i64,
    public_key: String,
}

async fn cmd_hive_pair_async() -> Result<()> {
    let settings = crate::config::load_server_settings(&crate::config::global_config_path())?;
    require_hive_ready(&settings)?;

    let resp = reqwest::Client::new()
        .post(format!(
            "http://127.0.0.1:{}/api/v1/hive/pairing-code",
            settings.port
        ))
        .send()
        .await
        .context("calling the local hive pairing-code endpoint")?;
    let ok = resp.status().is_success();
    let body: Value = resp.json().await.unwrap_or_default();
    if !ok {
        bail!("could not issue a pairing code: {}", error_message(&body));
    }
    let pairing: PairingCodeResponse =
        serde_json::from_value(body).context("parsing pairing-code response")?;

    let (_, pairing_port) = crate::hive::hive_ports(settings.port)?;
    let ip = local_ip_address::local_ip()
        .map(|ip| ip.to_string())
        .unwrap_or_else(|_| "<this-device-ip>".to_string());
    let peer_address = crate::hive::format_peer_authority(&ip, pairing_port);
    let ttl = (pairing.expires_at - chrono::Utc::now().timestamp()).max(0);

    println!("Pairing code:  {}", pairing.code);
    println!("Expires in:    {ttl}s");
    println!();
    println!("On the OTHER device, run:");
    println!(
        "  mynd hive join {} {} {}",
        pairing.code, peer_address, pairing.public_key
    );
    Ok(())
}

pub fn cmd_hive_join(code: String, peer_address: String, peer_public_key: String) -> Result<()> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(cmd_hive_join_async(code, peer_address, peer_public_key))
}

async fn cmd_hive_join_async(
    code: String,
    peer_address: String,
    peer_public_key: String,
) -> Result<()> {
    let settings = crate::config::load_server_settings(&crate::config::global_config_path())?;
    require_hive_ready(&settings)?;

    let resp = reqwest::Client::new()
        .post(format!(
            "http://127.0.0.1:{}/api/v1/hive/join",
            settings.port
        ))
        .json(&json!({
            "peer_address": peer_address,
            "pairing_code": code,
            "peer_public_key": peer_public_key,
        }))
        .send()
        .await
        .context("calling the local hive join endpoint")?;
    let ok = resp.status().is_success();
    let body: Value = resp.json().await.unwrap_or_default();
    if !ok {
        bail!("could not join the hive: {}", error_message(&body));
    }
    let roster_size = body
        .get("roster_size")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    println!("Joined the hive. Roster now has {roster_size} device(s).");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(hive_enabled: bool) -> crate::config::ServerSettings {
        crate::config::ServerSettings {
            host: "127.0.0.1".into(),
            // Port 1 is never listening in a test sandbox (no root, nothing
            // binds it), so probe_server_up reliably reports "not running"
            // without depending on any real mynd process.
            port: 1,
            dashboard_port: 2,
            api_url: "http://127.0.0.1:1".into(),
            cors_origin: "http://127.0.0.1:2".into(),
            sync: Default::default(),
            org_sync: None,
            update: Default::default(),
            agent: Default::default(),
            guard_predefined_namespaces: true,
            hive: crate::config::HiveSettings {
                enabled: hive_enabled,
                ..Default::default()
            },
        }
    }

    #[test]
    fn error_message_extracts_the_api_error_shape() {
        let body = json!({ "error": "invalid or expired pairing code" });
        assert_eq!(error_message(&body), "invalid or expired pairing code");
    }

    #[test]
    fn error_message_falls_back_on_an_unexpected_body_shape() {
        assert_eq!(error_message(&Value::Null), "request failed");
        assert_eq!(error_message(&json!({})), "request failed");
    }

    #[test]
    fn require_hive_ready_rejects_when_hive_disabled() {
        let err = require_hive_ready(&settings(false)).unwrap_err();
        assert!(err.to_string().contains("Hive Mode is not enabled"));
    }

    #[test]
    fn require_hive_ready_rejects_when_server_not_running() {
        let err = require_hive_ready(&settings(true)).unwrap_err();
        assert!(err.to_string().contains("mynd up is not running"));
    }
}
