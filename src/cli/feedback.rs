use anyhow::{Result, anyhow};
use clap::Subcommand;
use serde_json::json;

use super::common::{block_on, open_org_store, open_store, print_json};
use crate::store::{ConflictEntry, FeedbackEntry};

#[derive(Subcommand)]
pub enum FeedbackAction {
    /// List flagged-memory feedback
    List {
        /// Only show feedback for this memory id
        #[arg(long)]
        memory_id: Option<String>,
        /// Filter by status: pending|resolved|dismissed
        #[arg(long)]
        status: Option<String>,
        /// Emit machine-readable JSON instead of one line per item
        #[arg(long)]
        json: bool,
    },
    /// Flag a memory for review
    Add {
        /// The memory's id, e.g. mem_xxxxxxxx
        memory_id: String,
        /// incorrect|outdated|duplicate|other (free text also accepted)
        signal: String,
        /// Optional free-text note explaining the flag
        #[arg(long)]
        note: Option<String>,
    },
    /// Mark feedback resolved
    Resolve {
        /// The feedback's id, e.g. fb_xxxxxxxx
        id: String,
    },
    /// Dismiss feedback without acting on it
    Dismiss {
        /// The feedback's id, e.g. fb_xxxxxxxx
        id: String,
    },
}

#[derive(Subcommand)]
pub enum ConflictAction {
    /// List sync conflicts (a remote sync overwrote a local edit)
    List {
        /// Filter by status: pending|keep_local|keep_remote
        #[arg(long)]
        status: Option<String>,
        /// Emit machine-readable JSON instead of one line per conflict
        #[arg(long)]
        json: bool,
    },
    /// Resolve a conflict by keeping the local or remote version
    Resolve {
        /// The conflict's id, e.g. conflict_xxxxxxxx
        id: String,
        /// keep-local|keep-remote
        resolution: String,
    },
}

fn print_feedback_line(f: &FeedbackEntry) {
    println!(
        "{}  [{}]  {}  {}{}",
        f.id,
        f.status,
        f.memory_id,
        f.signal,
        f.note
            .as_deref()
            .map(|n| format!("  — {n}"))
            .unwrap_or_default()
    );
}

fn print_conflict_line(c: &ConflictEntry) {
    println!("{}  [{}]  memory {}", c.id, c.status, c.memory_id);
}

pub fn cmd_feedback(action: FeedbackAction) -> Result<()> {
    match action {
        FeedbackAction::List {
            memory_id,
            status,
            json,
        } => block_on(async {
            let store = open_store().await?;
            let items = store
                .list_feedback(memory_id.as_deref(), status.as_deref())
                .await?;
            if json {
                print_json(&json!({ "count": items.len(), "items": items }));
            } else if items.is_empty() {
                println!("No feedback.");
            } else {
                for f in &items {
                    print_feedback_line(f);
                }
            }
            Ok(())
        }),
        FeedbackAction::Add {
            memory_id,
            signal,
            note,
        } => block_on(async {
            let store = open_store().await?;
            let entry = store
                .create_feedback(&memory_id, &signal, note.as_deref())
                .await?;
            println!("created {}", entry.id);
            Ok(())
        }),
        FeedbackAction::Resolve { id } => set_feedback_status(id, "resolved"),
        FeedbackAction::Dismiss { id } => set_feedback_status(id, "dismissed"),
    }
}

fn set_feedback_status(id: String, status: &str) -> Result<()> {
    block_on(async {
        let store = open_store().await?;
        if !store.set_feedback_status(&id, status).await? {
            return Err(anyhow!("no feedback {id}"));
        }
        println!("{id} -> {status}");
        Ok(())
    })
}

pub fn cmd_conflict(action: ConflictAction) -> Result<()> {
    match action {
        ConflictAction::List { status, json } => block_on(async {
            let store = open_store().await?;
            let org_store = open_org_store().await;
            let mut items = store.list_conflicts(status.as_deref()).await?;
            if let Some(org) = &org_store {
                let mut org_items = org.list_conflicts(status.as_deref()).await?;
                items.append(&mut org_items);
            }
            if json {
                print_json(&json!({ "count": items.len(), "conflicts": items }));
            } else if items.is_empty() {
                println!("No conflicts.");
            } else {
                for c in &items {
                    print_conflict_line(c);
                }
            }
            Ok(())
        }),
        ConflictAction::Resolve { id, resolution } => {
            let resolution = match resolution.as_str() {
                "keep-local" | "keep_local" => "keep_local",
                "keep-remote" | "keep_remote" => "keep_remote",
                _ => return Err(anyhow!("resolution must be keep-local|keep-remote")),
            };
            block_on(async {
                let store = open_store().await?;
                let org_store = open_org_store().await;
                let resolved = if store.resolve_conflict(&id, resolution).await? {
                    true
                } else if let Some(org) = &org_store {
                    org.resolve_conflict(&id, resolution).await?
                } else {
                    false
                };
                if !resolved {
                    return Err(anyhow!("conflict {id} not found or already resolved"));
                }
                println!("{id} -> {resolution}");
                Ok(())
            })
        }
    }
}
