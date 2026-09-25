//! dahook — GUI em Rust (GTK4 + VTE). Terminal com abas e atalhos do kitty.
//!
//! Config própria em `~/.config/dahook/dahook.conf`, mesma sintaxe e
//! opções do kitty.conf (ver `config.rs`). Atalhos: defaults do kitty +
//! linhas `map` do conf (mesmo keystroke troca o default).
//!
//! Adaptações honestas (app é tabbed, sem splits/layouts do kitty):
//! - `new_window` abre nova **tab** (kitty abriria split);
//! - `next/prev/first..tenth_window` navegam **tabs**;
//! - `close_window` fecha a tab atual;
//! - hints/kittens/pagers/layouts/modo resize: avisados como não suportados.

mod config;

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use config::{CursorShape, DahookConfig, Mods};
use gtk4::gio::prelude::ActionMapExt as _;
use gtk4::glib::variant::StaticVariantType as _;
use gtk4::prelude::*;
use vte4::prelude::*;
use webkit6::prelude::*;

/// Defaults do kitty (`kitty/kitty/options/definition.py`, só Linux) para
/// as ações implementadas. Formato: (keystroke kitty, spec ação).
const DEFAULT_MAPS: &[(&str, &str)] = &[
    ("kitty_mod+c", "copy_to_clipboard"),
    ("kitty_mod+v", "paste_from_clipboard"),
    ("kitty_mod+s", "paste_from_selection"),
    ("shift+insert", "paste_from_selection"),
    ("kitty_mod+up", "scroll_line_up"),
    ("kitty_mod+k", "scroll_line_up"),
    ("kitty_mod+down", "scroll_line_down"),
    ("kitty_mod+j", "scroll_line_down"),
    ("kitty_mod+page_up", "scroll_page_up"),
    ("kitty_mod+page_down", "scroll_page_down"),
    ("kitty_mod+home", "scroll_home"),
    ("kitty_mod+end", "scroll_end"),
    ("kitty_mod+enter", "new_window"),
    ("kitty_mod+n", "new_os_window"),
    ("kitty_mod+w", "close_window"),
    ("kitty_mod+]", "next_window"),
    ("kitty_mod+[", "previous_window"),
    ("kitty_mod+f", "move_window_forward"),
    ("kitty_mod+b", "move_window_backward"),
    ("kitty_mod+1", "goto_tab 1"),
    ("kitty_mod+2", "goto_tab 2"),
    ("kitty_mod+3", "goto_tab 3"),
    ("kitty_mod+4", "goto_tab 4"),
    ("kitty_mod+5", "goto_tab 5"),
    ("kitty_mod+6", "goto_tab 6"),
    ("kitty_mod+7", "goto_tab 7"),
    ("kitty_mod+8", "goto_tab 8"),
    ("kitty_mod+9", "goto_tab 9"),
    ("kitty_mod+0", "goto_tab 10"),
    ("kitty_mod+right", "next_tab"),
    ("ctrl+tab", "next_tab"),
    ("kitty_mod+left", "previous_tab"),
    ("ctrl+shift+tab", "previous_tab"),
    ("kitty_mod+t", "new_tab"),
    ("kitty_mod+q", "close_tab"),
    ("kitty_mod+.", "move_tab_forward"),
    ("kitty_mod+,", "move_tab_backward"),
    ("kitty_mod+alt+t", "set_tab_title"),
    ("kitty_mod+equal", "change_font_size all +2.0"),
    ("kitty_mod+plus", "change_font_size all +2.0"),
    ("kitty_mod+minus", "change_font_size all -2.0"),
    ("kitty_mod+backspace", "change_font_size all 0"),
    ("kitty_mod+f11", "toggle_fullscreen"),
    ("kitty_mod+f10", "toggle_maximized"),
    ("kitty_mod+f5", "load_config_file"),
    ("kitty_mod+delete", "clear_terminal reset active"),
    // Navegação do browser (só vale em tab browser; em terminal a tecla
    // passa adiante para o shell — ex: Alt+Left no vim continua dele).
    ("alt+Left", "browser_back"),
    ("alt+Right", "browser_forward"),
    // Atalhos estilo Chrome, SÓ em tab browser (no terminal passam para
    // o shell: Ctrl+T transpõe, Ctrl+W apaga palavra, Ctrl+R busca...).
    ("ctrl+t", "browser_new_tab"),
    ("ctrl+w", "browser_close_tab"),
    ("ctrl+n", "browser_new_window"),
    ("ctrl+l", "focus_address_bar"),
    ("ctrl+r", "browser_reload"),
];

const APP_ID: &str = "dev.dahook.terminal";

/// Home do browser (`dahook` sem argumento).
const HOMEPAGE: &str = "https://duckduckgo.com";

const DEFAULT_PALETTE: [(f64, f64, f64); 16] = [
    (0.0, 0.0, 0.0),
    (0.8, 0.0, 0.0),
    (0.305, 0.603, 0.023),
    (0.768, 0.627, 0.0),
    (0.203, 0.396, 0.643),
    (0.458, 0.313, 0.482),
    (0.023, 0.596, 0.603),
    (0.827, 0.843, 0.811),
    (0.333, 0.341, 0.325),
    (0.937, 0.160, 0.160),
    (0.541, 0.886, 0.203),
    (0.988, 0.913, 0.309),
    (0.447, 0.623, 0.811),
    (0.678, 0.498, 0.658),
    (0.203, 0.886, 0.886),
    (0.933, 0.933, 0.925),
];

fn xterm256(i: u8) -> (f64, f64, f64) {
    let f = |v: u8| v as f64 / 255.0;
    match i {
        16..=231 => {
            let n = i - 16;
            let lv = |k: u8| if k == 0 { 0 } else { 55 + 40 * k };
            (f(lv(n / 36)), f(lv((n / 6) % 6)), f(lv(n % 6)))
        }
        _ => {
            let g = 8 + 10 * (i - 232);
            (f(g), f(g), f(g))
        }
    }
}

fn to_gdk(c: config::Rgba) -> gtk4::gdk::RGBA {
    gtk4::gdk::RGBA::new(c.r as f32, c.g as f32, c.b as f32, c.a as f32)
}

/// Título da tab: texto fixo (terminal) ou URL editável (browser).
#[derive(Clone)]
enum TabTitle {
    Text(gtk4::Label),
    Url(gtk4::Entry),
}

impl TabTitle {
    fn set_text(&self, t: &str) {
        match self {
            TabTitle::Text(l) => l.set_text(t),
            // Não sobrescreve enquanto o usuário digita.
            TabTitle::Url(e) => {
                if std::env::var("DAHOOK_DEBUG_KEYS").is_ok() {
                    eprintln!("dahook tabtext: focus={} text={t:?}", e.has_focus());
                }
                if !e.has_focus() {
                    e.set_text(t);
                }
            }
        }
    }

    fn text(&self) -> String {
        match self {
            TabTitle::Text(l) => l.text().to_string(),
            TabTitle::Url(e) => e.text().to_string(),
        }
    }

    fn as_entry(&self) -> Option<gtk4::Entry> {
        match self {
            TabTitle::Url(e) => Some(e.clone()),
            TabTitle::Text(_) => None,
        }
    }

    fn set_tooltip(&self, t: &str) {
        match self {
            TabTitle::Text(l) => l.set_tooltip_text(Some(t)),
            TabTitle::Url(e) => e.set_tooltip_text(Some(t)),
        }
    }
}

struct Tab {
    page: gtk4::Widget,
    title: TabTitle,
    kind: TabKind,
}

enum TabKind {
    Term {
        term: vte4::Terminal,
        scroll: gtk4::ScrolledWindow,
        /// PID do filho (shell). Preenchido no callback do spawn; serve
        /// para achar a tab de onde veio um comando `dahook <url>`.
        child: Rc<std::cell::Cell<Option<i32>>>,
    },
    Web {
        view: webkit6::WebView,
        /// Barra de URL abaixo das tabs. Visível só com 1 tab total
        /// (com 2+, edita direto na tab).
        urlrow: gtk4::Box,
        urlbar: gtk4::Entry,
        dlstop: DlStop,
    },
}

struct State {
    app: gtk4::Application,
    window: gtk4::ApplicationWindow,
    notebook: gtk4::Notebook,
    tabs: Vec<Tab>,
    cfg: DahookConfig,
    conf_path: PathBuf,
    font_size: f32,
    opacity: f64,
    fullscreen: bool,
    maximized: bool,
    controller: Option<gtk4::EventControllerKey>,
    /// Ação Gio do menu "Baixar mídia (yt-dlp)" (param = URL).
    dl_action: gtk4::gio::SimpleAction,
    /// (origem, tipo) -> permitido. Decisões de permissão lembradas na sessão.
    perms: std::collections::HashMap<(String, String), bool>,
}

type Shared = Rc<RefCell<State>>;

fn default_shell() -> String {
    std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string())
}

fn current_idx(st: &State) -> Option<usize> {
    // Único ponto de tradução notebook->vec: fora de alcance = None em vez
    // de pânico (abortaria o app dentro de handler GTK). Desync notebook/vec
    // nunca deveria acontecer; se acontecer, loga em debug.
    st.notebook.current_page().and_then(|n| {
        let i = n as usize;
        if i < st.tabs.len() {
            Some(i)
        } else {
            if std::env::var("DAHOOK_DEBUG_KEYS").is_ok() {
                eprintln!(
                    "dahook: desync notebook/vec (page={i}, tabs={})",
                    st.tabs.len()
                );
            }
            None
        }
    })
}

fn idx_of_page(st: &State, page: &gtk4::Widget) -> Option<usize> {
    st.tabs.iter().position(|t| t.page == *page)
}

