//! dahook: um app so.
//!
//! - Inicia como terminal normal (kitty real -> todos os atalhos do kitty nativos).
//! - Digitar `dahook [url]` no shell carrega o browser DENTRO do app
//!   (contencao visual na mesma janela/classe, perfil isolado).
//! - No modo web, o browser e real (chromium) -> todos os atalhos do chromium nativos.
//! - Um X no canto superior direito volta ao terminal (minimiza browser, restaura terminal).
//!
//! Orquestracao em Rust sobre os binarios reais (kitty + chromium/qutebrowser).
//! Sem reimplementar terminal/browser do zero no MVP.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

const TRIGGER_FILE: &str = "/tmp/dahook-trigger";
const BACK_FILE: &str = "/tmp/dahook-back";
const MODE_FILE: &str = "/tmp/dahook-mode";
const KITTY_SOCK: &str = "/tmp/dahook-kitty.sock";
const WEB_PROFILE: &str = "/tmp/dahook-webprofile";
const DEFAULT_URL: &str = "https://duckduckgo.com";

#[derive(Parser)]
#[command(name = "dahook", about = "um app so: terminal kitty que vira browser")]
struct Cli {
    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Roda o app completo: terminal -> (dahook) -> browser -> (X) -> terminal
    Run {
        /// URL inicial do modo browser
        #[arg(default_value = DEFAULT_URL)]
        url: String,
    },
    /// So abre o terminal kitty configurado (modo terminal)
    Terminal,
    /// So abre o browser isolado (modo web). Usado pelo orquestrador e pelo X.
    Browser {
        #[arg(default_value = DEFAULT_URL)]
        url: String,
    },
    /// Sinaliza "voltar ao terminal" (o que o botao X faz)
    Back,
    /// Mostra modo atual (terminal|browser) e PIDs
    Status,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd.unwrap_or(Cmd::Run {
        url: DEFAULT_URL.to_string(),
    }) {
        Cmd::Run { url } => run_app(&url),
        Cmd::Terminal => {
            launch_terminal()?;
            println!("terminal kitty aberto. Digite `dahook [url]` para virar browser.");
            Ok(())
        }
        Cmd::Browser { url } => {
            let mut child = launch_browser(&url)?;
            println!("browser pid={} (perfil isolado {})", child.id(), WEB_PROFILE);
            let _ = child.wait();
            Ok(())
        }
        Cmd::Back => {
            fs::write(BACK_FILE, "back").context("escrever back file")?;
            println!("sinal X enviado: voltando ao terminal.");
            Ok(())
        }
        Cmd::Status => {
            let mode = fs::read_to_string(MODE_FILE).unwrap_or_else(|_| "desconhecido".into());
            println!("modo: {}", mode.trim());
            println!("trigger: {} existe={}", TRIGGER_FILE, Path::new(TRIGGER_FILE).exists());
            println!("back: {} existe={}", BACK_FILE, Path::new(BACK_FILE).exists());
            Ok(())
        }
    }
}

// ---------------------------------------------------------------------------
// Orquestrador principal
// ---------------------------------------------------------------------------

