# Cocbot-rs

A Rust desktop rewrite of [BasePilot](https://github.com/efebolukbasi/BasePilot), focused on Home Village and Builder Base farming, loot tracking, and village progression through verified upgrades. The target platform is Windows 10/11 running Clash of Clans in Google Play Games for PC.

> **Status: early foundation (0.1.0).** This repository currently contains the desktop shell, configuration, event-driven worker structure, image matching, OCR adapter, and domain interfaces. It is not yet a working replacement for BasePilot: game-specific capture, screen recognition, attack execution, loot parsing, and purchase flows still need to be ported and tested.

## Included project scope

- Home Village farming and army deployment strategies.
- Builder Base farming, where supported by the original automation.
- Loot recognition and session/hourly loot statistics.
- Village status: builders, storages, and laboratory indicators.
- Upgrade selection (Maxer/Rusher policies), purchase verification, and wall batching.
- Idle/resume behaviour when resources or builders make further actions unproductive.
- Desktop interface for status, run controls, settings, and logs.

## Explicitly out of scope

This project will not implement dry-run/simulation mode, clan-related features, ranked attacks, automatic troop donations or requests, session/time limits, or star-bonus logic. There will be no configuration controls or background tasks for these features.

## Architecture

```text
src/
├── app.rs                  egui desktop application
├── config.rs               Typed TOML configuration
├── controller.rs           Background worker and cancellation
├── events.rs               UI/controller event protocol
├── bot/                    Automation state machine
├── vision/                 Frame handling and parallel template matching
├── ocr/                    Tesseract adapter
├── input/                  Windows input abstraction
├── window/                 Game-window discovery/capture abstraction
├── strategy/               Army deployment strategy contracts
├── upgrade/                Upgrade selection policies
├── state/                  Village snapshot and idle gating
└── diagnostics/            Structured diagnostic events
```

## Build

Rust 1.85+:

```powershell
cargo run
cargo test
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
```

Optional OpenCV integration: `cargo check --features opencv-vision`. The default build uses the Rust image pipeline.

## Porting priorities

1. Windows game-window enumeration, capture, DPI-aware coordinates, and input.
2. Port original screen templates and screen-state detection.
3. Port OCR parsing with multi-frame corroboration and confidence checks.
4. Port Home Village and Builder Base raid state machines and troop strategies.
5. Port loot accounting and live village status.
6. Port upgrade scanning, purchase verification, and wall batching.
7. Add screenshot fixtures, integration tests, and Windows packaging.

## Terms and attribution

Automating Clash of Clans may violate Supercell's Terms of Service and can result in account sanctions. This independent project is for educational and research purposes, is not affiliated with or endorsed by Supercell, and is used at your own risk.

MIT. This rewrite is based on the publicly available MIT-licensed BasePilot project by Efe Bolukbasi. The original copyright and license notice is retained in `LICENSE`; see `NOTICE`.
