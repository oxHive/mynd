use anyhow::Result;
use crossterm::{
    cursor::Show,
    execute,
    terminal::{disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, TerminalOptions, Viewport, backend::CrosstermBackend};
use std::io::IsTerminal as _;
use std::io::{Stdout, stdout};
use std::sync::Once;

pub mod header;
pub mod status_view;
pub mod up_view;

pub type Term = Terminal<CrosstermBackend<Stdout>>;

/// True when stdout is a real terminal and the caller did not pass `--plain`.
/// `NO_COLOR` does not affect this: it only strips styling inside the TUI.
pub fn is_interactive(plain: bool) -> bool {
    !plain && stdout().is_terminal()
}

/// True when the `NO_COLOR` env var is set (any value, per the convention at
/// https://no-color.org). Widgets check this to skip foreground colors while
/// still rendering the TUI layout, borders, and text.
pub fn no_color() -> bool {
    std::env::var_os("NO_COLOR").is_some()
}

fn restore_terminal() {
    let _ = disable_raw_mode();
    let _ = execute!(stdout(), Show);
}

static PANIC_HOOK_INSTALLED: Once = Once::new();

fn install_panic_hook() {
    PANIC_HOOK_INSTALLED.call_once(|| {
        let default_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            restore_terminal();
            default_hook(info);
        }));
    });
}

/// Owns the raw-mode terminal state for an inline (non-alt-screen) viewport.
/// Renders in place under the shell prompt, scrolling with the terminal like
/// normal output, rather than taking over the full screen. Restores the
/// terminal on Drop (normal exit) and via a panic hook (abnormal exit), so a
/// bug in rendering never leaves the user's shell in raw mode.
pub struct TerminalGuard {
    height: u16,
}

impl TerminalGuard {
    pub fn enter(height: u16) -> Result<Self> {
        install_panic_hook();
        enable_raw_mode()?;
        Ok(TerminalGuard { height })
    }

    pub fn terminal(&self) -> Result<Term> {
        let backend = CrosstermBackend::new(stdout());
        Ok(Terminal::with_options(
            backend,
            TerminalOptions {
                viewport: Viewport::Inline(self.height),
            },
        )?)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        restore_terminal();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_flag_forces_non_interactive() {
        assert!(!is_interactive(true));
    }

    #[test]
    fn no_color_is_false_when_env_var_unset() {
        let _lock = crate::test_env_lock::ENV_MUTEX.lock().unwrap();
        // SAFETY: test-only env var mutation; serialised by ENV_MUTEX.
        unsafe { std::env::remove_var("NO_COLOR") };
        assert!(!no_color());
    }

    #[test]
    fn no_color_is_true_when_env_var_set_to_any_value() {
        let _lock = crate::test_env_lock::ENV_MUTEX.lock().unwrap();
        // SAFETY: test-only env var mutation; serialised by ENV_MUTEX.
        unsafe { std::env::set_var("NO_COLOR", "1") };
        let result = no_color();
        unsafe { std::env::remove_var("NO_COLOR") };
        assert!(result);
    }
}
