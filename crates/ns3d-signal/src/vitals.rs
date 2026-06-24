//! Vital-sign extraction from CSI amplitude envelope.
//!
//! Algorithm:
//! 1. Compute mean amplitude across all subcarriers for each CSI frame.
//! 2. Buffer a sliding window of amplitudes.
//! 3. Run FFT on the window to get a frequency spectrum.
//! 4. Find the peak in the breathing band (0.1–0.5 Hz → 6–30 BPM).
//! 5. Find the peak in the heart-rate band (0.8–2.0 Hz → 48–120 BPM).
//!
//! Reference: RuView vital-sign pipeline (BVP filter + zero-crossing BPM).

use std::collections::VecDeque;

use rustfft::{num_complex::Complex, FftPlanner};

use ns3d_core::CSI_SAMPLE_RATE_HZ;

/// FFT window length.  Must be a power of two.
/// 512 / 100 Hz = 5.12 s of history → frequency resolution ≈ 0.2 Hz.
const FFT_LEN: usize = 512;
const MIN_BREATHING_SAMPLES: usize = 128;
const MIN_HR_SAMPLES: usize = 256;

// Frequency bands
const BREATH_LO: f32 = 0.10; // Hz
const BREATH_HI: f32 = 0.50; // Hz
const HEART_LO:  f32 = 0.80; // Hz
const HEART_HI:  f32 = 2.00; // Hz

pub struct VitalsExtractor {
    amplitude_buf: VecDeque<f32>,
    planner:       FftPlanner<f32>,
}

impl VitalsExtractor {
    pub fn new() -> Self {
        Self {
            amplitude_buf: VecDeque::with_capacity(FFT_LEN),
            planner:       FftPlanner::new(),
        }
    }

    /// Push one CSI frame into the buffer.
    pub fn update(&mut self, subcarriers: &[[f32; 2]]) {
        if subcarriers.is_empty() {
            return;
        }
        let mean_amp = subcarriers
            .iter()
            .map(|sc| (sc[0] * sc[0] + sc[1] * sc[1]).sqrt())
            .sum::<f32>()
            / subcarriers.len() as f32;

        if self.amplitude_buf.len() >= FFT_LEN {
            self.amplitude_buf.pop_front();
        }
        self.amplitude_buf.push_back(mean_amp);
    }

    /// Estimated breathing rate in BPM, or `None` if not enough data.
    pub fn breathing_rate_bpm(&mut self) -> Option<f32> {
        if self.amplitude_buf.len() < MIN_BREATHING_SAMPLES {
            return None;
        }
        self.peak_freq_bpm(BREATH_LO, BREATH_HI)
    }

    /// Estimated heart rate in BPM, or `None` if not enough data.
    pub fn heart_rate_bpm(&mut self) -> Option<f32> {
        if self.amplitude_buf.len() < MIN_HR_SAMPLES {
            return None;
        }
        self.peak_freq_bpm(HEART_LO, HEART_HI)
    }

    // ── Internal ─────────────────────────────────────────────────────────────

    fn peak_freq_bpm(&mut self, lo_hz: f32, hi_hz: f32) -> Option<f32> {
        let n = self.amplitude_buf.len();

        // Copy to owned buffer (releases borrow of self.amplitude_buf)
        let mut buf: Vec<Complex<f32>> = self
            .amplitude_buf
            .iter()
            .map(|&v| Complex { re: v, im: 0.0 })
            .collect();

        // Apply Hann window to reduce spectral leakage
        for (i, sample) in buf.iter_mut().enumerate() {
            let w = 0.5 * (1.0 - (2.0 * std::f32::consts::PI * i as f32 / (n - 1) as f32).cos());
            sample.re *= w;
        }

        let fft = self.planner.plan_fft_forward(n);
        fft.process(&mut buf);

        let freq_res = CSI_SAMPLE_RATE_HZ / n as f32;
        let lo_bin   = (lo_hz / freq_res).ceil() as usize;
        let hi_bin   = ((hi_hz / freq_res).floor() as usize).min(n / 2);

        if lo_bin >= hi_bin {
            return None;
        }

        let (peak_offset, _) = buf[lo_bin..hi_bin]
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.norm_sqr().total_cmp(&b.1.norm_sqr()))?;

        let peak_freq = (lo_bin + peak_offset) as f32 * freq_res;
        Some(peak_freq * 60.0) // Hz → BPM
    }
}

impl Default for VitalsExtractor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn breathing_detected_at_known_frequency() {
        let mut ext = VitalsExtractor::new();
        let rate_hz  = 0.25_f32; // 15 BPM
        let n_samples = 400usize;

        for i in 0..n_samples {
            let t = i as f32 / CSI_SAMPLE_RATE_HZ;
            let amp = 100.0 + 20.0 * (2.0 * std::f32::consts::PI * rate_hz * t).sin();
            ext.update(&[[amp, 0.0]]);
        }

        let bpm = ext.breathing_rate_bpm().expect("should have enough samples");
        // Allow ±3 BPM tolerance due to FFT resolution
        assert!((bpm - 15.0).abs() < 3.0, "got {bpm} BPM, expected ~15");
    }
}
