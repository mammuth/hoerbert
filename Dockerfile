# ---------- frontend ----------
FROM node:22-bookworm-slim AS web
WORKDIR /web
COPY web/package.json web/package-lock.json ./
RUN npm ci --no-audit --no-fund
COPY web/ ./
RUN npm run build

# ---------- rust ----------
FROM rust:1-bookworm AS build
RUN apt-get update && apt-get install -y --no-install-recommends libopus-dev pkg-config cmake libssl-dev libclang-dev clang && rm -rf /var/lib/apt/lists/*
WORKDIR /src
COPY Cargo.toml Cargo.lock build.rs ./
# Dependencies only (stub main): this layer stays cached until Cargo.* change.
RUN mkdir src && echo 'fn main() {}' > src/main.rs && cargo build --release --locked && rm -rf src
COPY src ./src
COPY examples ./examples
COPY --from=web /web/dist ./web/dist
# touch: the stub binary is newer than the checked-out sources
RUN touch src/main.rs && cargo build --release --locked && strip target/release/hoerbert \
    && mkdir /out && cp target/release/hoerbert target/release/*.so /out/

# ---------- runtime ----------
FROM debian:bookworm-slim
ARG DENO_VERSION=v2.9.8
RUN apt-get update && apt-get install -y --no-install-recommends \
      ffmpeg libopus0 ca-certificates curl unzip bzip2 python3 python3-venv tini \
    && rm -rf /var/lib/apt/lists/*
# Deno: JS runtime yt-dlp needs for YouTube's signature challenges
RUN ARCH="$(uname -m)" && case "$ARCH" in x86_64) DA=x86_64 ;; aarch64) DA=aarch64 ;; *) echo "unsupported arch $ARCH" && exit 1 ;; esac \
    && curl -fsSL "https://github.com/denoland/deno/releases/download/${DENO_VERSION}/deno-${DA}-unknown-linux-gnu.zip" -o /tmp/deno.zip \
    && unzip -q /tmp/deno.zip -d /usr/local/bin && rm /tmp/deno.zip && deno --version
# yt-dlp in its own venv so it can self-update at container start
RUN python3 -m venv /opt/ytdlp && /opt/ytdlp/bin/pip install --no-cache-dir -U pip "yt-dlp[default]" bgutil-ytdlp-pot-provider \
    && ln -s /opt/ytdlp/bin/yt-dlp /usr/local/bin/yt-dlp && yt-dlp --version
# sherpa-onnx/onnxruntime .so files sit next to the binary (rpath $ORIGIN)
COPY --from=build /out/ /usr/local/bin/
COPY docker-entrypoint.sh /usr/local/bin/docker-entrypoint.sh
RUN chmod +x /usr/local/bin/docker-entrypoint.sh && mkdir -p /data && useradd -r -u 1000 -d /data hoerbert && chown hoerbert /data
ENV DATA_DIR=/data WEB_BIND=0.0.0.0:8080 YTDLP_AUTO_UPDATE=true
VOLUME ["/data"]
EXPOSE 8080
USER hoerbert
ENTRYPOINT ["/usr/bin/tini", "--", "/usr/local/bin/docker-entrypoint.sh"]
CMD ["hoerbert"]
