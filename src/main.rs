use std::path::{Path, PathBuf};
use std::sync::Arc;

use clap::Parser;
use futures_util::{SinkExt, StreamExt};
use hyper::{Body, StatusCode};
use notify::{Config, Event, RecommendedWatcher, RecursiveMode, Watcher};
use regex::Regex;
use tokio::sync::mpsc;
use warp::path::{peek, Peek};
use warp::ws::Message;
use warp::Filter;

const INJECTED_SCRIPT: &str = include_str!("./injected_script.js");
const NOTIFY_CHANNEL_CAPACITY: usize = 5;

#[derive(Debug, Clone, Parser)]
#[clap(rename_all = "kebab-case")]
struct Args {
    #[clap(short, long)]
    target_dir: PathBuf,

    #[clap(short, long)]
    non_recursive: bool,

    #[clap(short, long, default_value = "100")]
    interval_ms: u64,

    #[clap(short, long, default_value = "5500")]
    port: u16,
}

async fn serve_file(
    target_dir: impl AsRef<Path>,
    path_in_request: &str,
) -> anyhow::Result<Option<Vec<u8>>> {
    let target_dir = target_dir.as_ref();

    let mut path = path_in_request;

    if path == "/" || path.is_empty() {
        path = "/index.html";
    }

    let path = path.strip_prefix('/').unwrap_or(path);

    let path_in_target_dir = target_dir.join(path);

    if !path_in_target_dir.exists() {
        return Ok(None);
    }

    let body = if path.ends_with(".html") {
        log::info!(
            "Serving HTML requested at {path} with {path_in_target_dir:?}"
        );

        let content = tokio::fs::read_to_string(path_in_target_dir).await?;

        let regex = Regex::new(r#"</body>"#)?;

        let data = regex
            .replace(
                content.as_str(),
                format!(r#"<script type="text/javascript">{INJECTED_SCRIPT}</script></body>"#),
            )
            .to_string();

        data.bytes().collect()
    } else {
        log::info!("Serving a non HTML file requested as {path_in_request} with {path_in_target_dir:?}");

        tokio::fs::read(path_in_target_dir).await?
    };

    Ok(Some(body))
}

fn async_watcher(
) -> notify::Result<(RecommendedWatcher, mpsc::Receiver<notify::Result<Event>>)>
{
    let (notify_event_tx, notify_event_rx) =
        mpsc::channel(NOTIFY_CHANNEL_CAPACITY);

    let watcher = RecommendedWatcher::new(
        move |res| {
            match notify_event_tx.try_send(res) {
            Ok(()) => (),
            Err(err) => log::error!("Failed to send an event to channel, the channel is likely closed: {err}"),
        }
        },
        Config::default(),
    )?;

    Ok((watcher, notify_event_rx))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv::dotenv()?;
    pretty_env_logger::try_init()?;

    let args = Arc::new(Args::parse());

    let ws_route = {
        let args = args.clone();
        warp::ws().map(move |ws: warp::ws::Ws| {
            let args = args.clone();

            ws.on_upgrade(move |websocket| async move {
                if let Err(err) = handle_websocket(args, websocket).await {
                    log::error!(
                        "An error occurred during websocket connection: {err}"
                    );
                }
            })
        })
    };

    let args_filter = {
        let args = args.clone();
        warp::any().map(move || args.clone())
    };

    let default_route = peek().and(args_filter).then(
        |peek: Peek, args: Arc<Args>| async move {
            let builder = warp::http::Response::builder();

            match serve_file(&args.as_ref().target_dir, peek.as_str()).await {
                Ok(Some(res)) => {
                    builder.status(StatusCode::OK).body(Body::from(res))
                }
                Ok(None) => {
                    builder.status(StatusCode::NOT_FOUND).body(Body::empty())
                }
                Err(err) => {
                    let err_msg = err.to_string();
                    builder
                        .status(StatusCode::INTERNAL_SERVER_ERROR)
                        .body(Body::from(err_msg))
                }
            }
        },
    );

    let routes = ws_route.or(default_route);

    let port = args.port;
    log::info!("Serving on localhost:{port}");

    warp::serve(routes).run(([127, 0, 0, 1], port)).await;

    Ok(())
}

async fn handle_websocket(
    args: Arc<Args>,
    websocket: warp::ws::WebSocket,
) -> anyhow::Result<()> {
    let (mut watcher, mut fs_events_receiver) = async_watcher()?;

    watcher.watch(&args.as_ref().target_dir, RecursiveMode::Recursive)?;

    let (mut websocket_tx, mut websocket_rx) = websocket.split();

    loop {
        tokio::select! {
            Some(Ok(_)) = fs_events_receiver.recv() => {
                let s = websocket_tx.send(Message::text("reload"));

                s.await?;
            }
            Some(message) = websocket_rx.next() => {
                match message {
                    Ok(msg) => {
                        if msg.is_close() {
                            log::debug!("Websocket connection is closed");
                            break;
                        }
                    }
                    Err(err) => {
                        log::error!("An error occured: {err}");
                        break;
                    }
                }
            }
        }
    }

    Ok(())
}