/// Aplica fonte/cores/cursor/scrollback/padding do conf (+overrides vivos).
fn apply_term(st: &State, term: &vte4::Terminal) {
    let cfg = &st.cfg;
    let font = format!("{} {}", cfg.font_family, st.font_size);
    if std::env::var("DAHOOK_DEBUG_KEYS").is_ok() {
        eprintln!("dahook font: {font}");
    }
    term
        .set_font(Some(&gtk4::pango::FontDescription::from_string(&font)));

    let mut bg = cfg.background;
    bg.a = st.opacity;
    let fg = to_gdk(cfg.foreground);
    let bg_gdk = to_gdk(bg);
    let max_idx = cfg.palette.keys().max().copied().unwrap_or(15).max(15);
    let owned: Vec<gtk4::gdk::RGBA> = (0..=max_idx)
        .map(|i| {
            if let Some(c) = cfg.palette.get(&i) {
                to_gdk(*c)
            } else if i < 16 {
                let (r, g, b) = DEFAULT_PALETTE[i as usize];
                gtk4::gdk::RGBA::new(r as f32, g as f32, b as f32, 1.0)
            } else {
                let (r, g, b) = xterm256(i);
                gtk4::gdk::RGBA::new(r as f32, g as f32, b as f32, 1.0)
            }
        })
        .collect();
    let palette: Vec<&gtk4::gdk::RGBA> = owned.iter().collect();
    term.set_colors(Some(&fg), Some(&bg_gdk), &palette);

    term.set_cursor_shape(match cfg.cursor_shape {
        CursorShape::Block => vte4::CursorShape::Block,
        CursorShape::Beam => vte4::CursorShape::Ibeam,
        CursorShape::Underline => vte4::CursorShape::Underline,
    });
    term.set_cursor_blink_mode(if cfg.cursor_blink {
        vte4::CursorBlinkMode::On
    } else {
        vte4::CursorBlinkMode::Off
    });
    if let Some(c) = cfg.cursor_color {
        term.set_color_cursor(Some(&to_gdk(c)));
    }
    if cfg.selection_fg.is_some() || cfg.selection_bg.is_some() {
        term
            .set_color_highlight(cfg.selection_bg.map(to_gdk).as_ref());
        term
            .set_color_highlight_foreground(cfg.selection_fg.map(to_gdk).as_ref());
    }
    term.set_scrollback_lines(cfg.scrollback_lines);
    if cfg.window_padding > 0 {
        let p = cfg.window_padding;
        term.set_margin_start(p);
        term.set_margin_end(p);
        term.set_margin_top(p);
        term.set_margin_bottom(p);
    }
}

fn shorten_title(t: &str) -> String {
    const MAX: usize = 28;
    if t.chars().count() <= MAX {
        return t.to_string();
    }
    format!("…{}", t.chars().skip(t.chars().count() - MAX + 1).collect::<String>())
}

/// file://[host]/path -> PathBuf. Host vazio/localhost/nome local =
/// path local (MVP assume máquina local); sem path -> None.
pub fn uri_to_path(uri: &str) -> Option<PathBuf> {
    let s = uri.strip_prefix("file://")?;
    let path = match s.find('/') {
        Some(i) => &s[i..],
        None => return None,
    };
    if path.is_empty() {
        return None;
    }
    Some(PathBuf::from(path))
}

/// Vaza Vec<String> como slice 'static. Necessário porque `spawn_async`
/// é assíncrono e o VTE não garante cópia do argv/envv antes de retornar;
/// sem isso o filho pode ler argv liberado e morrer na hora (foi o que
/// matava `launch sleep 60` enquanto o shell passava por sorte no timing).
/// Custo: alguns KB por tab, liberados com o processo.
fn leak_strs(v: Vec<String>) -> &'static [&'static str] {
    let leaked: Vec<&'static str> = v
        .into_iter()
        .map(|s| Box::leak(s.into_boxed_str()) as &'static str)
        .collect();
    Box::leak(leaked.into_boxed_slice())
}

fn spawn_in(
    term: &vte4::Terminal,
    cfg: &DahookConfig,
    prog: Option<Vec<String>>,
    cwd: Option<PathBuf>,
    child: Rc<std::cell::Cell<Option<i32>>>,
) {
    let argv_owned: Vec<String> = match prog {
        Some(p) if !p.is_empty() => p,
        _ => match &cfg.shell {
            Some((p, a)) => {
                let mut v = vec![p.clone()];
                v.extend(a.clone());
                v
            }
            None => vec![default_shell()],
        },
    };
    let argv: &'static [&'static str] = leak_strs(argv_owned);
    let mut full_env: Vec<String> =
        std::env::vars().map(|(k, v)| format!("{k}={v}")).collect();
    for (k, v) in &cfg.env {
        if let Some(pos) = full_env.iter().position(|e| e.starts_with(&format!("{k}="))) {
            full_env[pos] = format!("{k}={v}");
        } else {
            full_env.push(format!("{k}={v}"));
        }
    }
    let envv: &'static [&'static str] = leak_strs(full_env);
    let cwd_s: Option<&'static str> = cwd
        .as_ref()
        .and_then(|p| p.to_str())
        .map(|s| Box::leak(s.to_string().into_boxed_str()) as &'static str);
    term.spawn_async(
        vte4::PtyFlags::DEFAULT,
        cwd_s,
        argv,
        envv,
        gtk4::glib::SpawnFlags::DEFAULT,
        || {},
        -1,
        None::<&gtk4::gio::Cancellable>,
        move |res| {
            match res {
                Ok(pid) => child.set(Some(pid.0)),
                Err(e) => eprintln!("dahook: falha ao abrir shell: {e}"),
            }
        },
    );
    // argv/envv/cwd vazados como 'static (ver leak_strs).
}

/// Cria tab (shell do conf ou `prog`), aplica config, conecta sinais.
/// REGRA: nunca segurar borrow do state durante chamadas que emitem
/// sinais síncronos (append/set_current_page disparam switch-page).
fn new_tab(state: &Shared, prog: Option<Vec<String>>, cwd: Option<PathBuf>) {
    let term = vte4::Terminal::new();
    let scroll = gtk4::ScrolledWindow::new();
    scroll.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Never);
    scroll.set_child(Some(&term));
    scroll.set_vexpand(true);
    let page: gtk4::Widget = scroll.clone().upcast::<gtk4::Widget>();
    let (tabbox, title) = make_tab_label(state, &page, "Terminal", None, None);
    let child: Rc<std::cell::Cell<Option<i32>>> = Rc::new(std::cell::Cell::new(None));

    let cfg = {
        let st = state.borrow();
        apply_term(&st, &term);
        st.cfg.clone()
    };
    spawn_in(&term, &cfg, prog, cwd, child.clone());

    // Shell saiu -> fecha a tab (igual gnome-terminal); última -> fecha janela.
    let s = state.clone();
    let pg = page.clone();
    term.connect_child_exited(move |_, _| {
        close_page(&s, &pg);
    });

    // Título do shell -> label da tab + título da janela (se atual).
    let s = state.clone();
    let pg = page.clone();
    term.connect_notify_local(Some("window-title"), move |t, _| {
        let title: Option<gtk4::glib::GString> =
            gtk4::glib::object::ObjectExt::property(t, "window-title");
        let st = s.borrow();
        if let Some(i) = idx_of_page(&st, &pg) {
            let full = title.as_deref().unwrap_or("Terminal").to_string();
            st.tabs[i].title.set_text(&shorten_title(&full));
            if current_idx(&st) == Some(i) {
                st.window.set_title(Some(&full));
            }
        }
    });

    {
        let mut st = state.borrow_mut();
        let before = st.notebook.n_pages();
        let pos = st.notebook.append_page(&page, Some(&tabbox));
        st.tabs.push(Tab {
            page: page.clone(),
            title,
            kind: TabKind::Term { term, scroll, child },
        });
        st.notebook.set_show_tabs(st.tabs.len() > 1);
        if std::env::var("DAHOOK_DEBUG_KEYS").is_ok() {
            eprintln!(
                "dahook tabs: {} abertas (term nb {}->{} pos {})",
                st.tabs.len(),
                before,
                st.notebook.n_pages(),
                pos
            );
        }
    }
    update_chrome(state);
    let (notebook, n) = {
        let st = state.borrow();
        (st.notebook.clone(), st.tabs.len() as u32)
    };
    notebook.set_current_page(Some(n - 1));
    focus_current(state);
}

/// Host de uma URI para exibir/agrupar permissões ("https://ex.com:8443/p" -> "ex.com:8443").
pub fn uri_host(uri: &str) -> String {
    let after = uri.split("://").nth(1).unwrap_or(uri);
    after.split('/').next().unwrap_or(after).to_string()
}

/// Descreve o pedido de permissão da página (câmera/mic/localização/...).
fn perm_kind(req: &webkit6::PermissionRequest) -> &'static str {
    if let Some(m) = req.downcast_ref::<webkit6::UserMediaPermissionRequest>() {
        match (m.is_for_audio_device(), m.is_for_video_device()) {
            (true, true) => "câmera e microfone",
            (true, false) => "microfone",
            _ => "câmera",
        }
    } else if req
        .downcast_ref::<webkit6::GeolocationPermissionRequest>()
        .is_some()
    {
        "localização"
    } else if req
        .downcast_ref::<webkit6::NotificationPermissionRequest>()
        .is_some()
    {
        "notificações"
    } else {
        "permissão especial"
    }
}

/// Normaliza o argumento do comando `dahook`: vazio -> home,
/// sem esquema -> https://.
pub fn normalize_url(raw: &str) -> String {
    let t = raw.trim();
    if t.is_empty() {
        return HOMEPAGE.to_string();
    }
    if t.contains("://") {
        t.to_string()
    } else {
        format!("https://{t}")
    }
}

/// Domínios onde "baixar vídeo" faz sentido (streams: só via yt-dlp).
const VIDEO_SITES: &[&str] = &[
    "youtube.com",
    "youtu.be",
    "vimeo.com",
    "twitch.tv",
    "dailymotion.com",
    "dai.ly",
    "tiktok.com",
    "instagram.com",
    "facebook.com",
    "x.com",
    "twitter.com",
    "reddit.com",
    "odysee.com",
    "rumble.com",
    "bilibili.com",
];

fn host_is_video_site(uri: &str) -> bool {
    let host = uri_host(uri).to_lowercase();
    let host = host.split(':').next().unwrap_or(&host);
    VIDEO_SITES
        .iter()
        .any(|s| host == *s || host.ends_with(&format!(".{s}")))
}

