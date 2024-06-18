use eyre::Result;
use futures_util::{StreamExt, TryStreamExt};
use tokio_tungstenite::connect_async;

#[tokio::main]
async fn main() -> Result<()> {
    let (ws_stream, _) = connect_async("ws://127.0.0.1:3000/22333").await?;
    let (_, mut read) = ws_stream.split();

    while let Ok(Some(msg)) = read.try_next().await {
        dbg!(msg);
    }

    Ok(())
}
