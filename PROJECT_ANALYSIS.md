# Network-Scanner-3D — Project Analysis

> Based on [RuView](https://github.com/ruvnet/RuView) principles + ESP32-S3-N16R8 hardware  
> Target: First responders + building occupancy | Language: Rust | Status: Prototype phase

---

## Hardware Confirmed

**ESP32-S3-N16R8** — dual-core Xtensa LX7 @ 240 MHz, 16 MB Flash, 8 MB PSRAM  
This is the *exact* chip RuView is optimized for (ESP32-S3 is required for CSI — the original ESP32 and ESP32-C3 don't support it).

---

## What RuView Does (and What We're Borrowing)

RuView turns WiFi signals into a spatial sensing system using **Channel State Information (CSI)** — the fine-grained amplitude/phase data your ESP32-S3 can read from every received WiFi packet. When a person moves, breathes, or just stands still, they disturb those radio waves in measurable ways.

### Core Signal Pipeline (RuView)

```
ESP32-S3 → CSI capture (56 subcarriers × 3 channels)
         → serial / UDP stream to host
         → Rust signal processing:
             Hampel filter (outlier removal)
             SpotFi AoA estimation (direction of arrival)
             Fresnel-zone geometry (distance estimation)
             BVP filter (breathing/heart rate isolation)
             FFT spectrogram (motion features)
         → Neural network (Candle, 8 KB quantized model)
         → 17-keypoint pose + vital signs + presence
         → WebSocket → 3D visualization
```

### RuView Rust Workspace Structure

| Crate | Role |
|-------|------|
| `wifi-densepose-core` | Core types: Person, CSIFrame, PoseKeypoint |
| `wifi-densepose-signal` | DSP pipeline (FFT, Hampel, SpotFi, BVP) |
| `wifi-densepose-nn` | Neural net inference (Candle + ONNX) |
| `wifi-densepose-hardware` | Serial/pcap ESP32 comm |
| `wifi-densepose-sensing-server` | Axum HTTP + WebSocket |
| `wifi-densepose-pointcloud` | 3D point cloud fusion |
| `wifi-densepose-vitals` | Breathing (0.1–0.5 Hz) + HR (0.8–2.0 Hz) |
| `wifi-densepose-mat` | WiFi-Mat: through-rubble survivor detection |
| `wifi-densepose-ruvector` | AI backbone (attention + GNN) |

**Pretrained model**: `ruvnet/wifi-densepose-pretrained` on HuggingFace  
- 8 KB (4-bit quantized), 100% presence accuracy on validation set  
- 128-dim CSI embeddings for environment fingerprinting

---

## Our Architecture: Network-Scanner-3D

We adapt RuView's principles into a focused, prototype-ready project with two target use cases:

1. **First responders** — portable, rapid-deploy, through-wall survivor detection
2. **Building occupancy** — permanent multi-node mesh, floor plan overlay, real-time headcount

### Workspace Structure

```
network-scanner-3d/
├── Cargo.toml                 # workspace
├── crates/
│   ├── ns3d-core/             # shared types: Person, Building, CSIFrame, ScanResult
│   ├── ns3d-firmware/         # ESP32-S3 firmware (esp-idf-hal, CSI capture)
│   ├── ns3d-signal/           # DSP: FFT, Hampel, phase variance, breathing/HR
│   ├── ns3d-server/           # Axum server + WebSocket + REST API
│   ├── ns3d-viz/              # Web UI: Three.js 3D skeleton + point cloud
│   └── ns3d-scanner/          # CLI binary: portable first-responder mode
└── firmware/
    └── esp32s3/               # ESP-IDF project (C + Rust via esp-idf-sys)
```

### Data Flow

```
ESP32-S3-N16R8
  └─ CSI capture (802.11 monitor mode)
  └─ UDP broadcast → 192.168.x.x:5006
        │
        ▼
ns3d-server (Rust / Axum)
  └─ ns3d-signal: parse CSIFrame → Hampel → phase variance → breathing/HR
  └─ Presence detection (phase variance threshold, no model needed to start)
  └─ Person position estimation (Fresnel geometry, AoA)
  └─ ScanResult → WebSocket JSON stream
        │
        ▼
ns3d-viz (browser / Three.js)
  └─ 3D building floor plan
  └─ Real-time person markers (spheres → skeletons as model improves)
  └─ Vital sign overlay (breathing rate, motion level)
  └─ Heatmap mode for occupancy
```

---

## Prototype Build Plan

### Phase 1 — Presence Detection (MVP)
- ESP32-S3 firmware captures CSI and streams over UDP
- Rust server parses CSI frames, detects presence via phase variance
- Web UI shows real-time "person detected / not detected" with signal strength heatmap
- **Hardware needed**: 1× ESP32-S3-N16R8 (already have it)

### Phase 2 — Localization + Vitals
- Add breathing rate extraction (bandpass 0.1–0.5 Hz on wrapped phase)
- Add heart rate extraction (bandpass 0.8–2.0 Hz)
- Add Fresnel-zone geometry for rough distance estimation
- Web UI: 2D map with person dot + breathing rate badge

### Phase 3 — 3D Visualization
- Multi-node support (2–3 ESP32-S3 nodes for triangulation)
- 3D Three.js scene: building wireframe + person cylinders
- AoA estimation for position in 3D space
- Load pretrained model from HuggingFace for pose keypoints

### Phase 4 — First Responder Mode
- Offline CLI binary (`ns3d-scanner`) for laptop + single ESP32
- Battery-powered node configuration
- Alert mode: breathing detected / no motion for >60s / fall detected
- Export: GPX/JSON location + vital log for incident report

---

## Key Technical Decisions

| Decision | Choice | Reason |
|----------|--------|--------|
| Language | Rust | RuView proven, memory safety, bare-metal firmware support |
| Firmware framework | esp-idf-hal + esp-idf-sys | Most mature ESP32-S3 Rust support |
| Server framework | Axum + Tokio | Async, WebSocket support, used by RuView |
| Signal processing | rustfft + ndarray | No-dependency FFT, matrix ops |
| 3D visualization | Three.js (web) | Works in any browser, first responders can use on laptop |
| ML inference | Candle (Rust) | Pure Rust, no Python dep, RuView compatible |
| Protocol | UDP + WebSocket | Low latency, firewall-friendly |

---

## Competitive Differentiation for Funding

| Feature | Network-Scanner-3D | Thermal cameras | Commercial radar |
|---------|--------------------|-----------------|-----------------|
| Cost/node | ~$9 | $500–$2,000 | $5,000–$50,000 |
| Through-wall | Yes (WiFi penetrates walls) | No | Yes |
| Dark operation | Yes | Yes | Yes |
| Privacy (no video) | Yes | Partial | Yes |
| Portability | Pocket-sized | Bulky | Very bulky |
| Setup time | <2 min | 30+ min | Hours |
| Rust + open source | Yes | N/A | Closed |

---

## Next Steps

1. `cargo new --lib` workspace scaffold
2. Implement `ns3d-firmware` CSI capture + UDP stream
3. Implement `ns3d-signal` parser + phase variance presence detector
4. Basic Three.js web UI with WebSocket connection
5. Test with physical ESP32-S3-N16R8

---

*Analysis date: 2026-06-24 | Hardware: ESP32-S3-N16R8 confirmed*
