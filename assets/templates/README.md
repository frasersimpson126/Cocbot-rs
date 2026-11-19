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

## Additional transition and recovery screens

The detector also recognises these optional transition overlays when present:

- `battleending.png` — Home Village battle ending transition
- `bbbattleending.png` — Builder Base battle ending transition
- `returninghome.png` — Home Village return transition
- `bbreturninghome.png` — Builder Base return transition

The connection/lost-session overlay (`connectionerror.png` or `connection_error.png`) is required before farming can start, so a recognised disconnect pauses the active raid until explicit user resume.

A stable connection-error detection pauses the raid coordinator. The bot must not
resume automatically when the overlay disappears; recovery requires an explicit
resume action after the screen is revalidated.

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

## Runtime validation

On startup the application validates the selected pack and reports each logical
recognition group in the Farming log. Required groups are:

- Village entry: `attack.png` or `farmbattle.png` (Home Village), or `bbattack.png`
- Search: `findnow.png` or `bbfindnow.png`
- Matchmaking: `searching.png` or `bbsearching.png`
- Opponent preview: `opponentfound.png`
- Battle HUD: `endbattle.png` or `bbendbattle.png`
- Victory and defeat: `bstar.png` / `bbstar.png`, and `defeat.png` / `bbdefeat.png`
- Return: `returnhome.png` or `bbreturnhome.png`
- The next-opponent button (`next.png` or `bbnext.png`)
- A connection-error overlay (`connectionerror.png` or `connection_error.png`)
- The selected strategy's troop icon

Each required logical group must have at least one valid alternative. Images must decode
as PNG-compatible images, be between 8×8 and 2048×2048 pixels, and be no larger than
16 MiB. Corrupt, empty, oversized and undersized files are reported individually.
Optional groups for battle-ending/return transitions, hero ability, siege machine and spells are also listed. The next-opponent button is required because rejected targets must be safely skipped before the next search. A connection-error template is required so a recognised disconnect pauses the session until explicit user resume.

The validation only checks file integrity and presence. It does not prove that a crop
matches the current game UI. Calibrate every crop against real screenshots and keep
the high confidence thresholds; do not compensate for missing assets by lowering them.

## Important

The detector treats absent templates as unavailable, not as a positive match.
Do not lower confidence thresholds to compensate for missing or poorly cropped
assets. Recognition must be validated against real screenshots before enabling
mouse deployment. Opponent-loot OCR is separately disabled until its ROI is
calibrated against the actual game window.

BasePilot is MIT-licensed. Any copied upstream code should retain its applicable
copyright and licence attribution. Game artwork and trademarks remain the property
of their respective owners.
