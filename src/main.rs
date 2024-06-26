use std::{
    collections::HashMap,
    env,
    net::SocketAddr,
    sync::{Arc, Mutex},
};

use eyre::{Context, Result};
use futures_channel::mpsc::{unbounded, UnboundedSender};
use futures_util::{future, pin_mut, StreamExt, TryStreamExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::tungstenite::{
    handshake::server::{ErrorResponse, Request, Response},
    Message,
};
use tracing::{error, info, level_filters::LevelFilter};
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter, Layer};

type Tx = (UnboundedSender<Message>, String);
type PeerMap = Arc<Mutex<HashMap<SocketAddr, Tx>>>;

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    let env_log = EnvFilter::try_from_default_env();

    if let Ok(filter) = env_log {
        tracing_subscriber::registry()
            .with(
                fmt::layer()
                    .event_format(
                        tracing_subscriber::fmt::format()
                            .with_file(true)
                            .with_line_number(true),
                    )
                    .with_filter(filter),
            )
            .init();
    } else {
        tracing_subscriber::registry()
            .with(
                fmt::layer()
                    .event_format(
                        tracing_subscriber::fmt::format()
                            .with_file(true)
                            .with_line_number(true),
                    )
                    .with_filter(LevelFilter::INFO),
            )
            .init();
    }

    let state = PeerMap::new(Mutex::new(HashMap::new()));

    let addr = env::var("buildit_monitor_server")?;
    let listener = TcpListener::bind(&addr).await?;
    info!("Listening on: {}", addr);

    while let Ok((stream, addr)) = listener.accept().await {
        tokio::spawn(handle_connection(stream, addr, state.clone()));
    }

    Ok(())
}

async fn handle_connection(
    raw_stream: TcpStream,
    addr: SocketAddr,
    peer_map: PeerMap,
) -> Result<()> {
    info!("New connect: {addr}");
    let mut path = None;
    let callback = |req: &Request, res: Response| -> Result<Response, ErrorResponse> {
        path = Some(req.uri().path().to_string());
        Ok(res)
    };

    let ws_stream = tokio_tungstenite::accept_hdr_async(raw_stream, callback)
        .await
        .context("Error during the websocket handshake occurred")?;

    if path.is_none() {
        error!("{addr} request has no path params.");
        return Ok(());
    }

    let path = path.unwrap();

    let (outgoing, incoming) = ws_stream.split();

    let (tx, rx) = unbounded();
    peer_map.lock().unwrap().insert(addr, (tx, path.clone()));

    let broadcast_incoming = incoming.try_for_each(|msg| {
        info!(
            "Received a message from {}: {:?}",
            addr,
            msg
        );

        let peers = peer_map.lock().unwrap();

        // We want to broadcast the message to everyone except ourselves.
        let broadcast_recipients = peers
            .iter()
            .filter(|(peer_addr, _)| peer_addr != &&addr)
            .map(|(_, (ws_sink, port))| (ws_sink, port));

        for recp in broadcast_recipients {
            let recp_port = recp.1;

            if *recp_port == path {
                recp.0.unbounded_send(msg.clone()).unwrap();
            }
        }

        future::ok(())
    });

    let receive_from_others = rx.map(Ok).forward(outgoing);

    pin_mut!(broadcast_incoming, receive_from_others);
    future::select(broadcast_incoming, receive_from_others).await;

    info!("{} disconnected", &addr);
    peer_map.lock().unwrap().remove(&addr);

    Ok(())
}
