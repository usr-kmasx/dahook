//! dahook.conf — mesma sintaxe e opções do kitty.conf.
//!
//! Local: `$XDG_CONFIG_HOME/dahook/dahook.conf` (ou `~/.config/dahook/dahook.conf`).
//! O arquivo ACEITA qualquer opção do kitty.conf (não quebra); as que têm
//! equivalente no VTE são aplicadas, o resto é listado como ignorado.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rgba {
    pub r: f64,
    pub g: f64,
    pub b: f64,
    pub a: f64,
}

impl Rgba {
    pub fn rgb(r: f64, g: f64, b: f64) -> Self {
        Self { r, g, b, a: 1.0 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorShape {
    Block,
    Beam,
    Underline,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Mods {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub sup: bool,
}

impl Mods {
    pub fn or(&mut self, o: &Mods) {
        self.ctrl |= o.ctrl;
        self.shift |= o.shift;
        self.alt |= o.alt;
        self.sup |= o.sup;
    }
}

/// `ctrl+shift`, `super`, ... (kitty permite combinar com `+`).
pub fn parse_mods(s: &str) -> Mods {
    let mut m = Mods::default();
    for tok in s.split('+') {
        match tok.trim() {
            "ctrl" => m.ctrl = true,
            "shift" => m.shift = true,
            "alt" | "opt" => m.alt = true,
            "super" => m.sup = true,
            _ => {}
        }
    }
    m
}

/// Nome de tecla kitty -> nome GDK (para `ShortcutTrigger::parse_string`).
/// Letras/dígitos passam direto; pontuação precisa do nome GDK.
/// Case-insensitive (aceita `Left`, `TAB`, `Enter`...).
pub fn kitty_key_name(k: &str) -> Option<String> {
    let kl = k.to_lowercase();
    let k: &str = &kl;
    if k.len() == 1 {
        let gdk = match k {
            "]" => "bracketright",
            "[" => "bracketleft",
            "." => "period",
            "," => "comma",
            "/" => "slash",
            ";" => "semicolon",
            "'" => "apostrophe",
            "`" => "grave",
            "\\" => "backslash",
            "=" => "equal",
            "-" => "minus",
            _ => k,
        };
        return Some(gdk.to_string());
    }
    Some(
        match k {
            "enter" | "return" => "Return",
            "tab" => "Tab",
            "space" => "space",
            "backspace" => "BackSpace",
            "delete" => "Delete",
            "insert" => "Insert",
            "home" => "Home",
            "end" => "End",
            "page_up" => "Page_Up",
            "page_down" => "Page_Down",
            "up" => "Up",
            "down" => "Down",
            "left" => "Left",
            "right" => "Right",
            "escape" | "esc" => "Escape",
            "plus" => "plus",
            "minus" => "minus",
            "equal" => "equal",
            "comma" => "comma",
            "period" => "period",
            "slash" => "slash",
            "semicolon" => "semicolon",
            "apostrophe" => "apostrophe",
            "grave" | "backtick" => "grave",
            "bracket_left" => "bracketleft",
            "bracket_right" => "bracketright",
            "backslash" => "backslash",
            "kp_add" => "KP_Add",
            "kp_subtract" => "KP_Subtract",
            "f1" => "F1",
            "f2" => "F2",
            "f3" => "F3",
            "f4" => "F4",
            "f5" => "F5",
            "f6" => "F6",
            "f7" => "F7",
            "f8" => "F8",
            "f9" => "F9",
            "f10" => "F10",
            "f11" => "F11",
            "f12" => "F12",
            _ => return None,
        }
        .to_string(),
    )
}

/// Parse de `map <keystroke> <ação> [args...]`.
/// Retorna (mods efetivos, tecla GDK, ação, args). `None` = sequência (`a>b`)
/// ou tecla `cmd` (só macOS) ou tecla desconhecida.
pub fn parse_map(
    keystroke: &str,
    action: &str,
    km: &Mods,
) -> Option<(Mods, String, String, Vec<String>)> {
    if keystroke.contains('>') {
        return None; // sequências de teclas não suportadas
    }
    let mut mods = Mods::default();
    let mut key: Option<String> = None;
    for tok in keystroke.split('+') {
        match tok {
            "ctrl" => mods.ctrl = true,
            "shift" => mods.shift = true,
            "alt" | "opt" => mods.alt = true,
            "super" => mods.sup = true,
            "cmd" => return None, // só macOS
            "kitty_mod" => mods.or(km),
            k => {
                if key.is_some() {
                    return None;
                }
                key = Some(kitty_key_name(k)?);
            }
        }
    }
    let key = key?;
    // Ação: pula flags `--opt=val`, primeiro token = ação, resto = args.
    let mut toks = action.split_whitespace().filter(|t| !t.starts_with("--"));
    let name = toks.next()?.to_string();
    let args: Vec<String> = toks.map(|s| s.to_string()).collect();
    Some((mods, key, name, args))
}

#[derive(Debug, Clone)]
pub struct MapEntry {
    pub keystroke: String,
    pub action: String,
    pub args: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct DahookConfig {
    pub kitty_mod: Mods,
    pub maps: Vec<MapEntry>,
    pub font_family: String,
    pub font_size: f32,
    pub foreground: Rgba,
    pub background: Rgba,
    pub background_opacity: f64,
    pub palette: HashMap<u8, Rgba>,
    pub cursor_shape: CursorShape,
    pub cursor_blink: bool,
    pub cursor_color: Option<Rgba>,
    pub selection_fg: Option<Rgba>,
    pub selection_bg: Option<Rgba>,
    pub scrollback_lines: i64,
    pub window_padding: i32,
    pub shell: Option<(String, Vec<String>)>,
    pub env: HashMap<String, String>,
    /// Opções válidas do kitty sem equivalente no VTE (só aviso).
    pub ignored: Vec<String>,
}

impl Default for DahookConfig {
    fn default() -> Self {
        Self {
            kitty_mod: Mods { ctrl: true, shift: true, alt: false, sup: false },
            maps: Vec::new(),
            font_family: "Monospace".into(),
            font_size: 12.0,
            foreground: Rgba::rgb(0.867, 0.867, 0.867), // #dddddd
            background: Rgba::rgb(0.0, 0.0, 0.0),
            background_opacity: 1.0,
            palette: HashMap::new(),
            cursor_shape: CursorShape::Block,
            cursor_blink: true,
            cursor_color: None,
            selection_fg: None,
            selection_bg: None,
            scrollback_lines: 2000,
            window_padding: 0,
            shell: None,
            env: HashMap::new(),
            ignored: Vec::new(),
        }
    }
}

/// Opções do kitty aplicadas pelo dahook (`map`/`kitty_mod` viram atalhos).
const SUPPORTED: &[&str] = &[
    "kitty_mod",
    "map",
    "font_family",
    "font_size",
    "foreground",
    "background",
    "background_opacity",
    "cursor",
    "cursor_shape",
    "cursor_blink_interval",
    "selection_foreground",
    "selection_background",
    "scrollback_lines",
    "window_padding_width",
    "shell",
    "env",
    "include",
];

fn is_color_key(key: &str) -> bool {
    key.len() > 5
        && key.starts_with("color")
        && key[5..].bytes().all(|b| b.is_ascii_digit())
        && key[5..].parse::<u16>().is_ok_and(|n| n < 256)
}

const COLOR_NAMES: &[(&str, (u8, u8, u8))] = &[
    ("black", (0, 0, 0)),
    ("red", (205, 0, 0)),
    ("green", (0, 205, 0)),
    ("yellow", (205, 205, 0)),
    ("blue", (0, 0, 238)),
    ("magenta", (205, 0, 205)),
    ("cyan", (0, 205, 205)),
    ("white", (229, 229, 229)),
];

/// `#rgb` / `#rrggbb` / nome básico. Igual ao kitty para os formatos comuns.
pub fn parse_color(s: &str) -> Option<Rgba> {
    let s = s.trim().to_lowercase();
    if let Some(hex) = s.strip_prefix('#') {
        let v = |h: &str| u8::from_str_radix(h, 16).ok();
        let (r, g, b) = match hex.len() {
            3 => {
                let c: Vec<char> = hex.chars().collect();
                (v(&format!("{}{}", c[0], c[0]))?, v(&format!("{}{}", c[1], c[1]))?, v(&format!("{}{}", c[2], c[2]))?)
            }
            6 => (v(&hex[0..2])?, v(&hex[2..4])?, v(&hex[4..6])?),
            _ => return None,
        };
        return Some(Rgba::rgb(r as f64 / 255.0, g as f64 / 255.0, b as f64 / 255.0));
    }
    COLOR_NAMES
        .iter()
        .find(|(n, _)| *n == s)
        .map(|(_, (r, g, b))| Rgba::rgb(*r as f64 / 255.0, *g as f64 / 255.0, *b as f64 / 255.0))
}

/// Tira comentário do RESTO da linha: `#` só comenta se precedido de
/// espaço (senão `#rrggbb` no início do valor seria comido). Aspas protegem.
fn strip_comment(rest: &str) -> &str {
    let mut in_single = false;
    let mut in_double = false;
    let mut prev_space = false; // posição 0 conta como "início do valor"
    for (i, c) in rest.char_indices() {
        match c {
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            '#' if !in_single && !in_double && prev_space => return rest[..i].trim_end(),
            _ => {}
        }
        prev_space = c.is_whitespace();
    }
    rest.trim_end()
}

/// Divide `chave resto-da-linha` (comentário já removido do valor).
fn split_opt(line: &str) -> Option<(&str, &str)> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let mut parts = line.splitn(2, char::is_whitespace);
    let key = parts.next()?.trim();
    let rest = parts.next().unwrap_or("").trim();
    if key.is_empty() {
        return None;
    }
    Some((key, strip_comment(rest)))
}

/// Remove aspas simples/duplas ao redor do valor (kitty permite).
fn unquote(s: &str) -> &str {
    let s = s.trim();
    if s.len() >= 2
        && ((s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\'')))
    {
        &s[1..s.len() - 1]
    } else {
        s
    }
}

impl DahookConfig {
    pub fn parse(text: &str) -> Self {
        let mut cfg = Self::default();
        let mut seen = HashSet::new();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((key, val)) = split_opt(line) else {
                continue;
            };
            if key == "include" {
                // Tratado em load() (precisa do caminho base); aqui só registra.
                continue;
            }
            cfg.apply(key, val, &mut seen);
        }
        cfg
    }

    fn apply(&mut self, key: &str, val: &str, seen: &mut HashSet<String>) {
        match key {
            "kitty_mod" => self.kitty_mod = parse_mods(val),
            "map" => {
                let mut parts = val.splitn(2, char::is_whitespace);
                if let (Some(ks), Some(act)) = (parts.next(), parts.next()) {
                    self.maps.push(MapEntry {
                        keystroke: ks.to_string(),
                        action: act.trim().to_string(),
                        args: Vec::new(), // separado no parse_map (precisa do kitty_mod final)
                    });
                }
            }
            "font_family" => self.font_family = unquote(val).to_string(),
            "font_size" => {
                if let Ok(n) = val.parse::<f32>() {
                    if n.is_finite() {
                        self.font_size = n.clamp(6.0, 72.0);
                    }
                }
            }
            "foreground" => {
                if let Some(c) = parse_color(val) {
                    self.foreground = c;
                }
            }
            "background" => {
                if let Some(c) = parse_color(val) {
                    self.background = c;
                }
            }
            "background_opacity" => {
                if let Ok(n) = val.parse::<f64>() {
                    if n.is_finite() {
                        self.background_opacity = n.clamp(0.0, 1.0);
                    }
                }
            }
            "cursor" => {
                if let Some(c) = parse_color(val) {
                    self.cursor_color = Some(c);
                }
            }
            "cursor_shape" => {
                self.cursor_shape = match val {
                    "beam" => CursorShape::Beam,
                    "underline" | "underscore" => CursorShape::Underline,
                    _ => CursorShape::Block,
                };
            }
            "cursor_blink_interval" => {
                // kitty: 0 desliga o blink.
                self.cursor_blink = val.parse::<f64>().is_ok_and(|n| n != 0.0);
            }
            "selection_foreground" => {
                if let Some(c) = parse_color(val) {
                    self.selection_fg = Some(c);
                }
            }
            "selection_background" => {
                if let Some(c) = parse_color(val) {
                    self.selection_bg = Some(c);
                }
            }
            "scrollback_lines" => {
                if let Ok(n) = val.parse::<i64>() {
                    self.scrollback_lines = n.max(-1);
                }
            }
            "window_padding_width" => {
                if let Ok(n) = val.parse::<i32>() {
                    self.window_padding = n.clamp(0, 64);
                }
            }
            "shell" => {
                let parts: Vec<String> =
                    val.split_whitespace().map(|s| unquote(s).to_string()).collect();
                if let Some((prog, args)) = parts.split_first() {
                    if prog != "." && !prog.is_empty() {
                        self.shell = Some((prog.clone(), args.to_vec()));
                    }
                }
            }
            "env" => {
                if let Some((k, v)) = val.split_once('=') {
                    self.env.insert(k.trim().to_string(), v.trim().to_string());
                }
            }
            _ if is_color_key(key) => {
                if let (Ok(n), Some(c)) =
                    (key[5..].parse::<u8>(), parse_color(val))
                {
                    self.palette.insert(n, c);
                }
            }
            _ => {
                if SUPPORTED.contains(&key) {
                    return; // suportada mas sem efeito aqui (ex: include)
                }
                if seen.insert(key.to_string()) {
                    self.ignored.push(key.to_string());
                }
            }
        }
    }

    /// Carrega do disco, expandindo `include` inline (relativo ao dir do
    /// conf, ordem preservada como se colado no lugar) e delegando ao parse.
    pub fn load(path: &std::path::Path) -> Self {
        let mut visited = HashSet::new();
        let combined = Self::expand_includes(path, &mut visited);
        Self::parse(&combined)
    }

    fn expand_includes(path: &std::path::Path, visited: &mut HashSet<PathBuf>) -> String {
        if !visited.insert(path.to_path_buf()) {
            return String::new();
        }
        let Ok(text) = std::fs::read_to_string(path) else {
            return String::new();
        };
        let base = path.parent().map(|d| d.to_path_buf()).unwrap_or_default();
        let mut out = String::new();
        for line in text.lines() {
            let trimmed = line.trim();
            if let Some((key, val)) = split_opt(trimmed) {
                if key == "include" {
                    for inc in val.split_whitespace() {
                        for f in Self::resolve_include(&base, unquote(inc)) {
                            out.push_str(&Self::expand_includes(&f, visited));
                        }
                    }
                    continue;
                }
            }
            out.push_str(line);
            out.push('\n');
        }
        out
    }

    /// Resolve um `include` (caminho exato ou glob simples `prefix*suffix`).
    fn resolve_include(base: &std::path::Path, pat: &str) -> Vec<PathBuf> {
        let inc = base.join(pat);
        let lossy = inc.to_string_lossy().to_string();
        let Some(star) = lossy.find('*') else {
            return vec![inc];
        };
        let (dir, pat) = lossy.split_at(star);
        let suffix = &pat[1..];
        let prefix_end = dir.rfind('/').map(|i| i + 1).unwrap_or(0);
        let (dir, prefix) = dir.split_at(prefix_end);
        let Ok(rd) = std::fs::read_dir(dir) else {
            return Vec::new();
        };
        let mut names: Vec<_> = rd
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|q| {
                q.is_file()
                    && q.file_name().is_some_and(|n| {
                        let n = n.to_string_lossy();
                        n.starts_with(prefix) && n.ends_with(suffix)
                    })
            })
            .collect();
        names.sort();
        names
    }
}

/// Caminho XDG: `$XDG_CONFIG_HOME/dahook/dahook.conf` ou `~/.config/...`.
pub fn config_path() -> PathBuf {
    let base = std::env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
            PathBuf::from(home).join(".config")
        });
    base.join("dahook").join("dahook.conf")
}

