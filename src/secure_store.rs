//! Bootstraps the process-wide OS credential store used by Hive, Matrix, and
//! Discord to persist secrets (device signing keys, session tokens, bot
//! tokens).
//!
//! On Linux the desktop `keyring` ecosystem only talks to the D-Bus Secret
//! Service (gnome-keyring, kwallet, ...) by default, which isn't reachable
//! on a headless box with no D-Bus session -- e.g. `mynd` running as a
//! `systemd --user` service on a Raspberry Pi with no desktop login. When
//! the secret service can't be reached, this falls back to the Linux kernel
//! keyring (keyutils), which the keyring ecosystem's own docs recommend for
//! exactly this case. That fallback is in-memory only and does not survive
//! a reboot; callers already treat a missing/invalid entry as "generate and
//! persist a fresh credential" (see `hive::bootstrap_self_identity`) or
//! prompt the user to log in again, so this degrades gracefully.

use std::sync::{Arc, LazyLock};

pub use keyring_core::{Entry, Error, Result};

static INIT: LazyLock<Result<()>> = LazyLock::new(init_default_store);

fn init_default_store() -> Result<()> {
    #[cfg(target_os = "macos")]
    let store = apple_native_keyring_store::keychain::Store::new()?;

    #[cfg(target_os = "windows")]
    let store = windows_native_keyring_store::Store::new()?;

    #[cfg(all(
        unix,
        not(any(target_os = "macos", target_os = "ios", target_os = "android"))
    ))]
    let store: Arc<keyring_core::CredentialStore> =
        match zbus_secret_service_keyring_store::Store::new() {
            Ok(store) => store,
            Err(e) => {
                tracing::warn!(
                    "no D-Bus secret service reachable ({e}); falling back to the Linux kernel \
                 keyring (keyutils) -- entries won't survive a reboot. See the README's \
                 \"A background service on a headless box keeps restarting\" section for \
                 what this means and how to get persistence."
                );
                linux_keyutils_keyring_store::Store::new()?
            }
        };

    #[cfg(all(any(unix, windows), not(any(target_os = "ios", target_os = "android"))))]
    {
        keyring_core::set_default_store(store);
        Ok(())
    }

    #[cfg(not(all(any(unix, windows), not(any(target_os = "ios", target_os = "android")))))]
    Err(Error::Invalid(
        "platform".to_string(),
        "must be macOS, Windows, or a non-iOS, non-Android *nix variant".to_string(),
    ))
}

/// Creates a keyring entry, initializing the process-wide default credential
/// store on first use.
pub fn entry(service: &str, user: &str) -> Result<Entry> {
    if INIT.is_err() {
        return Err(Error::NoDefaultStore);
    }
    Entry::new(service, user)
}
