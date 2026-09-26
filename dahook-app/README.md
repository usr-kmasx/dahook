# dahook — terminal GTK4 em Rust

Terminal com abas, config e atalhos compatíveis com o `kitty.conf`.

## Rodar

```sh
cargo run
```

Janela 960x600 com o shell do `$SHELL` (ou `shell` do conf).

## Config: `~/.config/dahook/dahook.conf`

Mesma sintaxe do kitty (`opção valor`, `#` comentário, `include`).
Criado com exemplo comentado na primeira execução.

Aplicadas: `font_family`, `font_size`, `foreground`, `background`,
`background_opacity`, `color0`-`color255`, `cursor`, `cursor_shape`,
`cursor_blink_interval`, `selection_foreground/background`,
`scrollback_lines`, `window_padding_width`, `shell`, `env`,
`kitty_mod`, `map`, `include`. O resto é aceito e ignorado com aviso.

## Atalhos: os do kitty (`kitty_mod` = `ctrl+shift` por padrão)

Tabs: `T/W`, `→/←`, `Ctrl+Tab`, `1..0`, `.`/`,`, `Alt+T` título.
Clipboard: `C/V/S`. Scroll: setas, `PageUp/Down`, `Home/End`.
Fonte: `=/+/-BackSpace`. Janelas: `Enter/N/W`, `F10/F11`, `Delete` limpa,
`F5` recarrega o conf. `map` custom soma aos defaults (mesmo
keystroke troca). Hints/pagers/splits não têm equivalente no backend.

## Atalhos Chrome (só em tab browser)

No terminal essas teclas passam para o shell (`Ctrl+T` transpõe,
`Ctrl+W` apaga palavra, `Ctrl+R` busca...). Em tab browser:
`Ctrl+T` nova tab, `Ctrl+W` fecha, `Ctrl+N` nova janela,
`Ctrl+L` foca endereço, `Ctrl+R` recarrega. `Ctrl+Tab` vale igual
nos dois mundos.

## Dependências de sistema (não vêm com o cargo)

```sh
./tools/setup-deps.sh   # detecta Arch/Debian/Fedora/openSUSE e instala
```

Instala GTK4, libadwaita, VTE, WebKitGTK e plugins GStreamer
(inclui `gst-plugins-good`, sem o qual vídeo aborta o processo web).
Detalhes e nomes por base: ver o script. Sem Widevine: DRM nunca
(limite do WebKit Linux, não do app).

## Browser dentro do app

Digitar `dahook <url>` em qualquer shell abre a página em **nova tab**
do dahook (WebKit, sem toolbar; volta ao terminal trocando/fechando tab).
Sem argumento abre a home (DuckDuckGo); sem esquema assume `https://`.
Tabs browser têm `<` `>` à esquerda do × (só aparecem quando há para
onde ir), `Alt+←/→` e **botões laterais do mouse** (8 volta, 9 avança)
também navegam (só em tab browser).
A tab browser mostra a **URL editável direto na tab** (Enter carrega);
com 1 tab só, `<` `>` + URL aparecem também na **barra abaixo**
(com 2+, só na tab). Ambos acompanham o histórico real e as navegações,
sem roubar o que você digita.

O comando `dahook` (`~/.local/bin/dahook`) fala com a instância principal
via Gio actions (`open-url`); se o app estiver fechado, o script o sobe
destacado e espera o bus.
A tab terminal de onde o comando foi digitado fecha sozinha (terminal
vira browser); digitado fora do app, só abre. Tabs têm botão ×.

## Privacidade (sem histórico de URL em lugar nenhum)

- **Disco**: sessão WebKit efêmera — zero histórico/cookies/cache salvos.
- **Tela**: a tab digitada fecha ao abrir o browser.
- **Shell**: o app nunca escreve no seu history. Para a URL nem chegar
  lá, digite com **espaço na frente** (` dahook exemplo.com`) — o fish
  ignora comandos com espaço inicial (provado em teste).

## Mídia na tab browser

Vídeo/áudio/fotos funcionam via GStreamer do sistema (WebAudio,
MediaStream e inline ligados). Requer os plugins instalados —
`gst-plugins-good` incluso (traz `qtdemux`, `autovideosink` etc.):

```sh
sudo pacman -S gst-plugins-good
```

Botão direito no vídeo/página de stream → **"Baixar mídia (yt-dlp)"**
baixa pra `~/Downloads` (requer `yt-dlp`, no `setup-deps.sh`).
Arquivo direto (foto/mp3/mp4/zip) baixa byte-idêntico pelo fluxo
normal; stream (YouTube) só via yt-dlp — sem ele, nem o Chrome baixa.
Enquanto há download ativo, um **■** aparece à esquerda do × (na tab
e na barra de URL); hover no ■ mostra popover abaixo com o progresso
real (%). Clicar no ■ interrompe tudo da tab. Fechar a tab também
cancela os downloads dela. Fim de download avisa via notify-send.

Sem eles, vídeo aborta o processo web. Fotos e páginas sempre funcionam.
Pedidos de permissão (câmera/mic/localização/notificações) abrem
pergunta na tab e a escolha é lembrada por origem na sessão
(câmera/mic testados e funcionando).
DRM (Widevine/Spotify/Netflix) não existe no WebKit Linux.

## Notas

- Backend é VTE (widget do GNOME), não kitty.
- Ícone: `Logo/logo.png` instalado em
  `~/.local/share/icons/hicolor/512x512/apps/dahook.png`
  (+ `dev.dahook.terminal.desktop` no `applications`).
- `send_text`/`launch`/`load_config_file`/`set_background_opacity`
  implementados; resto além do MVP avisa e ignora.
- `DAHOOK_DEBUG_KEYS=1` loga teclas/ações; `DAHOOK_APP_ID` isola instâncias.
- Testes: `cargo test` (parser, maps, includes, URIs).
