use super::*;

// --- sync settings (read-only from file in v1) ---

pub(super) async fn get_sync_settings(Extension(sync): Extension<SyncSettings>) -> Json<Value> {
    Json(json!({
        "enabled": sync.enabled,
        "remote_url": sync.remote_url,
        "interval_seconds": sync.interval_seconds,
        "sync_on_store": sync.sync_on_store,
        "sync_on_startup": sync.sync_on_startup,
    }))
}

pub(super) async fn save_sync_settings(Json(_): Json<Value>) -> Json<Value> {
    Json(
        json!({ "saved": false, "message": "Sync settings are managed via config.toml — restart hivemind after editing." }),
    )
}

/// Each entry must be an object with a string `color` and an array-of-strings
/// `values`; `single_value` and `description`, if present, must be a bool
/// and a string respectively; `values_mode`, if present, must be the string
/// "suggestion" or "fixed" — "fixed" makes `values` an enforced allow-list
/// (checked in `store::validate_tags_against_registry`), "suggestion"
/// (the default when absent) keeps it purely advisory. Malformed entries
/// are rejected outright rather than silently persisted — a bad write here
/// previously corrupted the registry until the next `unwrap_or_else` reset.
pub(crate) fn validate_tag_namespaces(body: &Value) -> Result<(), String> {
    let obj = body
        .as_object()
        .ok_or_else(|| "tag settings must be a JSON object".to_string())?;
    for (ns, entry) in obj {
        let entry_obj = entry
            .as_object()
            .ok_or_else(|| format!("namespace {ns:?} must be an object"))?;
        match entry_obj.get("color") {
            Some(Value::String(_)) => {}
            _ => return Err(format!("namespace {ns:?} missing string \"color\"")),
        }
        match entry_obj.get("values") {
            Some(Value::Array(vals)) if vals.iter().all(|v| v.is_string()) => {}
            _ => return Err(format!("namespace {ns:?} missing string array \"values\"")),
        }
        if let Some(sv) = entry_obj.get("single_value")
            && !sv.is_boolean()
        {
            return Err(format!("namespace {ns:?} \"single_value\" must be a bool"));
        }
        if let Some(d) = entry_obj.get("description")
            && !d.is_string()
        {
            return Err(format!("namespace {ns:?} \"description\" must be a string"));
        }
        if let Some(vm) = entry_obj.get("values_mode")
            && vm.as_str() != Some("suggestion")
            && vm.as_str() != Some("fixed")
        {
            return Err(format!(
                "namespace {ns:?} \"values_mode\" must be \"suggestion\" or \"fixed\""
            ));
        }
    }
    Ok(())
}

/// True if `body[name]` matches `crate::store::default_tag_namespaces()[name]`
/// exactly — used to detect whether a predefined namespace was left alone.
fn matches_predefined_default(name: &str, body: &Value, defaults: &Value) -> bool {
    let default_entry = defaults.get(name);
    body.get(name) == default_entry
}

/// Rejects deleting or modifying any predefined namespace (name, color,
/// description, values, single_value, values_mode — the entry must match
/// `default_tag_namespaces()` exactly). Namespaces not in the predefined set
/// are unaffected — freely addable/editable/removable as before.
fn validate_predefined_namespaces_unchanged(body: &Value) -> Result<(), String> {
    let defaults = crate::store::default_tag_namespaces();
    let names = defaults
        .as_object()
        .expect("default_tag_namespaces returns an object")
        .keys();
    for name in names {
        if !matches_predefined_default(name, body, &defaults) {
            return Err(format!(
                "namespace {name:?} is predefined and cannot be deleted or modified. \
                 Disable this guard with [tags] guard_predefined_namespaces = false \
                 in the global hivemind config to allow it."
            ));
        }
    }
    Ok(())
}

pub(super) async fn get_tag_settings(
    State(store): State<Store>,
    Extension(guard): Extension<GuardPredefinedNamespaces>,
) -> Result<Json<Value>, ApiError> {
    let namespaces = store.tag_namespace_registry().await;
    let defaults = crate::store::default_tag_namespaces();
    let predefined: Vec<&String> = defaults
        .as_object()
        .expect("default_tag_namespaces returns an object")
        .keys()
        .collect();
    Ok(Json(json!({
        "namespaces": namespaces,
        "predefined": predefined,
        "guard_predefined_namespaces": guard.0,
    })))
}

pub(super) async fn save_tag_settings(
    State(store): State<Store>,
    Extension(guard): Extension<GuardPredefinedNamespaces>,
    Extension(hive): Extension<HivePushConfig>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    validate_tag_namespaces(&body).map_err(|e| ApiError(StatusCode::UNPROCESSABLE_ENTITY, e))?;
    if guard.0 {
        validate_predefined_namespaces_unchanged(&body)
            .map_err(|e| ApiError(StatusCode::UNPROCESSABLE_ENTITY, e))?;
    }
    store.set_meta("tag_namespaces", &body.to_string()).await?;
    store
        .set_meta(
            "tag_namespaces_updated_at",
            &chrono::Utc::now().timestamp().to_string(),
        )
        .await?;
    // Finding I3: propagate the tag-namespace change to online hive peers.
    crate::api::spawn_hive_tag_namespaces_push(&hive, &store);
    Ok(Json(json!({ "saved": true })))
}

// --- content-size guardrail (max_content_tokens) ---

pub(super) async fn get_content_limit_settings(
    State(store): State<Store>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(
        json!({ "max_content_tokens": store.max_content_tokens().await }),
    ))
}

pub(super) async fn save_content_limit_settings(
    State(store): State<Store>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    let tokens = body
        .get("max_content_tokens")
        .and_then(Value::as_i64)
        .filter(|v| *v > 0)
        .ok_or_else(|| {
            ApiError(
                StatusCode::UNPROCESSABLE_ENTITY,
                "max_content_tokens must be a positive integer".to_string(),
            )
        })?;
    store
        .set_meta("max_content_tokens", &tokens.to_string())
        .await?;
    Ok(Json(json!({ "saved": true })))
}
