use anyhow::{Result, anyhow};
use clap::Subcommand;
use serde_json::{Value, json};

use super::common::{block_on, confirm, find_owning, open_org_store, open_store, print_json};
use crate::store::MemoryEntry;

#[derive(Subcommand)]
pub enum MemoryAction {
    /// List memories
    List {
        /// Maximum number of memories to return
        #[arg(long, default_value_t = 200)]
        limit: i64,
        /// Number of memories to skip, for pagination
        #[arg(long, default_value_t = 0)]
        offset: i64,
        /// Filter by a tag expression, e.g. "tag:topic:sync" or "tag:status:done & tag:project:mynd"
        #[arg(long)]
        tag: Option<String>,
        /// Emit machine-readable JSON instead of one line per memory
        #[arg(long)]
        json: bool,
    },
    /// Show one memory by id
    Get {
        /// The memory's id, e.g. mem_xxxxxxxx
        id: String,
        /// Emit machine-readable JSON instead of a text summary
        #[arg(long)]
        json: bool,
    },
    /// Full-text search across memories
    Search {
        /// Search text, or a tag expression like "tag:topic:sync"
        query: String,
        /// Maximum number of results to return
        #[arg(long, default_value_t = 20)]
        limit: i64,
        /// Emit machine-readable JSON instead of one line per result
        #[arg(long)]
        json: bool,
    },
    /// Create a new memory
    Add {
        /// The memory's title
        #[arg(long)]
        title: String,
        /// The memory's body content
        #[arg(long)]
        content: String,
        /// Tag, repeatable: --tag topic:sync --tag status:done
        #[arg(long = "tag")]
        tags: Vec<String>,
        /// Which layer to store it in: personal|workspace|org
        #[arg(long, default_value = "workspace")]
        layer: String,
        /// The memory's type: preference|project|history
        #[arg(long = "type", default_value = "project")]
        memory_type: String,
    },
    /// Edit a memory's title, content, and/or tags (tags replace the full set)
    Edit {
        /// The memory's id, e.g. mem_xxxxxxxx
        id: String,
        /// New title; omit to leave unchanged
        #[arg(long)]
        title: Option<String>,
        /// New content; omit to leave unchanged
        #[arg(long)]
        content: Option<String>,
        /// Replaces the full tag set; repeatable: --tag a --tag b
        #[arg(long = "tag")]
        tags: Option<Vec<String>>,
    },
    /// Add tags to a memory without touching the rest
    TagAdd {
        /// The memory's id, e.g. mem_xxxxxxxx
        id: String,
        /// Tags to add, e.g. topic:sync status:done
        tags: Vec<String>,
    },
    /// Remove tags from a memory without touching the rest
    TagRemove {
        /// The memory's id, e.g. mem_xxxxxxxx
        id: String,
        /// Tags to remove, e.g. topic:sync status:done
        tags: Vec<String>,
    },
    /// Delete one memory
    Rm {
        /// The memory's id, e.g. mem_xxxxxxxx
        id: String,
        /// Skip the confirmation prompt
        #[arg(long)]
        yes: bool,
    },
}

