#!/usr/bin/env bash
# Instala o toolchain Go em user-space (sem root) para compilar o kitty da base.
# Uso: tools/install-go.sh   (idempotente)
# Padrão: ~/.local/go  (override: GO_PREFIX=/outro/lugar tools/install-go.sh)
set -eu
GO_VERSION="${GO_VERSION:-1.27.1}"
PREFIX="${GO_PREFIX:-$HOME/.local}"
DEST="$PREFIX/go"

if [ -x "$DEST/bin/go" ]; then
  echo "Go já instalado: $($DEST/bin/go version) em $DEST"
  exit 0
fi
echo "== baixando Go $GO_VERSION (user-space, sem root) =="
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
curl -fSL --retry 3 -o "$tmp/go.tgz" "https://go.dev/dl/go${GO_VERSION}.linux-amd64.tar.gz"
mkdir -p "$PREFIX"
tar -xzf "$tmp/go.tgz" -C "$PREFIX"
"$DEST/bin/go" version
echo "== ok: exporte PATH=\"$DEST/bin:\$PATH\" (e GOROOT=$DEST se preciso) =="
