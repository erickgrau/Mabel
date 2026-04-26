# Mabel

Privacy-first dictation for macOS. Hold a hotkey, speak, release. Mabel transcribes your voice and pastes the text wherever your cursor is. Audio never leaves your Mac unless you explicitly opt in to a cloud engine with your own API key.

Named after a long-haired Persian who would never share your transcripts with anyone.

Open source under the [MIT license](LICENSE). Fork it, build it, ship your own version.

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

| Engine | Where it runs | What's sent off-device |
|---|---|---|
| **Local (default)** | Whisper.cpp on your Mac via a sidecar binary | Nothing |
| **Groq cloud** | Groq's hosted Whisper, opt-in | The audio of each clip |

Local works completely offline once you download the model. Groq is faster and more accurate on long audio but requires a free API key from `console.groq.com`.

## System requirements

- macOS 12 (Monterey) or later
- Apple Silicon (M1 / M2 / M3 / M4). Intel build is not currently distributed.
- ~500 MB free for the Small Whisper model, ~1.5 GB for Medium.

## Install (end users)

Download the latest signed and notarized DMG from the [Releases](../../releases) page, mount it, drag Mabel to Applications, open it.

On first launch:

1. macOS asks for **Microphone** access. Click Allow.
2. Mabel auto-downloads the Whisper Small model (~500 MB) with a progress bar. One-time setup.
3. Mabel triggers macOS's **Accessibility** dialog. Click Open System Settings, flip the Mabel toggle on.
4. Press your hotkey and speak. macOS asks for **Automation (System Events)** the first time text is pasted. Click Allow.

A fourth Keychain prompt appears only if you save a Groq API key.

Default hotkey is `Cmd+Shift+/`. Rebind in Settings → General. The Help view inside the app has the full feature reference and troubleshooting.

---

# Build it yourself

If you want to fork Mabel, customize it, and ship your own signed/notarized DMG, the rest of this README walks through every step.

## Prerequisites

