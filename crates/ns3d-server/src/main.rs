//! ns3d-server — WebSocket + REST sensing server
//!
//! Listens for CSI UDP frames from ESP32-S3 nodes, runs the signal pipeline,
//! and broadcasts [`ScanResult`] JSON to all connected WebSocket clients.
//!
//! Usage:
//! ```bash
//! ns3d-server                         # default: UDP 5006, HTTP 3000
//! ns3d-server --demo                  # synthetic data, no hardware needed
//! ns3d-server --udp-port 5007 --http-port 8080
//! ```

use std::{net::SocketAddr, sync::Arc, time::Duration};

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    response::{Html, IntoResponse},
    routing::get,
    Json, Router,
};
use clap::Parser;
use ns3d_core::{parse_udp_frame, CsiFrame, ScanResult};
use ns3d_signal::SignalPipeline;
use serde_json::json;
use tokio::{net::UdpSocket, sync::broadcast, time::interval};
use tracing::{error, info, warn};

// ── CLI ───────────────────────────────────────────────────────────────────────

#[derive(Parser, Debug)]
#[command(name = "ns3d-server", about = "Network-Scanner-3D sensing server")]
struct Cli {
    /// UDP port to receive ESP32-S3 CSI frames on.
    #[arg(long, default_value = "5006", env = "NS3D_UDP_PORT")]
    udp_port: u16,

    /// HTTP/WebSocket port.
    #[arg(long, default_value = "3000", env = "NS3D_HTTP_PORT")]
    http_port: u16,

    /// Run with simulated CSI data (no ESP32 hardware required).
    #[arg(long)]
    demo: bool,
}

// ── Shared state ──────────────────────────────────────────────────────────────

type Tx = broadcast::Sender<String>;

#[derive(Clone)]
struct AppState {
    tx: Arc<Tx>,
}

// ── Entry point ───────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "ns3d_server=info,tower_http=warn".into()),
        )
        .init();

    let cli = Cli::parse();

    let (tx, _rx) = broadcast::channel::<String>(256);
    let tx = Arc::new(tx);

    // Start data source (real UDP or demo generator)
    let tx_bg = tx.clone();
    if cli.demo {
        info!("Running in DEMO mode — generating synthetic CSI data");
        tokio::spawn(run_demo(tx_bg));
    } else {
        info!("Listening for ESP32-S3 CSI frames on UDP :{}", cli.udp_port);
        tokio::spawn(run_udp_listener(tx_bg, cli.udp_port));
    }

    // Build HTTP/WS router
    let state = AppState { tx };
    let app = Router::new()
        .route("/",                get(index_handler))
        .route("/ws",              get(ws_handler))
        .route("/api/v1/status",   get(status_handler))
        .with_state(state)
        .layer(
            tower_http::cors::CorsLayer::permissive(),
        );

    let addr: SocketAddr = format!("0.0.0.0:{}", cli.http_port).parse()?;
    info!("HTTP server at http://{}", addr);
    info!("WebSocket   at ws://{}/ws", addr);
    info!("Open http://localhost:{} in your browser", cli.http_port);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

// ── Data sources ──────────────────────────────────────────────────────────────

/// Read UDP packets from ESP32 nodes, process them, broadcast results.
async fn run_udp_listener(tx: Arc<Tx>, port: u16) {
    let socket = match UdpSocket::bind(format!("0.0.0.0:{}", port)).await {
        Ok(s)  => s,
        Err(e) => { error!("Cannot bind UDP:{}: {}", port, e); return; }
    };

    let mut pipeline = SignalPipeline::new();
    let mut buf = [0u8; 2048];

    loop {
        match socket.recv_from(&mut buf).await {
            Ok((n, src)) => {
                if let Some(frame) = parse_udp_frame(&buf[..n]) {
                    info!(node = frame.node_id, rssi = frame.rssi, src = %src, "CSI frame received");
                    let result = pipeline.process(frame);
                    broadcast_result(&tx, &result);
                } else {
                    warn!(src = %src, "Received unrecognised UDP packet ({} bytes)", n);
                }
            }
            Err(e) => error!("UDP recv error: {}", e),
        }
    }
}

/// Generate synthetic CSI frames at ~20 Hz for demo / development.
async fn run_demo(tx: Arc<Tx>) {
    let mut pipeline = SignalPipeline::new();
    let mut ticker   = interval(Duration::from_millis(50)); // 20 Hz
    let mut t        = 0u64;

    loop {
        ticker.tick().await;
        t += 1;

        // Simulate 1 node, 1 person moving and breathing
        let frame = synthetic_frame(1, t);
        let result = pipeline.process(frame);
        broadcast_result(&tx, &result);
    }
}

/// Build a synthetic CSI frame with a breathing signature at ~0.25 Hz (15 BPM).
fn synthetic_frame(node_id: u8, tick: u64) -> CsiFrame {
    use ns3d_core::CSI_SUBCARRIERS;

    let t_sec = tick as f32 * 0.05; // 20 Hz
    let breath_phase = 2.0 * std::f32::consts::PI * 0.25 * t_sec; // 15 BPM
    let heart_phase  = 2.0 * std::f32::consts::PI * 1.2 * t_sec;  // 72 BPM

    // Base amplitude + breathing envelope + heart-rate ripple + noise
    let base_amp: f32 = 200.0;
    let breath_amp: f32 = base_amp + 30.0 * breath_phase.sin() + 5.0 * heart_phase.sin();

    let subcarriers = (0..CSI_SUBCARRIERS)
        .map(|i| {
            let phase = (i as f32) * 0.2 + t_sec * 0.1;
            // Add person-induced phase variation
            let person_phase = phase + 0.8 * breath_phase.sin();
            [
                breath_amp * person_phase.cos(),
                breath_amp * person_phase.sin(),
            ]
        })
        .collect();

    CsiFrame {
        timestamp_ms: tick * 50,
        node_id,
        channel: 6,
        rssi: -55,
        subcarriers,
    }
}

fn broadcast_result(tx: &Tx, result: &ScanResult) {
    if let Ok(json) = serde_json::to_string(result) {
        let _ = tx.send(json); // ignore "no receivers" error
    }
}

// ── HTTP handlers ─────────────────────────────────────────────────────────────

async fn index_handler() -> Html<&'static str> {
    Html(include_str!("../../../web/index.html"))
}

async fn status_handler() -> Json<serde_json::Value> {
    Json(json!({
        "status":   "ok",
        "version":  env!("CARGO_PKG_VERSION"),
        "ws_path":  "/ws",
    }))
}

// ── WebSocket handler ─────────────────────────────────────────────────────────

async fn ws_handler(
    ws:             WebSocketUpgrade,
    State(state):   State<AppState>,
) -> impl IntoResponse {
    ws.on_upgrade(|socket| handle_socket(socket, state.tx))
}

async fn handle_socket(mut socket: WebSocket, tx: Arc<Tx>) {
    let mut rx = tx.subscribe();
    info!("WebSocket client connected");

    loop {
        match rx.recv().await {
            Ok(msg) => {
                if socket.send(Message::Text(msg)).await.is_err() {
                    break; // client disconnected
                }
            }
            Err(broadcast::error::RecvError::Lagged(n)) => {
                warn!("WebSocket client lagged by {} messages", n);
            }
            Err(broadcast::error::RecvError::Closed) => break,
        }
    }

    info!("WebSocket client disconnected");
}
