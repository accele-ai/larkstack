//! Axum handler for `POST /github/webhook` — receives GitHub webhook payloads,
//! converts them to [`Event`]s, and dispatches immediately (no debounce).
//!
//! **Native** build: uses octocrab's strongly-typed `WebhookEvent` models.
//! **CF Worker** build: uses minimal hand-rolled thin structs (octocrab's
//! unconditional `hyper` dep pulls in `mio`, which does not compile to WASM).

use std::sync::Arc;

use axum::{
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
};
use tracing::{info, warn};

#[cfg(feature = "native")]
use octocrab::models::webhook_events::{
    WebhookEvent, WebhookEventPayload,
    payload::{
        DependabotAlertWebhookEventAction, IssuesWebhookEventAction, PullRequestWebhookEventAction,
        SecretScanningAlertWebhookEventAction, WorkflowRunWebhookEventAction,
    },
};

use crate::{
    config::{AppState, GitHubConfig},
    dispatch,
    event::{CommitSummary, Event},
};

use super::utils::{branch_from_ref, verify_github_signature};

const MAX_COMMITS: usize = 5;

// ---------------------------------------------------------------------------
// Shared thin structs (used by both native and cf-worker)
// ---------------------------------------------------------------------------

/// Pre-parse: extracts `repository.name` (whitelist) and `full_name` (display)
/// without deserializing the full payload.
#[derive(serde::Deserialize)]
struct RepoProbe {
    repository: RepoName,
}

#[derive(serde::Deserialize)]
struct RepoName {
    name: String,
    full_name: Option<String>,
}

/// Inner data for `workflow_run` events — stored as `serde_json::Value` in
/// octocrab, so we deserialize manually in both build targets.
#[derive(serde::Deserialize)]
struct WorkflowRunData {
    conclusion: Option<String>,
    name: String,
    head_branch: String,
    actor: WorkflowRunActor,
    html_url: String,
}

#[derive(serde::Deserialize)]
struct WorkflowRunActor {
    login: String,
}

#[derive(serde::Deserialize)]
struct SecretScanningAlertData {
    secret_type_display_name: Option<String>,
    secret_type: String,
    html_url: String,
}

#[derive(serde::Deserialize)]
struct DependabotAlertData {
    severity: String,
    dependency: Option<DependabotDependency>,
    security_advisory: Option<DependabotAdvisory>,
    html_url: String,
}

#[derive(serde::Deserialize)]
struct DependabotDependency {
    package: Option<DependabotPackage>,
}

#[derive(serde::Deserialize)]
struct DependabotPackage {
    name: String,
}

#[derive(serde::Deserialize)]
struct DependabotAdvisory {
    summary: String,
}

// ---------------------------------------------------------------------------
// CF Worker thin structs (replacing octocrab types that can't compile to WASM)
// ---------------------------------------------------------------------------

#[cfg(feature = "cf-worker")]
mod thin {
    use serde::Deserialize;

    #[derive(Deserialize)]
    pub struct PrPayload {
        pub action: String,
        pub number: u64,
        pub pull_request: PullRequest,
        pub requested_reviewer: Option<User>,
    }

    #[derive(Deserialize)]
    pub struct PullRequest {
        pub title: Option<String>,
        pub user: Option<User>,
        pub html_url: Option<String>,
        pub head: GitRef,
        pub base: GitRef,
        pub additions: Option<u64>,
        pub deletions: Option<u64>,
        pub merged_at: Option<String>,
        pub merged_by: Option<User>,
    }

    #[derive(Deserialize)]
    pub struct User {
        pub login: String,
    }

    #[derive(Deserialize)]
    pub struct GitRef {
        pub r#ref: String,
    }

    #[derive(Deserialize)]
    pub struct IssuesPayload {
        pub action: String,
        pub label: Option<Label>,
        pub issue: Issue,
    }

    #[derive(Deserialize)]
    pub struct Label {
        pub name: String,
    }

    #[derive(Deserialize)]
    pub struct Issue {
        pub number: u64,
        pub title: String,
        pub user: User,
        pub html_url: String,
    }

    #[derive(Deserialize)]
    pub struct PushPayload {
        pub r#ref: String,
        pub commits: Vec<Commit>,
        pub pusher: CommitUser,
        pub compare: String,
    }

