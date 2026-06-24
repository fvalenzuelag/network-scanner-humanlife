# Network Scanner 3D

WiFi CSI-based through-wall people detector with real-time 3D visualization.  
Detects presence, breathing rate, heart rate, and position — through walls, in the dark, without a camera.

Built in **Rust** + **ESP-IDF C firmware** for the **ESP32-S3**.  
Target use cases: first responders and smart building occupancy.

> Inspired by [RuView](https://github.com/ruvnet/RuView) by rUv. See [ATTRIBUTION.md](ATTRIBUTION.md).

---

## How It Works

Every 802.11 WiFi frame contains Channel State Information (CSI): per-subcarrier amplitude and phase data that describes how the signal traveled through the environment. When a person is present, their body absorbs and reflects radio waves. When they breathe, their chest movement creates a measurable periodic phase shift across 56 OFDM subcarriers.

```
ESP32-S3-N16R8
  └─ CSI capture @ 100 Hz (56 subcarriers, LLTF)
  └─ Binary UDP stream → ns3d-server

ns3d-server (Rust / Axum)
  └─ Phase extraction: φ = atan2(im, re)
  └─ Temporal variance → presence detection
  └─ 512-pt Hann-windowed FFT → breathing (0.1–0.5 Hz) + HR (0.8–2.0 Hz)
  └─ ScanResult → WebSocket broadcast

Browser (Three.js r160)
  └─ 3D room + humanoid figure per detected person
  └─ Vitals overlay: breathing rate, heart rate, motion level, confidence
```

---

## Hardware

| Component | Spec |
|-----------|------|
| ESP32-S3-N16R8 | Dual-core Xtensa LX7 @ 240 MHz, 16 MB Flash, 8 MB OPI PSRAM |
| Cost | ~$15 USD |
| Connection | USB-C (power + flash) |
| Network | WiFi 802.11 b/g/n (2.4 GHz) |

One node is enough for presence detection and vital sign extraction. Three or more nodes enable XY triangulation (Phase 2).

---

## Project Structure

```
network-scanner-3d/
├── Cargo.toml                    # Rust workspace
├── crates/
│   ├── ns3d-core/                # Shared types: CsiFrame, Person, ScanResult
│   ├── ns3d-signal/              # DSP pipeline: phase variance, FFT, vital signs
│   ├── ns3d-server/              # Axum WebSocket server + UDP listener
│   └── ns3d-scanner/             # CLI binary for field use
├── firmware/
│   └── esp32s3/                  # ESP-IDF v6 C firmware
│       └── main/csi_capture.c    # CSI capture + UDP stream
├── web/
│   └── index.html                # Single-file Three.js 3D UI
├── content/                      # LinkedIn + Medium posts
├── LICENSE
└── ATTRIBUTION.md
```

---

## NS3D Wire Format

Custom binary protocol. 240 bytes per 56-subcarrier frame at 100 Hz = 24 KB/s.

```
[magic:4] [node_id:1] [channel:1] [rssi:1] [timestamp_ms:8] [num_sub:1]
[re:2 im:2] × num_sub
```

---

## Quick Start

### Prerequisites

- Rust (stable) via [rustup](https://rustup.rs)
- ESP-IDF v6.x ([install guide](https://docs.espressif.com/projects/esp-idf/en/stable/esp32s3/get-started/))
- ESP32-S3-N16R8 board

### 1. Flash the firmware

Edit WiFi credentials in `firmware/esp32s3/main/csi_capture.c`:

```c
#define WIFI_SSID      "your_ssid"
#define WIFI_PASS      "your_password"
#define SERVER_IP      "192.168.x.x"   // IP of your computer
#define SERVER_PORT    5006
```

Then flash:

```bash
cd firmware/esp32s3
. $IDF_PATH/export.sh
idf.py build
idf.py -p /dev/cu.usbmodem1101 flash monitor
```

Note: Hold BOOT and press RESET before the first flash to enter download mode.

### 2. Start the server

```bash
cargo run -p ns3d-server
# Listening on 0.0.0.0:3000 (HTTP/WebSocket) and 0.0.0.0:5006 (UDP)
```

### 3. Open the UI

Navigate to [http://localhost:3000](http://localhost:3000) in any browser.

---

## Signal Processing Details

### Presence Detection

Maintains a 100-frame sliding window (~1 second). Computes temporal variance of unwrapped phase per subcarrier. Median variance > 0.4 rad² across the last 20 samples = person present. Confidence reported as 0.0–1.0.

### Vital Sign Extraction

Mean amplitude across all 56 subcarriers is buffered over 512 frames (~5 seconds). A 512-point FFT with Hann window extracts:

- **Breathing rate**: dominant frequency in 0.1–0.5 Hz (6–30 RPM)
- **Heart rate**: dominant frequency in 0.8–2.0 Hz (48–120 BPM)

Frequency resolution: 100/512 ≈ 0.2 Hz → ±1.2 BPM accuracy.

---

## Roadmap

**Phase 1 (done):** Single-node presence + vital sign detection, 3D browser visualization.

**Phase 2 (in progress):** Multi-node XY triangulation using time-difference-of-arrival and Fresnel zone geometry.

**Phase 3:** Integration of pretrained 8 KB quantized pose model (`ruvnet/wifi-densepose-pretrained`) for 17-keypoint body pose estimation from CSI.

**Phase 4:** Battery-powered node enclosures, offline CLI binary, incident report export (JSON/GPX).

---

## Use Cases

**First responders** — deploy a node into a burning building or collapsed structure. Know immediately if there are survivors, where they are, and whether they are breathing — before sending anyone in.

**Building occupancy** — distributed nodes across a floor provide real-time headcount for energy management, evacuation planning, and compliance.

---

## License

MIT License — see [LICENSE](LICENSE).

This project is inspired by [RuView](https://github.com/ruvnet/RuView) (MIT, Copyright © 2024 rUv). All code is original. See [ATTRIBUTION.md](ATTRIBUTION.md) for details.

---

## Author

Fran Valenzuela — [f.valenzuela.garrido@gmail.com](mailto:f.valenzuela.garrido@gmail.com)

GitHub: https://github.com/fvalenzuelag/3d-human-detection-scanner

If you work in first response, building management, or deep tech investment and want to talk, reach out.
