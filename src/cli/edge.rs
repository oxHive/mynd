use anyhow::{Result, anyhow};
use clap::Subcommand;
use serde_json::json;

use super::common::{block_on, open_org_store, open_store, print_json};
use crate::model::EdgeCreate;
use crate::store::EdgeEntry;

#[derive(Subcommand)]
pub enum EdgeAction {
    /// List edges (the memory relationship graph), optionally scoped to one memory
    List {
        /// Only show edges touching this memory id
        #[arg(long)]
        memory_id: Option<String>,
        /// Filter by status: active|pending|rejected
        #[arg(long)]
        status: Option<String>,
        /// Emit machine-readable JSON instead of one line per edge
        #[arg(long)]
        json: bool,
    },
    /// Create an edge between two memories
    Add {
        /// The source memory's id, e.g. mem_xxxxxxxx
        source_id: String,
        /// The target memory's id, e.g. mem_xxxxxxxx
        target_id: String,
        /// parent|child|sibling
        relationship: String,
    },
    /// Set an edge's status directly: active|pending|rejected
    Status {
        /// The edge's id, e.g. edge_xxxxxxxx
        id: String,
        /// New status: active|pending|rejected
        status: String,
    },
    /// Approve a pending (e.g. AI-suggested) edge — sets it active
    Approve {
        /// The edge's id, e.g. edge_xxxxxxxx
        id: String,
    },
    /// Reject a pending edge
    Reject {
        /// The edge's id, e.g. edge_xxxxxxxx
        id: String,
    },
}

fn print_edge_line(e: &EdgeEntry) {
    println!(
        "{}  {} --[{}]--> {}  ({}){}",
        e.id,
        e.source_id,
        e.relationship,
        e.target_id,
        e.status,
        e.reason
            .as_deref()
            .map(|r| format!("  {r}"))
            .unwrap_or_default()
    );
}

pub fn cmd_edge(action: EdgeAction) -> Result<()> {
    match action {
        EdgeAction::List {
            memory_id,
            status,
            json,
        } => cmd_list(memory_id, status, json),
        EdgeAction::Add {
            source_id,
            target_id,
            relationship,
        } => cmd_add(source_id, target_id, relationship),
        EdgeAction::Status { id, status } => cmd_set_status(id, status),
        EdgeAction::Approve { id } => cmd_set_status(id, "active".to_string()),
        EdgeAction::Reject { id } => cmd_set_status(id, "rejected".to_string()),
    }
}

fn cmd_list(memory_id: Option<String>, status: Option<String>, json: bool) -> Result<()> {
    block_on(async {
        let store = open_store().await?;
        let org_store = open_org_store().await;
        let mut edges = store.list_edges(memory_id.as_deref()).await?;
        if let Some(org) = &org_store {
            let mut org_edges = org.list_edges(memory_id.as_deref()).await?;
            edges.append(&mut org_edges);
        }
        if let Some(status) = &status {
            edges.retain(|e| &e.status == status);
        }
        if json {
            print_json(&json!({ "count": edges.len(), "edges": edges }));
        } else if edges.is_empty() {
            println!("No edges.");
        } else {
            for e in &edges {
                print_edge_line(e);
            }
        }
        Ok(())
    })
}

fn cmd_add(source_id: String, target_id: String, relationship: String) -> Result<()> {
    block_on(async {
        let store = open_store().await?;
        let org_store = open_org_store().await;
        let primary = store
            .create_edge(&source_id, &target_id, &relationship)
            .await?;
        let result = match (&primary, &org_store) {
            (EdgeCreate::MissingEndpoint, Some(org)) => {
                org.create_edge(&source_id, &target_id, &relationship)
                    .await?
            }
            _ => primary,
        };
        match result {
            EdgeCreate::Created(id) => {
                println!("created {id}");
                Ok(())
            }
            EdgeCreate::Duplicate => Err(anyhow!("edge already exists")),
            EdgeCreate::MissingEndpoint => Err(anyhow!(
                "source_id and target_id must be existing, distinct memory IDs"
            )),
            EdgeCreate::InvalidRelationship => Err(anyhow!(
                "invalid relationship; valid: {}",
                crate::store::VALID_RELATIONSHIPS.join(", ")
            )),
        }
    })
}

fn cmd_set_status(id: String, status: String) -> Result<()> {
    if !["active", "pending", "rejected"].contains(&status.as_str()) {
        return Err(anyhow!("status must be active|pending|rejected"));
    }
    block_on(async {
        let store = open_store().await?;
        let org_store = open_org_store().await;
        let updated = if store.set_edge_status(&id, &status).await? {
            true
        } else if let Some(org) = &org_store {
            org.set_edge_status(&id, &status).await?
        } else {
            false
        };
        if !updated {
            return Err(anyhow!("no edge {id}"));
        }
        println!("{id} -> {status}");
        Ok(())
    })
}
