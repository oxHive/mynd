//! Browser-facing request guard for the HTTP server.
//!
//! The REST API and the HTTP MCP endpoint are unauthenticated by design:
//! they bind to loopback and trust whatever can reach them. Two things a
//! browser can do break that assumption without ever leaving the user's
//! machine:
//!
//! - **DNS rebinding.** A page on `evil.example` re-resolves its own
//!   hostname to `127.0.0.1` after loading, and every subsequent request is
//!   same-origin from the browser's point of view — CORS never applies. The
//!   `Host` header still says `evil.example`, which is how we tell.
//! - **Cross-site request forgery.** A cross-origin `<form>` or a
//!   `fetch(..., {mode: "no-cors"})` is sent regardless of CORS; CORS only
//!   hides the *response*. Bodyless `POST`s (start a suggest session, apply
//!   an update) need no special content type, so they were reachable from
//!   any page. Browsers attach an `Origin` header to every such request,
//!   and a cross-site one carries the attacker's origin.
//!
//! So this layer rejects any request whose `Host` is not one of ours, and
//! any state-changing request whose `Origin` is not one of ours. Requests
//! with neither header (curl, the CLI, MCP clients, the integration tests)
//! are unaffected: no browser omits them, so nothing is lost by allowing
//! them through.

use axum::{
    Json,
    body::Body,
    extract::{Request, State},
    http::{Method, StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
use serde_json::json;

/// Everything the guard needs, derived once from `ServerSettings`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuardConfig {
    /// Lowercased host names (no port) accepted in `Host`, in addition to
    /// loopback. Empty when `enforce_host` is false.
    pub allowed_hosts: Vec<String>,
    /// Lowercased `scheme://host[:port]` origins accepted in `Origin`, in
    /// addition to any loopback origin on any port.
    pub allowed_origins: Vec<String>,
    /// False when the server is bound to a wildcard address (`0.0.0.0`,
    /// `::`): we cannot know which host names reach it, and it is exposed
    /// to the network anyway (`http::run_up` warns about that separately).
    pub enforce_host: bool,
}

impl GuardConfig {
    pub fn from_settings(settings: &crate::config::ServerSettings) -> Self {
        let wildcard = matches!(settings.host.as_str(), "0.0.0.0" | "::" | "[::]");
        let mut allowed_hosts = Vec::new();
        if !wildcard {
            allowed_hosts.push(settings.host.to_ascii_lowercase());
        }
        for url in [&settings.api_url, &settings.cors_origin] {
            if let Some(h) = origin_host(url) {
                allowed_hosts.push(h);
            }
        }
        allowed_hosts.sort();
        allowed_hosts.dedup();
        GuardConfig {
            allowed_hosts,
            allowed_origins: allowed_origin_list(&settings.cors_origin),
            enforce_host: !wildcard,
        }
    }

    /// A permissive config for tests and embedders that do not go through
    /// `ServerSettings`: loopback only, nothing else.
    pub fn loopback_only() -> Self {
        GuardConfig {
            allowed_hosts: Vec::new(),
            allowed_origins: Vec::new(),
            enforce_host: true,
        }
    }
}

/// The configured dashboard origin plus its `localhost` / `127.0.0.1`
/// sibling, lowercased, so the browser passes whichever loopback spelling
/// the user typed. Shared by the CORS allowlist and this guard so the two
/// can never disagree.
pub fn allowed_origin_list(origin: &str) -> Vec<String> {
    let origin = origin.trim().trim_end_matches('/').to_ascii_lowercase();
    let mut out = Vec::new();
    if !origin.is_empty() {
        out.push(origin.clone());
    }
    let sibling = if origin.contains("127.0.0.1") {
        origin.replace("127.0.0.1", "localhost")
    } else if origin.contains("localhost") {
        origin.replace("localhost", "127.0.0.1")
    } else {
        String::new()
    };
    if !sibling.is_empty() && sibling != origin {
        out.push(sibling);
    }
    out
}

/// Host part of a `Host` header value or a URL origin, lowercased, with
/// the port and any `scheme://` prefix removed. `[::1]:3456` → `[::1]`.
fn origin_host(value: &str) -> Option<String> {
    let v = value.trim();
    let v = v.split_once("://").map(|(_, rest)| rest).unwrap_or(v);
    let v = v.split(['/', '?', '#']).next().unwrap_or("");
    if v.is_empty() {
        return None;
    }
    let host = if let Some(rest) = v.strip_prefix('[') {
        // bracketed IPv6, possibly followed by :port
        let end = rest.find(']')?;
        format!("[{}]", &rest[..end])
    } else {
        v.rsplit_once(':')
            .filter(|(_, port)| !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()))
            .map(|(h, _)| h)
            .unwrap_or(v)
            .to_string()
    };
    Some(host.to_ascii_lowercase())
}

