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
        webhook_events::{WebhookEvent, WebhookEventType, payload::IssuesWebhookEventAction},
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
        WebhookEventType::PullRequest => info!("Received a pull request event"),
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

                                if let Some(_claim) = line.strip_prefix("bug") {
                                    let labels = issues
                                        .list_labels_for_issue(payload.issue.number)
                                        .send()
                                        .await
                                        .unwrap();
                                    let mut has_label = false;

                                    for page in labels {
                                        if page.name == "bug" {
                                            has_label = true;
                                        }
                                    }

                                    if has_label {
                                        issues
                                            .remove_label(payload.issue.number, "bug")
                                            .await
                                            .unwrap();
                                    } else {
                                        issues
                                            .add_labels(payload.issue.number, &["bug".to_string()])
                                            .await
                                            .unwrap();
                                    }
                                    continue;
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
