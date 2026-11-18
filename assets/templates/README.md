# Vision template assets

The Rust vision backend expects grayscale-compatible PNG crops in this directory.
No game screenshots or template images are bundled yet.

## Current screen-state template names

- `bstar.png` and `bbstar.png` — battle-win result anchors
- `defeat.png` and `bbdefeat.png` — battle-loss result anchors
- `opponentfound.png` — opponent preview / next action screen
- `endbattle.png` and `bbendbattle.png` — Home Village and Builder Base battle HUD anchors
- `findnow.png` and `bbfindnow.png` — matchmaking controls
- `attack.png` and `farmbattle.png` — Home Village entry controls
- `bbattack.png` — Builder Base entry control

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
ensure the template scaling has been calibrated against both 16:9 and 16:10 frames.

## Important

The detector currently treats absent templates as unavailable, not as a positive match.
Do not lower confidence thresholds to compensate for missing or poorly cropped assets.
Recognition must be validated against real screenshots before enabling mouse deployment.

BasePilot is MIT-licensed. Any copied upstream asset should retain its applicable
copyright and licence attribution.
