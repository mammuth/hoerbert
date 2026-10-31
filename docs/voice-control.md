# Voice control — handoff (2026-10-31)

State of the experimental spoken-command feature on branch `feat/voice-control`.
Not merged; Max wants more live testing first.

## What it does

With `VOICE=true` Hörbert listens to the voice of everyone in its channel:

- **One go:** "Hörbert play Bohemian Rhapsody", "Hörbert skip", "Hörbert pause",
  "Hörbert resume" (German: spiel, nächster/weiter, stopp, fortsetzen).
- **With beep:** "Hörbert" alone → bip-bip → the next utterance (within 6 s) is the
  command; without a command word it is taken as a song title (max 8 words).
- Recognised commands run through the normal chat command handler and are echoed
  in channel chat: `🎙 Max: play … ➕ queued: …`.
- Wake word is configurable: `VOICE_WAKE=hoerbert` (default) or `siri` (used for the
  live tests).

## How it works (`src/voice.rs`)

```
TS voice packet (ts.rs, StreamItem::Audio) ──try_send──▶ voice thread
  per speaker: Opus → 16 kHz mono (decoder runs at 16 kHz, no resampling)
  utterance ends: no packets for 500 ms (push-to-talk / voice activation)
                  or 600 ms silence inside the stream (open mic), or 8 s max
  ▶ Whisper base ("gate") on every utterance → wake word within first 3 words
    (or directly followed by a command word)? fuzzy match: Levenshtein,
    "Hør bært" two-word spellings, Herbette/Robert/Firi …
  ▶ wake + command: re-transcribe the same audio with Whisper small → parse
  ▶ bare wake word: TsEvent::VoiceListening → chat.rs plays data/voice-beep.wav
    via the soundboard mixer; next utterance → Whisper small → command
  ▶ TsEvent::Voice { text: "play …" } → chat::handle("!play …")
```

- 1-byte packets are TeamSpeak's end-of-transmission markers; ignored, pauses are
  detected by time (`tick`, every 100 ms).
- Spoken `pause`/`resume` only act if they change something (chat `!pause` toggles).
- `VOICE_DEBUG_USER=<nickname>` writes that speaker's utterances as raw f32le
  16 kHz files to `$DATA_DIR/voice-debug/` — for tuning only, delete afterwards.
- sherpa-rs 0.6.8 (sherpa-onnx v1.12.9) with **shared** libs: the upstream "static"
  archive contains no `.a` files. The `.so` files are copied next to the binary,
  `build.rs` sets `rpath=$ORIGIN`. The Dockerfile copies them to `/usr/local/bin`.
- Models: downloaded by `docker-entrypoint.sh` into `$DATA_DIR/models` on first
  start (int8 only, base ~160 MB + small ~360 MB).

## Config

| Variable | Default | |
| --- | --- | --- |
| `VOICE` | `false` | enable |
| `VOICE_WAKE` | `hoerbert` | `hoerbert` \| `siri` |
| `VOICE_GATE_MODEL` | `base` | Whisper for the wake word (`tiny` also works, slightly worse) |
| `VOICE_WHISPER_MODEL` | `small` | Whisper for commands / titles |
| `VOICE_LANGUAGE` | auto | Whisper language |
| `VOICE_MODEL_DIR` | `$DATA_DIR/models` | |
| `VOICE_DEBUG_USER` | — | dump one speaker's audio for tuning |

## What was learned (why it looks like this)

1. **Dedicated keyword spotter failed.** First version used sherpa-onnx KWS
   (zipformer gigaspeech 3.3M, English BPE). Synthetic TTS clips worked, real
   TeamSpeak voices did not: "Herbert" 0–1/5 even at threshold 0.01, "Siri" ~1/3
   live. Offline replay of the recordings gave the same misses → model limit, not
   a pipeline bug. Whisper `base` on the same recordings caught nearly every
   "Siri"/"Herbert" for ~6–10 % of one core while people talk (247 clips ≈ 10 min
   of speech in 58 s on 2 threads; `tiny`: 36 s).
2. **TeamSpeak voice activation splits speech** into separate transmissions
   ("Siri." / pause / "play …") and clips the start of each one ("play" →
   "lay", "Labohemian"). Hence: one-go commands work best, "lay" counts as play,
   and after a beep "play" is optional.
3. **Whisper `small` titles:** good for English ("Bohemian Rhapsody", "another one
   bites the dust"), weak on German band names ("Die Ärzte – Junge" → "die erste
   Junge"), occasional wrong language detection (`lang=da`). `medium` untested.
4. tsclientlib's audio feature already delivers per-client Opus packets; the TS
   server only sends voice while someone talks → zero cost in a quiet channel.

## Testing

```bash
# unit tests (wake word matching, command parsing)
cargo test --release
# replay recordings / clips (raw f32le 16 kHz mono) through the engine
TS_ADDRESS=x VOICE_MODEL_DIR=… VOICE_WAKE=siri VOICE_TEST_FILES=a.f32,b.f32 \
  cargo test --release wav_files -- --ignored --nocapture
# end-to-end over TeamSpeak: test client speaks a clip into the channel
SPEAK=clip.f32 cargo run --release --example listen -- 127.0.0.1 "" 10
```

Building needs libclang (bindgen) in addition to the usual deps; sherpa-rs
downloads the sherpa-onnx libs at build time (GitHub).

## Live test setup used (sandbox, 2026-10-31)

- Build container `hb-build` (`rust:1-bookworm` + libclang, yt-dlp, deno), repo at
  `/src`, scratchpad at `/scratch` (target dir, models, recordings).
- Dev bot "Hörbert Dev" on Nils' server `185.249.199.234` (own identity in
  `/scratch/data-live`, so it doesn't clash with the prod bot), channel
  `🍅 Ketten Fetisch 2.0`, `VOICE_WAKE=siri`, `VOICE_DEBUG_USER=MAMMUTH`.
- Sandbox needed `sbx policy allow network 185.249.199.234 --protocol udp`
  (the default rule is TCP only).
- Docker image `hoerbert:voice` was built and verified with the KWS version;
  the Whisper-gate version is only tested via the native binary so far.

## Open / next

- Live-test the Whisper gate with several people (only tested on Max's recordings
  + briefly live); measure CPU on the real host.
- Rebuild + verify the Docker image with the current code (entrypoint now
  downloads base + small instead of the KWS model).
- Delete the debug recordings (`voice-debug/`, contains Max's channel talk).
- Ideas: duck the music while a command is spoken; try Whisper `medium` or a
  forced language for titles; per-user cooldown against accidental triggers;
  mention voice commands in `!help`.
