# Cocbot-rs

A Rust desktop rewrite of [BasePilot](https://github.com/efebolukbasi/BasePilot), scoped to Clash of Clans farming and attack automation on Windows 10/11 with Google Play Games for PC.

> **Status: backend in progress.** The project now includes window capture, mouse-input primitives, Tesseract HUD OCR, resource tracking, template matching, battle-state recognition, loot target scoring, a raid workflow coordinator, and a guarded mouse deployment executor. Template assets are not yet bundled, the recognition pipeline is not connected to farming, and no attack clicks are issued. Start farming remains disabled until these pieces are implemented and validated.

## Included scope

- Home Village farming and attack strategies from the original project.
- Builder Base farming using supported strategies.
- Loot tracking for gold, elixir, and dark elixir, including session totals and hourly rates.
- A desktop interface for farming configuration, status, and logs.
- Continuous farming until manually stopped; no session timer.

## Explicitly out of scope

There will be no dry-run/simulation mode, clan-related features, ranked attacks, automatic troop donations or requests, time/session limits, star-bonus logic, builder management, building upgrades, wall upgrades, or purchase verification. No controls, settings, or background tasks for these features will be added.

## Current modules

```text
src/
├── app.rs         egui desktop interface and backend status
├── backend.rs     continuous game capture and resource monitoring worker
├── config.rs      typed TOML farming preferences
├── loot.rs        loot deltas, plausibility checks, and hourly rates
├── ocr.rs         Tesseract 5 profiles and HUD text parsing
├── platform.rs    desktop window capture and mouse input
├── strategy.rs    supported army strategy definitions and deployment plans
├── vision.rs      resolution-scaled template matching
├── battle.rs      battle-state and deployment-bar recognition
├── target.rs      confidence-gated loot target scoring
├── deployment.rs  battle-state and troop-gated deployment plan generation
├── raid.rs        continuous raid state machine and target decisions
├── executor.rs    validated troop selection and deployment input
└── main.rs        application entry point
```

## OCR performance

The resource HUD is cropped before OCR instead of passing the full game frame to Tesseract. The crop follows the upstream BasePilot reference regions (600×400 at 2560×1600 for 16:10, 550×350 at 2560×1440 for 16:9), then applies a 3× upscale and a strict bright-glyph threshold. Tesseract is invoked with:

- LSTM engine only (`--oem 1`)
- Single text block (`--psm 6`)
- Numeric whitelist (`0123456789l`)
- Numeric mode enabled and dictionaries disabled
- One OpenMP worker thread to reduce thread startup overhead

The OCR process receives the cropped PNG through stdin, so no temporary image files are created. Set the Tesseract executable path in Settings if it is not available on PATH. These settings are starting values based on BasePilot's HUD pipeline; they still need validation against screenshots from different game resolutions and UI scaling.

## Build

Rust 1.93.1+:

```powershell
cargo run
cargo test
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
```

Tesseract 5 must also be installed for live resource OCR. The Windows desktop capture and input backend targets Google Play Games for PC.

## Remaining backend work

1. Add the required upstream screen templates under `assets/templates/` and validate them at 16:9 and 16:10.
2. Calibrate recognition thresholds against real game screenshots; the current matcher is a foundation, not a validated detector.
3. Complete Home Village and Builder Base state recognition and troop/hero inventory detection.
4. Connect the raid coordinator and deployment executor to the live capture worker with cancellation and fresh-frame checks.
5. Complete troop/hero/spell recognition and strategy-specific secondary deployment actions.
6. Validate battle completion, retreat/return controls, and recovery paths against real screenshots before enabling Start farming.

## Terms and attribution

Automating Clash of Clans may violate Supercell's Terms of Service and can result in account sanctions. This independent project is not affiliated with or endorsed by Supercell and is used at your own risk.

MIT. This rewrite is based on the publicly available MIT-licensed BasePilot project by Efe Bolukbasi. The original copyright and license notice is retained in `LICENSE`; see `NOTICE`.