fn ytdlp_available() -> bool {
    std::process::Command::new("yt-dlp")
        .arg("--version")
        .output()
        .is_ok()
}

/// Navegador com cookies para o yt-dlp (YouTube exige login anti-bot;
/// sem cookies dá "Sign in to confirm you're not a bot").
fn cookie_browser() -> Option<&'static str> {
    let home = std::env::var("HOME").ok()?;
    let base = std::path::PathBuf::from(&home);
    let has = |rel: &str| base.join(rel).exists();
    // Firefox: qualquer perfil com cookies.sqlite.
    if let Ok(rd) = std::fs::read_dir(base.join(".mozilla/firefox")) {
        for e in rd.filter_map(|e| e.ok()) {
            if e.path().join("cookies.sqlite").exists() {
                return Some("firefox");
            }
        }
    }
    for (key, dirs) in [
        ("chromium", [".config/chromium", ".config/chromium/Default"]),
        ("chrome", [".config/google-chrome", ".config/google-chrome/Default"]),
        (
            "brave",
            [
                ".config/BraveSoftware/Brave-Browser",
                ".config/BraveSoftware/Brave-Browser/Default",
            ],
        ),
        (
            "edge",
            [".config/microsoft-edge", ".config/microsoft-edge/Default"],
        ),
        ("vivaldi", [".config/vivaldi", ".config/vivaldi/Default"]),
        ("opera", [".config/opera", ".config/opera/Default"]),
    ] {
        for d in dirs {
            if has(&format!("{d}/Network/Cookies")) || has(&format!("{d}/Cookies")) {
                return Some(key);
            }
        }
    }
    None
}

/// Extrai "12.3" de "[download]  12.3% of ...". Puro, testável.
/// É o % real que o hover do ■ exibe durante o yt-dlp.
fn parse_ytdlp_pct(line: &str) -> Option<f64> {
    let end = line.find('%')?;
    let b = line.as_bytes();
    let mut start = end;
    while start > 0 && (b[start - 1].is_ascii_digit() || b[start - 1] == b'.') {
        start -= 1;
    }
    line[start..end].parse::<f64>().ok().map(|n| n / 100.0)
}

enum YtMsg {
    Progress(f64),
    Done,
}

/// yt-dlp destacado: dispara numa thread, espera e notifica no fim.
/// Só notify-send no fim (ok/falha com motivo curto). Filho direto +
/// wait na thread = sem zumbi.
/// Com `ui`, mostra o ■ da tab enquanto roda (cancela via killpg no
/// grupo: mata yt-dlp + ffmpeg do merge, sem órfão) e o hover do ■
/// exibe o % real parseado da saída `--progress`.
fn start_ytdlp_download(url: &str, ui: Option<(&DlStop, &gtk4::Widget)>) {
    use std::io::{BufRead, BufReader};
    use std::os::unix::process::CommandExt as _;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;

    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    let out = format!("{home}/Downloads/%(title)s [%(id)s].%(ext)s");
    let url = url.to_string();
    let mut cmd_args: Vec<String> = vec![
        "--no-playlist".into(),
        "--no-overwrites".into(),
        "--newline".into(),
        "--progress".into(),
        "-o".into(),
        out,
    ];
    if let Some(browser) = cookie_browser() {
        // Sem isso o YouTube barra com "Sign in to confirm you're not a bot".
        cmd_args.push("--cookies-from-browser".into());
        cmd_args.push(browser.into());
        if std::env::var("DAHOOK_DEBUG_KEYS").is_ok() {
            eprintln!("dahook yt-dlp: cookies de {browser}");
        }
    }
    cmd_args.push(url);
    let mut child = match std::process::Command::new("yt-dlp")
        .args(&cmd_args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        // Grupo próprio: o ■ mata yt-dlp + ffmpeg + netos (killpg).
        // Sem isso o ffmpeg do merge ficava órfão convertendo.
        .process_group(0)
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            eprintln!("dahook: yt-dlp não executou: {e}");
            return;
        }
    };
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    // pgid == pid do líder (process_group(0) no spawn).
    let pgid = child.id() as i32;
    let cancelled = Arc::new(AtomicBool::new(false));
    // Aviso de fim: a thread sempre notifica; a main atualiza o hover
    // do ■ (% real) e o apaga no fim.
    let (tx, rx) = std::sync::mpsc::channel::<YtMsg>();
    if let Some((d, page)) = ui {
        {
            let flag = cancelled.clone();
            d.kills.borrow_mut().push(Box::new(move || {
                flag.store(true, Ordering::SeqCst);
                // Negativo = grupo todo. ESRCH (já morreu) é ok.
                unsafe {
                    libc::kill(-pgid, libc::SIGKILL);
                }
            }));
        }
        d.show();
        // Poll na main thread (drena tudo por tick); morre sozinho se
        // a tab fechar. % real -> hover do ■.
        let d = d.clone();
        let weak = page.downgrade();
        gtk4::glib::timeout_add_local(Duration::from_millis(200), move || {
            use gtk4::glib::ControlFlow;
            if weak.upgrade().is_none() {
                return ControlFlow::Break;
            }
            loop {
                match rx.try_recv() {
                    Ok(YtMsg::Progress(f)) => d.progress(f),
                    Ok(YtMsg::Done) => {
                        d.done();
                        return ControlFlow::Break;
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => break,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        d.done();
                        return ControlFlow::Break;
                    }
                }
            }
            ControlFlow::Continue
        });
    }
    std::thread::spawn(move || {
        // % real do stdout --progress (uma linha por update).
        if let Some(out) = stdout {
            for line in BufReader::new(out).lines().map_while(Result::ok) {
                if let Some(p) = parse_ytdlp_pct(&line) {
                    if tx.send(YtMsg::Progress(p)).is_err() {
                        break;
                    }
                }
            }
        }
        // Cauda do stderr para diagnóstico real no notify de falha.
        let mut err_tail: Vec<String> = Vec::new();
        if let Some(err) = stderr {
            for line in BufReader::new(err).lines().map_while(Result::ok) {
                let line = line.trim().to_string();
                if line.is_empty() {
                    continue;
                }
                // Pula WARNING genérico, guarda o erro de verdade.
                if !line.starts_with("WARNING:") {
                    err_tail.push(line);
                }
                while err_tail.len() > 3 {
                    err_tail.remove(0);
                }
            }
        }
        let status = child.wait();
        let detail = err_tail.join(" | ");
        if !detail.is_empty() {
            eprintln!("dahook: yt-dlp: {detail}");
        }
        let note = match &status {
            _ if cancelled.load(Ordering::SeqCst) => "Download cancelado".to_string(),
            Ok(s) if s.success() => "Download concluído (ver ~/Downloads)".to_string(),
            _ if !detail.is_empty() => {
                let short: String = detail.chars().take(100).collect();
                format!("Download falhou: {short}")
            }
            _ => "Download falhou (ver log)".to_string(),
        };
        notify(&note);
        let _ = tx.send(YtMsg::Done);
    });
}

/// Nome único no dir: `img.png` -> `img.2.png` (sufixo ANTES da
/// extensão, igual Chrome/Firefox — depois dela quebra a associação).
fn unique_name(dir: &std::path::Path, name: &str) -> PathBuf {
    let mut dest = dir.join(name);
    if !dest.exists() {
        return dest;
    }
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s, format!(".{e}")),
        _ => (name, String::new()),
    };
    let mut i = 2u32;
    loop {
        dest.set_file_name(format!("{stem}.{i}{ext}"));
        if !dest.exists() {
            return dest;
        }
        i += 1;
    }
}

/// Aviso desktop (início/fim de download). Melhor esforço: sem
/// servidor de notificação, só o log no stderr.
fn notify(msg: &str) {
    let _ = std::process::Command::new("notify-send")
        .args(["dahook", msg])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}

fn ytdlp_missing_dialog(parent: &gtk4::ApplicationWindow) {
    let dlg = gtk4::Window::builder()
        .title("yt-dlp ausente")
        .modal(true)
        .transient_for(parent)
        .default_width(380)
        .build();
    let label = gtk4::Label::new(Some(
        "Para baixar vídeos/streams instale o yt-dlp:\n./tools/setup-deps.sh",
    ));
    label.set_margin_start(16);
    label.set_margin_end(16);
    label.set_margin_top(16);
    label.set_margin_bottom(16);
    dlg.set_child(Some(&label));
    dlg.present();
}

