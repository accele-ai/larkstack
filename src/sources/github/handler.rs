//! Axum handler for `POST /github/webhook` — receives GitHub webhook payloads,
//! converts them to [`Event`]s, and dispatches immediately (no debounce).

use std::sync::Arc;

use axum::{
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
};
use octocrab::models::webhook_events::{
    WebhookEvent, WebhookEventPayload,
    payload::{
        DependabotAlertWebhookEventAction, IssuesWebhookEventAction, PullRequestWebhookEventAction,
        SecretScanningAlertWebhookEventAction, WorkflowRunWebhookEventAction,
    },
};
use tracing::{info, warn};

use crate::{
    config::{AppState, GitHubConfig},
    dispatch,
    event::{CommitSummary, Event},
};

use super::utils::{branch_from_ref, verify_github_signature};

const MAX_COMMITS: usize = 5;

/// Minimal struct for lightweight repo-name extraction before full deserialization.
#[derive(serde::Deserialize)]
struct RepoProbe {
    repository: RepoName,
}

#[derive(serde::Deserialize)]
struct RepoName {
    name: String,
}

// ---------------------------------------------------------------------------
// Thin helper structs for octocrab payloads stored as serde_json::Value
// ---------------------------------------------------------------------------

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

/// Handles incoming GitHub webhook requests.
///
/// 1. Verifies the `X-Hub-Signature-256` HMAC header.
/// 2. Routes by the `X-GitHub-Event` header via octocrab's `WebhookEvent`.
/// 3. Converts to an [`Event`] and dispatches immediately.
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

    // Repo whitelist filter — skip events from repos not on the list.
    if !github.repo_whitelist.is_empty() {
        match serde_json::from_slice::<RepoProbe>(&body) {
            Ok(probe) => {
                if !github.repo_whitelist.contains(&probe.repository.name) {
                    info!(
                        "ignoring event from non-whitelisted repo: {}",
                        probe.repository.name
                    );
                    return StatusCode::OK;
                }
            }
            Err(_) => {
                warn!("could not extract repository name for whitelist check");
            }
        }
    }

    let event_type = headers
        .get("x-github-event")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    let webhook = match WebhookEvent::try_from_header_and_body(event_type, &body) {
        Ok(ev) => ev,
        Err(e) => {
            warn!("failed to parse GitHub webhook event: {e}");
            return StatusCode::BAD_REQUEST;
        }
    };

    // Extract repo full_name from the top-level repository field.
    let repo = webhook
        .repository
        .as_ref()
        .and_then(|r| r.full_name.as_ref())
        .cloned()
        .unwrap_or_default();

    match webhook.specific {
        WebhookEventPayload::PullRequest(payload) => {
            handle_pull_request(&state, github, &repo, *payload).await
        }
        WebhookEventPayload::Issues(payload) => {
            handle_issues(&state, github, &repo, *payload).await
        }
        WebhookEventPayload::Push(payload) => handle_push(&state, &repo, *payload).await,
        WebhookEventPayload::WorkflowRun(payload) => {
            handle_workflow_run(&state, &repo, *payload).await
        }
        WebhookEventPayload::SecretScanningAlert(payload) => {
            handle_secret_scanning(&state, &repo, *payload).await
        }
        WebhookEventPayload::DependabotAlert(payload) => {
            handle_dependabot(&state, &repo, *payload).await
        }
        _ => {
            info!("ignoring GitHub event type: {event_type}");
            StatusCode::OK
        }
    }
}

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
            dispatch::dispatch(&event, state, None).await;
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
            dispatch::dispatch(&event, state, dm_email.as_deref()).await;
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
            dispatch::dispatch(&event, state, None).await;
            StatusCode::OK
        }
        _ => {
            info!("ignoring pull_request action for {repo}#{number}");
            StatusCode::OK
        }
    }
}

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
    dispatch::dispatch(&event, state, None).await;
    StatusCode::OK
}

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
    dispatch::dispatch(&event, state, None).await;
    StatusCode::OK
}

fn is_protected_branch(branch: &str) -> bool {
    matches!(branch, "main" | "master") || branch.starts_with("release")
}

async fn handle_workflow_run(
    state: &Arc<AppState>,
    repo: &str,
    payload: octocrab::models::webhook_events::payload::WorkflowRunWebhookEventPayload,
) -> StatusCode {
    if payload.action != WorkflowRunWebhookEventAction::Completed {
        info!("ignoring workflow_run action");
        return StatusCode::OK;
    }

    let run: WorkflowRunData = match serde_json::from_value(payload.workflow_run) {
        Ok(r) => r,
        Err(e) => {
            warn!("failed to parse workflow_run data: {e}");
            return StatusCode::BAD_REQUEST;
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
    dispatch::dispatch(&event, state, None).await;
    StatusCode::OK
}

async fn handle_secret_scanning(
    state: &Arc<AppState>,
    repo: &str,
    payload: octocrab::models::webhook_events::payload::SecretScanningAlertWebhookEventPayload,
) -> StatusCode {
    if payload.action != SecretScanningAlertWebhookEventAction::Created {
        info!("ignoring secret_scanning_alert action");
        return StatusCode::OK;
    }

    let alert: SecretScanningAlertData = match serde_json::from_value(payload.alert) {
        Ok(a) => a,
        Err(e) => {
            warn!("failed to parse secret_scanning_alert data: {e}");
            return StatusCode::BAD_REQUEST;
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
    dispatch::dispatch(&event, state, None).await;
    StatusCode::OK
}

async fn handle_dependabot(
    state: &Arc<AppState>,
    repo: &str,
    payload: octocrab::models::webhook_events::payload::DependabotAlertWebhookEventPayload,
) -> StatusCode {
    if payload.action != DependabotAlertWebhookEventAction::Created {
        info!("ignoring dependabot_alert action");
        return StatusCode::OK;
    }

    let alert: DependabotAlertData = match serde_json::from_value(payload.alert) {
        Ok(a) => a,
        Err(e) => {
            warn!("failed to parse dependabot_alert data: {e}");
            return StatusCode::BAD_REQUEST;
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
    dispatch::dispatch(&event, state, None).await;
    StatusCode::OK
}
