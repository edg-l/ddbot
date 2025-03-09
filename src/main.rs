use std::{collections::HashSet, error::Error, sync::Arc};

use axum::{
    Router,
    body::{Body, Bytes, to_bytes},
    extract::{Request, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use octocrab::{
    Octocrab, issues,
    models::{
        self, UserId,
        webhook_events::{
            WebhookEvent, WebhookEventType,
            payload::{IssuesWebhookEventAction, PullRequestWebhookEventAction},
        },
    },
    params,
};
use tracing::{info, warn};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    run().await?;
    Ok(())
}

#[derive(Debug, Clone)]
struct AppState {
    octo: Arc<Octocrab>,
    webhook_secret: Arc<String>,
    allowed_users: HashSet<UserId>,
}

pub async fn run() -> Result<(), Box<dyn Error>> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt::init();
    let app_id: u64 = std::env::var("GITHUB_APP_ID").unwrap().parse().unwrap();

    let private_key_path = std::env::var("APP_PRIVATE_KEY_PATH").unwrap();
    let webhook_secret = std::env::var("WEBHOOK_SECRET").unwrap();
    let private_key = std::fs::read_to_string(private_key_path).unwrap();
    let key = jsonwebtoken::EncodingKey::from_rsa_pem(private_key.as_bytes()).unwrap();

    let octocrab = Arc::new(Octocrab::builder().app(app_id.into(), key).build().unwrap());

    let mut allowed_users = HashSet::new();
    allowed_users.insert(15859336.into()); // edg-l

    let state = AppState {
        octo: octocrab.clone(),
        webhook_secret: Arc::new(webhook_secret),
        allowed_users,
    };

    // build our application with a single route
    let app = Router::new()
        .route("/", post(webhook_handler))
        .with_state(state);

    // run our app with hyper, listening globally on port 3000
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    axum::serve(listener, app).await.unwrap();

    Ok(())
}

