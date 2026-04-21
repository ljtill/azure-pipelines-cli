//! RAII guard that owns the TUI terminal state (raw mode, alternate screen, mouse capture).

use std::io::{Stdout, stdout};
use std::sync::Once;

use anyhow::Result;
use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

/// RAII guard that enables raw mode, the alternate screen, and mouse capture on
/// construction, and restores the terminal on drop. A panic hook is installed
/// during `init` so that unwinding also restores the terminal before the
/// previous hook prints its message.
pub struct TerminalGuard {
    _restore_on_drop: (),
}

impl TerminalGuard {
    /// Installs the panic hook, enables raw mode + alternate screen + mouse
    /// capture, and returns a ratatui `Terminal` together with the guard.
    pub fn init() -> Result<(Terminal<CrosstermBackend<Stdout>>, Self)> {
        Self::install_panic_hook();

        enable_raw_mode()?;
        execute!(stdout(), EnterAlternateScreen, EnableMouseCapture)?;

        let backend = CrosstermBackend::new(stdout());
        let terminal = Terminal::new(backend)?;

        Ok((
            terminal,
            Self {
                _restore_on_drop: (),
            },
        ))
    }

    /// Best-effort terminal restore. Safe to call multiple times and safe to
    /// call when the terminal was never initialized — every operation ignores
    /// errors.
    pub fn cleanup() {
        let _ = execute!(stdout(), DisableMouseCapture, LeaveAlternateScreen);
        let _ = disable_raw_mode();
    }

    /// Installs the process-wide panic hook exactly once. Wraps (rather than
    /// replaces) the previous hook so panic messages are still printed after
    /// the terminal has been restored.
    fn install_panic_hook() {
        static HOOK: Once = Once::new();
        HOOK.call_once(|| {
            let prev = std::panic::take_hook();
            std::panic::set_hook(Box::new(move |info| {
                Self::cleanup();
                prev(info);
            }));
        });
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        Self::cleanup();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Cleanup must be safe to call without ever having initialized the
    // terminal — this is the most common regression path (early-return
    // before TUI init, panic hook firing in a non-TUI subcommand, etc.).
    #[test]
    fn cleanup_is_idempotent_without_init() {
        TerminalGuard::cleanup();
        TerminalGuard::cleanup();
        TerminalGuard::cleanup();
    }

    // The panic hook wraps the previous hook. Installing it must not
    // panic, and invoking `cleanup` from within a panicking closure must
    // not abort the process — `catch_unwind` should observe the panic
    // normally.
    #[test]
    fn panic_hook_runs_cleanup_and_chains_previous() {
        use std::sync::atomic::{AtomicBool, Ordering};

        static PREV_CALLED: AtomicBool = AtomicBool::new(false);

        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            PREV_CALLED.store(true, Ordering::SeqCst);
            prev(info);
        }));

        // Now layer our cleanup wrapper on top, mirroring what
        // `TerminalGuard::init` does.
        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            TerminalGuard::cleanup();
            prev(info);
        }));

        let result = std::panic::catch_unwind(|| panic!("boom"));
        assert!(result.is_err());
        assert!(PREV_CALLED.load(Ordering::SeqCst));

        // Restore a default hook so we don't leak state into later tests.
        let _ = std::panic::take_hook();
    }
}