/// Abre URL em nova tab com WebView (sem toolbar). Volta ao terminal
/// trocando de tab / fechando a tab.
///
/// Mídia sem limites (dentro do possível no Linux): WebAudio, MediaStream,
/// inline playback e fullscreen de vídeo. Pedidos de permissão da página
/// (câmera/mic/localização/notificações) abrem pergunta na tab, como um
/// navegador comum, e a escolha é lembrada por origem na sessão.
/// Única exceção: DRM (Widevine) — só Chrome/Edge/Firefox têm licença.
fn new_browser_tab(state: &Shared, url: &str) {
    let settings = webkit6::Settings::new();
    settings.set_enable_webaudio(true);
    settings.set_enable_media_stream(true);
    settings.set_media_playback_allows_inline(true);
    // Sessão efêmera (modo privado): nada de histórico/cookies/cache em
    // disco — some com a tab. Requisito: URL nunca salva em nenhum lugar.
    let session = webkit6::NetworkSession::new_ephemeral();
    // Downloads ("salvar link como", anexos): vão para ~/Downloads.
    // Sem isso o item do menu morre em silêncio. O ■ aparece à
    // esquerda do × (tab e barra) só enquanto há download ativo;
    // hover no ■ mostra popover abaixo com o progresso real.
    let tstop = dl_stop_btn();
    let (tpop, tlabel) = dl_hover(&tstop);
    let bstop = dl_stop_btn();
    let (bpop, blabel) = dl_hover(&bstop);
    let dlstop = DlStop {
        btns: vec![tstop.clone(), bstop.clone()],
        pops: vec![tpop, bpop],
        labels: vec![tlabel, blabel],
        active: Rc::new(std::cell::Cell::new(0)),
        kills: Rc::new(std::cell::RefCell::new(Vec::new())),
    };
    // ■ interrompe TUDO da tab.
    for stop in [tstop.clone(), bstop.clone()] {
        let d = dlstop.clone();
        stop.connect_clicked(move |_| d.stop_all());
    }
    session.connect_download_started({
        let dlstop = dlstop.clone();
        move |_, dl| {
        // Nome: Content-Disposition, senão último segmento da URL.
        let from_url = dl.request().and_then(|r| r.uri()).and_then(|u| {
            let u = u.to_string();
            let seg = u.rsplit('/').next().unwrap_or("");
            let seg = seg.split(['?', '#']).next().unwrap_or("");
            if seg.is_empty() {
                None
            } else {
                Some(seg.to_string())
            }
        });
        let name = dl
            .response()
            .and_then(|r| r.suggested_filename())
            .map(|s| s.to_string())
            .or(from_url)
            .filter(|s| !s.is_empty() && !s.contains('/') && !s.contains('\0'))
            .unwrap_or_else(|| "download".to_string());
        let mut dest = std::path::PathBuf::from(
            std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string()),
        );
        dest.push("Downloads");
        let _ = std::fs::create_dir_all(&dest);
        dest = unique_name(&dest, &name);
        dl.set_destination(&dest.to_string_lossy());
        dlstop.show();
        {
            let dl2 = dl.clone();
            dlstop
                .kills
                .borrow_mut()
                .push(Box::new(move || dl2.cancel()));
        }
        {
            let d = dlstop.clone();
            let dlf = dest.clone();
            dl.connect_finished(move |_| {
                eprintln!("dahook: download salvo em {}", dlf.display());
                notify(&format!(
                    "Download concluído: {}",
                    dlf.file_name().unwrap_or_default().to_string_lossy()
                ));
                d.done();
            });
        }
        {
            let d = dlstop.clone();
            let dle = dest.clone();
            dl.connect_failed(move |_, e| {
                eprintln!("dahook: download falhou ({}): {e}", dle.display());
                // Parcial não serve (sem resume): remove, como browsers.
                let _ = std::fs::remove_file(&dle);
                // Cancelado pelo ■: silêncio (o usuário mandou parar).
                if !e.message().to_string().to_lowercase().contains("cancel") {
                    notify(&format!("Download falhou: {e}"));
                }
                d.done();
            });
        }
        // Progresso real do WebKit (0..1) -> hover do ■.
        {
            let d = dlstop.clone();
            dl.connect_estimated_progress_notify(move |x| {
                d.progress(x.estimated_progress());
            });
        }
        }
    });
    let view = webkit6::WebView::builder()
        .network_session(&session)
        .settings(&settings)
        .build();
    if std::env::var("DAHOOK_DEBUG_KEYS").is_ok() {
        view.connect_load_changed(|v, ev| {
            eprintln!("dahook load: {ev:?} {}", v.uri().as_deref().unwrap_or("?"));
        });
    }
    // Barra de URL: < > + campo + ■ (só baixando) + X.
    let urlrow = gtk4::Box::new(gtk4::Orientation::Horizontal, 4);
    urlrow.set_margin_start(4);
    urlrow.set_margin_end(4);
    let uback = gtk4::Button::with_label("<");
    let ufwd = gtk4::Button::with_label(">");
    urlrow.append(&uback);
    urlrow.append(&ufwd);
    wire_nav(&view, &uback, &ufwd);
    let urlbar = gtk4::Entry::new();
    urlbar.set_text(url);
    urlbar.set_hexpand(true);
    urlrow.append(&urlbar);
    let uv = view.clone();
    urlbar.connect_activate(move |e| {
        uv.load_uri(&normalize_url(&e.text()));
    });
    // "Abrir link em nova janela/aba" do menu: abre em nova tab dahook
    // e nega o popup do WebKit (sem isso o item morre em silêncio).
    let s = state.clone();
    view.connect_create(move |_, action| {
        if let Some(uri) = action.request().and_then(|r| r.uri()) {
            new_browser_tab(&s, &uri);
        }
        None
    });
    // ■ à esquerda do X (só aparece baixando), igual na tab.
    urlrow.append(&bstop);
    // X à direita da URL (fecha a tab), igual na tab.
    let xbtn = gtk4::Button::with_label("×");
    xbtn.add_css_class("flat");
    xbtn.set_focusable(false);
    urlrow.append(&xbtn);
    // Acompanha navegações (links, voltar/avançar); não sobrescreve
    // enquanto o usuário digita.
    let ub2 = urlbar.clone();
    view.connect_load_changed(move |v, ev| {
        if matches!(ev, webkit6::LoadEvent::Committed) {
            if !ub2.has_focus() {
                if let Some(uri) = v.uri() {
                    ub2.set_text(&uri);
                }
            }
        }
    });
    // Botões laterais do mouse: 8 volta, 9 avança (= Alt+←/→).
    // Ligado na view: só existe em tab browser.
    for (btn, back) in [(8u32, true), (9u32, false)] {
        let g = gtk4::GestureClick::new();
        g.set_button(btn);
        let v = view.clone();
        g.connect_pressed(move |_, _, _, _| {
            if back {
                v.go_back();
            } else {
                v.go_forward();
            }
        });
        view.add_controller(g);
    }
    // Faixa de perguntas de permissão + página, abaixo da barra.
    let slot = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    let col = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    col.append(&urlrow);
    col.append(&slot);
    col.append(&view);
    col.set_vexpand(true);
    // Sem expand na view, a Box a aloca com altura ~0 (preto total).
    view.set_vexpand(true);
    view.set_hexpand(true);
    let page: gtk4::Widget = col.upcast::<gtk4::Widget>();
    let s = state.clone();
    let pg = page.clone();
    xbtn.connect_clicked(move |_| close_page(&s, &pg));
    view.connect_permission_request({
        let s = state.clone();
        let slot = slot.clone();
        move |v, req| {
            let host = v
                .uri()
                .map(|u| uri_host(&u))
                .filter(|h| !h.is_empty())
                .unwrap_or_else(|| "esta página".to_string());
            let kind = perm_kind(req);
            let key = (host.clone(), kind.to_string());
            if let Some(&allowed) = s.borrow().perms.get(&key) {
                if allowed {
                    req.allow();
                } else {
                    req.deny();
                }
                return true;
            }
            let bar = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
            bar.set_margin_start(8);
            bar.set_margin_end(8);
            bar.set_margin_top(6);
            bar.set_margin_bottom(6);
            let label = gtk4::Label::new(Some(&format!("{host} quer acessar: {kind}")));
            label.set_hexpand(true);
            label.set_halign(gtk4::Align::Start);
            let ok_btn = gtk4::Button::with_label("Permitir");
            let no_btn = gtk4::Button::with_label("Negar");
            bar.append(&label);
            bar.append(&ok_btn);
            bar.append(&no_btn);
            let decide = {
                let held = req.clone();
                let s2 = s.clone();
                let key2 = key.clone();
                let slot2 = slot.clone();
                let bar2 = bar.clone();
                move |allowed: bool| {
                    if allowed {
                        held.allow();
                    } else {
                        held.deny();
                    }
                    s2.borrow_mut().perms.insert(key2.clone(), allowed);
                    slot2.remove(&bar2);
                }
            };
            let d1 = decide.clone();
            ok_btn.connect_clicked(move |_| d1(true));
            let d2 = decide.clone();
            no_btn.connect_clicked(move |_| d2(false));
            let _ = decide;
            slot.append(&bar);
            true
        }
    });
    view.load_uri(url);
    // Menu de contexto: "Baixar mídia (yt-dlp)" em elemento de mídia
    // ou página de site de vídeo. Resto do menu segue padrão WebKit.
    view.connect_context_menu({
        let s = state.clone();
        move |v, menu, hit| {
            if std::env::var("DAHOOK_DEBUG_KEYS").is_ok() {
                eprintln!(
                    "dahook ctxmenu: media={} link={} itens={}",
                    hit.media_uri().is_some(),
                    hit.link_uri().is_some(),
                    menu.n_items()
                );
            }
            let target: Option<String> = hit
                .media_uri()
                .map(|u| u.to_string())
                // blob:/data: não saem da página (só o extrator resolve);
                // nesses casos baixa a página atual (ex: watch do YouTube).
                .filter(|u| !u.starts_with("blob:") && !u.starts_with("data:"))
                .or_else(|| {
                    let uri = v.uri()?.to_string();
                    host_is_video_site(&uri).then_some(uri)
                });
            if let Some(target) = target {
                let act = s.borrow().dl_action.clone();
                let item = webkit6::ContextMenuItem::from_gaction(
                    &act,
                    "Baixar mídia (yt-dlp)",
                    Some(&target.to_variant()),
                );
                menu.append(&item);
            }
            // FALSE = deixa o menu padrão aparecer (com nosso item).
            // TRUE suprimiria o menu inteiro.
            false
        }
    });
    let (tabbox, title) = make_tab_label(state, &page, url, Some(&view), Some(&tstop));

    let s = state.clone();
    let pg = page.clone();
    view.connect_notify_local(Some("title"), move |v, _| {
        set_web_label(&s, &pg, v);
    });

    {
        let mut st = state.borrow_mut();
        let before = st.notebook.n_pages();
        let pos = st.notebook.append_page(&page, Some(&tabbox));
        st.tabs.push(Tab {
            page: page.clone(),
            title,
            kind: TabKind::Web { view: view.clone(), urlrow: urlrow.clone(), urlbar: urlbar.clone(), dlstop: dlstop.clone() },
        });
        st.notebook.set_show_tabs(st.tabs.len() > 1);
        if std::env::var("DAHOOK_DEBUG_KEYS").is_ok() {
            eprintln!(
                "dahook tabs: {} abertas (browser), notebook={}->{} pos={}",
                st.tabs.len(),
                before,
                st.notebook.n_pages(),
                pos
            );
        }
    }
    update_chrome(state);
    let (notebook, n) = {
        let st = state.borrow();
        (st.notebook.clone(), st.tabs.len() as u32)
    };
    notebook.set_current_page(Some(n - 1));
    view.grab_focus();
}