async fn webhook_handler(State(state): State<AppState>, req: Request) -> Response {
    let (parts, body) = req.into_parts();
    let header = parts
        .headers
        .get("X-GitHub-Event")
        .unwrap()
        .to_str()
        .unwrap();

    let bytes = to_bytes(body, 1024 * 50).await.unwrap();

    let event = WebhookEvent::try_from_header_and_body(header, &bytes).unwrap();

    let id = match event.installation {
        Some(x) => match x {
            models::webhook_events::EventInstallation::Full(installation) => installation.id,
            models::webhook_events::EventInstallation::Minimal(event_installation_id) => {
                event_installation_id.id
            }
        },
        None => {
            return StatusCode::OK.into_response();
        }
    };
    let client = state.octo.installation(id).unwrap();

    // Now you can match on event type and call any specific handling logic
    match event.kind {
        WebhookEventType::Ping => info!("Received a ping"),
        WebhookEventType::PullRequest => {
            info!("Received a pull request event");
            if let models::webhook_events::WebhookEventPayload::PullRequest(payload) =
                event.specific
            {
                let repo = event.repository.unwrap();
                match payload.action {
                    PullRequestWebhookEventAction::Edited => todo!(),
                    PullRequestWebhookEventAction::Opened
                    | PullRequestWebhookEventAction::Reopened => {
                        let pulls = client.pulls(repo.owner.unwrap().login, repo.name);
                        let issues = client.issues_by_id(repo.id);
                        let files = pulls.list_files(payload.pull_request.number).await.unwrap();

                        let mut add_labels: Vec<String> = Vec::new();

                        for file in files {
                            if file.filename.contains("client") {
                                add_labels.push("client".to_string());
                            }
                            if file.filename.contains("server") {
                                add_labels.push("server".to_string());
                            }
                            if file.filename.contains("demo") {
                                add_labels.push("demo".to_string());
                            }
                            if file.filename.contains("editor") {
                                add_labels.push("editor".to_string());
                            }
                            if file.filename.contains("engine") {
                                add_labels.push("engine".to_string());
                            }
                            if file.filename.contains("map") {
                                add_labels.push("maps".to_string());
                            }
                            if file.filename.contains("network") {
                                add_labels.push("network".to_string());
                            }
                        }
                        issues
                            .add_labels(payload.number, &add_labels)
                            .await
                            .unwrap();
                    }
                    _ => {}
                }
            }
        }
        WebhookEventType::Issues => {
            if let models::webhook_events::WebhookEventPayload::Issues(payload) = event.specific {
                match payload.action {
                    IssuesWebhookEventAction::Assigned => {}
                    IssuesWebhookEventAction::Closed => {}
                    IssuesWebhookEventAction::Deleted => {}
                    IssuesWebhookEventAction::Edited => {}
                    IssuesWebhookEventAction::Labeled => {}
                    IssuesWebhookEventAction::Opened => {
                        let repo = event.repository.unwrap();
                        let issues = client.issues_by_id(repo.id);
                        issues
                            .add_labels(payload.issue.number, &["triage-needed".to_string()])
                            .await
                            .unwrap();
                    }
                    IssuesWebhookEventAction::Reopened => {}
                    IssuesWebhookEventAction::Unassigned => {}
                    IssuesWebhookEventAction::Unlabeled => {}
                    _ => {}
                }
            }
        }
        WebhookEventType::IssueComment => {
            info!("Received a issue comment request event");
            match event.specific {
                models::webhook_events::WebhookEventPayload::IssueComment(payload) => {
                    let privilege_level = match payload.comment.author_association {
                        models::AuthorAssociation::Collaborator => 1,
                        models::AuthorAssociation::Contributor => 0,
                        models::AuthorAssociation::FirstTimer => 0,
                        models::AuthorAssociation::FirstTimeContributor => 0,
                        models::AuthorAssociation::Mannequin => 0,
                        models::AuthorAssociation::Member => 2,
                        models::AuthorAssociation::None => 0,
                        models::AuthorAssociation::Owner => 2,
                        models::AuthorAssociation::Other(_) => 0,
                        _ => 0,
                    };

                    if privilege_level == 0 && payload.comment.user.id != payload.issue.user.id {
                        return StatusCode::OK.into_response();
                    }

                    if let Some(body) = &payload.comment.body {
                        info!("comment: {:?}", body);
                        let repo = event.repository.unwrap();
                        let issues = client.issues_by_id(repo.id);

                        for line in body.lines() {
                            if let Some(line) = line.strip_prefix("!ddnetbot") {
                                let line = line.trim_start();
                                if let Some(_claim) = line.strip_prefix("claim") {
                                    issues
                                        .add_assignees(
                                            payload.issue.number,
                                            &[payload.comment.user.login.as_str()],
                                        )
                                        .await
                                        .unwrap();
                                    continue;
                                }

                                if let Some(_claim) = line.strip_prefix("unclaim") {
                                    issues
                                        .remove_assignees(
                                            payload.issue.number,
                                            &[payload.comment.user.login.as_str()],
                                        )
                                        .await
                                        .unwrap();
                                    continue;
                                }

                                if let Some(_claim) = line.strip_prefix("ready") {
                                    issues.add_labels(payload.issue.number, &["waiting-for-reviews".to_string()]).await.unwrap();
                                    issues.remove_label(payload.issue.number, "waiting-on-author".to_string()).await.unwrap();
                                    continue;
                                }

                                if let Some(_claim) = line.strip_prefix("author") {
                                    issues.add_labels(payload.issue.number, &["waiting-on-author".to_string()]).await.unwrap();
                                    issues.remove_label(payload.issue.number, "waiting-for-reviews".to_string()).await.unwrap();
                                    continue;
                                }

                                if let Some(cmd_labels) = line.strip_prefix("label") {
                                    let cmd_labels = cmd_labels.split_ascii_whitespace();

                                    let repo_labels =
                                        issues.list_labels_for_repo().send().await.unwrap();

                                    let repo_labels: HashSet<String> =
                                        repo_labels.into_iter().map(|x| x.name).collect();

                                    let labels = issues
                                        .list_labels_for_issue(payload.issue.number)
                                        .send()
                                        .await
                                        .unwrap();

                                    let mut current_labels = HashSet::new();

                                    for label in labels {
                                        current_labels.insert(label.name);
                                    }

                                    for label in cmd_labels {
                                        if let Some(add_label) = label.strip_prefix("+") {
                                            if repo_labels.contains(add_label) {
                                                current_labels.insert(add_label.to_string());
                                            }
                                        } else if let Some(remove_label) = label.strip_prefix("-") {
                                            if repo_labels.contains(remove_label) {
                                                current_labels.remove(remove_label);
                                            }
                                        }
                                    }

                                    let current_labels: Vec<_> =
                                        current_labels.into_iter().collect();

                                    issues
                                        .replace_all_labels(payload.issue.number, &current_labels)
                                        .await
                                        .unwrap();
                                }
                            }
                        }
                    }
                }
                _ => unreachable!(),
            }
        }
        // ...
        _ => warn!("Ignored event"),
    };

    //dbg!(&event);

    StatusCode::OK.into_response()
}
