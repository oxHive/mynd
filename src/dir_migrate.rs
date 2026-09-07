//! One-time relocation of the global config and data directories from the
//! old `hivemind` names to `mynd`. Invoked automatically at startup and also
//! reachable through `mynd migrate`.

use anyhow::{Context, Result};
use std::path::Path;

#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The destination already existed; nothing was touched.
    AlreadyPresent,
    /// No legacy directory was found; nothing to do.
    NoLegacy,
    /// The legacy directory was relocated to the new path.
    Migrated,
}

/// Relocate `old` to `new`, but only when `new` does not yet exist and `old`
/// does. Tries an atomic rename first and falls back to a recursive copy plus
/// remove when the two paths live on different filesystems.
pub fn relocate_dir(old: &Path, new: &Path) -> Result<Outcome> {
    if new.exists() {
        return Ok(Outcome::AlreadyPresent);
    }
    if !old.is_dir() {
        return Ok(Outcome::NoLegacy);
    }
    if let Some(parent) = new.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    match std::fs::rename(old, new) {
        Ok(()) => Ok(Outcome::Migrated),
        Err(_) => {
            copy_dir_all(old, new)
                .with_context(|| format!("copying {} to {}", old.display(), new.display()))?;
            std::fs::remove_dir_all(old)
                .with_context(|| format!("removing {}", old.display()))?;
            Ok(Outcome::Migrated)
        }
    }
}

/// Relocate a pre-0.3 single-file database (`~/.hivemind/memories.db`) into a
/// fresh data dir. Only acts when `new_dir` does not exist yet and the legacy
/// file does.
pub fn relocate_legacy_db_file(legacy_db: &Path, new_dir: &Path) -> Result<Outcome> {
    if new_dir.exists() {
        return Ok(Outcome::AlreadyPresent);
    }
    if !legacy_db.is_file() {
        return Ok(Outcome::NoLegacy);
    }
    std::fs::create_dir_all(new_dir)
        .with_context(|| format!("creating {}", new_dir.display()))?;
    let dest = new_dir.join("memories.db");
    if std::fs::rename(legacy_db, &dest).is_err() {
        std::fs::copy(legacy_db, &dest)
            .with_context(|| format!("copying {} to {}", legacy_db.display(), dest.display()))?;
        let _ = std::fs::remove_file(legacy_db);
    }
    Ok(Outcome::Migrated)
}

/// Run the one-time `hivemind` -> `mynd` relocation for the global config dir
/// and the data dir. Call once at process start, before any config or database
/// access. Prints a line to stderr for each directory actually moved; failures
/// are reported and swallowed so a migration hiccup never blocks startup.
pub fn run_startup_migration() {
    // Data dir: skipped entirely when the DB path is pinned by env.
    if std::env::var_os("HIVEMIND_DB_PATH").is_none() {
        let new_data = crate::db::xdg_data_dir();
        match relocate_dir(&crate::db::legacy_xdg_data_dir(), &new_data) {
            Ok(Outcome::Migrated) => {
                eprintln!("mynd: moved data directory to {}", new_data.display());
            }
            Ok(_) => match relocate_legacy_db_file(&crate::db::legacy_db_path(), &new_data) {
                Ok(Outcome::Migrated) => {
                    eprintln!("mynd: moved legacy database to {}", new_data.display());
                }
                Ok(_) => {}
                Err(e) => eprintln!("mynd: legacy database migration skipped ({e:#})"),
            },
            Err(e) => eprintln!("mynd: data directory migration skipped ({e:#})"),
        }
    }

    let new_cfg = crate::config::global_config_dir();
    match relocate_dir(&crate::config::legacy_global_config_dir(), &new_cfg) {
        Ok(Outcome::Migrated) => {
            eprintln!("mynd: moved config directory to {}", new_cfg.display());
        }
        Ok(_) => {}
        Err(e) => eprintln!("mynd: config directory migration skipped ({e:#})"),
    }
}

fn copy_dir_all(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_all(&from, &to)?;
        } else {
            std::fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn relocates_legacy_dir_when_destination_absent() {
        let tmp = tempfile::tempdir().unwrap();
        let old = tmp.path().join("hivemind");
        let new = tmp.path().join("mynd");
        fs::create_dir_all(old.join("sub")).unwrap();
        fs::write(old.join("config.toml"), "a").unwrap();
        fs::write(old.join("sub/nested.txt"), "b").unwrap();

        let outcome = relocate_dir(&old, &new).unwrap();

        assert_eq!(outcome, Outcome::Migrated);
        assert!(!old.exists());
        assert_eq!(fs::read_to_string(new.join("config.toml")).unwrap(), "a");
        assert_eq!(fs::read_to_string(new.join("sub/nested.txt")).unwrap(), "b");
    }

    #[test]
    fn leaves_everything_when_destination_already_exists() {
        let tmp = tempfile::tempdir().unwrap();
        let old = tmp.path().join("hivemind");
        let new = tmp.path().join("mynd");
        fs::create_dir_all(&old).unwrap();
        fs::write(old.join("config.toml"), "legacy").unwrap();
        fs::create_dir_all(&new).unwrap();
        fs::write(new.join("config.toml"), "current").unwrap();

        let outcome = relocate_dir(&old, &new).unwrap();

        assert_eq!(outcome, Outcome::AlreadyPresent);
        assert!(old.exists());
        assert_eq!(fs::read_to_string(new.join("config.toml")).unwrap(), "current");
    }

    #[test]
    fn reports_nothing_to_do_when_no_legacy_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let outcome =
            relocate_dir(&tmp.path().join("hivemind"), &tmp.path().join("mynd")).unwrap();
        assert_eq!(outcome, Outcome::NoLegacy);
    }

    #[test]
    fn relocates_pre_0_3_single_file_db_into_fresh_data_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let legacy_dir = tmp.path().join(".hivemind");
        let legacy_db = legacy_dir.join("memories.db");
        let new_dir = tmp.path().join("mynd");
        fs::create_dir_all(&legacy_dir).unwrap();
        fs::write(&legacy_db, "sqlite-bytes").unwrap();

        let outcome = relocate_legacy_db_file(&legacy_db, &new_dir).unwrap();

        assert_eq!(outcome, Outcome::Migrated);
        assert_eq!(
            fs::read_to_string(new_dir.join("memories.db")).unwrap(),
            "sqlite-bytes"
        );
        assert!(!legacy_db.exists());
    }

    #[test]
    fn skips_single_file_db_when_data_dir_already_exists() {
        let tmp = tempfile::tempdir().unwrap();
        let legacy_db = tmp.path().join(".hivemind").join("memories.db");
        let new_dir = tmp.path().join("mynd");
        fs::create_dir_all(legacy_db.parent().unwrap()).unwrap();
        fs::write(&legacy_db, "old").unwrap();
        fs::create_dir_all(&new_dir).unwrap();

        let outcome = relocate_legacy_db_file(&legacy_db, &new_dir).unwrap();

        assert_eq!(outcome, Outcome::AlreadyPresent);
        assert!(legacy_db.exists());
        assert!(!new_dir.join("memories.db").exists());
    }
}