/// Atualiza label da tab browser: título visível, URL completa no tooltip
/// (a URL editável mora na barra de endereço).
fn set_web_label(state: &Shared, page: &gtk4::Widget, view: &webkit6::WebView) {
    let title: Option<gtk4::glib::GString> =
        gtk4::glib::object::ObjectExt::property(view, "title");
    let url: Option<gtk4::glib::GString> = view.uri();
    let st = state.borrow();
    if let Some(i) = idx_of_page(&st, page) {
        // Tab browser mostra a URL (editável); título vai pro tooltip.
        let label_text = url.as_deref().unwrap_or("Browser").to_string();
        st.tabs[i].title.set_text(&label_text);
        let tip = match (title.as_deref(), url.as_deref()) {
            (Some(t), Some(u)) if t != u => format!("{t}\n{u}"),
            (Some(t), _) => t.to_string(),
            (None, Some(u)) => u.to_string(),
            (None, None) => String::new(),
        };
        st.tabs[i].title.set_tooltip(&tip);
        if current_idx(&st) == Some(i) {
            st.window.set_title(Some(&label_text));
        }
    }
}

fn open_url_in(state: &Shared, raw: &str) {
    // Protocolo do CLI `dahook`: "pid|url" (pid do chamador) ou só "url".
    let (sender, url) = match raw.split_once('|') {
        Some((p, u)) if p.bytes().all(|b| b.is_ascii_digit()) && !u.is_empty() => {
            (p.parse::<u32>().ok(), u)
        }
        _ => (None, raw),
    };
    let url = normalize_url(url);
    // Acha a tab terminal de onde o comando veio (filho == ancestral do pid).
    let origin: Option<gtk4::Widget> = sender.and_then(|pid| {
        let chain = ancestors(pid);
        let st = state.borrow();
        st.tabs.iter().find_map(|t| match &t.kind {
            TabKind::Term { child, .. } => match child.get() {
                Some(c) if chain.contains(&c) => Some(t.page.clone()),
                _ => None,
            },
            TabKind::Web { .. } => None,
        })
    });
    new_browser_tab(state, &url);
    // "Terminal vira browser": fecha a tab digitada. Sem match (comando
    // veio de fora do app), só abre.
    if let Some(pg) = origin {
        close_page(state, &pg);
    }
}

/// Cadeia de PIDs ancestrais via /proc (pid incluído, até 32 níveis).
fn ancestors(mut pid: u32) -> Vec<i32> {
    let mut chain = vec![pid as i32];
    for _ in 0..32 {
        let ppid: Option<u32> = std::fs::read_to_string(format!("/proc/{pid}/status"))
            .ok()
            .and_then(|s| {
                s.lines().find_map(|l| {
                    l.strip_prefix("PPid:")
                        .and_then(|v| v.trim().parse::<u32>().ok())
                })
            });
        match ppid {
            Some(0) | None => break,
            Some(p) => {
                pid = p;
                chain.push(pid as i32);
            }
        }
    }
    chain
}

/// Botão ■ de cancelar download da tab (vale tab e barra).
/// Começa ESCONDIDO: só aparece enquanto há download ativo.
/// Hover no ■ abre popover abaixo com o progresso real (%).
#[derive(Clone)]
struct DlStop {
    btns: Vec<gtk4::Button>,
    pops: Vec<gtk4::Popover>,
    labels: Vec<gtk4::Label>,
    active: Rc<std::cell::Cell<u32>>,
    /// Como matar cada download ativo (dl.cancel / killpg do yt-dlp).
    kills: Rc<std::cell::RefCell<Vec<Box<dyn Fn()>>>>,
}

impl DlStop {
    fn show(&self) {
        // Só zera o hover ao sair do zero: segundo download simultâneo
        // não pode jogar o % do primeiro de volta a 0.
        if self.active.get() == 0 {
            self.progress(0.0);
        }
        self.active.set(self.active.get() + 1);
        for b in &self.btns {
            b.set_visible(true);
        }
    }

    fn done(&self) {
        if self.active.get() > 0 {
            self.active.set(self.active.get() - 1);
        }
        if self.active.get() == 0 {
            self.kills.borrow_mut().clear();
            for p in &self.pops {
                p.popdown();
            }
            for b in &self.btns {
                b.set_visible(false);
            }
        }
    }

    /// Progresso real (0..1): atualiza os labels do hover.
    fn progress(&self, f: f64) {
        let f = f.clamp(0.0, 1.0);
        let txt = format!("Baixando… {:.1}%", f * 100.0);
        for l in &self.labels {
            l.set_text(&txt);
        }
    }

    /// Interrompe TUDO da tab: mata, zera, esconde. Fins tardios
    /// saturam em 0 (não reaparecem).
    fn stop_all(&self) {
        for k in self.kills.borrow_mut().drain(..) {
            k();
        }
        self.active.set(0);
        for p in &self.pops {
            p.popdown();
        }
        for b in &self.btns {
            b.set_visible(false);
        }
    }
}

/// Quadradinho ■ flat à esquerda do ×. Invisível até o download começar.
fn dl_stop_btn() -> gtk4::Button {
    let b = gtk4::Button::with_label("■");
    b.add_css_class("flat");
    b.set_focusable(false);
    b.set_visible(false);
    b
}

/// Hover no ■: popover abaixo com o progresso real. Devolve o label
/// para atualizar a cada tick de progresso.
fn dl_hover(stop: &gtk4::Button) -> (gtk4::Popover, gtk4::Label) {
    let pop = gtk4::Popover::new();
    pop.set_parent(stop);
    pop.set_position(gtk4::PositionType::Bottom);
    pop.set_autohide(false);
    pop.set_can_focus(false);
    let label = gtk4::Label::new(Some("Baixando… 0.0%"));
    label.set_margin_start(10);
    label.set_margin_end(10);
    label.set_margin_top(6);
    label.set_margin_bottom(6);
    pop.set_child(Some(&label));
    let motion = gtk4::EventControllerMotion::new();
    {
        let p = pop.clone();
        motion.connect_enter(move |_, _, _| p.popup());
    }
    {
        let p = pop.clone();
        motion.connect_leave(move |_| p.popdown());
    }
    stop.add_controller(motion);
    (pop, label)
}

/// Liga par `<` `>` ao histórico real da página (visíveis só quando
/// há para onde ir). Vale para tab e barra de URL.
fn wire_nav(view: &webkit6::WebView, back: &gtk4::Button, fwd: &gtk4::Button) {
    for b in [back, fwd] {
        b.add_css_class("flat");
        b.set_focusable(false);
    }
    let v = view.clone();
    back.connect_clicked(move |_| {
        v.go_back();
    });
    let v = view.clone();
    fwd.connect_clicked(move |_| {
        v.go_forward();
    });
    // Visibilidade acompanha o histórico real da página.
    // IMPORTANTE: usa WeakRef — closure conectada na própria view
    // segurando view forte cria ciclo e a tab nunca morre (mídia
    // continua tocando após fechar, como áudio fantasma).
    let sync = {
        let back = back.clone();
        let fwd = fwd.clone();
        let weak = view.downgrade();
        move || {
            let (b, f) = weak.upgrade().map_or((false, false), |v| {
                (v.can_go_back(), v.can_go_forward())
            });
            if std::env::var("DAHOOK_DEBUG_KEYS").is_ok() {
                eprintln!("dahook nav: back={b} fwd={f}");
            }
            back.set_visible(b);
            fwd.set_visible(f);
        }
    };
    sync();
    let s1 = sync.clone();
    view.connect_notify_local(Some("can-go-back"), move |_, _| s1());
    // can-go-back nem sempre notifica nesta versão do WebKit; o
    // caminho confiável (igual Epiphany) é atualizar a cada load.
    let s2 = sync.clone();
    view.connect_load_changed(move |_, ev| {
        if matches!(
            ev,
            webkit6::LoadEvent::Committed | webkit6::LoadEvent::Finished
        ) {
            s2();
        }
    });
}

/// Label da tab com botão X (fecha a tab). Padrão de browser.
/// Em tab browser, o título é a URL EDITÁVEL (Entry; Enter carrega) +
/// `<` `>` à esquerda do X, visíveis só quando houver para onde ir.
fn make_tab_label(
    state: &Shared,
    page: &gtk4::Widget,
    initial: &str,
    web: Option<&webkit6::WebView>,
    stop: Option<&gtk4::Button>,
) -> (gtk4::Box, TabTitle) {
    let hbox = gtk4::Box::new(gtk4::Orientation::Horizontal, 4);
    let title: TabTitle = match web {
        Some(view) => {
            let entry = gtk4::Entry::new();
            entry.set_text(initial);
            entry.set_width_chars(24);
            hbox.append(&entry);
            let v = view.clone();
            entry.connect_activate(move |e| {
                v.load_uri(&normalize_url(&e.text()));
            });
            TabTitle::Url(entry)
        }
        None => {
            let label = gtk4::Label::new(Some(initial));
            hbox.append(&label);
            TabTitle::Text(label)
        }
    };
    if let Some(view) = web {
        let back = gtk4::Button::with_label("<");
        let fwd = gtk4::Button::with_label(">");
        hbox.append(&back);
        hbox.append(&fwd);
        wire_nav(view, &back, &fwd);
        // ■ à esquerda do X (só aparece baixando), igual na barra.
        if let Some(stop) = stop {
            hbox.append(stop);
        }
        // Aproveita o Committed para refrescar a URL da tab (título
        // pode não mudar entre páginas). WeakRef: sem ciclo view->si.
        let sl = state.clone();
        let pgl = page.clone();
        let weak = view.clone().downgrade();
        view.connect_load_changed(move |_, ev| {
            if matches!(ev, webkit6::LoadEvent::Committed) {
                if let Some(v) = weak.upgrade() {
                    set_web_label(&sl, &pgl, &v);
                }
            }
        });
    }
    let close = gtk4::Button::with_label("×");
    close.add_css_class("flat");
    close.set_focusable(false);
    hbox.append(&close);
    let s = state.clone();
    let pg = page.clone();
    close.connect_clicked(move |_| close_page(&s, &pg));
    (hbox, title)
}

