# Hörbert

A TeamSpeak music bot for your own server: it joins a channel as a normal
voice client and plays audio for everyone in it. Control it from a shared web
UI (queue, search, history) or with chat commands in TeamSpeak.

- **Sources:** anything [yt-dlp](https://github.com/yt-dlp/yt-dlp) supports — YouTube (videos, playlists, search), SoundCloud, Bandcamp, … plus direct audio/radio URLs.
- **No accounts, no ads:** yt-dlp fetches the raw audio stream; YouTube's in-video sponsor segments are skipped with [SponsorBlock](https://sponsor.ajay.app/).
- **TS3 and TS6 servers** (uses the TS3 full-client protocol via [tsclientlib](https://github.com/ReSpeak/tsclientlib)).
- **Audio:** 48 kHz stereo Opus Music, 128 kbit/s, 20 ms frames.
- Single Rust binary with the web UI embedded, state in one JSON file.

## Quick start (Docker)

```bash
git clone <this repo> hoerbert && cd hoerbert
cp .env.example .env         # set TS_ADDRESS (and TS_CHANNEL etc.)
docker compose up -d --build
open http://localhost:8080
```

The bot generates a TeamSpeak identity on first start and keeps it in
`./data/identity.json`, so you can give it permissions (talk power!) once.

### Configuration

Everything is an environment variable, see [`.env.example`](.env.example):

| Variable | Default | Meaning |
| --- | --- | --- |
| `TS_ADDRESS` | — | `host[:port]` of the TeamSpeak server (required) |
| `TS_NAME` | `Hörbert` | bot nickname |
| `TS_CHANNEL` | server default | channel path to join, e.g. `Lounge/Music` |
| `TS_CHANNEL_PASSWORD`, `TS_SERVER_PASSWORD` | — | passwords, if any |
| `WEB_BIND` | `0.0.0.0:8080` | where the web UI listens |
| `PUBLIC_URL` | — | URL shown by `!link` / `!help` |
| `DEFAULT_VOLUME` | `0.1` | playback volume, 0.0 – 1.5 (fixed, no UI control) |
| `OPUS_BITRATE` | `128000` | Opus encoder bitrate |
| `SPONSORBLOCK` | `true` | skip SponsorBlock segments on YouTube |
| `SPONSORBLOCK_CATEGORIES` | `sponsor,selfpromo,interaction,music_offtopic` | which categories to skip |
| `COMMAND_PREFIX` | `!` | chat command prefix |
| `YTDLP_ARGS` | — | extra yt-dlp args (cookies, player clients, PO token provider) |
| `YTDLP_AUTO_UPDATE` | `true` | `pip install -U yt-dlp` on container start |
| `DATA_DIR` | `data` | state + identity |
| `RUST_LOG` | `info,hoerbert=debug` | log filter |

The web UI has no login. Run it on your LAN or behind a reverse proxy with
auth if the port is reachable from the internet.

### Chat commands

```
!play <link or search>   add to queue (playlist links queue the whole list)
!next <link or search>   play right after the current track
!skip  !prev  !pause
!now  !queue  !clear
```

## When YouTube says “Sign in to confirm you’re not a bot”

That is YouTube rate-limiting the IP, usually datacenter IPs. Residential
connections rarely see it. Options, in order:

1. Keep yt-dlp current (the container does this on every start).
2. Run the PO-token provider sidecar: `docker compose --profile pot up -d` and set
   `YTDLP_ARGS=--extractor-args "youtubepot-bgutilhttp:base_url=http://bgutil:4416"`.
3. Export cookies from a browser to `./data/cookies.txt` and set
   `YTDLP_ARGS=--cookies /data/cookies.txt` (yt-dlp wiki: *Exporting YouTube cookies*).

## How it works

```
web UI ──HTTP/WS──▶ axum ──▶ Player actor ──20 ms Opus frames──▶ TeamSpeak thread (tsclientlib)
                              │  ▲
                   yt-dlp -J ─┘  └─ ffmpeg → s16le PCM → gain → libopus
                   (stream URL, metadata, SponsorBlock segments)
```

- The player resolves the stream URL right before playback (googlevideo URLs
  expire) and pre-resolves the next track while the current one plays.
- If ffmpeg gets a 403 or the stream dies, it re-resolves and resumes at the
  same position; as a last resort it pipes `yt-dlp -o -` into ffmpeg.
- Pausing stops the Opus stream (and the TeamSpeak “talking” indicator); seeking
  restarts ffmpeg with `-ss`.

## Development

```bash
# backend (needs ffmpeg, yt-dlp, libopus-dev, pkg-config, cmake on PATH)
TS_ADDRESS=127.0.0.1 cargo run
# frontend with hot reload, proxied to :8080
cd web && npm install && npm run dev
```

`cargo run --example listen -- <server> [channel] [seconds]` joins as a second
client and counts the audio packets it receives — handy to verify the bot is
audible without starting a TeamSpeak client.

A local TeamSpeak server for testing:

```bash
docker run -d --name ts3 -e TS3SERVER_LICENSE=accept -p 9987:9987/udp teamspeak:latest
# or TS6
docker run -d --name ts6 -e TSSERVER_LICENSE_ACCEPTED=accept -p 9987:9987/udp teamspeaksystems/teamspeak6-server
```

## Deployment

Push to `main` → `.github/workflows/build.yml` publishes `ghcr.io/mammuth/hoerbert:latest` and
`:sha-<commit>` (public package). Deploy by pulling that image, e.g. with `docker-compose.yml` using
`image: ghcr.io/mammuth/hoerbert:latest` instead of `build: .`.

## License

MIT
