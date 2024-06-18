use std::{path::Path, process::Stdio, sync::Arc};

use eyre::Result;
use futures_util::{lock::Mutex, SinkExt, StreamExt};
use tokio::{io::{AsyncBufReadExt, BufReader}, process::Command};
use tokio_tungstenite::{connect_async, tungstenite::Message};

#[tokio::main]
async fn main() -> Result<()> {
    let path = Path::new("/home/saki/deploykit-backend");

    let (ws_stream, _) = connect_async("ws://127.0.0.1:3000/22333").await?;

    Command::new("cargo")
        .arg("clean")
        .current_dir(path)
        .output()
        .await?;

    let build = Command::new("cargo")
        .arg("build")
        .current_dir(path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    let build_stdout = build.stdout.unwrap();
    let build_stderr = build.stderr.unwrap();

    let (write, _) = ws_stream.split();
    let write = Arc::new(Mutex::new(write));
    let wc = write.clone();

    let mut out = BufReader::new(build_stdout).lines();
    let mut err = tokio::io::BufReader::new(build_stderr).lines();

    tokio::spawn(async move {
        while let Ok(Some(v)) = err.next_line().await {
            let mut lock = wc.lock().await;
            lock.send(Message::Text(v)).await.ok();
        }    
    });

    while let Ok(Some(v)) = out.next_line().await {
        let mut lock = write.lock().await;
        lock.send(Message::Text(v)).await?;
    }

    Ok(())
}
