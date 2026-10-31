#!/bin/sh
# Forced command of the GitHub Actions deploy key (see authorized_keys).
# Reads a short-lived GHCR token on stdin, pulls the new image, restarts.
set -eu
cd /root/hoerbert
docker login ghcr.io -u deploy --password-stdin >/dev/null
trap 'docker logout ghcr.io >/dev/null' EXIT
docker compose pull -q
docker compose up -d --remove-orphans
docker image prune -f >/dev/null
docker compose ps
