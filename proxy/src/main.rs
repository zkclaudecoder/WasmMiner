use clap::Parser;
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::tungstenite::Message;

#[derive(Parser)]
#[command(name = "wasmminer-proxy", about = "WebSocket-to-TCP proxy for stratum mining")]
struct Args {
    /// Port to listen on
    #[arg(long, default_value_t = 9144)]
    port: u16,

    /// Allowed pool addresses (host:port). If empty, all pools allowed.
    #[arg(long)]
    allowed_pools: Vec<String>,
}

#[derive(Deserialize)]
struct ConnectMsg {
    pool: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let addr = format!("0.0.0.0:{}", args.port);

    let listener = TcpListener::bind(&addr).await?;
    eprintln!("WasmMiner proxy listening on ws://{}", addr);

    if !args.allowed_pools.is_empty() {
        eprintln!("Allowed pools: {:?}", args.allowed_pools);
    }

    let allowed_pools: std::sync::Arc<Vec<String>> = std::sync::Arc::new(args.allowed_pools);

    loop {
        let (stream, peer) = listener.accept().await?;
        let allowed = allowed_pools.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_connection(stream, allowed).await {
                eprintln!("[{}] Connection error: {}", peer, e);
            } else {
                eprintln!("[{}] Connection closed", peer);
            }
        });
    }
}

async fn handle_connection(
    stream: TcpStream,
    allowed_pools: std::sync::Arc<Vec<String>>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let ws_stream = tokio_tungstenite::accept_async(stream).await?;
    let (mut ws_sink, mut ws_stream_rx) = ws_stream.split();

    // First message must be {"pool": "host:port"}
    let pool_addr = loop {
        let msg = ws_stream_rx
            .next()
            .await
            .ok_or("WebSocket closed before connect message")??;

        match msg {
            Message::Text(text) => {
                let connect: ConnectMsg = serde_json::from_str(&text)?;
                break connect.pool;
            }
            Message::Ping(data) => {
                ws_sink.send(Message::Pong(data)).await?;
                continue;
            }
            Message::Close(_) => return Ok(()),
            _ => continue,
        }
    };

    // Check allowed pools
    if !allowed_pools.is_empty() && !allowed_pools.contains(&pool_addr) {
        ws_sink
            .send(Message::Text(
                serde_json::json!({"error": "Pool not in allowed list"}).to_string().into(),
            ))
            .await?;
        ws_sink.close().await?;
        return Ok(());
    }

    eprintln!("Connecting to pool: {}", pool_addr);
    let tcp_stream = TcpStream::connect(&pool_addr).await?;
    let (tcp_reader, mut tcp_writer) = tcp_stream.into_split();
    let mut tcp_lines = BufReader::new(tcp_reader).lines();

    // Send confirmation
    ws_sink
        .send(Message::Text(
            serde_json::json!({"connected": true, "pool": pool_addr}).to_string().into(),
        ))
        .await?;

    // Bridge: TCP lines -> WS messages
    let (ws_tx, mut ws_rx) = tokio::sync::mpsc::channel::<String>(64);

    // Task: read TCP lines and send to channel
    let tcp_to_ws = tokio::spawn(async move {
        while let Ok(Some(line)) = tcp_lines.next_line().await {
            if ws_tx.send(line).await.is_err() {
                break;
            }
        }
    });

    // Task: read from channel and WS, bridge both directions
    let bridge = tokio::spawn(async move {
        loop {
            tokio::select! {
                // TCP -> WS
                Some(line) = ws_rx.recv() => {
                    if ws_sink.send(Message::Text(line.into())).await.is_err() {
                        break;
                    }
                }
                // WS -> TCP
                msg = ws_stream_rx.next() => {
                    match msg {
                        Some(Ok(Message::Text(text))) => {
                            let mut data = Vec::from(text.as_bytes());
                            data.push(b'\n');
                            if tcp_writer.write_all(&data).await.is_err() {
                                break;
                            }
                            if tcp_writer.flush().await.is_err() {
                                break;
                            }
                        }
                        Some(Ok(Message::Close(_))) | None => break,
                        Some(Err(_)) => break,
                        _ => continue,
                    }
                }
                else => break,
            }
        }
    });

    let _ = tokio::try_join!(tcp_to_ws, bridge);
    Ok(())
}