fn entry_json(e: &MemoryEntry) -> Value {
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

fn print_entry_line(e: &MemoryEntry) {
    println!(
        "{}  [{}/{}]  {}{}",
        e.id,
        e.layer,
        e.memory_type,
        e.title,
        if e.tags.is_empty() {
            String::new()
        } else {
            format!("  ({})", e.tags.join(", "))
        }
    );
}

fn print_entry_detail(e: &MemoryEntry) {
    println!("{}", e.id);
    println!("title:   {}", e.title);
    println!("layer:   {}/{}", e.layer, e.memory_type);
    println!(
        "tags:    {}",
        if e.tags.is_empty() {
            "(none)".to_string()
        } else {
            e.tags.join(", ")
        }
    );
    println!("created: {}", e.created_at);
    println!("updated: {}", e.updated_at);
    println!();
    println!("{}", e.content);
}

pub fn cmd_memory(action: MemoryAction) -> Result<()> {
    match action {
        MemoryAction::List {
            limit,
            offset,
            tag,
            json,
        } => cmd_list(limit, offset, tag, json),
        MemoryAction::Get { id, json } => cmd_get(id, json),
        MemoryAction::Search { query, limit, json } => cmd_search(query, limit, json),
        MemoryAction::Add {
            title,
            content,
            tags,
            layer,
            memory_type,
        } => cmd_add(title, content, tags, layer, memory_type),
        MemoryAction::Edit {
            id,
            title,
            content,
            tags,
        } => cmd_edit(id, title, content, tags),
        MemoryAction::TagAdd { id, tags } => cmd_tag_add(id, tags),
        MemoryAction::TagRemove { id, tags } => cmd_tag_remove(id, tags),
        MemoryAction::Rm { id, yes } => cmd_rm(id, yes),
    }
}

fn cmd_list(limit: i64, offset: i64, tag: Option<String>, json: bool) -> Result<()> {
    block_on(async {
        let store = open_store().await?;
        let org_store = open_org_store().await;
        let mut entries = if let Some(expr) = &tag {
            let parsed = crate::tag_query::parse(expr).map_err(|e| anyhow!("{e}"))?;
            store.find_by_tag_expr(&parsed).await?
        } else {
            store
                .list_memories(limit.clamp(1, 1000), offset.max(0))
                .await?
        };
        if let Some(org) = &org_store {
            let mut org_entries = if let Some(expr) = &tag {
                let parsed = crate::tag_query::parse(expr).map_err(|e| anyhow!("{e}"))?;
                org.find_by_tag_expr(&parsed).await?
            } else {
                org.list_memories(limit.clamp(1, 1000), offset.max(0))
                    .await?
            };
            entries.append(&mut org_entries);
        }
        if json {
            print_json(&json!({
                "count": entries.len(),
                "memories": entries.iter().map(entry_json).collect::<Vec<_>>(),
            }));
        } else if entries.is_empty() {
            println!("No memories.");
        } else {
            for e in &entries {
                print_entry_line(e);
            }
        }
        Ok(())
    })
}

fn cmd_get(id: String, json: bool) -> Result<()> {
    block_on(async {
        let store = open_store().await?;
        let org_store = open_org_store().await;
        let owning = find_owning(&store, &org_store, &id).await?;
        let entry = match owning {
            Some(s) => s.recall_by_id(&id).await?,
            None => None,
        }
        .ok_or_else(|| anyhow!("no memory {id}"))?;
        if json {
            print_json(&entry_json(&entry));
        } else {
            print_entry_detail(&entry);
        }
        Ok(())
    })
}

fn cmd_search(query: String, limit: i64, json: bool) -> Result<()> {
    block_on(async {
        let store = open_store().await?;
        let org_store = open_org_store().await;
        let limit = limit.clamp(1, 50);
        let mut hits = if crate::tag_query::looks_like_tag_expr(&query) {
            let expr = crate::tag_query::parse(&query).map_err(|e| anyhow!("{e}"))?;
            let mut m = store.find_by_tag_expr(&expr).await?;
            m.truncate(limit as usize);
            m
        } else {
            store.search(&query, limit).await?
        };
        if hits.len() < limit as usize
            && let Some(org) = &org_store
        {
            let remaining = limit - hits.len() as i64;
            let mut org_hits = if crate::tag_query::looks_like_tag_expr(&query) {
                let expr = crate::tag_query::parse(&query).map_err(|e| anyhow!("{e}"))?;
                org.find_by_tag_expr(&expr).await?
            } else {
                org.search(&query, remaining).await?
            };
            org_hits.truncate(remaining as usize);
            hits.append(&mut org_hits);
        }
        if json {
            print_json(&json!({
                "count": hits.len(),
                "results": hits.iter().map(entry_json).collect::<Vec<_>>(),
            }));
        } else if hits.is_empty() {
            println!("No matches.");
        } else {
            for e in &hits {
                print_entry_line(e);
            }
        }
        Ok(())
    })
}

fn cmd_add(
    title: String,
    content: String,
    tags: Vec<String>,
    layer: String,
    memory_type: String,
) -> Result<()> {
    block_on(async {
        let layer: crate::model::Layer = layer.parse()?;
        let memory_type: crate::model::MemoryType = memory_type.parse()?;
        let store = open_store().await?;
        let org_store = open_org_store().await;
        let target = if matches!(layer, crate::model::Layer::Org) {
            org_store.as_ref().ok_or_else(|| {
                anyhow!("org layer not configured — set [org_sync] in the global config")
            })?
        } else {
            &store
        };
        let id = format!("mem_{}", uuid::Uuid::new_v4().simple());
        target
            .store(&crate::store::NewMemoryRow {
                id: &id,
                title: &title,
                content: &content,
                tags: &tags,
                token_count: None,
                layer: &layer.to_string(),
                memory_type: &memory_type.to_string(),
            })
            .await?;
        println!("created {id}");
        Ok(())
    })
}

fn cmd_edit(
    id: String,
    title: Option<String>,
    content: Option<String>,
    tags: Option<Vec<String>>,
) -> Result<()> {
    block_on(async {
        let store = open_store().await?;
        let org_store = open_org_store().await;
        let owning = find_owning(&store, &org_store, &id)
            .await?
            .ok_or_else(|| anyhow!("no memory {id}"))?;
        let current = owning
            .recall_by_id(&id)
            .await?
            .ok_or_else(|| anyhow!("no memory {id}"))?;
        let title = title.as_deref().unwrap_or(&current.title);
        let content = content.as_deref().unwrap_or(&current.content);
        let tags = tags.as_deref().unwrap_or(&current.tags);
        if !owning.update(&id, title, content, tags).await? {
            return Err(anyhow!("no memory {id}"));
        }
        println!("updated {id}");
        Ok(())
    })
}

fn cmd_tag_add(id: String, tags: Vec<String>) -> Result<()> {
    block_on(async {
        let store = open_store().await?;
        let org_store = open_org_store().await;
        let owning = find_owning(&store, &org_store, &id)
            .await?
            .ok_or_else(|| anyhow!("no memory {id}"))?;
        if !owning.add_tags(&id, &tags).await? {
            return Err(anyhow!("no memory {id}"));
        }
        println!("tagged {id}");
        Ok(())
    })
}

fn cmd_tag_remove(id: String, tags: Vec<String>) -> Result<()> {
    block_on(async {
        let store = open_store().await?;
        let org_store = open_org_store().await;
        let owning = find_owning(&store, &org_store, &id)
            .await?
            .ok_or_else(|| anyhow!("no memory {id}"))?;
        if !owning.remove_tags(&id, &tags).await? {
            return Err(anyhow!("no memory {id}"));
        }
        println!("untagged {id}");
        Ok(())
    })
}

fn cmd_rm(id: String, yes: bool) -> Result<()> {
    if !confirm(&format!("Delete memory {id}?"), yes)? {
        println!("Cancelled.");
        return Ok(());
    }
    block_on(async {
        let store = open_store().await?;
        let org_store = open_org_store().await;
        let owning = find_owning(&store, &org_store, &id)
            .await?
            .ok_or_else(|| anyhow!("no memory {id}"))?;
        if !owning.delete(&id).await? {
            return Err(anyhow!("no memory {id}"));
        }
        println!("deleted {id}");
        Ok(())
    })
}
