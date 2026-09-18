use anyhow::Result;
use clap::Subcommand;
use serde::Deserialize;
use serde_json::{Value, json};
use std::path::PathBuf;

use super::common::{block_on, confirm, open_store};

#[derive(Subcommand)]
pub enum DataAction {
    /// Export personal and workspace memories + edges to JSON (org-layer memories are not included)
    Export {
        /// Write to this file instead of stdout
        #[arg(long)]
        output: Option<PathBuf>,
    },
    /// Import memories + edges from a previous export
    Import {
        /// Path to a JSON file previously produced by `mynd data export`
        input: PathBuf,
    },
    /// Permanently delete all memories, edges, feedback, and conflicts
    Wipe {
        /// Skip the confirmation prompt
        #[arg(long)]
        yes: bool,
    },
}

fn entry_json(e: &crate::store::MemoryEntry) -> Value {
    json!({
        "id": e.id,
        "title": e.title,
        "content": e.content,
        "tags": e.tags,
        "created_at": e.created_at,
        "updated_at": e.updated_at,
        "token_count": e.token_count,
        "layer": e.layer,
        "memory_type": e.memory_type,
    })
}

#[derive(Deserialize, Default)]
struct ImportBody {
    #[serde(default)]
    memories: Vec<ImportMemory>,
    #[serde(default)]
    edges: Vec<ImportEdge>,
}

#[derive(Deserialize)]
struct ImportMemory {
    id: String,
    title: String,
    content: String,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    token_count: Option<i64>,
    #[serde(default = "default_layer")]
    layer: String,
    #[serde(default = "default_memory_type")]
    memory_type: String,
}

fn default_layer() -> String {
    "workspace".into()
}

fn default_memory_type() -> String {
    "project".into()
}

#[derive(Deserialize)]
struct ImportEdge {
    source_id: String,
    target_id: String,
    relationship: String,
    #[serde(default = "default_edge_status")]
    status: String,
    #[serde(default)]
    link_text: Option<String>,
    #[serde(default)]
    reason: Option<String>,
}

fn default_edge_status() -> String {
    "active".into()
}

pub fn cmd_data(action: DataAction) -> Result<()> {
    match action {
        DataAction::Export { output } => cmd_export(output),
        DataAction::Import { input } => cmd_import(input),
        DataAction::Wipe { yes } => cmd_wipe(yes),
    }
}

fn cmd_export(output: Option<PathBuf>) -> Result<()> {
    block_on(async {
        let store = open_store().await?;
        let memories = store.list_memories(100_000, 0).await?;
        let edges = store.list_edges(None).await?;
        let body = json!({
            "version": env!("CARGO_PKG_VERSION"),
            "exported_at": std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            "memories": memories.iter().map(entry_json).collect::<Vec<_>>(),
            "edges": edges,
        });
        let text = serde_json::to_string_pretty(&body)?;
        match output {
            Some(path) => {
                std::fs::write(&path, text)?;
                println!(
                    "exported {} memories, {} edges to {}",
                    memories.len(),
                    edges.len(),
                    path.display()
                );
            }
            None => println!("{text}"),
        }
        Ok(())
    })
}

fn cmd_import(input: PathBuf) -> Result<()> {
    block_on(async {
        let text = std::fs::read_to_string(&input)?;
        let body: ImportBody = serde_json::from_str(&text)?;
        let store = open_store().await?;
        let mut mem_count = 0usize;
        for m in &body.memories {
            store
                .store(&crate::store::NewMemoryRow {
                    id: &m.id,
                    title: &m.title,
                    content: &m.content,
                    tags: &m.tags,
                    token_count: m.token_count,
                    layer: &m.layer,
                    memory_type: &m.memory_type,
                })
                .await?;
            mem_count += 1;
        }
        let mut edge_count = 0usize;
        for e in &body.edges {
            if !["active", "pending", "rejected"].contains(&e.status.as_str()) {
                continue;
            }
            if matches!(
                store
                    .create_edge_with_status(
                        &e.source_id,
                        &e.target_id,
                        &e.relationship,
                        &e.status,
                        e.link_text.as_deref(),
                        e.reason.as_deref(),
                    )
                    .await?,
                crate::model::EdgeCreate::Created(_)
            ) {
                edge_count += 1;
            }
        }
        println!("imported {mem_count} memories, {edge_count} edges");
        Ok(())
    })
}

fn cmd_wipe(yes: bool) -> Result<()> {
    if !confirm(
        "Permanently delete all memories, edges, feedback, and conflicts?",
        yes,
    )? {
        println!("Cancelled.");
        return Ok(());
    }
    block_on(async {
        let store = open_store().await?;
        let deleted = store.delete_all().await?;
        println!("deleted {deleted} memories (and their edges/feedback/conflicts)");
        Ok(())
    })
}
