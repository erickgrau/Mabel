# What's New in Mabel

The in-app first-launch popup reads from this file. Every Mabel release MUST add an entry here. Newest version on top.

## v1.1.6 (2026-05-13)

### New
- Signed and notarized test build. Mabel now uses a stable Developer ID identity so macOS Accessibility and Automation permissions stick across installs.
- In-app updates are ready. Settings → About now includes a signed update checker backed by GitHub Releases, so future versions can be installed from inside Mabel.
- The share DMG now includes the Mabel app icon, volume icon, and Applications shortcut before signing and notarization.

### Fixed
- Push-to-talk transcription no longer stalls after releasing the hotkey.
- Paste failures now report the real AppleScript error in the debug log.
- If automatic paste fails, Mabel restores the previous clipboard instead of leaving the dictated text there.
- Image-only clipboard contents are preserved around dictation paste.

## v1.1.3 (2026-05-02)

### New
- English-only Whisper models. Settings → Engine → Language lets you pick English-only (recommended) or Multilingual. The English-only models are noticeably more accurate for English dictation. New installs default to English-only Small. Existing installs keep their current model and can switch in Settings, which will download the matching new model.
- Custom Dictionary. Add proper nouns, acronyms, and jargon you dictate often, and Mabel will spell them correctly. Open the Dictionary tab in the sidebar. Words are stored locally on this Mac and never uploaded, even when using the cloud engine.

### Fixed
- More reliable transcription. Whisper now runs with steadier defaults. A previous tuning pass that bumped thread count caused streaming chunks to come back blank or with the classic "Thanks for watching" hallucination on quieter speech. Reverted.

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
- AI cleanup tier (opt-in, ~5 GB download) now only appears when the local LLM runtime is available. When enabled, dictations are run through a local LLM after Whisper to remove filler words, fix punctuation, and normalize numbers and proper nouns. Runs fully on-device. Toggle in Settings → Engine.
