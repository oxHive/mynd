use anyhow::{Result, anyhow};
use clap::Subcommand;
use serde_json::{Value, json};

use super::common::{block_on, open_store, print_json};

#[derive(Subcommand)]
pub enum TagsAction {
    /// Show the tag namespace registry (colors, values, descriptions)
    List {
        /// Emit machine-readable JSON instead of a text summary
        #[arg(long)]
        json: bool,
    },
    /// Create a new tag namespace
    Add {
        /// The namespace name, e.g. "topic" for tags like topic:sync
        name: String,
        /// Hex color shown for this namespace's tags, e.g. #4a9eff
        #[arg(long, default_value = "#4a9eff")]
        color: String,
        /// What this namespace means — shown to AI agents and in the dashboard
        #[arg(long)]
        description: Option<String>,
        /// Restrict memories to at most one tag in this namespace
        #[arg(long)]
        single_value: bool,
        /// suggestion|fixed — "fixed" enforces the value list, "suggestion" is advisory only
        #[arg(long, default_value = "suggestion")]
        values_mode: String,
    },
    /// Edit an existing namespace's color/description/single-value/values-mode
    Set {
        /// The namespace name to edit
        name: String,
        /// New hex color; omit to leave unchanged
        #[arg(long)]
        color: Option<String>,
        /// New description; omit to leave unchanged
        #[arg(long)]
        description: Option<String>,
        /// Restrict memories to at most one tag in this namespace; omit to leave unchanged
        #[arg(long)]
        single_value: Option<bool>,
        /// suggestion|fixed; omit to leave unchanged
        #[arg(long)]
        values_mode: Option<String>,
    },
    /// Delete a namespace (predefined namespaces are guarded by default)
    Rm {
        /// The namespace name to delete
        name: String,
    },
    /// Add an accepted/suggested value to a namespace
    ValueAdd {
        /// The namespace name
        name: String,
        /// The value to add, e.g. "sync" for topic:sync
        value: String,
    },
    /// Remove a value from a namespace
    ValueRemove {
        /// The namespace name
        name: String,
        /// The value to remove
        value: String,
    },
}

/// Mirrors `api::settings::validate_tag_namespaces` — each entry needs a
/// string `color` and a string-array `values`, with optional `single_value`
/// (bool), `description` (string), and `values_mode` ("suggestion"|"fixed").
fn validate_tag_namespaces(body: &Value) -> Result<()> {
    let obj = body
        .as_object()
        .ok_or_else(|| anyhow!("tag settings must be a JSON object"))?;
    for (ns, entry) in obj {
        let entry_obj = entry
            .as_object()
            .ok_or_else(|| anyhow!("namespace {ns:?} must be an object"))?;
        match entry_obj.get("color") {
            Some(Value::String(_)) => {}
            _ => return Err(anyhow!("namespace {ns:?} missing string \"color\"")),
        }
        match entry_obj.get("values") {
            Some(Value::Array(vals)) if vals.iter().all(|v| v.is_string()) => {}
            _ => return Err(anyhow!("namespace {ns:?} missing string array \"values\"")),
        }
        if let Some(sv) = entry_obj.get("single_value")
            && !sv.is_boolean()
        {
            return Err(anyhow!("namespace {ns:?} \"single_value\" must be a bool"));
        }
        if let Some(d) = entry_obj.get("description")
            && !d.is_string()
        {
            return Err(anyhow!("namespace {ns:?} \"description\" must be a string"));
        }
        if let Some(vm) = entry_obj.get("values_mode")
            && vm.as_str() != Some("suggestion")
            && vm.as_str() != Some("fixed")
        {
            return Err(anyhow!(
                "namespace {ns:?} \"values_mode\" must be \"suggestion\" or \"fixed\""
            ));
        }
    }
    Ok(())
}

/// Mirrors `api::settings::validate_predefined_namespaces_unchanged`.
fn validate_predefined_namespaces_unchanged(body: &Value) -> Result<()> {
    let defaults = crate::store::default_tag_namespaces();
    let names = defaults
        .as_object()
        .expect("default_tag_namespaces returns an object")
        .keys();
    for name in names {
        if body.get(name) != defaults.get(name) {
            return Err(anyhow!(
                "namespace {name:?} is predefined and cannot be deleted or modified. \
                 Disable this guard with [tags] guard_predefined_namespaces = false \
                 in the global mynd config to allow it."
            ));
        }
    }
    Ok(())
}

async fn save_registry(store: &crate::store::SqliteStore, body: Value) -> Result<()> {
    let settings = crate::config::load_server_settings(&crate::config::global_config_path())?;
    validate_tag_namespaces(&body)?;
    if settings.guard_predefined_namespaces {
        validate_predefined_namespaces_unchanged(&body)?;
    }
    store.set_meta("tag_namespaces", &body.to_string()).await?;
    Ok(())
}

