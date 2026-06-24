//! Presence detection via CSI phase-variance.
//!
//! When a person is present, body movement (including breathing) causes the
//! WiFi channel to fluctuate. We measure the temporal variance of the
//! unwrapped phase across all subcarriers: a high variance means someone
//! is there; a low variance means the room is empty.
//!
//! Reference: RuView ADR-024 (contrastive CSI embedding), phase-variance
//! fallback section.

use std::collections::VecDeque;

use ns3d_core::CsiFrame;

/// Number of frames kept in the sliding window (~1 s at 100 fps).
const WINDOW: usize = 100;

/// Phase-variance threshold for presence classification.
/// Tunable: lower = more sensitive (more false positives in noisy RF
/// environments), higher = less sensitive.
const PRESENCE_THRESHOLD: f32 = 0.4;

/// Minimum frames before we emit a reliable reading.
const MIN_FRAMES: usize = 15;

/// Tracks per-node presence using phase variance.
pub struct PresenceDetector {
    /// Circular buffer of per-frame phase vectors.
    phase_history: VecDeque<Vec<f32>>,
    /// Smoothed variance values (for stability).
    variance_history: VecDeque<f32>,
}

impl PresenceDetector {
    pub fn new() -> Self {
        Self {
            phase_history:   VecDeque::with_capacity(WINDOW),
            variance_history: VecDeque::with_capacity(WINDOW),
        }
    }

    /// Feed a new CSI frame; returns confidence score in [0, 1].
    pub fn update(&mut self, frame: &CsiFrame) -> f32 {
        // 1. Extract phase angle per subcarrier.
        let phases: Vec<f32> = frame
            .subcarriers
            .iter()
            .map(|sc| sc[1].atan2(sc[0]))
            .collect();

        if self.phase_history.len() >= WINDOW {
            self.phase_history.pop_front();
        }
        self.phase_history.push_back(phases);

        if self.phase_history.len() < MIN_FRAMES {
            return 0.0;
        }

        // 2. Compute mean phase variance across subcarriers.
        let n_sub    = self.phase_history[0].len();
        let n_frames = self.phase_history.len() as f32;
        let mut total_var = 0.0f32;

        for sub in 0..n_sub {
            let vals: Vec<f32> = self.phase_history.iter().map(|p| p[sub]).collect();
            let mean = vals.iter().copied().sum::<f32>() / n_frames;
            let var  = vals.iter().map(|v| (v - mean).powi(2)).sum::<f32>() / n_frames;
            total_var += var;
        }

        let avg_var = total_var / n_sub as f32;

        if self.variance_history.len() >= WINDOW {
            self.variance_history.pop_front();
        }
        self.variance_history.push_back(avg_var);

        // 3. Map to [0, 1]: saturates at 5× the threshold.
        (avg_var / (PRESENCE_THRESHOLD * 5.0)).min(1.0)
    }

    /// True if the recent variance consistently exceeds the threshold.
    pub fn is_present(&self) -> bool {
        if self.variance_history.len() < MIN_FRAMES {
            return false;
        }
        // Use the median of the last 20 samples for robustness.
        let n = self.variance_history.len().min(20);
        let mut recent: Vec<f32> = self.variance_history.iter().rev().take(n).copied().collect();
        recent.sort_by(f32::total_cmp);
        let median = recent[n / 2];
        median > PRESENCE_THRESHOLD
    }

    /// Latest smoothed variance value (diagnostic / visualisation).
    pub fn variance(&self) -> f32 {
        self.variance_history.back().copied().unwrap_or(0.0)
    }
}

impl Default for PresenceDetector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_frame(node_id: u8, phase_offset: f32) -> CsiFrame {
        use ns3d_core::CsiFrame;
        let n = 56;
        let subcarriers = (0..n)
            .map(|i| {
                let p = phase_offset + (i as f32) * 0.1;
                [p.cos() * 100.0, p.sin() * 100.0]
            })
            .collect();
        CsiFrame {
            timestamp_ms: 0,
            node_id,
            channel: 6,
            rssi: -60,
            subcarriers,
        }
    }

    #[test]
    fn no_presence_on_static_channel() {
        let mut det = PresenceDetector::new();
        for _ in 0..50 {
            det.update(&make_frame(1, 0.0));
        }
        // Static channel → low variance → no presence
        assert!(!det.is_present());
    }

    #[test]
    fn presence_on_varying_channel() {
        let mut det = PresenceDetector::new();
        for i in 0..50 {
            // Vary phase by a large amount each frame
            det.update(&make_frame(1, (i as f32) * 0.5));
        }
        assert!(det.is_present());
    }
}
