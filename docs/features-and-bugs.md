# Mabel — Features & Bugs

Living plan. Add, reorder, strike. Today: 2026-04-30. Last shipped: v1.0.5.

## Bugs

### B1. Mistranscriptions
Whisper Small misses or substitutes words. Need concrete examples (said vs pasted) to pick a fix path.
- Candidate fixes: ship Whisper Medium/Large as opt-in, tighten VAD, tune temperature/beam size, add custom dictionary biasing.
- Decision pending examples.

### B2. Dropped words
Whole words missing from output. Need to know if drops cluster at start/end of utterances (VAD truncation) or scatter through the middle (decoder issue).
- If start/end: extend VAD pre/post padding, lower silence threshold.
- If scattered: model upgrade is the lever.

### B3. First-press latency
Glaido explicitly fixed this in late April 2026. Worth measuring our cold-start time on first hotkey press after idle.

### B5. SHIPPED v1.0.5 BROKEN ON ALL NON-DEV MACS — FIXED IN-TREE
The bundled `whisper-cpp` sidecar in v1.0.5 has rpaths pointing to the developer's local whisper.cpp build directory (`/Users/erick/Antigravity/whisper.cpp/build/...`). On any other Mac those paths don't exist, so dyld can't load `libwhisper.1.dylib` or any of the 6 ggml dylibs, and Whisper transcription silently fails. The wave-pulse overlay still moves (pure RMS calculation in Rust, no Whisper needed) so the app appears to work, but nothing ever pastes.

**Discovered:** 2026-05-01 when Erick installed v1.0.5 on a second Mac and saw exactly this symptom.

**Fix landed (uncommitted, in dev_erick branch):**
- Copied 6 required dylibs into `src-tauri/dylibs/`.
- Added `bundle.macOS.frameworks` array in `tauri.conf.json` so Tauri places them in `Contents/Frameworks/` and signs each.
- Added `@executable_path/../Frameworks` rpath to `binaries/whisper-cpp-aarch64-apple-darwin` (kept the absolute dev rpaths so cargo dev still works on the dev machine).
- Verified end-to-end: built v1.1.0-dev .app, dylibs present in Frameworks/, codesign valid, sidecar binary loads against the bundled libs.

**Ship action (RECOMMENDED HOTFIX):** cherry-pick this fix onto main, bump to v1.0.6, sign + notarize, push out. Every user on v1.0.5 needs this — they currently have a non-functional app.

### B4. Stats lost on app update
Reported by user during 1.1.0-dev testing. Stats currently live in app config dir but apparently not surviving updates. Need to verify and either change persistence path or back up before update.

---

## Features (ranked by impact-to-effort)

Reference: Wispr Flow and Glaido are the two leading dictation apps. Both share the same core loop (hotkey → speak → cleaned text pasted anywhere). Their moat is post-processing and customization, not raw Whisper accuracy.

### F1.1. Apple Intelligence cleanup tier (NEXT)
macOS 26 ships the Foundation Models framework — exposes the same ~3B on-device LLM that powers Apple Intelligence to any third-party app via Swift API. Available on M1+ Macs with Apple Intelligence enabled.

**Why it matters for Mabel:**
- Zero download (already on the system)
- Zero RAM overhead beyond what Apple Intelligence already uses
- First-class system API — likely faster than llama.cpp
- Solves our Tauri sidecar bundling problem cleanly (Apple guarantees the framework is present)
- Comparable cleanup quality to Gemma 4 in early reports

**Tradeoffs:** requires macOS 26 + Apple Intelligence eligibility (M1+). Older macOS or Intel Macs need the Gemma fallback.

