use std::path::PathBuf;
use std::sync::Arc;

use clap::Parser;
use hyper::{Body, StatusCode};
use livereload_server::{handle_websocket, serve_file};
use warp::path::{peek, Peek};
use warp::Filter;

const INJECTED_SCRIPT: &str = include_str!("./injected_script.js");

lazy_static::lazy_static! {
    static ref INJECTION_PAYLOAD: String = {
        format!(r#"<script type="text/javascript">{INJECTED_SCRIPT}</script>"#)
    };
}

#[derive(Debug, Clone, Parser)]
#[clap(rename_all = "kebab-case")]
struct Args {
    #[clap(short, long)]
    pub target_dir: PathBuf,

    #[clap(short, long, default_value = "5500")]
    pub port: u16,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv::dotenv()?;
    pretty_env_logger::try_init()?;

    let args = Arc::new(Args::parse());

    let args_filter = {
        let args = args.clone();
        warp::any().map(move || args.clone())
    };

    let ws_route = warp::ws().and(args_filter.clone()).map(
        move |ws: warp::ws::Ws, args: Arc<Args>| {
            ws.on_upgrade(move |websocket| async move {
                if let Err(err) =
                    handle_websocket(&args.as_ref().target_dir, websocket).await
                {
                    log::error!(
                        "An error occurred during websocket connection: {err}"
                    );
                }
            })
        },
    );

    let default_route = peek().and(args_filter).then(
        |peek: Peek, args: Arc<Args>| async move {
            let builder = warp::http::Response::builder();

            match serve_file(
                &args.as_ref().target_dir,
                peek.as_str(),
                INJECTION_PAYLOAD.as_str(),
            )
            .await
            {
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
