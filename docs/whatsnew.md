# What's New in Mabel

The in-app first-launch popup (F10, not yet built) reads from this file. Every Mabel release MUST add an entry here. Newest version on top.

## v1.1.3 (UNRELEASED)

### New
- _(work in progress — entries get added as features land)_

### Fixed
- _(work in progress)_

## v1.1.2 (2026-05-01)

### Fixed
- English transcription accuracy. v1.1.1 enabled Whisper auto-detect for multilingual support, but auto-detect picks the wrong language on short utterances and produces garbage that looks English-ish but isn't what you said. Reverted to English-only by default. A proper language picker is coming in the next release.
- "What's New" popup can no longer be dismissed by an accidental backdrop click before you've actually read it. Only the "Got it" button dismisses now.

## v1.1.1 (2026-05-01)

### New
- Mabel can now walk across your desktop. Optional, off by default. Settings → System → "Mabel on your desktop". Pick how big she is, how often she visits, and how long each visit lasts.
- Multilingual transcription. Mabel auto-detects the language you're speaking and transcribes in the source language. Works for 100+ languages.
- This "What's New" popup. From now on, every Mabel update will show you what changed on first launch.

### Fixed
- Default hotkey changed from Cmd+Shift+Space to Cmd+D (the old default conflicted with several other apps).
- Groq API key no longer prompts for keychain access on every dictation. The key is cached in memory after the first read.

## v1.1.0 (2026-05-01)

### Fixed
- Mabel now actually transcribes on every Mac, not just the build machine. Previous v1.0.5 had a missing-library bug that silently broke dictation on fresh installs (the wave overlay still moved, but nothing pasted).
- AppleEvents permission is now requested during first-run setup alongside Accessibility, so you grant both up front instead of being interrupted on first paste.
- Groq API key now shows as "Saved" after a fresh install if the key is still in your macOS Keychain from a previous version.

### New
- AI cleanup tier (opt-in, ~5 GB download). When enabled, dictations are run through a local LLM after Whisper to remove filler words, fix punctuation, and normalize numbers and proper nouns. Runs fully on-device. Toggle in Settings → Engine.
