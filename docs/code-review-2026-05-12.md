# Mabel Code Review

Date: 2026-05-12
Mode: Read-only audit followed by scoped fix cycle
Scope: Full repository review with second-pass cross-layer and stale-routine checks

## Current Status

The highest-risk audit items have been addressed in the working tree:

- Retained debug audio was removed from the dictation stop path.
- AI cleanup is now gated by actual local runtime availability.
- Live dictation is explicitly paused in settings and documentation while the streaming worker is disabled.
- Text clipboard contents are restored after paste instead of always being cleared.
- What’s New model detection now checks English and multilingual model variants.
- The stale default hotkey test now matches the current `CmdOrCtrl+D` runtime default.

## Findings and Fix Notes

### High: Retained debug audio contradicted privacy claims

**What:** The app copied every stopped recording to `last_recording.wav`.

**How:** Found in `src-tauri/src/recorder.rs` during privacy and workflow review. The temp WAV was deleted, but a second debug copy was retained.

**Why:** README, app privacy copy, and Tauri bundle metadata promise audio is deleted after transcription. Keeping the last recording violated that contract.

**Status:** Fixed. The debug copy was removed; the temp WAV is still deleted after transcription.

### High: AI cleanup depended on an unbundled runtime

**What:** The UI exposed local AI cleanup, but only the model download was handled by the app. The required `llama-server` binary was resolved from `MABEL_LLAMA_SERVER` or Homebrew paths.

**How:** Found by tracing `src/main.ts`, `src-tauri/src/main.rs`, `src-tauri/src/llm.rs`, and `src-tauri/tauri.conf.json`.

**Why:** Users could download a large model and still silently fall back to rules because the runtime executable was missing.

**Status:** Fixed for current release safety. The UI now checks runtime availability and disables AI cleanup when the runtime is not present.

### Medium: Live dictation setting was a no-op

**What:** The UI allowed enabling live dictation, but the Rust recorder intentionally disabled the streaming worker.

**How:** Found by comparing the Settings UI with `Recorder::start_recording`.

**Why:** A clickable setting that does nothing creates a broken settings contract.

**Status:** Fixed. Streaming now defaults off, the toggle is disabled, and user-facing copy says the feature is paused.

### Medium: Paste path destroyed clipboard text

**What:** Dictation paste replaced the clipboard and then cleared it.

**How:** Found in `src-tauri/src/paste.rs`.

**Why:** Users could lose unrelated clipboard text on every dictation.

**Status:** Fixed. The previous text clipboard is restored after paste, image-only clipboard contents are restored, and paste-command failures now restore or clear the clipboard instead of leaving the dictated text behind.

### Medium: What’s New model detection ignored language variants

**What:** First-run downloads English-only Whisper by default, while the What’s New gate checked only the default multilingual model names.

**How:** Found by tracing `maybeRunFirstTimeSetup`, `maybeShowWhatsNew`, and `check_model_downloaded`.

**Why:** Users with only an English model could be treated as having no model and miss the version popup.

**Status:** Fixed. The gate checks small/medium across both `en` and `multi`.

### Low: Stale default hotkey test

**What:** Runtime default was `CmdOrCtrl+D`; the test still expected `CmdOrCtrl+Shift+Space`.

**How:** Confirmed by `cargo test`, which failed only `settings::tests::test_default_settings` before the fix.

**Why:** A red test suite weakens release confidence.

**Status:** Fixed. The test now matches the current hotkey and paused streaming default.

## Verification

- `npm run build` passed after frontend changes.
- Focused Rust tests passed for recorder, LLM, settings, and paste areas during the fix cycle.
- Full verification should finish with `cargo test` and a final `npm run build`.

## Remaining Follow-Up

- Bundle a production `llama-server` sidecar if AI cleanup should be available to all users.
- Re-enable live dictation only after the streaming shutdown hang is fixed and covered by tests.
- Clipboard restoration now covers text and image-only clipboard contents. Rich multi-format clipboard payloads still degrade to text because the current `arboard` flow cannot round-trip every macOS pasteboard type.
