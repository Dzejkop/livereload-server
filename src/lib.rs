use std::path::Path;

use futures_util::{SinkExt, StreamExt};
use lazy_static::lazy_static;
use notify::{Config, Event, RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::mpsc;
use warp::ws::Message;

const INJECTED_SCRIPT: &str = include_str!("./injected_script.js");
const NOTIFY_CHANNEL_CAPACITY: usize = 5;

lazy_static! {
    static ref INJECTION_PAYLOAD: String = {
        format!(r#"<script type="text/javascript">{INJECTED_SCRIPT}</script>"#)
    };
}

mod inject;
mod serve;

pub use self::serve::serve_file;

pub fn async_watcher(
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

pub async fn handle_websocket(
    target_dir_path: impl AsRef<Path>,
    websocket: warp::ws::WebSocket,
) -> anyhow::Result<()> {
    let (mut watcher, mut fs_events_receiver) = async_watcher()?;

    let target_dir_path = target_dir_path.as_ref();

    watcher.watch(target_dir_path, RecursiveMode::Recursive)?;

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