fn close_page(state: &Shared, page: &gtk4::Widget) {
    // remove_page pode emitir switch-page síncrono -> sem borrow segurado.
    let (notebook, window, idx) = {
        let st = state.borrow();
        (st.notebook.clone(), st.window.clone(), idx_of_page(&st, page))
    };
    let Some(i) = idx else {
        return;
    };
    // Mata a mídia na hora: descarrega a página antes de remover.
    // (Sem isso + sem os WeakRefs, o WebView ficava vivo e o áudio
    // continuava após fechar a tab.)
    // Downloads também morrem com a tab: sem ■ depois de fechada não
    // haveria como parar (yt-dlp ficaria órfão até o notify).
    let dlstop = match &state.borrow().tabs[i].kind {
        TabKind::Web { view, dlstop, .. } => {
            view.stop_loading();
            view.load_uri("about:blank");
            Some(dlstop.clone())
        }
        TabKind::Term { .. } => None,
    };
    if let Some(d) = dlstop {
        d.stop_all();
    }
    state.borrow_mut().tabs.remove(i);
    notebook.remove_page(Some(i as u32));
    if std::env::var("DAHOOK_DEBUG_KEYS").is_ok() {
        eprintln!("dahook tabs: fechada {i}");
    }
    focus_current(state);
    let st = state.borrow();
    let empty = st.tabs.is_empty();
    st.notebook.set_show_tabs(st.tabs.len() > 1);
    drop(st);
    update_chrome(state);
    if empty {
        window.close();
    }
}

fn close_current(state: &Shared) {
    let page = {
        let st = state.borrow();
        match current_idx(&st) {
            Some(i) => st.tabs[i].page.clone(),
            None if st.tabs.is_empty() => {
                st.window.close();
                return;
            }
            None => return, // desync (já logado): não fecha nada
        }
    };
    close_page(state, &page);
}

fn focus_current(state: &Shared) {
    let st = state.borrow();
    if let Some(i) = current_idx(&st) {
        match &st.tabs[i].kind {
            TabKind::Term { term, .. } => term.grab_focus(),
            TabKind::Web { view, .. } => view.grab_focus(),
        };
    }
}

/// Barra de URL de baixo: só com 1 tab total (com 2+, edita na tab).
fn update_chrome(state: &Shared) {
    let st = state.borrow();
    let single = st.tabs.len() == 1;
    for t in &st.tabs {
        if let TabKind::Web { urlrow, .. } = &t.kind {
            urlrow.set_visible(single);
        }
    }
}

fn current_term(state: &Shared) -> Option<vte4::Terminal> {
    let st = state.borrow();
    current_idx(&st).and_then(|i| match &st.tabs[i].kind {
        TabKind::Term { term, .. } => Some(term.clone()),
        TabKind::Web { .. } => None,
    })
}

fn current_scroll(state: &Shared) -> Option<gtk4::ScrolledWindow> {
    let st = state.borrow();
    current_idx(&st).and_then(|i| match &st.tabs[i].kind {
        TabKind::Term { scroll, .. } => Some(scroll.clone()),
        TabKind::Web { .. } => None,
    })
}

fn current_web(state: &Shared) -> Option<webkit6::WebView> {
    let st = state.borrow();
    current_idx(&st).and_then(|i| match &st.tabs[i].kind {
        TabKind::Web { view, .. } => Some(view.clone()),
        TabKind::Term { .. } => None,
    })
}

/// Dispatch com consumo condicional: ações `browser_*` só valem em
/// tab browser (no terminal, passam para o shell). Retorna se consumiu.
fn dispatch(state: &Shared, name: &str, args: &[String]) -> bool {
    // Gate único: fora de tab browser, tudo abaixo passa adiante.
    let web_only = matches!(
        name,
        "browser_back"
            | "browser_forward"
            | "browser_new_tab"
            | "browser_close_tab"
            | "browser_new_window"
            | "browser_reload"
            | "focus_address_bar"
    );
    if web_only {
        let st = state.borrow();
        let is_web = current_idx(&st).is_some_and(|i| {
            matches!(st.tabs[i].kind, TabKind::Web { .. })
        });
        drop(st);
        if !is_web {
            return false;
        }
    }
    match name {
        "browser_back" => match current_web(state) {
            Some(v) => {
                v.go_back();
                true
            }
            None => false,
        },
        "browser_forward" => match current_web(state) {
            Some(v) => {
                v.go_forward();
                true
            }
            None => false,
        },
        "browser_new_tab" => {
            new_browser_tab(state, HOMEPAGE);
            true
        }
        "browser_close_tab" => {
            close_current(state);
            true
        }
        "browser_new_window" => {
            new_os_window_action(state);
            true
        }
        "browser_reload" => match current_web(state) {
            Some(v) => {
                v.reload();
                true
            }
            None => false,
        },
        "focus_address_bar" => {
            // Barra de baixo se visível, senão a URL da tab.
            let target: Option<gtk4::Widget> = {
                let st = state.borrow();
                current_idx(&st).and_then(|i| match &st.tabs[i].kind {
                    TabKind::Web { urlrow, urlbar, .. } if urlrow.is_visible() => {
                        Some(urlbar.clone().upcast())
                    }
                    _ => st.tabs[i].title.as_entry().map(|e| e.upcast()),
                })
            };
            match target {
                Some(w) => {
                    w.grab_focus();
                    true
                }
                None => false,
            }
        }
        _ => {
            do_action(state, name, args);
            true
        }
    }
}

fn scroll_by(state: &Shared, kind: &str) {
    let Some(scroll) = current_scroll(state) else {
        return;
    };
    let adj = scroll.vadjustment();
    let (lower, upper, page) = (adj.lower(), adj.upper(), adj.page_size());
    let line = state.borrow().font_size as f64;
    let v = adj.value();
    adj.set_value(match kind {
        "line_up" => (v - line).max(lower),
        "line_down" => (v + line).min((upper - page).max(lower)),
        "page_up" => (v - page).max(lower),
        "page_down" => (v + page).min((upper - page).max(lower)),
        "home" => lower,
        "end" => (upper - page).max(lower),
        _ => v,
    });
}

fn apply_all_terms(state: &Shared) {
    let terms: Vec<vte4::Terminal> = {
        let st = state.borrow();
        st.tabs
            .iter()
            .filter_map(|t| match &t.kind {
                TabKind::Term { term, .. } => Some(term.clone()),
                TabKind::Web { .. } => None,
            })
            .collect()
    };
    for term in terms {
        let st = state.borrow();
        apply_term(&st, &term);
    }
}

fn set_tab_title_dialog(state: &Shared) {
    let (window, current) = {
        let st = state.borrow();
        // Só tab terminal tem título editável por diálogo (browser edita a URL na tab).
        let cur = current_idx(&st).and_then(|i| match &st.tabs[i].title {
            TabTitle::Text(_) => Some(st.tabs[i].title.text()),
            TabTitle::Url(_) => None,
        });
        (st.window.clone(), cur)
    };
    let Some(current) = current else {
        return;
    };
    let dlg = gtk4::Window::builder()
        .title("Tab title")
        .modal(true)
        .transient_for(&window)
        .default_width(320)
        .build();
    let entry = gtk4::Entry::new();
    entry.set_text(&current);
    entry.set_activates_default(true);
    dlg.set_child(Some(&entry));
    let s = state.clone();
    entry.connect_activate(move |e| {
        let title = e.text().to_string();
        let st = s.borrow();
        if let Some(i) = current_idx(&st) {
            st.tabs[i].title.set_text(&shorten_title(&title));
            st.window.set_title(Some(&title));
        }
        if let Some(w) = e.root().and_downcast::<gtk4::Window>() {
            w.close();
        }
    });
    dlg.present();
}

fn new_os_window_action(state: &Shared) {
    // In-process: activate() abre nova janela no mesmo app (sem processo
    // órfão, sem zumbi, instantâneo). Re-exec externo delegaria aqui anyway.
    state.borrow().app.clone().activate();
}