    #[derive(Deserialize)]
    pub struct Commit {
        pub id: String,
        pub message: String,
        pub author: CommitUser,
    }

    #[derive(Deserialize)]
    pub struct CommitUser {
        pub name: String,
    }

    #[derive(Deserialize)]
    pub struct WorkflowRunPayload {
        pub action: String,
        pub workflow_run: serde_json::Value,
    }

    #[derive(Deserialize)]
    pub struct SecretScanningPayload {
        pub action: String,
        pub alert: serde_json::Value,
    }

    #[derive(Deserialize)]
    pub struct DependabotPayload {
        pub action: String,
        pub alert: serde_json::Value,
    }
}

// ---------------------------------------------------------------------------
// Shared outer handler
// ---------------------------------------------------------------------------

/// Handles incoming GitHub webhook requests.
///
/// 1. Verifies the `X-Hub-Signature-256` HMAC header.
/// 2. Extracts repo name for whitelist filtering.
/// 3. Dispatches via octocrab (native) or thin structs (cf-worker).
pub async fn webhook_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> StatusCode {
    let github = match &state.github {
        Some(cfg) => cfg,
        None => {
            warn!("received GitHub webhook but GITHUB_WEBHOOK_SECRET not configured");
            return StatusCode::NOT_FOUND;
        }
    };

    let signature = match headers
        .get("x-hub-signature-256")
        .and_then(|v| v.to_str().ok())
    {
        Some(s) => s,
        None => {
            warn!("missing x-hub-signature-256 header");
            return StatusCode::UNAUTHORIZED;
        }
    };

    if !verify_github_signature(&github.webhook_secret, &body, signature) {
        warn!("invalid GitHub webhook signature");
        return StatusCode::UNAUTHORIZED;
    }

    // Extract repo name and full_name in one pass (used for whitelist + display).
    let (repo, repo_name) = match serde_json::from_slice::<RepoProbe>(&body) {
        Ok(probe) => {
            let full = probe
                .repository
                .full_name
                .clone()
                .unwrap_or_else(|| probe.repository.name.clone());
            (full, probe.repository.name)
        }
        Err(_) => {
            warn!("could not extract repository name from payload");
            (String::new(), String::new())
        }
    };

    if !github.repo_whitelist.is_empty()
        && !repo_name.is_empty()
        && !github.repo_whitelist.contains(&repo_name)
    {
        info!("ignoring event from non-whitelisted repo: {repo_name}");
        return StatusCode::OK;
    }

    let event_type = headers
        .get("x-github-event")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    #[cfg(feature = "native")]
    return dispatch_native(&state, github, &repo, event_type, &body).await;

    #[cfg(feature = "cf-worker")]
    dispatch_cf(&state, github, &repo, event_type, &body).await
}

// ---------------------------------------------------------------------------
// Native dispatch — uses octocrab WebhookEvent
// ---------------------------------------------------------------------------

#[cfg(feature = "native")]
async fn dispatch_native(
    state: &Arc<AppState>,
    github: &GitHubConfig,
    repo: &str,
    event_type: &str,
    body: &[u8],
) -> StatusCode {
    let webhook = match WebhookEvent::try_from_header_and_body(event_type, body) {
        Ok(ev) => ev,
        Err(e) => {
            warn!("failed to parse GitHub webhook event: {e}");
            return StatusCode::BAD_REQUEST;
        }
    };

    match webhook.specific {
        WebhookEventPayload::PullRequest(payload) => {
            handle_pull_request(state, github, repo, *payload).await
        }
        WebhookEventPayload::Issues(payload) => handle_issues(state, github, repo, *payload).await,
        WebhookEventPayload::Push(payload) => handle_push(state, repo, *payload).await,
        WebhookEventPayload::WorkflowRun(payload) => {
            handle_workflow_run(state, repo, *payload).await
        }
        WebhookEventPayload::SecretScanningAlert(payload) => {
            handle_secret_scanning(state, repo, *payload).await
        }
        WebhookEventPayload::DependabotAlert(payload) => {
            handle_dependabot(state, repo, *payload).await
        }
        _ => {
            info!("ignoring GitHub event type: {event_type}");
            StatusCode::OK
        }
    }
}

