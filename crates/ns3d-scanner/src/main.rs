//! ns3d-scanner — terminal scanner for first responders
//!
//! Listens for CSI frames from ESP32-S3 nodes and prints human-readable
//! alerts to stdout. Designed for laptop-in-field use without a browser.
//!
//! Usage:
//! ```bash
//! ns3d-scanner                         # listen on 0.0.0.0:5006
//! ns3d-scanner --port 5007             # custom port
//! ns3d-scanner --log scan.jsonl        # log all results to file
//! ```
//!
//! Output example:
//! ```
//! [10:32:05] NODE 1  ch6  -58 dBm  ██ PERSON DETECTED  breath=14.8 bpm  HR=71 bpm  confidence=93%
//! [10:32:05] NODE 1  ALERT: NO MOTION FOR 65s — check on person
//! ```

use std::{
    net::SocketAddr,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::Context;
use clap::Parser;
use ns3d_core::{parse_udp_frame, ScanResult};
use ns3d_signal::SignalPipeline;
use tokio::{fs::OpenOptions, io::AsyncWriteExt, net::UdpSocket};
use tracing::{error, info};

// ── CLI ───────────────────────────────────────────────────────────────────────

#[derive(Parser, Debug)]
#[command(
    name    = "ns3d-scanner",
    about   = "Network-Scanner-3D — first-responder terminal scanner",
    version
)]
struct Cli {
    /// UDP port to listen on.
    #[arg(long, short, default_value = "5006", env = "NS3D_UDP_PORT")]
    port: u16,

    /// Bind address.
    #[arg(long, default_value = "0.0.0.0")]
    bind: String,

    /// Optional JSONL log file path.
    #[arg(long)]
    log: Option<PathBuf>,

    /// Alert if no motion detected for this many seconds.
    #[arg(long, default_value = "60")]
    stillness_alert_secs: u64,
}

// ── Entry point ───────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    tracing_subscriber::fmt()
        .with_env_filter("ns3d_scanner=info")
        .without_time()
        .init();

    let addr: SocketAddr = format!("{}:{}", cli.bind, cli.port)
        .parse()
        .context("Invalid bind address")?;

    let socket = UdpSocket::bind(addr)
        .await
        .with_context(|| format!("Cannot bind UDP {}", addr))?;

    // Optional log file
    let mut log_file = if let Some(ref path) = cli.log {
        let f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .await
            .context("Cannot open log file")?;
        info!("Logging scan results to {}", path.display());
        Some(f)
    } else {
        None
    };

    print_banner(&cli);

    let mut pipeline = SignalPipeline::new();
    let mut buf = [0u8; 2048];

    // Track last-motion timestamps for stillness alerts
    let mut last_motion: std::collections::HashMap<u32, u64> = std::collections::HashMap::new();

    loop {
        let (n, src) = match socket.recv_from(&mut buf).await {
            Ok(v)  => v,
            Err(e) => { error!("UDP error: {}", e); continue; }
        };

        let frame = match parse_udp_frame(&buf[..n]) {
            Some(f) => f,
            None    => continue,
        };

        let result = pipeline.process(frame);
        print_result(&result, &mut last_motion, cli.stillness_alert_secs);

        if let Some(ref mut f) = log_file {
            if let Ok(line) = serde_json::to_string(&result) {
                let _ = f.write_all(format!("{}\n", line).as_bytes()).await;
            }
        }

        let _ = src; // suppress unused warning
    }
}

// ── Output formatting ─────────────────────────────────────────────────────────

fn print_banner(cli: &Cli) {
    println!();
    println!("╔══════════════════════════════════════════════════════╗");
    println!("║   NETWORK SCANNER 3D  —  First Responder Terminal   ║");
    println!("╚══════════════════════════════════════════════════════╝");
    println!("  Listening on UDP :{}", cli.port);
    println!("  Stillness alert: {}s", cli.stillness_alert_secs);
    println!();
}

fn print_result(
    result: &ScanResult,
    last_motion: &mut std::collections::HashMap<u32, u64>,
    stillness_secs: u64,
) {
    let ts = hms_now();

    if result.persons.is_empty() {
        // Only print once every ~5s to avoid noise
        if result.timestamp_ms % 5000 < 100 {
            println!("[{}] — no persons detected  (nodes: {:?})", ts, result.node_ids);
        }
        return;
    }

    for p in &result.persons {
        // Stillness check
        if p.vitals.motion_level > 0.15 {
            last_motion.insert(p.id, result.timestamp_ms);
        }
        let still_secs = last_motion
            .get(&p.id)
            .map(|&t| (result.timestamp_ms.saturating_sub(t)) / 1000)
            .unwrap_or(0);

        let motion_bar = motion_bar(p.vitals.motion_level);
        let breath_str = p
            .vitals
            .breathing_rate_bpm
            .map(|b| format!("breath={:.1}bpm", b))
            .unwrap_or_else(|| "breath=...".into());
        let hr_str = p
            .vitals
            .heart_rate_bpm
            .map(|h| format!("HR={:.0}bpm", h))
            .unwrap_or_else(|| "HR=...".into());
        let alive = if p.alive { "✓ ALIVE" } else { "DETECTING..." };

        println!(
            "[{}] PERSON {:>2}  {alive}  {motion_bar}  {}  {}  conf={:.0}%",
            ts,
            p.id,
            breath_str,
            hr_str,
            p.confidence * 100.0,
        );

        if still_secs >= stillness_secs {
            println!(
                "[{}] ⚠  ALERT: Person {} NO MOTION for {}s — check status!",
                ts, p.id, still_secs
            );
        }
    }
}

fn motion_bar(level: f32) -> String {
    let blocks = (level * 8.0).round() as usize;
    let filled = "█".repeat(blocks);
    let empty  = "░".repeat(8usize.saturating_sub(blocks));
    format!("[{}{}]", filled, empty)
}

fn hms_now() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let h = (secs / 3600) % 24;
    let m = (secs / 60)   % 60;
    let s =  secs          % 60;
    format!("{:02}:{:02}:{:02}", h, m, s)
}