**Implementation path:**
1. Write a small Swift CLI (~50 lines) that takes a prompt on stdin and writes cleaned text on stdout. Reference: [apfel](https://github.com/presswizards/apfel-Apple-Native-LLM).
2. Build with `swiftc`, produce `binaries/mabel-fm-aarch64-apple-darwin`.
3. Re-enable `externalBin` in `tauri.conf.json` for this sidecar (the FM framework is system-provided so no dylib chasing).
4. Add a third cleanup engine variant in `llm.rs` that spawns the Swift CLI per cleanup call.
5. Capability check: detect macOS ≥ 26 + Apple Intelligence eligible. Expose as a Tauri command.
6. Settings UI: add "System AI (recommended)" option, gated by capability check. Default to it on first launch when available.

**Three-tier story after F1.1:**
- **System AI** — Apple FM, zero download, default on supported Macs
- **Local AI** — Gemma 4 E4B, 5 GB download, fallback for older macOS / Intel
- **Rules only** — current default for users who don't want any AI pass

### F1. AI post-processing pass (the big one) — DEV-COMPLETE, NEEDS BINARY BUNDLING
Both Wispr and Glaido run an LLM cleanup after transcription: strip filler ("um", "uh", "like"), fix grammar, add punctuation, normalize capitalization. This is what makes voice-to-text feel "polished" instead of "raw transcript."

**Architecture decision (2026-04-30): local-only via llama.cpp**, mirroring our Whisper download UX. Note: Wispr (fine-tuned Llama 3.1 on Baseten + OpenAI) and Glaido (private servers) both go cloud — we're choosing differentiation on privacy.

**Model tiers (auto-download on first use):**
- **Light** — SmolLM3-3B Q4 (~2 GB). For 8 GB RAM machines.
- **Standard** — Gemma 4 E4B Q4 (~4 GB). Default. Released April 2, 2026, Apache 2.0, built for edge deployment.
- No Large tier. Sub-7B is the sweet spot; bigger models break the latency budget for dictation.

**On model choice:** there is no widely-available open-weight "dictation cleanup specialist." Specialized speech-augmented LLMs (NVIDIA Canary-Qwen, IBM Granite-Speech, Phi-4-Multimodal) do ASR+cleanup in one pass but require replacing Whisper, not adding to it. Closed alternatives (Onit) ship small fine-tuned cleanup models on-device — we'd need to build similar in-house if we want a true specialist later. For now, Gemma 4 E4B with a tightly-scoped cleanup-only system prompt (forbid rewriting/embellishing) is the best path.

**Future arc (F1.5):** Gemma 4 E2B/E4B have **native audio input**. Once the text-cleanup pipeline ships, spike a single-model architecture: audio → Gemma 4 → cleaned text, no Whisper. Could collapse our two-model stack into one and unlock streaming cleanup.

**Implementation path:**
1. Bundle `llama-server` (from llama.cpp) as a Tauri sidecar — persistent HTTP daemon, model stays loaded so we avoid the 1–2s cold-load on every cleanup.
2. Settings UI: model picker mirroring the Whisper picker, with download progress.
3. Cleanup prompt: short, fixed system prompt + per-call user prompt prefix ("Clean this transcript directly. Do not think, reason, or explain..."). Both halves needed — Gemma 4's reasoning mode otherwise leaks intermittently.
4. Use `--reasoning-budget 0 -rea off`. Strip leading `<think>...</think>` blocks from output (SmolLM3 emits empty ones).
5. Toggle: cleanup on/off. Default on once a model is downloaded.
6. Latency budget: target <800ms on M-series for typical 10–30 word utterance. With llama-server keeping the model warm, generation alone is 0.3–0.5s.

**Empirical tests (2026-04-30 to 2026-05-01, M5 Max, cold-load each call):**
- Test rig: `~/mabel-llm-test/` with 6 realistic dictation samples.
- **Gemma 4 E4B Q4_K_M (5 GB)**: 6/6 clean, 1.6–2.1s, normalized "v one point oh five" → "v1.05" and "Gemma four E four B" → "Gemma 4 E4B". **Selected as the AI tier.**
- **Gemma 4 E2B Q4_K_M (3 GB)**: 1/6 clean. Smaller model can't suppress reasoning; 5/6 leaked `<|channel>thought` blocks despite `--reasoning-budget 0`. Tripwires catch them but user gets rules-only fallback every time. **Unusable.**
- **SmolLM3-3B Q4_K_M (1.8 GB)**: 6/6 ran, 1.1s avg, but weaker on number/term normalization (got "v1.5" instead of "v1.05") and reasoning leaks. **Removed.**
- **Apple Foundation Models (zero download)**: After 4 prompt iterations including few-shot, still fails on questions and technical content (writes Python code instead of cleaning, hallucinates content away). Architectural mismatch — Apple's helpful-assistant tuning fights "transparent text transformer" use case. Backlogged as F1.1.

**Implementation status (2026-05-01):**
- `src-tauri/src/llm.rs` — model spec, server lifecycle, OpenAI-compatible `/v1/chat/completions` call, reasoning-block stripping. 7 unit tests passing.
- `Settings` extended with `cleanupMode` ("rules" | "llm") and `llmModel` ("light" | "standard"). Backward-compatible defaults.
- `recorder.rs` and `streaming.rs` cleanup paths route through LLM when enabled, fall back to rules on any error.
- Tauri commands: `check_llm_model_downloaded`, `download_llm_model`, `ensure_llm_started`.
- Settings UI: Cleanup mode dropdown, LLM model picker with download button + progress bar (mirrors Whisper).
- Eager warm-start in `setup` hook when LLM mode is configured and model is on disk.
- Server cleanly killed on app exit via `WindowEvent::Destroyed`.
- End-to-end smoke verified: real `llama-server` started with our exact args returns the expected cleaned output for our exact prompt structure.

**Ship-blocker — binary bundling:**
- Currently resolves `llama-server` from `MABEL_LLAMA_SERVER` env, then `/opt/homebrew/bin/llama-server`, then `/usr/local/bin`. Fine for dev but assumes user has llama.cpp installed via brew.
- Brew binary has rpath deps to `/opt/homebrew/Cellar/ggml/...` so it can't be copied as a Tauri sidecar directly.
- Path forward: vendor the official llama.cpp macOS-arm64 release zip into `src-tauri/binaries/` (binary + dylibs) and either fix rpaths or ship a wrapper that sets `DYLD_LIBRARY_PATH`. Re-add to `tauri.conf.json` `externalBin` once that's solved.

### F2. Custom dictionary / personal vocabulary
Wispr "learns" proper nouns and jargon automatically. Glaido lets users add terms manually. Easiest version: a settings pane with a textarea of names/terms/acronyms, injected as Whisper's `initial_prompt`.

### F3. Snippets (voice shortcuts)
"insert my calendar link" → expands to a stored URL. "standard reply" → full email template. Dictionary of trigger phrase → replacement string, applied during the cleanup pass.

### F4. Larger model option
Whisper Medium or Large as opt-in download. Bigger file, slower, much more accurate. UI: model picker in settings with size/speed/accuracy tradeoff shown.

### F5. App-aware tone
Wispr adjusts tone by foreground app (Slack casual, Gmail formal, VS Code terse code-comment style). Tauri can read the active app on macOS via NSWorkspace. Pair with F1 by varying the cleanup prompt per app bundle ID.

### F6. Multilingual auto-detect
Whisper supports it natively, just need to expose. Both competitors highlight 100+ languages.

### F7. Agent mode (Glaido Pro beta)
Voice commands that do things instead of typing: "summarize the selected text", "rewrite this more concisely", "translate to Spanish". Selection comes from accessibility API, result pasted back. This is differentiated territory worth exploring once F1 lands.

### F9. Animated desktop companion ("Mabel-cat-on-desktop")
Idea: animate the Mabel cat character as a desktop companion that occasionally walks around, idles, blinks, sits. Pure brand/delight play. Native macOS path: NSWindow with transparent background hosting a sprite-sheet PNG cycle, Core Animation timing. Out of scope for v1.1 (cleanup focus); revisit after Pro features ship.

**Asset needs (from ChatGPT brainstorm 2026-04-30):**
- 6–8 walk cycle frames, transparent PNG
- Sit, idle breathing, blink, tail flick states
- Consistent proportions across frames

### F8. Cross-device sync (Wispr feature)
Settings + dictionary + snippets sync across machines. Probably out of scope until we have a backend.

---

## Next session

Bring transcription examples (said vs pasted) and pattern of dropped words (start/end vs middle). That decides whether B1/B2 get fixed by F4 (larger model), VAD tuning, or both — and orders the rest of the feature work behind it.