const EXAMPLE: &str = r#"# dahook.conf — mesma sintaxe e opções do kitty.conf.
# Opções sem equivalente no backend são aceitas e ignoradas (aviso no stderr).

# font_family "Maple Mono"
# font_size 13.0

# foreground #dddddd
# background #000000
# background_opacity 1.0

# cursor_shape block
# cursor_blink_interval 0.5
# cursor #dddddd

# selection_foreground #000000
# selection_background #fffacd

# color0 #000000
# color1 #cc0000
# color15 #ffffff

# scrollback_lines 2000
# window_padding_width 4

# shell fish
# env EDITOR=nvim

# kitty_mod ctrl+shift
# Atalhos: mesma sintaxe `map` do kitty. Sem nenhum map no conf,
# valem os defaults do kitty (ctrl+shift+t nova tab, ctrl+shift+q fecha, ...).
# map kitty_mod+t new_tab
# map kitty_mod+w close_window

# include local.conf
"#;

/// Cria o exemplo comentado se não existir. Retorna o caminho usado.
pub fn ensure_default() -> PathBuf {
    let path = config_path();
    if !path.exists() {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(&path, EXAMPLE);
    }
    path
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colors() {
        assert_eq!(parse_color("#000"), Some(Rgba::rgb(0.0, 0.0, 0.0)));
        assert_eq!(parse_color("#ff0000").unwrap().r, 1.0);
        assert_eq!(parse_color("#abc").unwrap().b, 0xcc as f64 / 255.0);
        assert!(parse_color("notacolor").is_none());
        assert!(parse_color("#12345").is_none());
    }

    #[test]
    fn full_parse_kitty_style() {
        let c = DahookConfig::parse(
            "font_family \"Fira Code\"\nfont_size 14\nbackground #1a1b26 # tema\nforeground #c0caf5\nbackground_opacity 0.9\ncursor_shape beam\ncursor_blink_interval 0\ncolor1 #f7768e\nscrollback_lines 5000\nwindow_padding_width 8\nshell fish --login\nallow_remote_control yes\nmap kitty_mod+t new_tab\n",
        );
        assert_eq!(c.font_family, "Fira Code");
        assert_eq!(c.font_size, 14.0);
        assert_eq!(c.background_opacity, 0.9);
        assert_eq!(c.cursor_shape, CursorShape::Beam);
        assert!(!c.cursor_blink);
        assert_eq!(c.palette.get(&1), Some(&Rgba::rgb(0xf7 as f64 / 255.0, 0x76 as f64 / 255.0, 0x8e as f64 / 255.0)));
        assert_eq!(c.shell, Some(("fish".into(), vec!["--login".into()])));
        // Opções do kitty sem equivalente: aceitas, listadas como ignoradas.
        // (`map` agora é suportado de verdade: vai para cfg.maps.)
        assert!(c.ignored.contains(&"allow_remote_control".to_string()));
        assert!(!c.ignored.contains(&"map".to_string()));
        assert_eq!(c.maps.len(), 1);
        assert_eq!(c.maps[0].keystroke, "kitty_mod+t");
        assert_eq!(c.maps[0].action, "new_tab");
    }

    #[test]
    fn comment_after_value() {
        // `#rrggbb` no início do valor NÃO é comentário; `#` depois sim.
        let c = DahookConfig::parse("background #1a1b26 # tema escuro\nforeground #c0caf5\n");
        assert_eq!(
            c.background,
            Rgba::rgb(0x1a as f64 / 255.0, 0x1b as f64 / 255.0, 0x26 as f64 / 255.0)
        );
        assert_eq!(c.foreground.r, 0xc0 as f64 / 255.0);
    }

    #[test]
    fn load_expands_include_in_order() {
        let dir = std::env::temp_dir().join(format!("dahook-test-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(dir.join("main.conf"), "font_size 12\ninclude extra.conf\nfont_size 20\n").unwrap();
        std::fs::write(dir.join("extra.conf"), "font_size 14\nbackground #111111\n").unwrap();
        let c = DahookConfig::load(&dir.join("main.conf"));
        // Ordem preservada: 12 -> (include: 14) -> 20. Último vence.
        assert_eq!(c.font_size, 20.0);
        assert_eq!(c.background, Rgba::rgb(0x11 as f64 / 255.0, 0x11 as f64 / 255.0, 0x11 as f64 / 255.0));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn shell_dot_means_default() {
        let c = DahookConfig::parse("shell .\n");
        assert_eq!(c.shell, None);
    }

    #[test]
    fn kitty_maps() {        let km = Mods { ctrl: true, shift: true, alt: false, sup: false };
        let (m, k, a, args) = parse_map("kitty_mod+t", "new_tab", &km).unwrap();
        assert!(m.ctrl && m.shift && !m.alt && !m.sup);
        assert_eq!(k, "t");
        assert_eq!(a, "new_tab");
        assert!(args.is_empty());

        let (m, k, a, _args) =
            parse_map("ctrl+tab", "next_tab", &km).unwrap();
        assert!(m.ctrl && !m.shift);
        assert_eq!(k, "Tab");
        assert_eq!(a, "next_tab");

        // Flags --opt são puladas; resto vira args.
        let (_, _, a, args) = parse_map(
            "kitty_mod+equal",
            "change_font_size --foo=1 all +2.0",
            &km,
        )
        .unwrap();
        assert_eq!(a, "change_font_size");
        assert_eq!(args, vec!["all", "+2.0"]);

        // Sequências e cmd (macOS) são rejeitadas.
        assert!(parse_map("kitty_mod+p>f", "x", &km).is_none());
        assert!(parse_map("cmd+t", "new_tab", &km).is_none());
        assert!(parse_map("kitty_mod+unknownkey", "x", &km).is_none());

        // Pontuação vira nome GDK (match é feito por keyval, não literal).
        let (m, k, _, _) = parse_map("kitty_mod+]", "next_window", &km).unwrap();
        assert!(m.ctrl && m.shift);
        assert_eq!(k, "bracketright");
        let (m, k, _, _) = parse_map("kitty_mod+.", "move_tab_forward", &km).unwrap();
        assert_eq!(k, "period");
        assert!(m.ctrl && m.shift);
        // Case-insensitive (Alt+Left de browser).
        let (m, k, _, _) = parse_map("alt+Left", "browser_back", &km).unwrap();
        assert!(m.alt && !m.ctrl);
        assert_eq!(k, "Left");
    }

    #[test]
    fn kitty_mod_opt() {
        let c = DahookConfig::parse("kitty_mod super\nmap kitty_mod+t new_tab\n");
        assert!(c.kitty_mod.sup && !c.kitty_mod.ctrl);
        assert_eq!(c.maps.len(), 1);
        let (m, k, _, _) = parse_map(&c.maps[0].keystroke, &c.maps[0].action, &c.kitty_mod).unwrap();
        assert!(m.sup && !m.ctrl);
        assert_eq!(k, "t");
    }
}