fn run_app(default_url: &str) -> Result<()> {
    rm_if_exists(TRIGGER_FILE);
    rm_if_exists(BACK_FILE);
    fs::create_dir_all(WEB_PROFILE).ok();
    set_mode("terminal")?;

    println!("== dahook: modo TERMINAL (kitty real, shell real) ==");
    println!("Digite `dahook` (abre home) ou `dahook [url]` no prompt para virar browser.");
    println!("No modo browser, o X (canto superior direito) minimiza e volta ao terminal.\n");

    let mut kitty = launch_terminal().context("abrir kitty")?;
    // Browser persistente: o X minimiza em vez de matar (retorno rapido,
    // abas preservadas). So morre se o usuario fechar a janela ou o app sair.
    let mut browser: Option<Child> = None;

    // Loop terminal -> browser -> terminal ...
    loop {
        // 1. espera o comando `dahook [url]` (shell hook escreve o trigger).
        // Sem arg = home (DEFAULT_URL).
        println!("[dahook] aguardando `dahook` no terminal...");
        let url = wait_for_trigger()?;
        let url = if url.is_empty() {
            default_url.to_string()
        } else {
            url
        };
        println!("[dahook] `dahook` detectado -> virando BROWSER: {url}");
        set_mode("browser")?;

        // 2. esconde o terminal (contencao visual: mesma app, nao outra janela)
        hide_kitty_window();

        // 3. reusa browser minimizado ou abre novo isolado.
        // Com chromium + mesmo --user-data-dir, `chromium <url>` abre nova aba
        // na instancia existente em vez de duplicar processo.
        let browser_alive_now = browser.as_mut().map(child_alive).unwrap_or(false);
        if browser_alive_now {
            let pid = browser.as_ref().map(|b| b.id()).unwrap_or(0);
            restore_browser_window_by_pid(pid);
            open_url_in_existing_profile(&url);
        } else {
            browser = Some(launch_browser(&url)?);
            // traz para frente na mesma posicao aparente (launch_browser ja tenta)
            thread::sleep(Duration::from_millis(200));
        }
        // 4. abre overlay X (canto superior direito) que escreve BACK_FILE ao clicar
        let mut overlay = launch_x_overlay();

        // 5. espera sair do modo browser: X clicado (minimiza) ou janela fechada
        let reason = wait_for_back_or_close(browser.as_mut());
        let browser_closed_by_user = matches!(reason, BackReason::Closed);

        // 6. X = minimiza e volta ao terminal (mantem processo/abas).
        // Fechar janela = descarta o handle (proximo `dahook` abre novo).
        if browser_closed_by_user {
            browser = None;
        } else if let Some(b) = browser.as_mut() {
            minimize_browser_window(b);
        }
        if let Some(o) = overlay.as_mut() {
            kill_child(o, "x-overlay");
        }
        rm_if_exists(TRIGGER_FILE);
        rm_if_exists(BACK_FILE);
        restore_kitty_window();
        set_mode("terminal")?;
        if browser_closed_by_user {
            println!("[dahook] browser fechado -> de volta ao TERMINAL (proximo `dahook` abre novo).");
        } else {
            println!("[dahook] X minimizou o browser -> de volta ao TERMINAL (abas preservadas).");
        }

        if !kitty_alive(&mut kitty) {
            println!("[dahook] kitty foi fechado, encerrando app.");
            if let Some(b) = browser.as_mut() {
                kill_child(b, "browser");
            }
            break;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Terminal (kitty real)
// ---------------------------------------------------------------------------

fn kitty_bin() -> String {
    for c in ["kitty", "/usr/bin/kitty"] {
        if Command::new(c).arg("--version").output().map(|o| o.status.success()).unwrap_or(false) {
            return c.to_string();
        }
    }
    "kitty".to_string()
}

fn dahook_shell_hook() -> PathBuf {
    // Hook sourced pelo shell do kitty: define `dahook()` que escreve o trigger.
    // O kitty roda bash real; o usuario faz `source shell/dahook.sh` ou o
    // orquestrador passa via --override shell=... Em MVP, instruimos o source.
    let p = PathBuf::from("/tmp/dahook-rc.sh");
    let content = r#"# dahook shell hook (source este arquivo no shell do kitty)
dahook() {
  local url="${1:-https://duckduckgo.com}"
  echo "$url" > /tmp/dahook-trigger
  echo "[dahook] carregando browser dentro do app: $url"
}
"#;
    let _ = fs::write(&p, content);
    p
}

fn launch_terminal() -> Result<Child> {
    rm_if_exists(KITTY_SOCK);
    let rc = dahook_shell_hook();
    let kitty = kitty_bin();
    // allow_remote_control permite esconder/mostrar a janela sem abrir outro app.
    // --listen-on cria o socket para `kitty @`.
    let child = Command::new(&kitty)
        .args([
            "--class",
            "dahook",
            "--title",
            "dahook — terminal",
            "-o",
            "allow_remote_control=yes",
            "--listen-on",
            &format!("unix:{KITTY_SOCK}"),
            "-o",
            &format!("shell_integration=no"),
            "bash",
            "--rcfile",
        ])
        .arg(rc.to_string_lossy().to_string())
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .with_context(|| format!("spawn {kitty}. Instale kitty ou ajuste PATH."))?;
    // da tempo do socket subir
    thread::sleep(Duration::from_millis(600));
    println!("(dica shell) rode: source /tmp/dahook-rc.sh  — depois: dahook https://example.com");
    Ok(child)
}

fn kitty_remote(args: &[&str]) -> bool {
    Command::new(kitty_bin())
        .arg("@")
        .arg("--to")
        .arg(format!("unix:{KITTY_SOCK}"))
        .args(args)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn hide_kitty_window() {
    // Primeiro tenta remote control do kitty; fallback xdotool (X11).
    if kitty_remote(&["set-window-visibility", "no"]) {
        return;
    }
    let _ = Command::new("xdotool")
        .args(["search", "--class", "dahook", "windowunmap"])
        .output();
}

fn restore_kitty_window() {
    if kitty_remote(&["set-window-visibility", "yes"]) {
        let _ = Command::new(kitty_bin())
            .args(["@", "--to", &format!("unix:{KITTY_SOCK}"), "focus-window"])
            .output();
        return;
    }
    let _ = Command::new("xdotool")
        .args(["search", "--class", "dahook", "windowmap"])
        .output();
    let _ = Command::new("xdotool")
        .args(["search", "--class", "dahook", "windowactivate"])
        .output();
}

fn kitty_alive(kitty: &mut Child) -> bool {
    match kitty.try_wait() {
        Ok(None) => true,
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// Browser (chromium real com perfil isolado; fallback qutebrowser leve)
// ---------------------------------------------------------------------------

fn chromium_candidates() -> Vec<String> {
    vec![
        "chromium".into(),
        "/usr/bin/chromium".into(),
        "chromium-browser".into(),
        "google-chrome".into(),
        "google-chrome-stable".into(),
    ]
}

fn find_chromium() -> Option<String> {
    chromium_candidates().into_iter().find(|c| {
        Command::new(c).arg("--version").output().map(|o| o.status.success()).unwrap_or(false)
    })
}

fn launch_browser(url: &str) -> Result<Child> {
    fs::create_dir_all(WEB_PROFILE).ok();
    if let Some(chrome) = find_chromium() {
        // Perfil isolado (--user-data-dir) = "flatpak-like" em nivel de perfil:
        // cookies/historico/extensoes do browser-modo nao tocam no perfil principal.
        // Contencao visual: mesma class `dahook`, mesma geometria do terminal.
        let child = Command::new(&chrome)
            .args([
                &format!("--class=dahook"),
                &format!("--user-data-dir={WEB_PROFILE}"),
                "--no-first-run",
                "--no-default-browser-check",
                "--disable-features=Translate",
            ])
            .arg(url)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .with_context(|| format!("spawn {chrome}"))?;
        // traz para frente na mesma posicao aparente
        thread::sleep(Duration::from_millis(400));
        let _ = Command::new("xdotool")
            .args(["search", "--class", "dahook", "windowactivate"])
            .output();
        return Ok(child);
    }
    // Fallback: base leve que ja temos (browser/qutebrowser, QtWebEngine).
    // Midia: parcial (ver README.md). Prefira chromium para compat total.
    let qb = PathBuf::from("../browser/qutebrowser.py");
    if qb.exists() {
        let child = Command::new("python3")
            .arg(qb.to_string_lossy().to_string())
            .args(["-B", WEB_PROFILE])
            .arg(url)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .context("spawn qutebrowser fallback")?;
        return Ok(child);
    }
    anyhow::bail!("nenhum browser encontrado (chromium nem ../browser/qutebrowser.py). Instale chromium.");
}

// ---------------------------------------------------------------------------
// Botao X (canto superior direito) -> volta ao terminal
// ---------------------------------------------------------------------------

fn launch_x_overlay() -> Option<Child> {
    // MVP: janelinha tkinter 120x40 fixa no topo-direita com botao X.
    // Gerenciada pelo Rust (spawn/kill). Troca futura: overlay nativo winit/egui.
    let py = r##"
import tkinter as tk, pathlib
root = tk.Tk()
root.title("dahook")
root.geometry("120x40+0+0")
root.attributes("-topmost", True)
# canto superior direito
root.update_idletasks()
w = root.winfo_screenwidth()
root.geometry(f"120x40+{w-140}+10")
root.overrideredirect(False)
def go_back():
    pathlib.Path("/tmp/dahook-back").write_text("back")
    root.destroy()
b = tk.Button(root, text="X dahook", command=go_back, bg="#c00", fg="white", font=("bold", 12))
b.pack(fill="both", expand=True)
root.mainloop()
"##;
    match Command::new("python3")
        .args(["-c", py])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(c) => Some(c),
        Err(e) => {
            eprintln!("[dahook] overlay X indisponivel (tkinter): {e}. Use `dahook back` ou feche o browser.");
            None
        }
    }
}

fn wait_for_back_or_close(mut browser: Option<&mut Child>) -> BackReason {
    loop {
        // X clicado? minimiza em vez de matar.
        if Path::new(BACK_FILE).exists() {
            return BackReason::Back;
        }
        if let Some(b) = browser.as_mut() {
            match b.try_wait() {
                Ok(Some(_)) => return BackReason::Closed, // usuario fechou a janela
                Ok(None) => {}
                Err(_) => return BackReason::Closed,
            }
        } else {
            thread::sleep(Duration::from_millis(200));
            return BackReason::Closed;
        }
        thread::sleep(Duration::from_millis(200));
    }
}

#[derive(PartialEq)]
enum BackReason {
    Back,
    Closed,
}

fn child_alive(child: &mut Child) -> bool {
    matches!(child.try_wait(), Ok(None))
}

/// X minimiza em vez de matar: preserva processo e abas.
/// X11: xdotool pelo PID (preciso mesmo com --class igual ao kitty).
/// Wayland: xdotool falha — mantem processo vivo e so restaura o kitty;
/// o compositor agrupa pela mesma classe `dahook`.
fn minimize_browser_window(browser: &mut Child) {
    let pid = browser.id().to_string();
    let minimized = Command::new("xdotool")
        .args(["search", "--pid", &pid, "windowminimize"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if !minimized {
        // fallback: unmap (esconde sem matar)
        let _ = Command::new("xdotool")
            .args(["search", "--pid", &pid, "windowunmap"])
            .output();
    }
    // Wayland sem xdotool: processo continua vivo em background; o kitty
    // restaurado cobre a tela e o overlay X e destruido. Proximo `dahook`
    // so traz a janela de volta (restore_browser_window).
}

fn restore_browser_window_by_pid(pid: u32) {
    let pid = pid.to_string();
    // re-mapeia caso tenha sido unmap, depois ativa
    let _ = Command::new("xdotool")
        .args(["search", "--pid", &pid, "windowmap"])
        .output();
    let _ = Command::new("xdotool")
        .args(["search", "--pid", &pid, "windowactivate"])
        .output();
}

/// `dahook` sem arg abre a home (DEFAULT_URL); com URL abre nova aba na
/// instancia existente (mesmo --user-data-dir).
/// Processo curto que delega e sai; nao rastreamos o Child.
fn open_url_in_existing_profile(url: &str) {
    let url = if url.trim().is_empty() {
        DEFAULT_URL
    } else {
        url.trim()
    };
    if let Some(chrome) = find_chromium() {
        let _ = Command::new(&chrome)
            .args([
                &format!("--user-data-dir={WEB_PROFILE}"),
                "--no-first-run",
                "--no-default-browser-check",
            ])
            .arg(url)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map(|mut c| {
                // processo delegante sai sozinho; nao bloqueia
                thread::sleep(Duration::from_millis(300));
                let _ = c.try_wait().map(|s| {
                    if s.is_none() {
                        let _ = c.kill();
                    }
                });
            });
    }
    // fallback qutebrowser: sem single-instance confiavel; mantem abas atuais.
}

fn kill_child(child: &mut Child, name: &str) {
    let _ = child.kill();
    let _ = child.wait();
    let _ = name;
}

// ---------------------------------------------------------------------------
// Trigger / modo (arquivos em /tmp)
// ---------------------------------------------------------------------------

fn wait_for_trigger() -> Result<String> {
    loop {
        if let Ok(s) = fs::read_to_string(TRIGGER_FILE) {
            let s = s.trim().to_string();
            if !s.is_empty() {
                return Ok(s);
            }
        }
        thread::sleep(Duration::from_millis(250));
    }
}

fn set_mode(mode: &str) -> Result<()> {
    let mut f = fs::File::create(MODE_FILE)?;
    writeln!(f, "{mode}")?;
    Ok(())
}

fn rm_if_exists(p: &str) {
    let _ = fs::remove_file(p);
    let _ = mtime_touch(p);
}

fn mtime_touch(_p: &str) {
    let _ = SystemTime::now();
}
