"""Neural text to speech for Hörbert: Piper voices on sherpa-onnx, kept in
memory so a message only costs the synthesis (loading a voice takes ~1 s).
Voices load on first use. After TTS_IDLE minutes (default 30, 0: never)
without a request the server exits to free their memory (~150 MB each);
docker-entrypoint.sh starts a fresh, empty one right away.

    POST /<lang>   body: text   →  audio/wav (mono, 16 bit)

One voice per directory in TTS_VOICES_DIR, named after its language (`en`,
`de`, …) and holding `<model>.onnx` + `tokens.txt`. `en-us` falls back to
`en`, unknown languages to TTS_VOICE.
"""

import array
import io
import math
import os
import sys
import threading
import time
import wave
from http.server import BaseHTTPRequestHandler, HTTPServer

import sherpa_onnx

VOICES_DIR = os.environ.get("TTS_VOICES_DIR", "/opt/tts/voices")
# Piper's phonemizer data (from espeak-ng), shared by all voices.
DATA_DIR = os.environ.get("TTS_DATA_DIR", "/opt/tts/espeak-ng-data")
PORT = int(os.environ.get("TTS_PORT", "5002"))
DEFAULT = os.environ.get("TTS_VOICE", "en").split("+")[0].strip().lower()


def speeds(spec):
    """Talking speed per language, e.g. `de=1.2,en=1.0`; 1.0 if not listed."""
    out = {}
    for part in filter(None, (p.strip() for p in spec.split(","))):
        lang, _, speed = part.partition("=")
        try:
            out[lang.strip().lower()] = float(speed)
        except ValueError:
            # e.g. `140`, words per minute from the espeak-ng days
            print(f"[tts] ignoring TTS_SPEED entry {part!r}, expected language=factor like de=1.2", flush=True)
    return out


SPEED = speeds(os.environ.get("TTS_SPEED", ""))
IDLE = float(os.environ.get("TTS_IDLE", "30")) * 60


def load(lang):
    path, speed = os.path.join(VOICES_DIR, lang), SPEED.get(lang, 1.0)
    t = time.monotonic()
    model = next(f for f in os.listdir(path) if f.endswith(".onnx"))
    vits = sherpa_onnx.OfflineTtsVitsModelConfig(model=os.path.join(path, model), tokens=os.path.join(path, "tokens.txt"), data_dir=DATA_DIR)
    tts = sherpa_onnx.OfflineTts(sherpa_onnx.OfflineTtsConfig(model=sherpa_onnx.OfflineTtsModelConfig(vits=vits, num_threads=1)))
    tts.generate("Hi.", sid=0, speed=speed)  # the first run is slow, get it out of the way
    print(f"[tts] loaded {lang} ×{speed} in {time.monotonic() - t:.1f}s", flush=True)
    return tts, speed


def voice(lang):
    """The voice for `lang`, loaded on first use; `en-us` → `en` → TTS_VOICE → any."""
    for name in (lang, lang.split("-")[0], DEFAULT, *LANGS):
        if name in LANGS:
            if name not in LOADED:
                LOADED[name] = load(name)
            return LOADED[name]
    return None


def gain(samples):
    """Voices differ by 15 dB and more: bring every voice to -20 dBFS RMS,
    so one TTS_VOLUME fits all, without clipping."""
    if not samples:
        return 1.0
    rms = math.sqrt(sum(s * s for s in samples) / len(samples))
    peak = max(abs(s) for s in samples)
    return min(0.1 / rms if rms else 1.0, 0.9 / peak if peak else 1.0)


def to_wav(audio):
    g = gain(audio.samples)
    pcm = array.array("h", (int(max(-1.0, min(1.0, s * g)) * 32767) for s in audio.samples))
    if sys.byteorder == "big":
        pcm.byteswap()
    buf = io.BytesIO()
    with wave.open(buf, "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(audio.sample_rate)
        w.writeframes(pcm.tobytes())
    return buf.getvalue()


class Handler(BaseHTTPRequestHandler):
    def do_POST(self):
        global last_request
        with BUSY:
            last_request = time.monotonic()
            v = voice(self.path.strip("/").lower())
            if v is None:
                self.send_error(404, "no voice for this language")
                return
            text = self.rfile.read(int(self.headers.get("content-length", 0))).decode("utf-8", "replace")
            tts, speed = v
            body = to_wav(tts.generate(text, sid=0, speed=speed))
            last_request = time.monotonic()
        self.send_response(200)
        self.send_header("content-type", "audio/wav")
        self.send_header("content-length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *args):
        pass


def exit_when_idle():
    """Python rarely hands freed memory back to the system, so leave instead
    of unloading. Only with voices loaded, or an empty server would cycle."""
    while True:
        time.sleep(30)
        if LOADED and time.monotonic() - last_request > IDLE and BUSY.acquire(blocking=False):
            print(f"[tts] idle for {IDLE / 60:g} min, exiting to free memory", flush=True)
            os._exit(0)


LANGS = sorted(os.listdir(VOICES_DIR))
LOADED = {}
BUSY = threading.Lock()
last_request = time.monotonic()
if IDLE > 0:
    threading.Thread(target=exit_when_idle, daemon=True).start()
print(f"[tts] voices {', '.join(LANGS)} on 127.0.0.1:{PORT}, loaded on first use", flush=True)
# One request at a time: synthesis keeps a core busy anyway.
HTTPServer(("127.0.0.1", PORT), Handler).serve_forever()