/// Dispatcher de ações do kitty (nomes iguais aos do kitty.conf).
fn do_action(state: &Shared, name: &str, args: &[String]) {
    if std::env::var("DAHOOK_DEBUG_KEYS").is_ok() {
        eprintln!("dahook action: {name} {args:?}");
    }
    match name {
        "new_tab" => new_tab(state, None, None),
        "close_tab" | "close_window" => close_current(state),
        "next_tab" | "next_window" => {
            let nb = state.borrow().notebook.clone();
            nb.next_page();
            focus_current(state);
        }
        "previous_tab" | "previous_window" => {
            let nb = state.borrow().notebook.clone();
            nb.prev_page();
            focus_current(state);
        }
        "goto_tab" => {
            if let Some(n) = args.first().and_then(|a| a.parse::<u32>().ok()) {
                if n >= 1 {
                    let nb = state.borrow().notebook.clone();
                    nb.set_current_page(Some(n - 1));
                    focus_current(state);
                }
            }
        }
        // Aliases de window->tab (adaptação documentada no cabeçalho).
        "first_window" => do_action(state, "goto_tab", &["1".to_string()]),
        "second_window" => do_action(state, "goto_tab", &["2".to_string()]),
        "third_window" => do_action(state, "goto_tab", &["3".to_string()]),
        "fourth_window" => do_action(state, "goto_tab", &["4".to_string()]),
        "fifth_window" => do_action(state, "goto_tab", &["5".to_string()]),
        "sixth_window" => do_action(state, "goto_tab", &["6".to_string()]),
        "seventh_window" => do_action(state, "goto_tab", &["7".to_string()]),
        "eighth_window" => do_action(state, "goto_tab", &["8".to_string()]),
        "ninth_window" => do_action(state, "goto_tab", &["9".to_string()]),
        "tenth_window" => do_action(state, "goto_tab", &["10".to_string()]),
        "move_tab_forward" | "move_window_forward" => {
            let st = state.borrow();
            if let (Some(i), n) = (current_idx(&st), st.tabs.len() as u32) {
                if n > 1 {
                    let page = st.tabs[i].page.clone();
                    let to = ((i as u32 + 1) % n.max(1)).min(n - 1);
                    st.notebook.reorder_child(&page, Some(to));
                }
            }
        }
        "move_tab_backward" | "move_window_backward" => {
            let st = state.borrow();
            if let (Some(i), n) = (current_idx(&st), st.tabs.len() as u32) {
                if n > 1 {
                    let page = st.tabs[i].page.clone();
                    let to = (i as u32 + n - 1) % n;
                    st.notebook.reorder_child(&page, Some(to));
                }
            }
        }
        "set_tab_title" => set_tab_title_dialog(state),
        // new_window (split no kitty) -> nova tab; new_os_window -> processo novo.
        "new_window" => new_tab(state, None, None),
        "new_os_window" => new_os_window_action(state),
        "copy_to_clipboard" => {
            if let Some(t) = current_term(state) {
                t.copy_clipboard_format(vte4::Format::Text);
            }
        }
        "paste_from_clipboard" => {
            if let Some(t) = current_term(state) {
                t.paste_clipboard();
            }
        }
        "paste_from_selection" => {
            if let Some(t) = current_term(state) {
                t.paste_primary();
            }
        }
        "scroll_line_up" => scroll_by(state, "line_up"),
        "scroll_line_down" => scroll_by(state, "line_down"),
        "scroll_page_up" => scroll_by(state, "page_up"),
        "scroll_page_down" => scroll_by(state, "page_down"),
        "scroll_home" => scroll_by(state, "home"),
        "scroll_end" => scroll_by(state, "end"),
        "change_font_size" => {
            // change_font_size all +2.0 | all 0
            if args.first().is_some_and(|s| s == "all") {
                let new_size = {
                    let st = state.borrow();
                    if args.get(1).is_some_and(|s| s == "0") {
                        st.cfg.font_size
                    } else if let Some(d) = args.get(1).and_then(|s| s.parse::<f32>().ok()) {
                        if d.is_finite() {
                            (st.font_size + d).clamp(6.0, 72.0)
                        } else {
                            st.font_size
                        }
                    } else {
                        st.font_size
                    }
                };
                state.borrow_mut().font_size = new_size;
            } else {
                eprintln!("dahook: change_font_size só com 'all' no MVP");
            }
            apply_all_terms(state);
        }
        "clear_terminal" => {
            // Igual ao `clear` do shell: limpa tudo (tela+scrollback) E
            // repinta o prompt na linha 1. Só o reset deixava o cursor no
            // topo sem prompt — o próximo Return criava "1 linha vazia".
            // O \x0c (Ctrl+L) pede repaint ao line editor (fish/bash/zsh);
            // em outros programas é inofensivo (redraw/no-op).
            if let Some(t) = current_term(state) {
                t.reset(true, true);
                t.feed_child(b"\x0c");
            }
        }
        "toggle_fullscreen" => {
            let mut st = state.borrow_mut();
            st.fullscreen = !st.fullscreen;
            if st.fullscreen {
                st.window.fullscreen();
            } else {
                st.window.unfullscreen();
            }
        }
        "toggle_maximized" => {
            let mut st = state.borrow_mut();
            st.maximized = !st.maximized;
            if st.maximized {
                st.window.maximize();
            } else {
                st.window.unmaximize();
            }
        }
        "load_config_file" => {
            let path;
            {
                let mut st = state.borrow_mut();
                path = st.conf_path.clone();
                st.cfg = DahookConfig::load(&path);
                st.font_size = st.cfg.font_size;
                st.opacity = st.cfg.background_opacity;
                if !st.cfg.ignored.is_empty() {
                    eprintln!(
                        "dahook: ignoradas: {}",
                        st.cfg.ignored.join(", ")
                    );
                }
            }
            apply_all_terms(state);
            rebuild_shortcuts(state);
        }
        "send_text" => {
            // send_text all Hello World -> alimenta a tab atual.
            let text: Vec<&str> = args
                .iter()
                .skip_while(|a| *a == "all")
                .map(|s| s.as_str())
                .collect();
            if let Some(t) = current_term(state) {
                t.feed_child(text.join(" ").as_bytes());
            }
        }
        "launch" => {
            // launch [--type=tab|os-window] [--cwd=current] prog...
            let mut typ = "tab";
            let mut cwd_current = false;
            let mut prog: Vec<String> = Vec::new();
            for a in args {
                if let Some(v) = a.strip_prefix("--type=") {
                    typ = v;
                } else if *a == "--cwd=current" {
                    cwd_current = true;
                } else if !a.starts_with("--") {
                    prog.push(a.clone());
                }
            }
            if typ == "os-window" {
                eprintln!("dahook: launch os-window abre shell default (prog ignorado no MVP)");
                new_os_window_action(state);
                return;
            }
            let cwd = if cwd_current {
                let uri: Option<gtk4::glib::GString> = current_term(state).and_then(|t| {
                    gtk4::glib::object::ObjectExt::property(&t, "current-directory-uri")
                });
                if std::env::var("DAHOOK_DEBUG_KEYS").is_ok() {
                    eprintln!("dahook cwd uri: {uri:?}");
                }
                uri.as_deref().and_then(uri_to_path)
            } else {
                None
            };
            new_tab(state, if prog.is_empty() { None } else { Some(prog) }, cwd);
        }
        "set_background_opacity" => {
            // +0.1 | -0.1 | 1 | default
            let new_opacity = {
                let st = state.borrow();
                match args.first().map(|s| s.as_str()) {
                    Some("default") => st.cfg.background_opacity,
                    Some(d) if d.starts_with('+') || d.starts_with('-') => match d.parse::<f64>() {
                        Ok(n) if n.is_finite() => (st.opacity + n).clamp(0.1, 1.0),
                        _ => st.opacity,
                    },
                    Some(v) => v
                        .parse::<f64>()
                        .ok()
                        .filter(|n| n.is_finite())
                        .map(|n| n.clamp(0.1, 1.0))
                        .unwrap_or(st.opacity),
                    None => st.opacity,
                }
            };
            state.borrow_mut().opacity = new_opacity;
            apply_all_terms(state);
        }
        other => eprintln!("dahook: ação do kitty não suportada no MVP: {other}"),
    }
}

/// Ações conhecidas (para avisar só o resto).
fn is_known_action(name: &str) -> bool {
    matches!(
        name,
        "new_tab"
            | "close_tab"
            | "next_tab"
            | "previous_tab"
            | "goto_tab"
            | "move_tab_forward"
            | "move_tab_backward"
            | "set_tab_title"
            | "new_window"
            | "new_os_window"
            | "close_window"
            | "next_window"
            | "previous_window"
            | "move_window_forward"
            | "move_window_backward"
            | "first_window"
            | "second_window"
            | "third_window"
            | "fourth_window"
            | "fifth_window"
            | "sixth_window"
            | "seventh_window"
            | "eighth_window"
            | "ninth_window"
            | "tenth_window"
            | "copy_to_clipboard"
            | "paste_from_clipboard"
            | "paste_from_selection"
            | "scroll_line_up"
            | "scroll_line_down"
            | "scroll_page_up"
            | "scroll_page_down"
            | "scroll_home"
            | "scroll_end"
            | "change_font_size"
            | "clear_terminal"
            | "toggle_fullscreen"
            | "toggle_maximized"
            | "load_config_file"
            | "send_text"
            | "launch"
            | "set_background_opacity"
            | "browser_back"
            | "browser_forward"
            | "browser_new_tab"
            | "browser_close_tab"
            | "browser_new_window"
            | "browser_reload"
            | "focus_address_bar"
    )
}

/// Atalho ligado: compara (mods, keyval) na fase Capture, antes do VTE
/// consumir a tecla (ShortcutController não dispara com VTE focado).
#[derive(Clone)]
struct BoundKey {
    mods: Mods,
    lo: gtk4::gdk::Key,
    hi: gtk4::gdk::Key,
    name: String,
    args: Vec<String>,
}

/// (Re)cria os atalhos: `map` do conf, ou defaults do kitty se vazio.
fn rebuild_shortcuts(state: &Shared) {
    let (specs, km) = {
        let st = state.borrow();
        // Igual ao kitty: defaults + maps do conf (mesmo keystroke troca).
        let mut specs: Vec<(String, String)> = DEFAULT_MAPS
            .iter()
            .map(|(ks, spec)| (ks.to_string(), spec.to_string()))
            .collect();
        for m in &st.cfg.maps {
            let full = if m.args.is_empty() {
                m.action.clone()
            } else {
                format!("{} {}", m.action, m.args.join(" "))
            };
            if let Some(pos) = specs.iter().position(|(ks, _)| *ks == m.keystroke) {
                specs[pos].1 = full;
            } else {
                specs.push((m.keystroke.clone(), full));
            }
        }
        (specs, st.cfg.kitty_mod)
    };

    let mut bindings: Vec<BoundKey> = Vec::new();
    let mut warned: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (keystroke, spec) in specs {
        // Nome = primeiro token; args = resto VERBATIM (flags --type/--cwd
        // pertencem à ação, ex: `launch --cwd=current sh ...`).
        let mut toks = spec.split_whitespace();
        let name = toks.next().unwrap_or("").to_string();
        let args: Vec<String> = toks.map(|s| s.to_string()).collect();
        if !is_known_action(&name) {
            if warned.insert(name.clone()) {
                eprintln!("dahook: ação do kitty não suportada no MVP: {name}");
            }
            continue;
        }
        let Some((mods, key)) =
            config::parse_map(&keystroke, &spec, &km).map(|(m, k, _, _)| (m, k))
        else {
            eprintln!("dahook: map ignorado (sequência/cmd/tecla?): {keystroke}");
            continue;
        };
        let Some(k) = gtk4::gdk::Key::from_name(&key) else {
            eprintln!("dahook: tecla desconhecida pro GDK: {key}");
            continue;
        };
        bindings.push(BoundKey {
            mods,
            lo: k.to_lower(),
            hi: k.to_upper(),
            name,
            args,
        });
    }

    let s = state.clone();
    let mut st = state.borrow_mut();
    if std::env::var("DAHOOK_DEBUG_KEYS").is_ok() {
        eprintln!("dahook: {} atalhos ligados", bindings.len());
    }
    if let Some(old) = st.controller.take() {
        st.window.remove_controller(&old);
    }
    let ctl = gtk4::EventControllerKey::new();
    ctl.set_propagation_phase(gtk4::PropagationPhase::Capture);
    ctl.connect_key_pressed(move |_, keyval, keycode, held| {
        use gtk4::gdk::ModifierType as MT;
        use gtk4::gdk::prelude::DisplayExtManual as _;
        let got = held & (MT::SHIFT_MASK | MT::CONTROL_MASK | MT::ALT_MASK | MT::SUPER_MASK);
        // Tecla física (nível 0 do keymap): com Shift segurado, `1` chega
        // como `!` — mas a tecla continua sendo `1`. Vale o recebido OU
        // qualquer keysym de nível 0 daquela tecla física.
        let mut cands = vec![keyval];
        if let Some(display) = gtk4::gdk::Display::default() {
            if let Some(entries) = display.map_keycode(keycode) {
                cands.extend(
                    entries
                        .into_iter()
                        .filter(|(k, _)| k.level() == 0)
                        .map(|(_, kv)| kv),
                );
            }
        }
        if std::env::var("DAHOOK_DEBUG_KEYS").is_ok() {
            eprintln!("dahook keys: keyval={keyval:?} keycode={keycode} mods={got:?} cands={cands:?}");
        }
        for b in &bindings {
            let mut want = MT::empty();
            if b.mods.ctrl {
                want |= MT::CONTROL_MASK;
            }
            if b.mods.shift {
                want |= MT::SHIFT_MASK;
            }
            if b.mods.alt {
                want |= MT::ALT_MASK;
            }
            if b.mods.sup {
                want |= MT::SUPER_MASK;
            }
            if got == want && cands.iter().any(|kv| *kv == b.lo || *kv == b.hi) {
                if dispatch(&s, &b.name, &b.args) {
                    return gtk4::glib::Propagation::Stop;
                }
                return gtk4::glib::Propagation::Proceed;
            }
        }
        gtk4::glib::Propagation::Proceed
    });
    st.window.add_controller(ctl.clone());
    st.controller = Some(ctl);
}