fn is_loopback_host(host: &str) -> bool {
    if host == "localhost" {
        return true;
    }
    let bare = host.trim_start_matches('[').trim_end_matches(']');
    bare.parse::<std::net::IpAddr>()
        .map(|ip| ip.is_loopback())
        .unwrap_or(false)
}

/// True if `host_header` names this server. A missing header is accepted
/// (see module docs); an unparseable one is not.
pub fn host_allowed(cfg: &GuardConfig, host_header: Option<&str>) -> bool {
    if !cfg.enforce_host {
        return true;
    }
    let Some(raw) = host_header else {
        return true;
    };
    match origin_host(raw) {
        Some(h) => is_loopback_host(&h) || cfg.allowed_hosts.contains(&h),
        None => false,
    }
}

/// True if `origin_header` is one of ours. A missing header is accepted
/// (non-browser client); `null` and anything unparseable are not.
pub fn origin_allowed(cfg: &GuardConfig, origin_header: Option<&str>) -> bool {
    let Some(raw) = origin_header else {
        return true;
    };
    let origin = raw.trim().trim_end_matches('/').to_ascii_lowercase();
    if origin.is_empty() || origin == "null" {
        return false;
    }
    if cfg.allowed_origins.contains(&origin) {
        return true;
    }
    // Any loopback origin on any port: the vite dev server, a second
    // dashboard port, etc. Still local to this machine.
    let is_http = origin.starts_with("http://") || origin.starts_with("https://");
    is_http && origin_host(&origin).is_some_and(|h| is_loopback_host(&h))
}

fn is_state_changing(method: &Method) -> bool {
    !matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS)
}

fn reject(reason: String) -> Response {
    tracing::warn!("rejected request: {reason}");
    (StatusCode::FORBIDDEN, Json(json!({ "error": reason }))).into_response()
}

