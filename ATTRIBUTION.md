# Attribution

## RuView

This project is inspired by [RuView](https://github.com/ruvnet/RuView) by [rUv](https://github.com/ruvnet), licensed under the MIT License (Copyright © 2024 rUv).

The following architectural concepts and technical approaches were derived from studying the RuView project:

- Overall system architecture: ESP32-S3 → UDP stream → Rust server → WebSocket → browser visualization
- Signal processing pipeline structure: phase extraction → temporal variance → FFT → vital sign extraction
- Frequency band definitions for vital sign detection (breathing: 0.1–0.5 Hz, heart rate: 0.8–2.0 Hz)
- Rust workspace crate organization pattern (core, signal, server, scanner)
- Use of Axum + Tokio for the WebSocket/REST server layer
- Fresnel zone geometry for distance estimation (Phase 2)
- Reference to the pretrained 8 KB quantized pose model (`ruvnet/wifi-densepose-pretrained` on HuggingFace)

**Repository**: https://github.com/fvalenzuelag/network-scanner-humanlife

**All code in this repository is original work written from scratch.** No source files from RuView were copied or directly modified. The algorithms are based on peer-reviewed academic research on WiFi Channel State Information (CSI) sensing, which RuView also draws from.

## Academic Research

The WiFi CSI sensing techniques used in this project are based on the following body of research:

- Domino et al., "Passive Indoor Localization and Tracking Using WiFi CSI" (2020)
- Wang et al., "We Can Hear You with Wi-Fi!" (MobiCom 2014) — breathing/HR detection via CSI
- Kotaru et al., "SpotFi: Decimeter Level Localization Using WiFi" (SIGCOMM 2015) — AoA estimation
- Adib & Katabi, "See Through Walls with WiFi!" (SIGCOMM 2013) — through-wall presence detection

## Espressif Systems

Firmware written for the **ESP-IDF v6** framework by Espressif Systems.  
ESP-IDF is licensed under the Apache License 2.0.  
https://github.com/espressif/esp-idf

## Third-Party Libraries

| Library | License | Use |
|---------|---------|-----|
| Tokio | MIT | Async runtime |
| Axum | MIT | HTTP/WebSocket server |
| rustfft | MIT/Apache 2.0 | FFT computation |
| Three.js r160 | MIT | 3D browser visualization |
| clap | MIT/Apache 2.0 | CLI argument parsing |
| serde / serde_json | MIT/Apache 2.0 | Serialization |
| tracing | MIT | Logging |