- macOS 12+ on Apple Silicon (build host).
- **Xcode Command Line Tools**: `xcode-select --install`
- **Rust** (1.78+ recommended): `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`
- **Node.js 20+** and npm: install via [nodejs.org](https://nodejs.org) or `brew install node`
- **Apple Developer Program membership** ($99/year) — only required if you want to ship a DMG that opens cleanly without Gatekeeper warnings. Not needed for personal/dev builds.

## Clone, install, run in dev

```bash
git clone https://github.com/erickgrau/Mabel.git
cd Mabel
npm install
npm run tauri dev
```

That gives you a hot-reloading dev build. Permissions, hotkey, recording, transcription all work.

## Run tests

```bash
cd src-tauri && cargo test --lib
```

## Build a local production DMG (unsigned)

```bash
npm run tauri build
```

The DMG lands at `src-tauri/target/release/bundle/dmg/Mabel_<version>_aarch64.dmg`. It's unsigned, so macOS Gatekeeper will warn anyone who tries to open it. Fine for solo use; not for distribution.

## Make it your own (forking)

If you fork Mabel and plan to ship your own builds, you must change four identifiers so your build doesn't collide with the upstream signing identity:

1. **Bundle identifier** — `src-tauri/tauri.conf.json` field `identifier`. Change `com.mabel.app` to your own reverse-DNS string (e.g. `com.yourname.mydictate`).
2. **Product name** — `src-tauri/tauri.conf.json` field `productName`.
3. **Signing identity** — `src-tauri/tauri.conf.json` field `bundle.macOS.signingIdentity`. Change to your own Developer ID (created below). Same for `providerShortName` (your team ID).
4. **Apple Events / TCC reset paths** — anywhere code references `com.mabel.app` directly (e.g. `tccutil reset Microphone com.mabel.app`), update to your bundle ID.

Search the repo for `com.mabel.app` and `Erick Grau (DF9FB764AR)` to find every occurrence.

---

# Sign and notarize a distributable DMG

This is the part that turns "a DMG" into "a DMG users can open without macOS yelling at them." Required if you're distributing to anyone other than yourself.

## One-time Apple Developer setup

### 1. Sign up for the Apple Developer Program

Go to [developer.apple.com/programs](https://developer.apple.com/programs/) and join. $99/year. Approval takes 24-48 hours typically.

### 2. Create a Developer ID Application certificate

This certificate is what makes macOS trust your builds.

a. Open **Keychain Access** → menu → `Certificate Assistant` → `Request a Certificate from a Certificate Authority`.
   - Email: your Apple ID
   - Common Name: your name
   - Choose `Saved to disk` → save the `.certSigningRequest` file.

b. Go to [developer.apple.com/account/resources/certificates/list](https://developer.apple.com/account/resources/certificates/list).

c. Click `+` → choose **Developer ID Application** → Continue → upload the `.certSigningRequest` file → Continue → download the resulting `.cer` file.

d. Double-click the `.cer` file. It installs into your Keychain.

e. Verify it's there:
   ```bash
   security find-identity -v -p codesigning
   ```
   You should see a line like:
   ```
   1) ABCD1234... "Developer ID Application: Your Name (TEAMID)"
   ```

### 3. Create an app-specific password for notarization

Apple's notarization service uses an app-specific password (not your main Apple ID password).

a. Go to [appleid.apple.com/account/manage](https://appleid.apple.com/account/manage) → sign in.
b. Find the **App-Specific Passwords** section → click `+` → label it "Mabel Notarization" (or anything).
c. Apple shows a password like `abcd-efgh-ijkl-mnop`. Copy it. This is shown only once.

### 4. Store notarization credentials in your keychain

```bash
xcrun notarytool store-credentials AC_PASSWORD \
  --apple-id "your-apple-id-email@example.com" \
  --team-id "YOURTEAMID" \
  --password "abcd-efgh-ijkl-mnop"
```

You'll see `Credentials saved to Keychain.` The profile name `AC_PASSWORD` is what you'll reference later.

## Configure tauri.conf.json

In `src-tauri/tauri.conf.json`, the `bundle.macOS` section already has the signing setup. Update three fields with your values:

```json
"macOS": {
  "minimumSystemVersion": "11.0",
  "signingIdentity": "Developer ID Application: Your Name (YOURTEAMID)",
  "providerShortName": "YOURTEAMID",
  "entitlements": "entitlements.plist",
  "hardenedRuntime": true
}
```

The `entitlements.plist` file is already in `src-tauri/`. It declares:
- Audio input (mic capture)
- Network client (Groq API, model download)
- Apple Events (paste via System Events)
- JIT and unsigned executable memory (required by WebKit)
- Disable library validation (required to load whisper-cpp sidecar binary)

## Build with signing + notarization

```bash
APPLE_ID="your-apple-id@example.com" \
APPLE_PASSWORD="abcd-efgh-ijkl-mnop" \
APPLE_TEAM_ID="YOURTEAMID" \
npm run tauri build
```

The build:
1. Compiles release Rust binary
2. Bundles into `Mabel.app`
3. Signs the binary, sidecar, and `.app` with your Developer ID
4. Submits the `.app` to Apple's notary service (waits ~3-5 minutes)
5. Staples the notarization ticket to the `.app`
6. Builds and signs a `.dmg`

## Notarize and staple the DMG

Tauri's bundler notarizes the `.app` but **not** the DMG itself. Apple recommends notarizing the DMG too. Run these after `npm run tauri build`:

```bash
DMG="src-tauri/target/release/bundle/dmg/Mabel_$(node -p 'require("./package.json").version')_aarch64.dmg"

xcrun notarytool submit "$DMG" --keychain-profile AC_PASSWORD --wait
xcrun stapler staple "$DMG"
spctl -a -t open --context context:primary-signature -vv "$DMG"
```

The last command should print `accepted / source=Notarized Developer ID`. If so, the DMG is fully notarized and stapled.

## Optional: include a FIRST LAUNCH.txt in the DMG

Mabel's distributed DMG includes a `FIRST LAUNCH.txt` next to `Mabel.app` so first-time users see permission setup notes. To include your own:

```bash
WORK="/tmp/dmg-work" && rm -rf "$WORK" && mkdir -p "$WORK"
hdiutil convert "$DMG" -format UDRW -o "$WORK/rw.dmg" -quiet
MOUNT=$(hdiutil attach -nobrowse -noverify -noautoopen "$WORK/rw.dmg" | tail -1 | awk '{ for(i=3;i<=NF;i++) printf "%s ", $i; print "" }' | sed 's/ *$//')
cp /path/to/your/FIRST\ LAUNCH.txt "$MOUNT/"
hdiutil detach "$MOUNT" -quiet
hdiutil convert "$WORK/rw.dmg" -format UDZO -imagekey zlib-level=9 -o "$WORK/final.dmg" -quiet
mv "$WORK/final.dmg" "$DMG"
codesign --sign "Developer ID Application: Your Name (YOURTEAMID)" --timestamp "$DMG"
xcrun notarytool submit "$DMG" --keychain-profile AC_PASSWORD --wait
xcrun stapler staple "$DMG"
```

The final DMG is signed, notarized, stapled, and ready to share.

---

# How it's built

Mabel is a [Tauri 2](https://tauri.app) desktop app. The UI is vanilla TypeScript with no framework. The backend is Rust.

## Stack

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

## Architecture

```
src/                       Frontend (TypeScript + HTML + CSS)
  main.ts                  Settings, hotkey rebind, stats wiring,
                           first-run model download, Accessibility request
  overlay.html             Floating dictation panel
  style.css                All styles

src-tauri/
  build.rs                 Embeds git hash + version at compile time
  entitlements.plist       Hardened-runtime entitlements for notarization
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
    system_ui.rs           Dock visibility, sounds, Accessibility request
    secrets.rs             Keychain read/write via the keyring crate
    stats.rs               Local-only daily counts, WPM, streak
    downloader.rs          Whisper model fetcher with progress events
```

## Audio path

cpal captures 16 kHz mono PCM into a ring buffer. With streaming on, a tokio worker watches RMS levels and slices the buffer at silence boundaries, shipping each chunk to Whisper while recording continues. Without streaming, the entire buffer is written to a WAV file when you stop, transcribed once, pasted once.

## Overlay

The overlay window is converted from a standard NSWindow to a non-activating NSPanel via `tauri-nspanel`. This is the same primitive Spotlight uses. It floats over fullscreen apps, joins all Spaces, and never takes key/main status, so the app you are dictating into keeps focus.

## Build-time version stamping

`src-tauri/build.rs` embeds the short git hash as an env var at compile time. Reachable from Rust as `mabel_lib::MABEL_GIT_HASH`. Surfaced in the UI footer and About pane via the `get_version` command.

## Project layout note

`tauri-plugin-single-instance` is intentionally not registered while a startup race is being investigated. Avoid double-launching during development.

---

## Contributing

Forks are welcome. PRs are welcome. The codebase is small and the patterns are obvious — read [main.rs](src-tauri/src/main.rs) and [main.ts](src/main.ts) and you'll have the model in 20 minutes.

If you ship a fork as your own product, please change the bundle identifier and signing identity (see [Make it your own](#make-it-your-own-forking)) so it doesn't collide with the upstream.

## License

MIT. See [LICENSE](LICENSE).
