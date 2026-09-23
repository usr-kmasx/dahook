# dahook shell hook — source no shell que roda dentro do kitty do app
# uso: source /tmp/dahook-rc.sh   (o orquestrador Rust ja gera esse arquivo)
# depois: dahook [url]

dahook() {
  local url="${1:-https://duckduckgo.com}"
  echo "$url" > /tmp/dahook-trigger
  echo "[dahook] carregando browser dentro do app: $url"
}

dahook-back() {
  echo "back" > /tmp/dahook-back
  echo "[dahook] voltando ao terminal (mesmo que clicar o X)."
}
