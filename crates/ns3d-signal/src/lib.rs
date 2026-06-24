//! ns3d-signal — CSI signal processing pipeline
//!
//! Processing stages:
//! 1. **Presence** — phase variance across subcarriers detects motion/people.
//! 2. **Vitals**   — FFT on amplitude envelope extracts breathing & heart rate.
//! 3. **Pipeline** — ties everything together per ESP32 node.

pub mod pipeline;
pub mod presence;
pub mod vitals;

pub use pipeline::SignalPipeline;
pub use presence::PresenceDetector;
pub use vitals::VitalsExtractor;
