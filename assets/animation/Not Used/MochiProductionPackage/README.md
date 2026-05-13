# Mochi Production Package

Drop this into the local Mabel repo.

## Included

- `Assets/Mochi/Sprites/mochi_walk_16.png` — 8192 x 512 transparent PNG sprite sheet
- `Assets/Mochi/Sprites/frames/` — individual 512 x 512 transparent PNG frames
- `Assets/Mochi/Sprites/mochi_walk_preview.gif` — preview animation
- `Sources/Mochi/MochiNode.swift` — SpriteKit behavior node
- `Sources/Mochi/MochiScene.swift` — minimal scene wrapper
- `Assets/Mochi/mochi_animation_config.json` — animation metadata

## Mochi behavior

- walks s l o w
- cautious micro-pauses every 4 frames
- slight outward/corgi-mix gait timing
- subtle breathing
- rare ear twitch
- cursor-aware look
- rain-mode shiver

## Note

This package is finished for repo testing. It was productionized from the current generated concept art with true alpha and exact sprite dimensions. A final hand-cleaned illustration pass can still improve edge fidelity and anatomy.