#[cfg(feature = "native")]
async fn handle_pull_request(
    state: &Arc<AppState>,
    github: &GitHubConfig,
    repo: &str,
    payload: octocrab::models::webhook_events::payload::PullRequestWebhookEventPayload,
) -> StatusCode {
    let pr = &payload.pull_request;
    let number = payload.number;
    let title = pr.title.clone().unwrap_or_default();
    let author = pr
        .user
        .as_ref()
        .map(|u| u.login.clone())
        .unwrap_or_default();
    let html_url = pr
        .html_url
        .as_ref()
        .map(|u| u.to_string())
        .unwrap_or_default();

    match payload.action {
        PullRequestWebhookEventAction::Opened => {
            info!("GitHub PR opened: {repo}#{number}");
            let event = Event::PrOpened {
                repo: repo.to_string(),
                number,
                title,
                author,
                head_branch: pr.head.ref_field.clone(),
                base_branch: pr.base.ref_field.clone(),
                additions: pr.additions.unwrap_or(0),
                deletions: pr.deletions.unwrap_or(0),
                url: html_url,
            };
            dispatch::dispatch_github(&event, state, None).await;
            StatusCode::OK
        }
        PullRequestWebhookEventAction::ReviewRequested => {
            let reviewer = match &payload.requested_reviewer {
                Some(u) => u.login.clone(),
                None => {
                    info!("review_requested without requested_reviewer, ignoring");
                    return StatusCode::OK;
                }
            };
            info!("GitHub review requested: {repo}#{number} reviewer={reviewer}");
            let reviewer_lark_id = github.user_map.get(&reviewer).cloned();
            let dm_email = reviewer_lark_id.clone();
            let event = Event::PrReviewRequested {
                repo: repo.to_string(),
                number,
                title,
                author,
                reviewer,
                reviewer_lark_id,
                url: html_url,
            };
            dispatch::dispatch_github(&event, state, dm_email.as_deref()).await;
            StatusCode::OK
        }
        PullRequestWebhookEventAction::Closed if pr.merged_at.is_some() => {
            let merged_by = pr
                .merged_by
                .as_ref()
                .map(|u| u.login.clone())
                .unwrap_or_else(|| author.clone());
            info!("GitHub PR merged: {repo}#{number} by {merged_by}");
            let event = Event::PrMerged {
                repo: repo.to_string(),
                number,
                title,
                author,
                merged_by,
                url: html_url,
            };
            dispatch::dispatch_github(&event, state, None).await;
            StatusCode::OK
        }
        _ => {
            info!("ignoring pull_request action for {repo}#{number}");
            StatusCode::OK
        }
    }
}

#[cfg(feature = "native")]
async fn handle_issues(
    state: &Arc<AppState>,
    github: &GitHubConfig,
    repo: &str,
    payload: octocrab::models::webhook_events::payload::IssuesWebhookEventPayload,
) -> StatusCode {
    if payload.action != IssuesWebhookEventAction::Labeled {
        info!("ignoring issues action");
        return StatusCode::OK;
    }
    let label = match &payload.label {
        Some(l) => l.name.clone(),
        None => return StatusCode::OK,
    };
    if !github.alert_labels.contains(&label.to_lowercase()) {
        info!("ignoring non-alert label: {label}");
        return StatusCode::OK;
    }
    let issue = &payload.issue;
    let number = issue.number;
    let title = issue.title.clone();
    let author = issue.user.login.clone();
    let html_url = issue.html_url.to_string();
    info!("GitHub issue labeled alert: {repo}#{number} label={label}");
    let event = Event::IssueLabeledAlert {
        repo: repo.to_string(),
        number,
        title,
        label,
        author,
        url: html_url,
    };
    dispatch::dispatch_github(&event, state, None).await;
    StatusCode::OK
}

