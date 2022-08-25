use std::path::{Path, PathBuf};
use std::sync::Arc;

use clap::Parser;
use futures_util::{SinkExt, StreamExt};
use notify::{Config, Event, RecommendedWatcher, RecursiveMode, Watcher};
use regex::Regex;
use tokio::sync::mpsc;
use warp::path::{peek, Peek};
use warp::ws::Message;
use warp::Filter;

const INJECTED_SCRIPT: &str = include_str!("./injected_script.js");

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
    mut path: &str,
) -> anyhow::Result<Vec<u8>> {
    let target_dir = target_dir.as_ref();

    if path == "/" || path.is_empty() {
        path = "/index.html";
    }

    let path = path.strip_prefix('/').unwrap_or(path);

    log::info!("Serving {path}");

    let body = if path.ends_with(".html") {
        let content = tokio::fs::read_to_string(target_dir.join(path)).await?;

        let regex = Regex::new(r#"</body>"#)?;

        let data = regex
            .replace(
                content.as_str(),
                format!(r#"<script type="text/javascript">{INJECTED_SCRIPT}</script></body>"#),
            )
            .to_string();

        data.bytes().collect()
    } else {
        tokio::fs::read(target_dir.join(path)).await?
    };

    Ok(body)
}

fn async_watcher(
) -> notify::Result<(RecommendedWatcher, mpsc::Receiver<notify::Result<Event>>)>
{
    let (tx, rx) = mpsc::channel(1);

    let watcher = RecommendedWatcher::new(
        move |res| {
            tx.try_send(res).unwrap();
        },
        Config::default(),
    )?;

    Ok((watcher, rx))
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
                let (mut watcher, mut rx) = async_watcher().unwrap();

                watcher
                    .watch(&args.as_ref().target_dir, RecursiveMode::Recursive)
                    .unwrap();

                let (mut tx, _) = websocket.split();

                while let Some(Ok(_)) = rx.recv().await {
                    let s = tx.send(Message::text("reload"));

                    s.await.unwrap();
                }
            })
        })
    };

    let args_filter = {
        let args = args.clone();
        warp::any().map(move || args.clone())
    };

    let default_route = peek().and(args_filter).and_then(
        |peek: Peek, args: Arc<Args>| async move {
            log::info!("Requested = {}", peek.as_str());

            match serve_file(&args.as_ref().target_dir, peek.as_str()).await {
                Ok(res) => Ok(warp::reply::html(res)),
                Err(_) => Err(warp::reject()),
            }
        },
    );

    let routes = ws_route.or(default_route);

    let port = args.port;
    log::info!("Serving on localhost:{port}");

    warp::serve(routes).run(([127, 0, 0, 1], port)).await;

    Ok(())
}
