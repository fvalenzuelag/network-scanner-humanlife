//! End-to-end signal processing pipeline.
//!
//! Maintains one [`NodeState`] per ESP32-S3 node (keyed by `node_id`).
//! Each call to [`SignalPipeline::process`] returns an updated [`ScanResult`]
//! that can be serialised and sent over WebSocket.

use std::{collections::HashMap, time::{SystemTime, UNIX_EPOCH}};

use ns3d_core::{CsiFrame, Person, PersonVitals, Position3D, ScanResult};
use tracing::debug;

use crate::{PresenceDetector, VitalsExtractor};

// ── Per-node state ────────────────────────────────────────────────────────────

struct NodeState {
    presence:  PresenceDetector,
    vitals:    VitalsExtractor,
    person_id: u32,
    frames:    u64,
}

// ── Pipeline ──────────────────────────────────────────────────────────────────

/// Processes [`CsiFrame`]s from one or more ESP32-S3 nodes and maintains a
/// live map of detected persons.
pub struct SignalPipeline {
    nodes:          HashMap<u8, NodeState>,
    next_person_id: u32,
}

impl SignalPipeline {
    pub fn new() -> Self {
        Self {
            nodes:          HashMap::new(),
            next_person_id: 1,
        }
    }

    /// Process one CSI frame and return the current scan result.
    pub fn process(&mut self, frame: CsiFrame) -> ScanResult {
        let now_ms  = unix_ms();
        let node_id = frame.node_id;

        // ── Ensure node state exists ──────────────────────────────────────────
        if !self.nodes.contains_key(&node_id) {
            let pid = self.next_person_id;
            self.next_person_id += 1;
            debug!(node_id, person_id = pid, "new node registered");
            self.nodes.insert(
                node_id,
                NodeState {
                    presence:  PresenceDetector::new(),
                    vitals:    VitalsExtractor::new(),
                    person_id: pid,
                    frames:    0,
                },
            );
        }

        // ── Run detectors (in a scoped block to release the borrow) ──────────
        let (confidence, person_id, breathing, heart_rate, is_present) = {
            let node = self.nodes.get_mut(&node_id).unwrap();
            node.frames += 1;

            let conf    = node.presence.update(&frame);
            node.vitals.update(&frame.subcarriers);
            let breath  = node.vitals.breathing_rate_bpm();
            let hr      = node.vitals.heart_rate_bpm();
            let present = node.presence.is_present();

            (conf, node.person_id, breath, hr, present)
        };

        // ── Build person list ─────────────────────────────────────────────────
        let mut persons = Vec::new();

        if is_present {
            persons.push(Person {
                id:       person_id,
                // Phase 1: single-node, no triangulation yet.
                // Phase 2 will compute AoA + Fresnel distance for real position.
                position: Some(Position3D { x: 0.0, y: 0.0, z: 0.0 }),
                vitals: PersonVitals {
                    breathing_rate_bpm: breathing,
                    heart_rate_bpm:     heart_rate,
                    motion_level:       confidence,
                },
                confidence,
                last_seen_ms: now_ms,
                alive: breathing.is_some(),
            });
        }

        let node_ids: Vec<u8> = self.nodes.keys().copied().collect();

        ScanResult {
            timestamp_ms: now_ms,
            person_count: persons.len(),
            persons,
            node_ids,
        }
    }

    /// Number of frames processed for a given node.
    pub fn node_frames(&self, node_id: u8) -> u64 {
        self.nodes.get(&node_id).map(|n| n.frames).unwrap_or(0)
    }
}

impl Default for SignalPipeline {
    fn default() -> Self {
        Self::new()
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
