# Cocbot-rs

A Rust desktop rewrite of [BasePilot](https://github.com/efebolukbasi/BasePilot), scoped to Clash of Clans farming and attack automation on Windows 10/11 with Google Play Games for PC.

> **Status: early development.** The current build contains the desktop shell, persistent farming preferences, attack-strategy models, and loot-accounting logic with unit tests. It is not yet a working game bot: Windows game capture, screen recognition, troop-bar detection, input injection, and the raid state machine remain to be implemented and validated.

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
├── app.rs       egui desktop interface
├── config.rs    typed TOML farming preferences
├── loot.rs      loot deltas, plausibility checks, and hourly rates
├── strategy.rs  supported army strategy definitions and deployment plans
└── main.rs      application entry point
```

## Build

Rust 1.85+:

```powershell
cargo run
cargo test
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
```

## Porting plan

1. Implement Windows game-window discovery and reliable frame capture.
2. Port the original screen templates and state recognition needed for farming.
3. Implement OCR for resource HUD readings and validate it against screenshot fixtures.
4. Port Home Village and Builder Base raid state machines and the selected army strategies.
5. Connect loot tracking to corroborated before/after raid snapshots.
6. Add Windows input handling, cancellation, recovery, and end-to-end tests.

## Terms and attribution

Automating Clash of Clans may violate Supercell's Terms of Service and can result in account sanctions. This independent project is not affiliated with or endorsed by Supercell and is used at your own risk.

MIT. This rewrite is based on the publicly available MIT-licensed BasePilot project by Efe Bolukbasi. The original copyright and license notice is retained in `LICENSE`; see `NOTICE`.
