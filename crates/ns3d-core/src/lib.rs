//! ns3d-core — shared types for Network-Scanner-3D
//!
//! Wire format (UDP frame from ESP32-S3):
//! ```text
//! [magic:4][node_id:1][channel:1][rssi:1][timestamp_ms:8][num_sub:1][i16,i16 per sub...]
//! ```

use serde::{Deserialize, Serialize};

// ── Wire protocol ─────────────────────────────────────────────────────────────

/// Magic bytes that identify an NS3D UDP frame: "NS3D"
pub const NS3D_MAGIC: u32 = 0x4E533344;

/// Number of OFDM subcarriers per CSI frame (802.11n, 20 MHz channel)
pub const CSI_SUBCARRIERS: usize = 56;

/// Approximate CSI frame rate from ESP32-S3 (packets/second).
/// Actual rate depends on traffic; ~100 Hz when pinging at 100 pps.
pub const CSI_SAMPLE_RATE_HZ: f32 = 100.0;

// ── Core types ────────────────────────────────────────────────────────────────

/// One CSI snapshot captured by an ESP32-S3 node.
///
/// Contains complex-valued channel estimates for each OFDM subcarrier.
/// Each element is `[real, imag]` expressed as f32 (scaled from raw i16).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CsiFrame {
    /// Wall-clock milliseconds (ESP32 NTP or relative boot time)
    pub timestamp_ms: u64,
    /// Which ESP32-S3 node sent this frame (1-based)
    pub node_id: u8,
    /// WiFi channel (1, 6, or 11 for 2.4 GHz)
    pub channel: u8,
    /// Received signal strength in dBm (negative)
    pub rssi: i8,
    /// Complex CSI per subcarrier: `[[re, im], …]`
    pub subcarriers: Vec<[f32; 2]>,
}

/// 3-D position in room coordinates (metres, origin = first node).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Position3D {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

/// Vital-sign estimates derived from CSI phase/amplitude fluctuations.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PersonVitals {
    /// Breathing rate (Hz × 60), extracted from 0.1–0.5 Hz band.
    /// `None` until enough samples are buffered (~3 s).
    pub breathing_rate_bpm: Option<f32>,

    /// Heart rate (Hz × 60), extracted from 0.8–2.0 Hz band.
    /// `None` until enough samples are buffered (~5 s).
    pub heart_rate_bpm: Option<f32>,

    /// Normalised motion level 0.0 (still) – 1.0 (fast movement).
    pub motion_level: f32,
}

/// A person detected in the scan area.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Person {
    /// Stable ID assigned when first detected (per node in Phase 1).
    pub id: u32,
    /// 3-D position if multi-node triangulation is available.
    pub position: Option<Position3D>,
    pub vitals: PersonVitals,
    /// Detection confidence 0.0 – 1.0.
    pub confidence: f32,
    /// Last time this person produced a signal (ms).
    pub last_seen_ms: u64,
    /// True when breathing signal is actively detected.
    pub alive: bool,
}

/// Complete scan snapshot broadcast over WebSocket.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanResult {
    pub timestamp_ms: u64,
    pub person_count: usize,
    pub persons: Vec<Person>,
    /// IDs of nodes that contributed to this result.
    pub node_ids: Vec<u8>,
}

/// Per-node health reported via REST /api/v1/status.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeStatus {
    pub node_id: u8,
    pub channel: u8,
    pub rssi: i8,
    pub frames_received: u64,
    pub last_seen_ms: u64,
}

// ── UDP frame parser ──────────────────────────────────────────────────────────

/// Try to parse a raw UDP payload into a [`CsiFrame`].
///
/// Returns `None` if the magic bytes don't match or the buffer is too short.
pub fn parse_udp_frame(data: &[u8]) -> Option<CsiFrame> {
    if data.len() < 16 {
        return None;
    }

    let magic = u32::from_be_bytes(data[0..4].try_into().ok()?);
    if magic != NS3D_MAGIC {
        return None;
    }

    let node_id     = data[4];
    let channel     = data[5];
    let rssi        = data[6] as i8;
    let timestamp_ms = u64::from_be_bytes(data[7..15].try_into().ok()?);
    let num_sub     = data[15] as usize;

    let expected_len = 16 + num_sub * 4;
    if data.len() < expected_len {
        return None;
    }

    let mut subcarriers = Vec::with_capacity(num_sub);
    for i in 0..num_sub {
        let base = 16 + i * 4;
        let re = i16::from_be_bytes([data[base],     data[base + 1]]) as f32;
        let im = i16::from_be_bytes([data[base + 2], data[base + 3]]) as f32;
        subcarriers.push([re, im]);
    }

    Some(CsiFrame {
        timestamp_ms,
        node_id,
        channel,
        rssi,
        subcarriers,
    })
}

/// Encode a [`CsiFrame`] into the NS3D UDP wire format.
pub fn encode_udp_frame(frame: &CsiFrame) -> Vec<u8> {
    let num_sub = frame.subcarriers.len();
    let mut buf = Vec::with_capacity(16 + num_sub * 4);

    buf.extend_from_slice(&NS3D_MAGIC.to_be_bytes());
    buf.push(frame.node_id);
    buf.push(frame.channel);
    buf.push(frame.rssi as u8);
    buf.extend_from_slice(&frame.timestamp_ms.to_be_bytes());
    buf.push(num_sub as u8);

    for sc in &frame.subcarriers {
        buf.extend_from_slice(&(sc[0] as i16).to_be_bytes());
        buf.extend_from_slice(&(sc[1] as i16).to_be_bytes());
    }

    buf
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let frame = CsiFrame {
            timestamp_ms: 123_456_789,
            node_id: 1,
            channel: 6,
            rssi: -55,
            subcarriers: vec![[100.0, -50.0], [200.0, 10.0]],
        };
        let encoded = encode_udp_frame(&frame);
        let decoded = parse_udp_frame(&encoded).expect("decode failed");
        assert_eq!(decoded.node_id, frame.node_id);
        assert_eq!(decoded.channel, frame.channel);
        assert_eq!(decoded.rssi, frame.rssi);
        assert_eq!(decoded.subcarriers.len(), frame.subcarriers.len());
    }
}