fn build_ui(app: &gtk4::Application) {
    let conf_path = config::ensure_default();
    let cfg = DahookConfig::load(&conf_path);
    if !cfg.ignored.is_empty() {
        eprintln!(
            "dahook: opções do kitty sem equivalente no backend (ignoradas): {}",
            cfg.ignored.join(", ")
        );
    }

    let window = gtk4::ApplicationWindow::new(app);
    window.set_title(Some("dahook"));
    window.set_icon_name(Some("dahook"));
    window.set_default_size(960, 600);

    let notebook = gtk4::Notebook::new();
    notebook.set_show_border(false);
    notebook.set_scrollable(true);
    window.set_child(Some(&notebook));
    // Troca de tab por clique mostra o título certo (conectado por tab em new_tab).

    let state: Shared = Rc::new(RefCell::new(State {
        app: app.clone(),
        window: window.clone(),
        notebook: notebook.clone(),
        tabs: Vec::new(),
        font_size: cfg.font_size,
        opacity: cfg.background_opacity,
        conf_path,
        cfg,
        fullscreen: false,
        maximized: false,
        controller: None,
        perms: std::collections::HashMap::new(),
        dl_action: gtk4::gio::SimpleAction::new(
            "dl-media",
            Some(&String::static_variant_type()),
        ),
    }));

    // Ação do item "Baixar mídia (yt-dlp)" do menu de contexto (param = URL).
    // Propositalmente fora do app (cada janela tem a sua; add_action
    // duplicaria o nome).
    {
        let s = state.clone();
        let act = s.borrow().dl_action.clone();
        act.connect_activate(move |_, param| {
            let Some(url) = param.and_then(|v| v.get::<String>()) else {
                return;
            };
            if std::env::var("DAHOOK_DEBUG_KEYS").is_ok() {
                eprintln!("dahook yt-dlp alvo: {url}");
            }
            // Com ■ na tab atual (menu foi clicado nela em 99% dos
            // casos); sem tab browser, destacado sem ■. De todo modo,
            // só notify no fim (o % real vai para o hover do ■).
            let cur: Option<(gtk4::Widget, DlStop)> = {
                let st = s.borrow();
                current_idx(&st).and_then(|i| match &st.tabs[i].kind {
                    TabKind::Web { dlstop, .. } => {
                        Some((st.tabs[i].page.clone(), dlstop.clone()))
                    }
                    TabKind::Term { .. } => None,
                })
            };
            let window = s.borrow().window.clone();
            if !ytdlp_available() {
                ytdlp_missing_dialog(&window);
                return;
            }
            match cur {
                Some((page, d)) => start_ytdlp_download(&url, Some((&d, &page))),
                None => start_ytdlp_download(&url, None),
            }
        });
    }

    rebuild_shortcuts(&state);
    new_tab(&state, None, None);

    // Ação remota `open-url`: o comando `dahook <url>` (CLI) abre browser
    // na instância principal via Gio actions (single-instance).
    {
        let s = state.clone();
        let act = gtk4::gio::SimpleAction::new(
            "open-url",
            Some(&String::static_variant_type()),
        );
        act.connect_activate(move |_, param| {
            let raw = param
                .and_then(|v| v.get::<String>())
                .unwrap_or_default();
            open_url_in(&s, &raw);
        });
        // Segunda janela não re-registra: o open-url segue na primeira.
        if app.lookup_action("open-url").is_none() {
            app.add_action(&act);
        }
    }

    // Clique na tab -> título da janela acompanha (conectado uma vez só).
    let s = state.clone();
    let nb = notebook.clone();
    nb.connect_switch_page(move |nb, _, num| {
        let st = s.borrow();
        if let Some(pg) = nb.nth_page(Some(num)) {
            if let Some(j) = idx_of_page(&st, &pg) {
                let title = st.tabs[j].title.text();
                st.window.set_title(Some(&title));
            }
        }
    });

    window.present();
}

fn main() {
    // DAHOOK_APP_ID permite instâncias isoladas (ex: testes sem colidir
    // com o app principal, que é single-instance por APP_ID).
    let app_id =
        std::env::var("DAHOOK_APP_ID").unwrap_or_else(|_| APP_ID.to_string());
    let app = gtk4::Application::new(Some(&app_id), Default::default());
    app.connect_activate(build_ui);
    app.run();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uri_to_path_file_urls() {        assert_eq!(
            uri_to_path("file:///home/usr/x"),
            Some(PathBuf::from("/home/usr/x"))
        );
        assert_eq!(
            uri_to_path("file://localhost/home/usr/x"),
            Some(PathBuf::from("/home/usr/x"))
        );
        assert_eq!(
            uri_to_path("file://usr-pc/home/usr/x"),
            Some(PathBuf::from("/home/usr/x"))
        );
        assert_eq!(uri_to_path("https://x"), None);
        assert_eq!(uri_to_path(""), None);
        assert_eq!(uri_to_path("file://hostonly"), None);
    }

    #[test]
    fn normalize_url_home_and_scheme() {        assert_eq!(normalize_url(""), "https://duckduckgo.com");
        assert_eq!(normalize_url("example.com"), "https://example.com");
        assert_eq!(normalize_url("http://x"), "http://x");
        assert_eq!(normalize_url("file:///a"), "file:///a");
    }

    #[test]
    fn uri_host_strips_scheme_and_path() {        assert_eq!(uri_host("https://ex.com:8443/p?q=1"), "ex.com:8443");
        assert_eq!(uri_host("http://ex.com"), "ex.com");
        assert_eq!(uri_host("sem-esquema"), "sem-esquema");
    }

    #[test]
    fn unique_name_before_extension() {        let dir = std::env::temp_dir().join(format!("dahook-dltest-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        // Livre: usa direto.
        assert_eq!(unique_name(&dir, "img.png").file_name().unwrap(), "img.png");
        std::fs::write(dir.join("img.png"), b"x").unwrap();
        // Ocupado: sufixo ANTES da extensão (img.2.png, não img.png.2).
        assert_eq!(
            unique_name(&dir, "img.png").file_name().unwrap(),
            "img.2.png"
        );
        std::fs::write(dir.join("img.2.png"), b"x").unwrap();
        assert_eq!(
            unique_name(&dir, "img.png").file_name().unwrap(),
            "img.3.png"
        );
        // Sem extensão e dotfile.
        assert_eq!(unique_name(&dir, "README").file_name().unwrap(), "README");
        std::fs::write(dir.join("README"), b"x").unwrap();
        assert_eq!(unique_name(&dir, "README").file_name().unwrap(), "README.2");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn parse_ytdlp_pct_percent_lines() {        let close = |got: Option<f64>, want: f64| {
            assert!((got.unwrap() - want).abs() < 1e-9, "{got:?} != {want}");
        };
        close(parse_ytdlp_pct("[download]   0.0% of 10MiB"), 0.0);
        close(parse_ytdlp_pct("[download]  12.3% of ~5MiB"), 0.123);
        close(parse_ytdlp_pct("[download] 100% of 1GiB"), 1.0);
        assert_eq!(parse_ytdlp_pct("[download] Destination: x.mp4"), None);
        assert_eq!(parse_ytdlp_pct("[info] nada"), None);
        assert_eq!(parse_ytdlp_pct(""), None);
    }

    /// Mata o grupo inteiro (pai + netos, ex: yt-dlp + ffmpeg do merge).
    /// Sem killpg, o filho morria e o neto órfão continuava — parecia
    /// "não parar em tempo real".
    #[test]
    fn killpg_mata_grupo() {
        use std::os::unix::process::CommandExt;
        use std::time::Duration;
        let mut c = std::process::Command::new("bash")
            .args(["-c", "sleep 60 & wait"])
            .process_group(0)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let pgid = c.id() as i32;
        std::thread::sleep(Duration::from_millis(300));
        unsafe {
            libc::kill(-pgid, libc::SIGKILL);
        }
        let st = c.wait().unwrap();
        assert!(!st.success());
        // Grupo vazio (pai recolhido pelo wait, neto pelo init).
        let mut gone = false;
        for _ in 0..30 {
            if unsafe { libc::kill(-pgid, 0) } != 0 {
                gone = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        assert!(gone, "grupo sobreviveu ao killpg");
    }
}
