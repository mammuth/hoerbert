#!/bin/sh
set -e
# YouTube breaks yt-dlp every few weeks; refresh it on start unless disabled.
if [ "${YTDLP_AUTO_UPDATE:-true}" = "true" ]; then
  echo "[entrypoint] updating yt-dlp…"
  /opt/ytdlp/bin/pip install --no-cache-dir -q -U "yt-dlp[default]" bgutil-ytdlp-pot-provider 2>&1 | tail -1 || echo "[entrypoint] yt-dlp update failed, using bundled $(yt-dlp --version)"
fi
# Voice commands: fetch the Whisper models once (int8 only).
if [ "${VOICE:-false}" = "true" ]; then
  dir="${VOICE_MODEL_DIR:-${DATA_DIR:-/data}/models}"
  for spec in "asr-models/sherpa-onnx-whisper-${VOICE_GATE_MODEL:-base}" "asr-models/sherpa-onnx-whisper-${VOICE_WHISPER_MODEL:-small}"; do
    name="${spec#*/}"
    [ -d "$dir/$name" ] && continue
    echo "[entrypoint] downloading voice model $name…"
    mkdir -p "$dir/.tmp" && rm -rf "$dir/.tmp/$name"
    if curl -fsSL "https://github.com/k2-fsa/sherpa-onnx/releases/download/$spec.tar.bz2" | tar xj -C "$dir/.tmp" --wildcards '*int8.onnx' '*tokens.txt'; then
      mv "$dir/.tmp/$name" "$dir/$name"
    else
      echo "[entrypoint] downloading $name failed, voice commands will be off"
    fi
  done
fi
echo "[entrypoint] yt-dlp $(yt-dlp --version), $(ffmpeg -version | head -1 | cut -d' ' -f1-3), deno $(deno --version | head -1 | cut -d' ' -f2)"
exec "$@"
