use anyhow::{Result, anyhow};
use clap::Subcommand;
use serde_json::json;

use super::common::{block_on, open_store, print_json};

#[derive(Subcommand)]
pub enum LimitsAction {
    /// Show the max-content-tokens guardrail
    Show {
        /// Emit machine-readable JSON instead of a text summary
        #[arg(long)]
        json: bool,
    },
    /// Set the max-content-tokens guardrail — a single memory's title+content
    /// can't exceed this many tokens, enforced for AI agents too
    Set {
        /// The new max-content-tokens value; must be a positive integer
        tokens: i64,
    },
}

pub fn cmd_limits(action: LimitsAction) -> Result<()> {
    match action {
        LimitsAction::Show { json } => block_on(async {
            let store = open_store().await?;
            let tokens = store.max_content_tokens().await;
            if json {
                print_json(&json!({ "max_content_tokens": tokens }));
            } else {
                println!("max_content_tokens: {tokens}");
            }
            Ok(())
        }),
        LimitsAction::Set { tokens } => block_on(async move {
            if tokens <= 0 {
                return Err(anyhow!("max_content_tokens must be a positive integer"));
            }
            let store = open_store().await?;
            store
                .set_meta("max_content_tokens", &tokens.to_string())
                .await?;
            println!("max_content_tokens: {tokens}");
            Ok(())
        }),
    }
}
