//! Serializes tests across the crate that read or mutate process-global env
//! vars (`HOME`, `XDG_CONFIG_HOME`, `XDG_DATA_HOME`, `MYND_DB_PATH` /
//! `HIVEMIND_DB_PATH`).
//! `cargo test` runs tests in parallel by default, and these vars are process
//! state, not per-thread — without a shared lock, a mutation in cli.rs's
//! tests can race a read in db.rs's or config.rs's, flipping results
//! nondeterministically. Every test that touches one of these vars must hold
//! this lock for the duration of the mutation and the assertion.
use std::sync::Mutex;

pub(crate) static ENV_MUTEX: Mutex<()> = Mutex::new(());
