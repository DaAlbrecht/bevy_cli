use std::{
    net::{IpAddr, SocketAddr, TcpListener},
    path::PathBuf,
    str::FromStr,
    sync::Arc,
};

use anyhow::Context;
use axum::{
    Router,
    extract::{State, WebSocketUpgrade, ws::WebSocket},
    response::Response,
    routing::{any, get, get_service},
};
use futures::{SinkExt, StreamExt};
use tokio::sync::broadcast;
use tower_http::services::ServeDir;
use tracing::info;

use crate::commands::watch::builder::AppBuilder;

pub struct WebServer {
    pub tx: broadcast::Sender<String>,
}

struct AppState {
    tx: broadcast::Sender<String>,
}

impl WebServer {
    pub fn start(builder: &AppBuilder) -> anyhow::Result<Self> {
        let port = 4000;
        let host = IpAddr::from_str("127.0.0.1")?;
        let addr = SocketAddr::new(host, port);

        let (tx, _rx) = broadcast::channel(100);

        let web_server = WebServer { tx: tx.clone() };

        let listener = std::net::TcpListener::bind(addr).with_context(|| {
            anyhow::anyhow!(
                "Failed open your bevy app at: {addr}, is there another process running?"
            )
        })?;

        let _ = listener.set_nonblocking(true);

        let base_path = builder
            .target_dir
            .join("bevy_web")
            .join(builder.build_args.profile())
            .join(builder.bin_target.bin_name.as_str());

        let router = WebServer::build_router(base_path);

        tokio::spawn(serve(listener, router, tx));
        Ok(web_server)
    }

    fn build_router(path: PathBuf) -> Router<Arc<AppState>> {
        // TODO: Fix this, either have a "dev" default index.html or make it possible to inject the
        // auto_reload script
        let mut router = Router::new();

        router = router
            .route(
                "/_bevy_dev/auto_reload.js",
                get(async || {
                    (
                        [(http::header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
                        include_str!(concat!(
                            env!("CARGO_MANIFEST_DIR"),
                            "/assets/web/_bevy_dev/auto_reload.js"
                        )),
                    )
                }),
            )
            // Open a websocket for automatic reloading
            // For now, just echo the messages back
            .route("/_bevy_dev/websocket", any(dev_websocket))
            .fallback_service(get_service(ServeDir::new(path)));
        router
    }

    pub fn reload(&self) {
        let _ = self
            .tx
            .send(serde_json::json!({"type":"reload"}).to_string());
    }
}

async fn serve(
    listener: TcpListener,
    router: Router<Arc<AppState>>,
    tx: broadcast::Sender<String>,
) -> anyhow::Result<()> {
    let app_state = Arc::new(AppState { tx });
    let router = router.with_state(app_state);
    let _ = axum::serve(
        tokio::net::TcpListener::from_std(listener).unwrap(),
        router.into_make_service(),
    )
    .await;

    Ok(())
}
async fn dev_websocket(ws: WebSocketUpgrade, State(state): State<Arc<AppState>>) -> Response {
    ws.on_upgrade(|socket| handle_socket(socket, state))
}

async fn handle_socket(socket: WebSocket, state: Arc<AppState>) {
    // By splitting, we can send and receive at the same time.
    let (mut sender, mut receiver) = socket.split();
    let mut rx = state.tx.subscribe();

    // forward broadcast messages → websocket
    let mut send_task = tokio::spawn(async move {
        while let Ok(msg) = rx.recv().await {
            let _ = sender
                .send(axum::extract::ws::Message::Text(msg.into()))
                .await;
        }
    });

    // read websocket messages
    let mut recv_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = receiver.next().await {
            if let axum::extract::ws::Message::Text(text) = msg {
                info!("Received from client: {}", text);
            }
        }
    });

    // Stop both tasks when one finishes
    tokio::select! {
        _ = (&mut send_task) => recv_task.abort(),
        _ = (&mut recv_task) => send_task.abort(),
    }
}
