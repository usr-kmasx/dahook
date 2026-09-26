# dahook — terminal GTK4 em Rust, com browser privativo embutido

Terminal com abas, config e atalhos compatíveis com o `kitty.conf`,
mais tabs de browser (WebKitGTK) com sessão efêmera: zero histórico
de URL em disco, na tela ou no shell.

- **Terminal**: backend VTE, `~/.config/dahook/dahook.conf` na sintaxe
  do kitty (cores, fonte, cursor, scrollback, padding, `map`, `include`,
  `env`, `shell`), atalhos iguais aos do kitty.
- **Browser embutido**: `dahook <url>` no shell abre a página em nova
  tab e fecha a tab digitada; sem argumento abre DuckDuckGo. URL
  editável na tab, `<` `>` + `Alt+←/→` + botões laterais do mouse,
  câmera/mic com pergunta por origem (testados e funcionando).
- **Downloads**: diretos byte-idênticos + mídia/stream via yt-dlp para
  `~/Downloads`; **■** à esquerda do × só durante download (hover
  mostra o progresso real, clique interrompe, fechar a tab cancela).
- **Privacidade**: sessão WebKit efêmera, sem cookies/cache/histórico;
  o app nunca escreve no seu shell history (digite com espaço na
  frente: ` dahook exemplo.com`). Sem DRM (limite do WebKit Linux).

Detalhes de uso: [`dahook-app/README.md`](dahook-app/README.md).

## Instalar

```sh
cd dahook-app
./tools/setup-deps.sh   # Arch/Debian/Fedora/openSUSE (best-effort)
cargo build
```

Integração com o sistema (ícone, menu de apps, comando `dahook`):

```sh
APP=$PWD
install -Dm644 Logo/logo.png \
  ~/.local/share/icons/hicolor/512x512/apps/dahook.png
printf '%s\n' \
  '[Desktop Entry]' \
  'Type=Application' \
  'Name=dahook' \
  'Comment=Terminal GTK4 em Rust (atalhos e config estilo kitty)' \
  'Icon=dahook' \
  "Exec=$APP/target/debug/dahook" \
  'Terminal=false' \
  'Categories=System;TerminalEmulator;' \
  'StartupWMClass=dahook' \
  > ~/.local/share/applications/dev.dahook.terminal.desktop
```

O comando `dahook <url>` é um script em `~/.local/bin/dahook` que fala
com a instância principal via Gio actions (`open-url`, single-instance)
e sobe o app destacado se estiver fechado. Para (re)criá-lo:

```sh
cat > ~/.local/bin/dahook <<'EOF'
#!/bin/sh
# dahook <url> — abre a URL em nova tab browser dentro do app dahook.
APP_ID=dev.dahook.terminal
APP_BIN=/home/usr/Projetos/dahook/dahook-app/target/debug/dahook
[ -x "$APP_BIN" ] || APP_BIN=/home/usr/Projetos/dahook/dahook-app/target/release/dahook
if [ ! -x "$APP_BIN" ]; then
  echo "dahook: binário não encontrado; rode \`cargo build\` em ~/Projetos/dahook/dahook-app" >&2
  exit 1
fi
url="${1:-https://duckduckgo.com}"
case "$url" in
http://* | https://* | file://* | about:* | data:* | view-source:*) ;;
*) url="https://$url" ;;
esac
has_owner() {
  dbus-send --session --print-reply --dest=org.freedesktop.DBus \
    /org/freedesktop/DBus org.freedesktop.DBus.NameHasOwner "string:$APP_ID" 2>/dev/null |
    grep -q "boolean true"
}
if ! has_owner; then
  setsid "$APP_BIN" >/dev/null 2>&1 < /dev/null &
  for _ in $(seq 1 100); do
    has_owner && break
    sleep 0.1
  done
fi
# Protocolo: "pid|url" — o app fecha a tab terminal de onde o comando veio.
exec gapplication action "$APP_ID" open-url "'$$|$url'"
EOF
chmod +x ~/.local/bin/dahook
```

## Estrutura

- `dahook-app/src/main.rs` — app inteiro (terminal, browser, downloads)
- `dahook-app/src/config.rs` — parser de `dahook.conf` (sintaxe kitty)
- `dahook-app/tools/setup-deps.sh` — deps de sistema multi-distro
- `dahook-app/Logo/logo.png` — ícone do app
