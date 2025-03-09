use std::{error::Error, sync::Arc};

use axum::{
    body::{to_bytes, Body, Bytes}, extract::{Request, State}, http::StatusCode, response::{IntoResponse, Response}, routing::{get, post}, Router
};
use octocrab::{models::{self, webhook_events::{WebhookEvent, WebhookEventType}}, params, Octocrab};
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

    let state = AppState {
        octo: octocrab.clone(),
        webhook_secret: Arc::new(webhook_secret)
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

    let bytes = to_bytes(body, 1024 * 10).await.unwrap();

    let event = WebhookEvent::try_from_header_and_body(header, &bytes).unwrap();

    let id = match event.installation {
        Some(x) => match x {
            models::webhook_events::EventInstallation::Full(installation) => installation.id,
            models::webhook_events::EventInstallation::Minimal(event_installation_id) => event_installation_id.id,
        },
        None => panic!(),
    };
    let client = state.octo.installation(id).unwrap();

    // Now you can match on event type and call any specific handling logic
    match event.kind {
        WebhookEventType::Ping => info!("Received a ping"),
        WebhookEventType::PullRequest => info!("Received a pull request event"),
        WebhookEventType::Issues => info!("Received a issue request event"),
        WebhookEventType::IssueComment => {
            info!("Received a issue comment request event");
            match event.specific {
                models::webhook_events::WebhookEventPayload::IssueComment(payload) => {
                    info!("comment: {:?}", payload.comment.body_text);
                    info!("comment: {:?}", payload.comment.body);
                    info!("comment: {:?}", payload.comment.body);
                    let repo = event.repository.as_ref().unwrap();
                    dbg!(repo.id);
                    dbg!(&repo.issues_url);
                    let issues = client.issues_by_id(repo.id);

                    issues.update(payload.issue.number).labels(&["bug".to_string()]).send().await.unwrap();
                    issues.create_comment(payload.issue.number, "hello world").await.unwrap();
                },
                _ => unreachable!(),
            }
        },
        // ...
        _ => warn!("Ignored event"),
    };

    //dbg!(&event);

    StatusCode::OK.into_response()
}
