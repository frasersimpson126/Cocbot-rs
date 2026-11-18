# Vision template assets

The Rust vision backend loads one aspect-specific PNG template pack based on the
captured game window:

- `16_9/` for widescreen captures (aspect ratio above 1.70)
- `16_10/` for 16:10 captures

Place grayscale-compatible PNG crops in the matching directory. The repository
does not currently bundle game templates. The runtime looks beside the executable
for `assets/templates/<aspect>/`, then falls back to that relative path from the
current working directory.

## Current screen-state template names

- `bstar.png` and `bbstar.png` — battle-win result anchors
- `defeat.png` and `bbdefeat.png` — battle-loss result anchors
- `opponentfound.png` — opponent preview / next action screen
- `endbattle.png` and `bbendbattle.png` — Home Village and Builder Base battle HUD anchors
- `bbsearching.png` and `searching.png` — active matchmaking indicators
- `bbfindnow.png` and `findnow.png` — attack-menu search controls
- `bbattack.png` — Builder Base entry control
- `attack.png` and `farmbattle.png` — Home Village entry controls
- `returnhome.png` and `bbreturnhome.png` — result-screen return controls

## Troop icon templates

The deployment-bar recogniser accepts a caller-supplied list of PNG names.
The planner currently expects these name fragments:

- `sneaky_goblin`
- `valkyrie`
- `super_minion`
- `electro_dragon`
- `super_dragon`
- `baby_dragon`

Use tightly cropped icons from the deployment bar, excluding adjacent troop counts
where possible. Capture separate assets for materially different game UI scales or
ensure template scaling has been calibrated against both aspect ratios.

## Important

The detector treats absent templates as unavailable, not as a positive match.
Do not lower confidence thresholds to compensate for missing or poorly cropped
assets. Recognition must be validated against real screenshots before enabling
mouse deployment. Opponent-loot OCR is separately disabled until its ROI is
calibrated against the actual game window.

BasePilot is MIT-licensed. Any copied upstream code should retain its applicable
copyright and licence attribution. Game artwork and trademarks remain the property
of their respective owners.
