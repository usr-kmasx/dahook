#!/usr/bin/env bash
# Compila o kitty da base (kitty/) a partir do source.
# Uso: tools/build-kitty.sh   (rode na raiz do projeto)
# Saída: kitty/launcher/kitty  (binário real, backend do dahook)
# Requer: gcc, pkg-config, Go user-space (tools/install-go.sh), X11/DBus do sistema.
set -eu
cd "$(dirname "$0")/../kitty"
export PATH="$HOME/.local/go/bin:$PATH"
export GOROOT="${GOROOT:-$HOME/.local/go}"
go version
./dev.sh build "$@"
echo "== ok: rode ./kitty/launcher/kitty --version =="
