# Mabel Sprite Assets

Files:
- mabel_sprite_4096x512.png: final transparent sprite sheet
- mabel_frame_01.png through mabel_frame_08.png: individual normalized frames
- mabel_walk_preview.gif: quick walk-cycle preview
- MabelScene.swift: SpriteKit animation scene
- MabelDesktopWindowController.swift: transparent floating macOS desktop window wrapper

Frame map:
0-5 = walk cycle
6 = sit idle
7 = blink

Notes:
- Canvas: 4096x512
- Tile size: 512x512
- Background: transparent alpha
- Foot baseline normalized across frames
