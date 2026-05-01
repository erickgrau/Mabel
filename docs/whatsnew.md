# What's New in Mabel

The in-app first-launch popup (F10, not yet built) reads from this file. Every Mabel release MUST add an entry here. Newest version on top.

## v1.1.0 (2026-05-01)

### Fixed
- Mabel now actually transcribes on every Mac, not just the build machine. Previous v1.0.5 had a missing-library bug that silently broke dictation on fresh installs (the wave overlay still moved, but nothing pasted).
- AppleEvents permission is now requested during first-run setup alongside Accessibility, so you grant both up front instead of being interrupted on first paste.
- Groq API key now shows as "Saved" after a fresh install if the key is still in your macOS Keychain from a previous version.

### New
- AI cleanup tier (opt-in, ~5 GB download). When enabled, dictations are run through a local LLM after Whisper to remove filler words, fix punctuation, and normalize numbers and proper nouns. Runs fully on-device. Toggle in Settings → Engine.
