use anyhow::Result;
use std::io::Write as _;

// ── matrix ────────────────────────────────────────────────────────────────

pub fn cmd_matrix_login() -> Result<()> {
    print!("Homeserver URL [https://matrix.org]: ");
    std::io::stdout().flush()?;
    let mut homeserver_url = String::new();
    std::io::stdin().read_line(&mut homeserver_url)?;
    let homeserver_url = homeserver_url.trim();
    let homeserver_url = if homeserver_url.is_empty() {
        "https://matrix.org".to_string()
    } else {
        homeserver_url.to_string()
    };

    print!("User ID (e.g. @mynd-bot:matrix.org): ");
    std::io::stdout().flush()?;
    let mut user_id = String::new();
    std::io::stdin().read_line(&mut user_id)?;
    let user_id = user_id.trim().to_string();

    let password = rpassword::prompt_password("Password: ")?;

    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(async {
            let client = matrix_sdk::Client::builder()
                .homeserver_url(&homeserver_url)
                .sqlite_store(crate::db::xdg_data_dir().join("matrix-store"), None)
                .build()
                .await?;
            let response = client
                .matrix_auth()
                .login_username(&user_id, &password)
                .initial_device_display_name("Mynd bot")
                .await?;
            drop(password);
            let session = client
                .matrix_auth()
                .session()
                .ok_or_else(|| anyhow::anyhow!("login succeeded but no session was created"))?;
            let session_json = serde_json::to_string(&session)?;
            let store = crate::matrix::keyring_store::KeyringSessionStore;
            crate::matrix::login::persist_login(
                &homeserver_url,
                &user_id,
                &session_json,
                &store,
                &crate::config::global_config_path(),
            )?;
            println!(
                "Logged in as {} (device {}).",
                response.user_id, response.device_id
            );
            println!(
                "Session saved to the OS keyring. Run `mynd matrix run` to start the bot."
            );
            anyhow::Ok(())
        })
}

pub fn cmd_matrix_status() -> Result<()> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(async {
            let socket_path = crate::matrix::status::socket_path();
            match crate::matrix::status::query_status(&socket_path).await {
                Ok(reply) => {
                    println!("logged_in:  {}", reply.logged_in);
                    println!("user_id:    {}", reply.user_id);
                    println!("sync_state: {}", reply.sync_state);
                    if let Some(t) = &reply.last_sync_at {
                        println!("last_sync:  {t}");
                    }
                    if reply.rooms.is_empty() {
                        println!("rooms:      (none)");
                    } else {
                        println!("rooms:");
                        for room in &reply.rooms {
                            let label = room.alias.as_deref().unwrap_or(&room.room_id);
                            let session = if room.active_session { "active session" } else { "no active session" };
                            println!("  {label}  ({session})");
                        }
                    }
                    Ok(())
                }
                Err(crate::matrix::status::QueryError::NotRunning) => {
                    println!("mynd matrix is not running.");
                    println!("Start it with: mynd matrix run");
                    Ok(())
                }
                Err(crate::matrix::status::QueryError::Protocol(msg)) => {
                    println!("mynd matrix appears to be running but returned invalid status data: {msg}");
                    Ok(())
                }
            }
        })
}