#[cfg(feature = "native")]
async fn handle_push(
    state: &Arc<AppState>,
    repo: &str,
    payload: octocrab::models::webhook_events::payload::PushWebhookEventPayload,
) -> StatusCode {
    let branch = branch_from_ref(&payload.r#ref);
    if !is_protected_branch(branch) {
        info!("ignoring push to non-protected branch: {branch}");
        return StatusCode::OK;
    }
    info!(
        "GitHub push to {repo}@{branch}: {} commit(s)",
        payload.commits.len()
    );
    let commits: Vec<CommitSummary> = payload
        .commits
        .iter()
        .take(MAX_COMMITS)
        .map(|c| CommitSummary {
            sha_short: c.id.chars().take(7).collect(),
            message_line: c.message.lines().next().unwrap_or("").to_string(),
            author: c.author.user.name.clone(),
        })
        .collect();
    let event = Event::BranchPush {
        repo: repo.to_string(),
        branch: branch.to_string(),
        pusher: payload.pusher.user.name.clone(),
        commits,
        compare_url: payload.compare.to_string(),
    };
    dispatch::dispatch_github(&event, state, None).await;
    StatusCode::OK
}

#[cfg(feature = "native")]
async fn handle_workflow_run(
    state: &Arc<AppState>,
    repo: &str,
    payload: octocrab::models::webhook_events::payload::WorkflowRunWebhookEventPayload,
) -> StatusCode {
    if payload.action != WorkflowRunWebhookEventAction::Completed {
        info!("ignoring workflow_run action");
        return StatusCode::OK;
    }
    dispatch_workflow_run(state, repo, payload.workflow_run).await
}

#[cfg(feature = "native")]
async fn handle_secret_scanning(
    state: &Arc<AppState>,
    repo: &str,
    payload: octocrab::models::webhook_events::payload::SecretScanningAlertWebhookEventPayload,
) -> StatusCode {
    if payload.action != SecretScanningAlertWebhookEventAction::Created {
        info!("ignoring secret_scanning_alert action");
        return StatusCode::OK;
    }
    dispatch_secret_scanning(state, repo, payload.alert).await
}

#[cfg(feature = "native")]
async fn handle_dependabot(
    state: &Arc<AppState>,
    repo: &str,
    payload: octocrab::models::webhook_events::payload::DependabotAlertWebhookEventPayload,
) -> StatusCode {
    if payload.action != DependabotAlertWebhookEventAction::Created {
        info!("ignoring dependabot_alert action");
        return StatusCode::OK;
    }
    dispatch_dependabot(state, repo, payload.alert).await
}

// ---------------------------------------------------------------------------
// CF Worker dispatch — uses thin hand-rolled structs
// ---------------------------------------------------------------------------

#[cfg(feature = "cf-worker")]
async fn dispatch_cf(
    state: &Arc<AppState>,
    github: &GitHubConfig,
    repo: &str,
    event_type: &str,
    body: &[u8],
) -> StatusCode {
    match event_type {
        "pull_request" => {
            let payload: thin::PrPayload = match serde_json::from_slice(body) {
                Ok(p) => p,
                Err(e) => {
                    warn!("failed to parse pull_request payload: {e}");
                    return StatusCode::OK;
                }
            };
            handle_pr_cf(state, github, repo, payload).await
        }
        "issues" => {
            let payload: thin::IssuesPayload = match serde_json::from_slice(body) {
                Ok(p) => p,
                Err(e) => {
                    warn!("failed to parse issues payload: {e}");
                    return StatusCode::OK;
                }
            };
            handle_issues_cf(state, github, repo, payload).await
        }
        "push" => {
            let payload: thin::PushPayload = match serde_json::from_slice(body) {
                Ok(p) => p,
                Err(e) => {
                    warn!("failed to parse push payload: {e}");
                    return StatusCode::OK;
                }
            };
            handle_push_cf(state, repo, payload).await
        }
        "workflow_run" => {
            let payload: thin::WorkflowRunPayload = match serde_json::from_slice(body) {
                Ok(p) => p,
                Err(e) => {
                    warn!("failed to parse workflow_run payload: {e}");
                    return StatusCode::OK;
                }
            };
            if payload.action != "completed" {
                info!("ignoring workflow_run action");
                return StatusCode::OK;
            }
            dispatch_workflow_run(state, repo, payload.workflow_run).await
        }
        "secret_scanning_alert" => {
            let payload: thin::SecretScanningPayload = match serde_json::from_slice(body) {
                Ok(p) => p,
                Err(e) => {
                    warn!("failed to parse secret_scanning_alert payload: {e}");
                    return StatusCode::OK;
                }
            };
            if payload.action != "created" {
                info!("ignoring secret_scanning_alert action");
                return StatusCode::OK;
            }
            dispatch_secret_scanning(state, repo, payload.alert).await
        }
        "dependabot_alert" => {
            let payload: thin::DependabotPayload = match serde_json::from_slice(body) {
                Ok(p) => p,
                Err(e) => {
                    warn!("failed to parse dependabot_alert payload: {e}");
                    return StatusCode::OK;
                }
            };
            if payload.action != "created" {
                info!("ignoring dependabot_alert action");
                return StatusCode::OK;
            }
            dispatch_dependabot(state, repo, payload.alert).await
        }
        _ => {
            info!("ignoring GitHub event type: {event_type}");
            StatusCode::OK
        }
    }
}

#[cfg(feature = "cf-worker")]
async fn handle_pr_cf(
    state: &Arc<AppState>,
    github: &GitHubConfig,
    repo: &str,
    payload: thin::PrPayload,
) -> StatusCode {
    let pr = &payload.pull_request;
    let number = payload.number;
    let title = pr.title.clone().unwrap_or_default();
    let author = pr
        .user
        .as_ref()
        .map(|u| u.login.clone())
        .unwrap_or_default();
    let html_url = pr.html_url.clone().unwrap_or_default();

    match payload.action.as_str() {
        "opened" => {
            info!("GitHub PR opened: {repo}#{number}");
            let event = Event::PrOpened {
                repo: repo.to_string(),
                number,
                title,
                author,
                head_branch: pr.head.r#ref.clone(),
                base_branch: pr.base.r#ref.clone(),
                additions: pr.additions.unwrap_or(0),
                deletions: pr.deletions.unwrap_or(0),
                url: html_url,
            };
            dispatch::dispatch_github(&event, state, None).await;
            StatusCode::OK
        }
        "review_requested" => {
            let reviewer = match &payload.requested_reviewer {
                Some(u) => u.login.clone(),
                None => {
                    info!("review_requested without requested_reviewer, ignoring");
                    return StatusCode::OK;
                }
            };
            info!("GitHub review requested: {repo}#{number} reviewer={reviewer}");
            let reviewer_lark_id = github.user_map.get(&reviewer).cloned();
            let dm_email = reviewer_lark_id.clone();
            let event = Event::PrReviewRequested {
                repo: repo.to_string(),
                number,
                title,
                author,
                reviewer,
                reviewer_lark_id,
                url: html_url,
            };
            dispatch::dispatch_github(&event, state, dm_email.as_deref()).await;
            StatusCode::OK
        }
        "closed" if pr.merged_at.is_some() => {
            let merged_by = pr
                .merged_by
                .as_ref()
                .map(|u| u.login.clone())
                .unwrap_or_else(|| author.clone());
            info!("GitHub PR merged: {repo}#{number} by {merged_by}");
            let event = Event::PrMerged {
                repo: repo.to_string(),
                number,
                title,
                author,
                merged_by,
                url: html_url,
            };
            dispatch::dispatch_github(&event, state, None).await;
            StatusCode::OK
        }
        _ => {
            info!("ignoring pull_request action for {repo}#{number}");
            StatusCode::OK
        }
    }
}

#[cfg(feature = "cf-worker")]
async fn handle_issues_cf(
    state: &Arc<AppState>,
    github: &GitHubConfig,
    repo: &str,
    payload: thin::IssuesPayload,
) -> StatusCode {
    if payload.action != "labeled" {
        info!("ignoring issues action");
        return StatusCode::OK;
    }
    let label = match &payload.label {
        Some(l) => l.name.clone(),
        None => return StatusCode::OK,
    };
    if !github.alert_labels.contains(&label.to_lowercase()) {
        info!("ignoring non-alert label: {label}");
        return StatusCode::OK;
    }
    let issue = &payload.issue;
    info!(
        "GitHub issue labeled alert: {repo}#{} label={label}",
        issue.number
    );
    let event = Event::IssueLabeledAlert {
        repo: repo.to_string(),
        number: issue.number,
        title: issue.title.clone(),
        label,
        author: issue.user.login.clone(),
        url: issue.html_url.clone(),
    };
    dispatch::dispatch_github(&event, state, None).await;
    StatusCode::OK
}

#[cfg(feature = "cf-worker")]
async fn handle_push_cf(
    state: &Arc<AppState>,
    repo: &str,
    payload: thin::PushPayload,
) -> StatusCode {
    let branch = branch_from_ref(&payload.r#ref);
    if !is_protected_branch(branch) {
        info!("ignoring push to non-protected branch: {branch}");
        return StatusCode::OK;
    }
    info!(
        "GitHub push to {repo}@{branch}: {} commit(s)",
        payload.commits.len()
    );
    let commits: Vec<CommitSummary> = payload
        .commits
        .iter()
        .take(MAX_COMMITS)
        .map(|c| CommitSummary {
            sha_short: c.id.chars().take(7).collect(),
            message_line: c.message.lines().next().unwrap_or("").to_string(),
            author: c.author.name.clone(),
        })
        .collect();
    let event = Event::BranchPush {
        repo: repo.to_string(),
        branch: branch.to_string(),
        pusher: payload.pusher.name.clone(),
        commits,
        compare_url: payload.compare.clone(),
    };
    dispatch::dispatch_github(&event, state, None).await;
    StatusCode::OK
}

// ---------------------------------------------------------------------------
// Shared inner dispatch helpers (workflow_run / secret_scanning / dependabot)
// Both native and cf-worker extract inner data as serde_json::Value, then
// call these shared functions.
// ---------------------------------------------------------------------------

async fn dispatch_workflow_run(
    state: &Arc<AppState>,
    repo: &str,
    value: serde_json::Value,
) -> StatusCode {
    let run: WorkflowRunData = match serde_json::from_value(value) {
        Ok(r) => r,
        Err(e) => {
            warn!("failed to parse workflow_run data: {e}");
            return StatusCode::OK;
        }
    };
    let conclusion = run.conclusion.unwrap_or_else(|| "unknown".to_string());
    if conclusion != "failure" {
        info!("ignoring workflow_run with conclusion: {conclusion}");
        return StatusCode::OK;
    }
    info!(
        "GitHub workflow_run failed: {repo} workflow={} branch={}",
        run.name, run.head_branch
    );
    let event = Event::WorkflowRunFailed {
        repo: repo.to_string(),
        workflow_name: run.name,
        branch: run.head_branch,
        actor: run.actor.login,
        conclusion,
        url: run.html_url,
    };
    dispatch::dispatch_github(&event, state, None).await;
    StatusCode::OK
}

async fn dispatch_secret_scanning(
    state: &Arc<AppState>,
    repo: &str,
    value: serde_json::Value,
) -> StatusCode {
    let alert: SecretScanningAlertData = match serde_json::from_value(value) {
        Ok(a) => a,
        Err(e) => {
            warn!("failed to parse secret_scanning_alert data: {e}");
            return StatusCode::OK;
        }
    };
    let secret_type = alert
        .secret_type_display_name
        .as_deref()
        .unwrap_or(&alert.secret_type);
    info!("GitHub secret scanning alert: {repo} type={secret_type}");
    let event = Event::SecretScanningAlert {
        repo: repo.to_string(),
        secret_type: secret_type.to_string(),
        url: alert.html_url,
    };
    dispatch::dispatch_github(&event, state, None).await;
    StatusCode::OK
}

async fn dispatch_dependabot(
    state: &Arc<AppState>,
    repo: &str,
    value: serde_json::Value,
) -> StatusCode {
    let alert: DependabotAlertData = match serde_json::from_value(value) {
        Ok(a) => a,
        Err(e) => {
            warn!("failed to parse dependabot_alert data: {e}");
            return StatusCode::OK;
        }
    };
    let severity = alert.severity.to_lowercase();
    if severity != "critical" && severity != "high" {
        info!("ignoring dependabot_alert with severity: {severity}");
        return StatusCode::OK;
    }
    let package = alert
        .dependency
        .as_ref()
        .and_then(|d| d.package.as_ref())
        .map(|p| p.name.as_str())
        .unwrap_or("unknown");
    let summary = alert
        .security_advisory
        .as_ref()
        .map(|a| a.summary.as_str())
        .unwrap_or("No summary available");
    info!("GitHub dependabot alert: {repo} pkg={package} severity={severity}");
    let event = Event::DependabotAlert {
        repo: repo.to_string(),
        package: package.to_string(),
        severity,
        summary: summary.to_string(),
        url: alert.html_url,
    };
    dispatch::dispatch_github(&event, state, None).await;
    StatusCode::OK
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn is_protected_branch(branch: &str) -> bool {
    matches!(branch, "main" | "master") || branch.starts_with("release")
}