pub fn cmd_tags(action: TagsAction) -> Result<()> {
    match action {
        TagsAction::List { json } => block_on(async {
            let store = open_store().await?;
            let namespaces = store.tag_namespace_registry().await;
            let defaults = crate::store::default_tag_namespaces();
            let predefined: Vec<&String> = defaults
                .as_object()
                .expect("default_tag_namespaces returns an object")
                .keys()
                .collect();
            if json {
                print_json(&json!({ "namespaces": namespaces, "predefined": predefined }));
                return Ok(());
            }
            let Some(obj) = namespaces.as_object() else {
                println!("No namespaces.");
                return Ok(());
            };
            if obj.is_empty() {
                println!("No namespaces.");
            }
            for (name, ns) in obj {
                let mark = if predefined.contains(&name) {
                    " [predefined]"
                } else {
                    ""
                };
                println!("{name}{mark}");
                println!(
                    "  color:        {}",
                    ns.get("color").and_then(Value::as_str).unwrap_or("")
                );
                println!(
                    "  single_value: {}",
                    ns.get("single_value")
                        .and_then(Value::as_bool)
                        .unwrap_or(false)
                );
                println!(
                    "  values_mode:  {}",
                    ns.get("values_mode")
                        .and_then(Value::as_str)
                        .unwrap_or("suggestion")
                );
                if let Some(d) = ns.get("description").and_then(Value::as_str)
                    && !d.is_empty()
                {
                    println!("  description:  {d}");
                }
                let values: Vec<&str> = ns
                    .get("values")
                    .and_then(Value::as_array)
                    .map(|a| a.iter().filter_map(Value::as_str).collect())
                    .unwrap_or_default();
                println!(
                    "  values:       {}",
                    if values.is_empty() {
                        "(none)".to_string()
                    } else {
                        values.join(", ")
                    }
                );
            }
            Ok(())
        }),
        TagsAction::Add {
            name,
            color,
            description,
            single_value,
            values_mode,
        } => block_on(async move {
            if !["suggestion", "fixed"].contains(&values_mode.as_str()) {
                return Err(anyhow!("values-mode must be suggestion|fixed"));
            }
            let store = open_store().await?;
            let mut registry = store.tag_namespace_registry().await;
            let obj = registry
                .as_object_mut()
                .ok_or_else(|| anyhow!("tag registry is not an object"))?;
            if obj.contains_key(&name) {
                return Err(anyhow!("namespace {name:?} already exists"));
            }
            obj.insert(
                name,
                json!({
                    "color": color,
                    "values": Vec::<String>::new(),
                    "single_value": single_value,
                    "description": description.unwrap_or_default(),
                    "values_mode": values_mode,
                }),
            );
            save_registry(&store, registry).await?;
            println!("saved");
            Ok(())
        }),
        TagsAction::Set {
            name,
            color,
            description,
            single_value,
            values_mode,
        } => block_on(async move {
            if let Some(vm) = &values_mode
                && !["suggestion", "fixed"].contains(&vm.as_str())
            {
                return Err(anyhow!("values-mode must be suggestion|fixed"));
            }
            let store = open_store().await?;
            let mut registry = store.tag_namespace_registry().await;
            let obj = registry
                .as_object_mut()
                .ok_or_else(|| anyhow!("tag registry is not an object"))?;
            let entry = obj
                .get_mut(&name)
                .ok_or_else(|| anyhow!("no namespace {name:?}"))?
                .as_object_mut()
                .ok_or_else(|| anyhow!("namespace {name:?} is not an object"))?;
            if let Some(c) = color {
                entry.insert("color".into(), json!(c));
            }
            if let Some(d) = description {
                entry.insert("description".into(), json!(d));
            }
            if let Some(sv) = single_value {
                entry.insert("single_value".into(), json!(sv));
            }
            if let Some(vm) = values_mode {
                entry.insert("values_mode".into(), json!(vm));
            }
            save_registry(&store, registry).await?;
            println!("saved");
            Ok(())
        }),
        TagsAction::Rm { name } => block_on(async move {
            let store = open_store().await?;
            let mut registry = store.tag_namespace_registry().await;
            let obj = registry
                .as_object_mut()
                .ok_or_else(|| anyhow!("tag registry is not an object"))?;
            if obj.remove(&name).is_none() {
                return Err(anyhow!("no namespace {name:?}"));
            }
            save_registry(&store, registry).await?;
            println!("removed {name}");
            Ok(())
        }),
        TagsAction::ValueAdd { name, value } => block_on(async move {
            let store = open_store().await?;
            let mut registry = store.tag_namespace_registry().await;
            let obj = registry
                .as_object_mut()
                .ok_or_else(|| anyhow!("tag registry is not an object"))?;
            let entry = obj
                .get_mut(&name)
                .ok_or_else(|| anyhow!("no namespace {name:?}"))?
                .as_object_mut()
                .ok_or_else(|| anyhow!("namespace {name:?} is not an object"))?;
            let values = entry
                .entry("values".to_string())
                .or_insert_with(|| json!([]))
                .as_array_mut()
                .ok_or_else(|| anyhow!("namespace {name:?} \"values\" is not an array"))?;
            let value = value.trim().to_lowercase();
            if !values.iter().any(|v| v.as_str() == Some(value.as_str())) {
                values.push(json!(value));
            }
            save_registry(&store, registry).await?;
            println!("saved");
            Ok(())
        }),
        TagsAction::ValueRemove { name, value } => block_on(async move {
            let store = open_store().await?;
            let mut registry = store.tag_namespace_registry().await;
            let obj = registry
                .as_object_mut()
                .ok_or_else(|| anyhow!("tag registry is not an object"))?;
            let entry = obj
                .get_mut(&name)
                .ok_or_else(|| anyhow!("no namespace {name:?}"))?
                .as_object_mut()
                .ok_or_else(|| anyhow!("namespace {name:?} is not an object"))?;
            if let Some(values) = entry.get_mut("values").and_then(Value::as_array_mut) {
                values.retain(|v| v.as_str() != Some(value.as_str()));
            }
            save_registry(&store, registry).await?;
            println!("saved");
            Ok(())
        }),
    }
}
