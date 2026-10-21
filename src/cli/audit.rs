use anyhow::{Context, Result, anyhow};
use clap::Subcommand;
use serde_json::{Value, json};

use super::common::{block_on, print_json};

#[derive(Subcommand)]
pub enum AuditAction {
    /// Start a background audit of a project's memory index tree
    Start {
        /// Project to audit (default: this directory's .mynd.toml project)
        #[arg(long)]
        project: Option<String>,
        /// Audit the org store instead of the workspace store
        #[arg(long)]
        org: bool,
    },
    /// Show the current audit session's phase
    Status {
        /// Emit machine-readable JSON instead of a text summary
        #[arg(long)]
        json: bool,
    },
    /// List pending proposals
    Proposals {
        /// Only this project's proposals
        #[arg(long)]
        project: Option<String>,
        /// Emit machine-readable JSON instead of a text summary
        #[arg(long)]
        json: bool,
    },
    /// Apply a pending proposal
    Apply { id: String },
    /// Reject a pending proposal
    Reject { id: String },
    /// Ask the agent to revise a pending proposal
    Revise {
        /// The proposal's id, e.g. prop_xxxxxxxx
        proposal_id: String,
        /// What should change
        feedback: String,
    },
    /// End the current audit session
    End,
}

fn api_base() -> Result<String> {
    let settings = crate::config::load_server_settings(&crate::config::global_config_path())?;
    Ok(settings.api_url)
}

fn not_running_hint(e: reqwest::Error) -> anyhow::Error {
    if e.is_connect() {
        anyhow!("could not reach the Mynd server: start it first with `mynd up`")
    } else {
        anyhow::Error::new(e)
    }
}

/// Sends a request and returns the JSON body, or an error carrying the API's
/// own message.
async fn call(method: reqwest::Method, path: &str, body: Option<Value>) -> Result<Value> {
    let base = api_base()?;
    let mut req = reqwest::Client::new().request(method, format!("{base}{path}"));
    if let Some(b) = body {
        req = req.json(&b);
    }
    let resp = req.send().await.map_err(not_running_hint)?;
    let status = resp.status();
    let v: Value = resp.json().await.unwrap_or(Value::Null);
    if !status.is_success() {
        let msg = v
            .get("error")
            .and_then(|e| e.as_str())
            .unwrap_or("request failed");
        return Err(anyhow!("{msg} ({status})"));
    }
    Ok(v)
}

fn current_project() -> Result<String> {
    let cwd = std::env::current_dir()?;
    let root = crate::config::discover_project_root(&cwd).unwrap_or(cwd);
    let cfg =
        crate::config::load_config(&root).context("reading .mynd.toml (or pass --project)")?;
    Ok(cfg.project_name)
}

pub fn cmd_audit(action: AuditAction) -> Result<()> {
    use reqwest::Method;
    block_on(async {
        match action {
            AuditAction::Start { project, org } => {
                let project = match project {
                    Some(p) => p,
                    None => current_project()?,
                };
                let layer = if org { "org" } else { "workspace" };
                call(
                    Method::POST,
                    "/api/v1/audit-sessions",
                    Some(json!({ "project": project, "layer": layer })),
                )
                .await?;
                println!("audit of {project} started in the background");
                println!("review proposals in the dashboard or with: mynd audit proposals");
            }
            AuditAction::Status { json } => {
                let s = call(Method::GET, "/api/v1/audit-sessions/current", None).await?;
                if json {
                    print_json(&s);
                } else {
                    println!("active:  {}", s["active"].as_bool().unwrap_or(false));
                    println!("phase:   {}", s["phase"].as_str().unwrap_or("idle"));
                    if let Some(p) = s["project"].as_str() {
                        println!("project: {p}");
                    }
                    if let Some(r) = s["revising_proposal_id"].as_str() {
                        println!("revising: {r}");
                    }
                }
            }
            AuditAction::Proposals { project, json } => {
                let path = match &project {
                    Some(p) => format!("/api/v1/audit-proposals?project={p}"),
                    None => "/api/v1/audit-proposals".to_string(),
                };
                let v = call(Method::GET, &path, None).await?;
                if json {
                    print_json(&v);
                } else {
                    let list = v["proposals"].as_array().cloned().unwrap_or_default();
                    if list.is_empty() {
                        println!("no pending proposals");
                    }
                    for p in list {
                        println!(
                            "{}  [{}] {}  ({} ops)",
                            p["id"].as_str().unwrap_or(""),
                            p["kind"].as_str().unwrap_or(""),
                            p["summary"].as_str().unwrap_or(""),
                            p["ops"].as_array().map_or(0, |o| o.len())
                        );
                    }
                }
            }
            AuditAction::Apply { id } => {
                let v = call(
                    Method::POST,
                    &format!("/api/v1/audit-proposals/{id}/apply"),
                    None,
                )
                .await?;
                println!(
                    "applied {id}: {} memories written",
                    v["touched"].as_array().map_or(0, |t| t.len())
                );
            }
            AuditAction::Reject { id } => {
                call(
                    Method::POST,
                    &format!("/api/v1/audit-proposals/{id}/reject"),
                    None,
                )
                .await?;
                println!("rejected {id}");
            }
            AuditAction::Revise {
                proposal_id,
                feedback,
            } => {
                call(
                    Method::POST,
                    "/api/v1/audit-sessions/current/revise",
                    Some(json!({ "proposal_id": proposal_id, "feedback": feedback })),
                )
                .await?;
                println!("revision queued for {proposal_id}");
            }
            AuditAction::End => {
                call(Method::DELETE, "/api/v1/audit-sessions/current", None).await?;
                println!("audit session ended");
            }
        }
        Ok(())
    })
}