/// The middleware itself. Install with
/// `axum::middleware::from_fn_with_state(cfg, guard)`.
pub async fn guard(State(cfg): State<GuardConfig>, req: Request<Body>, next: Next) -> Response {
    let host = req
        .headers()
        .get(header::HOST)
        .and_then(|v| v.to_str().ok());
    if !host_allowed(&cfg, host) {
        return reject(format!(
            "Host {:?} does not name this server (possible DNS rebinding); \
             use 127.0.0.1 or localhost, or set [server] host / [dashboard] api_url",
            host.unwrap_or("")
        ));
    }
    if is_state_changing(req.method()) {
        let origin = req
            .headers()
            .get(header::ORIGIN)
            .and_then(|v| v.to_str().ok());
        if !origin_allowed(&cfg, origin) {
            return reject(format!(
                "cross-site request from Origin {:?} refused; \
                 set [dashboard] cors_origin to allow a different dashboard origin",
                origin.unwrap_or("")
            ));
        }
    }
    next.run(req).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> GuardConfig {
        GuardConfig::from_settings(&crate::config::ServerSettings {
            host: "127.0.0.1".into(),
            port: 3456,
            dashboard_port: 3457,
            api_url: "http://127.0.0.1:3456".into(),
            cors_origin: "http://127.0.0.1:3457".into(),
            sync: Default::default(),
            org_sync: None,
            update: Default::default(),
            agent: Default::default(),
            guard_predefined_namespaces: true,
            hive: Default::default(),
        })
    }

    #[test]
    fn origin_host_strips_scheme_port_and_path() {
        assert_eq!(
            origin_host("http://Localhost:3457/x").as_deref(),
            Some("localhost")
        );
        assert_eq!(origin_host("127.0.0.1:3456").as_deref(), Some("127.0.0.1"));
        assert_eq!(origin_host("[::1]:3456").as_deref(), Some("[::1]"));
        assert_eq!(origin_host("[::1]").as_deref(), Some("[::1]"));
        assert_eq!(origin_host("evil.example").as_deref(), Some("evil.example"));
        assert_eq!(origin_host(""), None);
        assert_eq!(origin_host("http://"), None);
    }

    #[test]
    fn host_check_accepts_loopback_spellings_and_configured_hosts() {
        let c = cfg();
        for h in [
            "127.0.0.1:3456",
            "127.0.0.1",
            "localhost:3456",
            "LOCALHOST",
            "[::1]:3456",
            "127.0.0.2:3456",
        ] {
            assert!(host_allowed(&c, Some(h)), "{h} should be allowed");
        }
        assert!(
            host_allowed(&c, None),
            "no Host header = non-browser client"
        );
    }

    #[test]
    fn host_check_rejects_foreign_and_garbage_hosts() {
        let c = cfg();
        for h in [
            "evil.example",
            "evil.example:3456",
            "127.0.0.1.evil.example",
            "localhost.evil.example",
            "10.0.0.5:3456",
            "",
        ] {
            assert!(!host_allowed(&c, Some(h)), "{h} should be rejected");
        }
    }

    #[test]
    fn host_check_honours_a_custom_configured_host() {
        let mut c = cfg();
        c.allowed_hosts.push("mynd.lan".into());
        assert!(host_allowed(&c, Some("mynd.lan:3456")));
        assert!(!host_allowed(&c, Some("other.lan:3456")));
    }

    #[test]
    fn host_check_is_skipped_for_wildcard_binds() {
        let mut settings = crate::config::ServerSettings {
            host: "0.0.0.0".into(),
            ..cfg_settings()
        };
        settings.api_url = "http://0.0.0.0:3456".into();
        let c = GuardConfig::from_settings(&settings);
        assert!(!c.enforce_host);
        assert!(host_allowed(&c, Some("anything.example")));
    }

    fn cfg_settings() -> crate::config::ServerSettings {
        crate::config::ServerSettings {
            host: "127.0.0.1".into(),
            port: 3456,
            dashboard_port: 3457,
            api_url: "http://127.0.0.1:3456".into(),
            cors_origin: "http://127.0.0.1:3457".into(),
            sync: Default::default(),
            org_sync: None,
            update: Default::default(),
            agent: Default::default(),
            guard_predefined_namespaces: true,
            hive: Default::default(),
        }
    }

    #[test]
    fn origin_check_accepts_dashboard_sibling_and_any_loopback_port() {
        let c = cfg();
        for o in [
            "http://127.0.0.1:3457",
            "http://localhost:3457",
            "http://localhost:5173",
            "http://[::1]:9999",
            "HTTP://LOCALHOST:3457",
        ] {
            assert!(origin_allowed(&c, Some(o)), "{o} should be allowed");
        }
        assert!(origin_allowed(&c, None), "no Origin = non-browser client");
    }

    #[test]
    fn origin_check_rejects_cross_site_null_and_lookalikes() {
        let c = cfg();
        for o in [
            "https://evil.example",
            "http://evil.example:3457",
            "null",
            "",
            "http://localhost.evil.example:3457",
            "http://127.0.0.1.nip.io:3457",
            "file://",
        ] {
            assert!(!origin_allowed(&c, Some(o)), "{o} should be rejected");
        }
    }

    #[test]
    fn allowed_origin_list_adds_the_loopback_sibling() {
        assert_eq!(
            allowed_origin_list("http://127.0.0.1:3457"),
            vec!["http://127.0.0.1:3457", "http://localhost:3457"]
        );
        assert_eq!(
            allowed_origin_list("http://Localhost:3457/"),
            vec!["http://localhost:3457", "http://127.0.0.1:3457"]
        );
        assert_eq!(
            allowed_origin_list("https://dash.example"),
            vec!["https://dash.example"]
        );
        assert!(allowed_origin_list("").is_empty());
    }

    #[test]
    fn from_settings_collects_hosts_from_bind_api_url_and_cors_origin() {
        let mut s = cfg_settings();
        s.host = "mynd.lan".into();
        s.api_url = "http://api.lan:3456".into();
        s.cors_origin = "http://dash.lan:3457".into();
        let c = GuardConfig::from_settings(&s);
        assert_eq!(c.allowed_hosts, vec!["api.lan", "dash.lan", "mynd.lan"]);
        assert!(c.enforce_host);
    }
}
