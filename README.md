# Mabel

Privacy-first dictation for macOS. Hold a hotkey, speak, release. Mabel transcribes your voice and pastes the text wherever your cursor is. Audio never leaves your Mac unless you explicitly opt in to a cloud engine with your own API key.

Named after a long-haired Persian who would never share your transcripts with anyone.

---

## What it does

- Press a global hotkey from any app, dictate, and the text appears at your cursor.
- Toggle mode (press to start, press to stop) or push-to-talk (hold while speaking).
- Live dictation: transcription streams in chunks while you speak, instead of one big pause at the end.
- Floating overlay shows a live waveform while recording. It floats over fullscreen apps and never steals focus from the app you are typing into.
- Local stats on usage: words per minute, total words dictated, daily streak. Counts only, never content.
- Voice command: end a dictation with "press enter" / "new line" and Mabel submits after pasting.

## Privacy by default

- Audio is recorded to a temp file, transcribed, then deleted.
- Transcripts are never logged to disk.
- No telemetry, no analytics, no remote logging.
- API keys live in the macOS Keychain, never in a config file.
- Insights are local-only counts, never the words.

## Engines

Mabel ships with two transcription backends:

| Engine | Where it runs | What's sent off-device |
|---|---|---|
| **Local (default)** | Whisper.cpp on your Mac via a sidecar binary | Nothing |
| **Groq cloud** | Groq's hosted Whisper, opt-in | The audio of each clip |

Local works completely offline once you download the model. Groq is faster and more accurate on long audio but requires a free API key from `console.groq.com`.

## System requirements

- macOS 12 (Monterey) or later
- Apple Silicon (M1 / M2 / M3 / M4). Intel build is not currently distributed.
- ~500 MB free for the Small Whisper model, ~1.5 GB for Medium

## Install

Download the latest DMG from the releases folder, mount it, and drag Mabel to Applications.

On first launch macOS will ask for three permissions. All three are required:

1. **Microphone** — to record audio.
2. **Accessibility** — to paste at the cursor (System Settings → Privacy & Security → Accessibility).
3. **Automation (System Events)** — to drive the paste keystroke.

A fourth Keychain prompt appears only if you save a Groq API key.

## Usage

1. Open Mabel. The Home view shows the current hotkey and status.
2. Pick a microphone in Settings → General.
3. Pick an engine in Settings → Engine. If Local, click Download to fetch the Whisper model.
4. Place your cursor in any text field and press the hotkey.

Default hotkey is `Cmd+Shift+D`. Rebind in Settings → General.

The Help view inside the app has the full feature reference and troubleshooting.

---

## How it's built

Mabel is a [Tauri 2](https://tauri.app) desktop app. The UI is vanilla TypeScript with no framework. The backend is Rust.

### Stack

| Layer | Tech |
|---|---|
| Shell | Tauri 2 (`macos-private-api` feature) |
| Frontend | TypeScript 5, Vite 6, vanilla DOM, custom CSS |
| Backend | Rust 2021 edition |
| Audio capture | `cpal` 0.15 |
| WAV encoding | `hound` 3.5 |
| Local transcription | `whisper.cpp` sidecar binary (Apple Silicon Metal build) |
| Cloud transcription | `reqwest` 0.12 against Groq's `whisper-large-v3` |
| Async runtime | `tokio` 1 (full features) |
| Floating overlay | `tauri-nspanel` v2 (NSPanel-backed window) |
| AppKit interop | `objc2` 0.6 + `objc2-app-kit` 0.3 |
| Secrets | `keyring` 3 (macOS Keychain via `apple-native`) |
| Clipboard | `arboard` 3 |
| Global hotkey | `tauri-plugin-global-shortcut` 2 |
| Autostart | `tauri-plugin-autostart` 2 |
| Shell open | `tauri-plugin-shell` 2 |
| Date / time | `chrono` 0.4 |

### Architecture

```
src/                       Frontend (TypeScript + HTML + CSS)
  main.ts                  Settings, hotkey rebind, stats wiring
  overlay.html             Floating dictation panel
  style.css                All styles

src-tauri/
  build.rs                 Embeds git hash + version at compile time
  src/
    main.rs                Tauri commands, plugin registration, app setup
    lib.rs                 Module roots + version constants
    settings.rs            Persisted user prefs (config.json)
    audio.rs               cpal recorder, ring buffer, RMS metering
    recorder.rs            Recording state machine, orchestration
    streaming.rs           VAD-driven chunking for live dictation
    transcribe_local.rs    whisper.cpp sidecar invocation
    transcribe_groq.rs     Groq HTTP client
    cleanup.rs             Whisper output post-processing
    paste.rs               Clipboard + osascript paste, Return keystroke
    overlay_macos.rs       NSPanel conversion for the overlay window
    system_ui.rs           Dock visibility toggle, system sounds
    secrets.rs             Keychain read/write via the keyring crate
    stats.rs               Local-only daily counts, WPM, streak
    downloader.rs          Whisper model fetcher with progress events
```

### Audio path

cpal captures 16 kHz mono PCM into a ring buffer. With streaming on, a tokio worker watches RMS levels and slices the buffer at silence boundaries, shipping each chunk to Whisper while recording continues. Without streaming, the entire buffer is written to a WAV file when you stop, transcribed once, pasted once.

### Overlay

The overlay window is converted from a standard NSWindow to a non-activating NSPanel via `tauri-nspanel`. This is the same primitive Spotlight uses. It floats over fullscreen apps, joins all Spaces, and never takes key/main status, so the app you are dictating into keeps focus.

## Development

```bash
# Install deps
npm install

# Dev (hot reload)
npm run tauri dev

# Production build (DMG + .app for Apple Silicon)
npm run tauri build
```

The DMG ends up at `src-tauri/target/release/bundle/dmg/`.

### Build-time version stamping

`src-tauri/build.rs` embeds the short git hash and a dirty flag as env vars at compile time. Reachable from Rust as `mabel_lib::MABEL_GIT_HASH`. Surfaced in the UI footer and About pane via the `get_version` command.

### Tests

```bash
cd src-tauri && cargo test --lib
```

### Project layout note

`tauri-plugin-single-instance` is intentionally not registered while a startup race is being investigated. Avoid double-launching during development.

---

## License

Copyright (c) 2026 Chibitek Labs. All rights reserved.
