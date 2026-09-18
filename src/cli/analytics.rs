use anyhow::Result;
use serde_json::{Value, json};
use std::collections::HashMap;

use super::common::{block_on, open_org_store, open_store, print_json};
use crate::store::MemoryEntry;

const BAR_WIDTH: usize = 30;

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn day_of(created_at: i64) -> String {
    chrono::DateTime::from_timestamp(created_at, 0)
        .map(|d| d.format("%Y-%m-%d").to_string())
        .unwrap_or_default()
}

fn project_tag_of(tags: &[String]) -> Option<String> {
    tags.iter()
        .find(|t| t.to_lowercase().starts_with("project:"))
        .map(|t| t["project:".len()..].to_string())
}

/// Counts occurrences of `key_of(entry)` across all entries — `None` skips
/// the entry (e.g. a memory with no project:* tag doesn't count anywhere).
fn counts_by<'a, F>(entries: &'a [MemoryEntry], key_of: F) -> HashMap<String, i64>
where
    F: Fn(&'a MemoryEntry) -> Option<String>,
{
    let mut counts = HashMap::new();
    for e in entries {
        if let Some(k) = key_of(e) {
            *counts.entry(k).or_insert(0) += 1;
        }
    }
    counts
}

/// Like `counts_by`, but for a field that can hold several values per entry
/// (tags) rather than at most one.
fn multi_counts_by<'a, F>(entries: &'a [MemoryEntry], values_of: F) -> HashMap<String, i64>
where
    F: Fn(&'a MemoryEntry) -> &'a [String],
{
    let mut counts = HashMap::new();
    for e in entries {
        for v in values_of(e) {
            *counts.entry(v.clone()).or_insert(0) += 1;
        }
    }
    counts
}

fn sorted_desc(counts: HashMap<String, i64>) -> Vec<(String, i64)> {
    let mut v: Vec<(String, i64)> = counts.into_iter().collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    v
}

fn print_bars(rows: &[(String, i64)]) {
    let max = rows.iter().map(|(_, c)| *c).max().unwrap_or(1).max(1);
    let label_width = rows
        .iter()
        .map(|(l, _)| l.chars().count())
        .max()
        .unwrap_or(0);
    for (label, count) in rows {
        let filled = ((*count as f64 / max as f64) * BAR_WIDTH as f64).round() as usize;
        let filled = filled.clamp(if *count > 0 { 1 } else { 0 }, BAR_WIDTH);
        println!(
            "  {:width$}  {}{}  {}",
            label,
            "█".repeat(filled),
            " ".repeat(BAR_WIDTH - filled),
            count,
            width = label_width
        );
    }
}

fn counts_to_json(rows: &[(String, i64)], key_name: &str) -> Vec<Value> {
    rows.iter()
        .map(|(k, v)| json!({ key_name: k, "count": v }))
        .collect()
}

pub fn cmd_analytics(json: bool, days: i64, limit: i64) -> Result<()> {
    block_on(async move {
        let store = open_store().await?;
        let org_store = open_org_store().await;

        let mut entries = store.list_memories(100_000, 0).await?;
        if let Some(org) = &org_store {
            let mut org_entries = org.list_memories(100_000, 0).await?;
            entries.append(&mut org_entries);
        }

        let total_memories = entries.len() as i64;
        let week_ago = now_unix() - 7 * 24 * 60 * 60;
        let added_last_7_days = entries.iter().filter(|e| e.created_at >= week_ago).count() as i64;

        let tag_counts = sorted_desc(multi_counts_by(&entries, |e| e.tags.as_slice()));
        let distinct_tags = tag_counts.len() as i64;

        let type_counts = sorted_desc(counts_by(&entries, |e| {
            Some(if e.memory_type.is_empty() {
                "unknown".to_string()
            } else {
                e.memory_type.clone()
            })
        }));

        let project_counts = sorted_desc(counts_by(&entries, |e| project_tag_of(&e.tags)));
        let total_projects = project_counts.len() as i64;

        let cutoff_day = day_of(now_unix() - days.max(0) * 24 * 60 * 60);
        let day_totals = counts_by(&entries, |e| Some(day_of(e.created_at)));
        let mut activity_by_day: Vec<(String, i64)> = day_totals
            .into_iter()
            .filter(|(day, _)| day.as_str() >= cutoff_day.as_str())
            .collect();
        activity_by_day.sort_by(|a, b| a.0.cmp(&b.0));

        let session_logs = store.list_session_logs(limit.clamp(1, 200)).await?;

        if json {
            print_json(&json!({
                "total_memories": total_memories,
                "distinct_tags": distinct_tags,
                "total_projects": total_projects,
                "added_last_7_days": added_last_7_days,
                "tag_counts": counts_to_json(&tag_counts, "tag"),
                "type_counts": counts_to_json(&type_counts, "type"),
                "project_counts": counts_to_json(&project_counts, "project"),
                "activity_by_day": counts_to_json(&activity_by_day, "day"),
                "session_logs": session_logs,
            }));
            return Ok(());
        }

        println!("Total memories:      {total_memories}");
        println!("Distinct tags:       {distinct_tags}");
        println!("Projects:            {total_projects}");
        println!("Added last 7 days:   {added_last_7_days}");

        println!("\nTop tags:");
        if tag_counts.is_empty() {
            println!("  No tags yet.");
        } else {
            print_bars(&tag_counts[..tag_counts.len().min(10)]);
        }

        println!("\nMemory types:");
        if type_counts.is_empty() {
            println!("  No memories yet.");
        } else {
            print_bars(&type_counts);
        }

        println!("\nBy project:");
        if project_counts.is_empty() {
            println!("  No project-tagged memories yet — add a project:* tag to see it here.");
        } else {
            print_bars(&project_counts);
        }

        println!("\nActivity by day (last {days} days):");
        if activity_by_day.is_empty() {
            println!("  No activity recorded yet.");
        } else {
            print_bars(&activity_by_day);
        }

        println!("\nRecall sessions (last {}):", session_logs.len());
        if session_logs.is_empty() {
            println!("  No session-start runs logged yet.");
        } else {
            for log in &session_logs {
                let when = chrono::DateTime::from_timestamp(log.created_at, 0)
                    .map(|d| d.format("%Y-%m-%d %H:%M").to_string())
                    .unwrap_or_default();
                let flag = if log.truncated { "  [truncated]" } else { "" };
                println!(
                    "  {:<20} {:<16} {}/{} tok{}",
                    log.project_name, when, log.used_tokens, log.max_tokens, flag
                );
            }
        }

        Ok(())
    })
}
