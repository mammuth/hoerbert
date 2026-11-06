#!/bin/sh
set -e
# YouTube breaks yt-dlp every few weeks; refresh it on start unless disabled.
if [ "${YTDLP_AUTO_UPDATE:-true}" = "true" ]; then
  echo "[entrypoint] updating yt-dlp…"
  /opt/ytdlp/bin/pip install --no-cache-dir -q -U "yt-dlp[default]" bgutil-ytdlp-pot-provider 2>&1 | tail -1 || echo "[entrypoint] yt-dlp update failed, using bundled $(yt-dlp --version)"
fi
# TTS server, restarted when it exits: right away when it quit on purpose to
# free memory (idle), after a pause when it crashed.
if [ "${TTS:-true}" != "false" ] && [ "${TTS:-true}" != "0" ] && [ -n "${TTS_URL:-}" ] && [ -f /opt/tts/server.py ]; then
  (while true; do /opt/tts/bin/python /opt/tts/server.py || { echo "[entrypoint] tts server crashed, restarting…"; sleep 5; }; done) &
fi
echo "[entrypoint] yt-dlp $(yt-dlp --version), $(ffmpeg -version | head -1 | cut -d' ' -f1-3), deno $(deno --version | head -1 | cut -d' ' -f2)"
exec "$@"
