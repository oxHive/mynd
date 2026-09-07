use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "mynd",
    version,
    about = "Mynd — persistent memory for AI coding agents"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Subcommand)]
pub enum Command {
    /// Scaffold .mynd.toml + CLAUDE.md integration for this project
    Init,
    /// Show config and preview what session start will inject
    Status {
        /// Force plain-text output even on a real terminal
        #[arg(long)]
        plain: bool,
    },
    /// Start the HTTP server: MCP at /mcp, REST at /api/v1, plus the dashboard
    Up {
        /// Serve only MCP + REST API — no dashboard
        #[arg(long)]
        headless: bool,
        /// Force plain log output even on a real terminal
        #[arg(long)]
        plain: bool,
    },
    /// Serve the dashboard only, attached to an already-running server
    Dashboard {
        /// Open the dashboard in a browser
        #[arg(long)]
        open: bool,
    },
    /// Manage MCP client integrations
    Mcp {
        #[command(subcommand)]
        action: McpAction,
    },
    /// Manage Mynd as a background service
    Service {
        #[command(subcommand)]
        action: ServiceAction,
    },
    /// Matrix chat interface: capture/recall Mynd memories from a room or DM
    Matrix {
        #[command(subcommand)]
        action: MatrixAction,
    },
    /// Migrate the database from the legacy ~/.hivemind/ path to XDG data dir
    Migrate,
    /// Print the session-start memory context for the current project (for hooks and scripts)
    SessionStart {
        /// Emit machine-readable JSON instead of tagged text
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum ServiceAction {
    /// Install and enable Mynd as a user-level background service
    Install {
        /// Also serve the dashboard from the background service
        #[arg(long)]
        dashboard: bool,
        /// Also install the Matrix bot unit (requires `mynd matrix login` first)
        #[arg(long)]
        matrix: bool,
    },
    /// Stop and remove the Mynd background service
    Uninstall,
    /// Show the status of the Mynd background service
    Status,
}

#[derive(Subcommand)]
pub enum MatrixAction {
    /// Log into a Matrix account once; persists the session to the OS keyring
    Login,
    /// Run the Matrix bot daemon (requires `mynd matrix login` first)
    Run {
        /// Print verbose connection/message logs to stderr
        #[arg(long)]
        debug: bool,
    },
    /// Show whether the daemon is running and its sync/session state
    Status,
    /// Send a one-off DM to a user (connectivity smoke test, no daemon needed)
    Send {
        /// Recipient's Matrix user ID (e.g. @oxgrad:matrix.org)
        user_id: String,
        /// Message text to send
        message: String,
    },
}

#[derive(Subcommand)]
pub enum McpAction {
    /// Register Mynd as an MCP server in a supported AI coding client
    Install {
        /// Client to register with: claude, opencode, kimi, codex, cursor, windsurf
        client: String,
    },
}

mod init;
mod matrix_cmds;
mod mcp_install;
mod service;
mod status;
#[cfg(test)]
mod tests;

pub use init::*;
pub use matrix_cmds::*;
pub use mcp_install::*;
pub use service::*;
pub use status::*;
