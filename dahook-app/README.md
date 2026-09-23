# dahook — um app só (terminal kitty que vira browser)

Rust orquestra binários. Contenção só visual + perfil isolado (não é sandbox bwrap).

## Veredito mídia da base que já temos (`../browser` = qutebrowser/QtWebEngine)

- Fotos: OK (PNG/JPEG/GIF/WebP/SVG).
- Áudio/vídeo: PARCIAL. QtWebEngine = Chromium, mas builds padrão do Qt
  vêm SEM codecs proprietários (H.264/MP4/AAC) e SEM Widevine DRM.
  Na prática: muito YouTube (VP9/WebM) funciona, mas MP4/H.264, Spotify,
  Netflix, Meet/Zoom com H.264 falham ou pedem mpv externo
  (o FAQ do qutebrowser manda tocar via mpv — `browser/doc/faq.asciidoc`).
- Por isso o modo browser do MVP prefere `/usr/bin/chromium` real
  (compat total de mídia) com `--user-data-dir=/tmp/dahook-webprofile`.
  O `../browser` fica como fallback leve.

## Como funciona (mesma janela, não outro app)

1. `cargo run -- run` abre kitty real (`--class dahook`, remote control).
   Shell real (bash). Todos os atalhos kitty nativos.
2. No prompt: `source /tmp/dahook-rc.sh` uma vez, depois `dahook` (abre home)
   ou `dahook https://example.com`. O hook escreve `/tmp/dahook-trigger`.
3. Rust esconde o kitty (`kitty @ set-window-visibility no`, fallback xdotool
   X11) e abre/mostra o browser com perfil isolado + overlay X no canto superior direito.
   Todos os atalhos chromium nativos; kitty suspenso.
4. Clicar X (ou `dahook-back`, ou `dahook back`) MINIMIZA o browser (processo e
   abas preservados) e restaura o kitty. Volta ao terminal. Proximo `dahook`
   reexibe a instancia (e abre nova aba se URL dada). Fechar a janela do
   browser descarta a instancia; o proximo `dahook` abre uma nova.

Isolamento = visual + perfil (`/tmp/dahook-webprofile`). FS/rede do host
continuam visíveis. Sandbox real (bwrap/namespaces) é fase 2.

## Rodar

```bash
cd dahook-app
cargo run -- run
# em outro teste:
cargo run -- browser https://example.com
cargo run -- status
cargo run -- back
```

X11: `xdotool` usado para esconder/mostrar. No Wayland, o fallback xdotool
não funciona — usa `kitty @` remote control (funciona) e o compositor
mantém a mesma classe `dahook`.

## Layout

- `src/main.rs` — orquestrador
- `shell/dahook.sh` — hook `dahook()` / `dahook-back()`
- `config/kitty-dahook.conf` — modo terminal (atalhos kitty nativos)
- `config/chromium-shortcuts.md` — modo web (só atalhos chromium)
