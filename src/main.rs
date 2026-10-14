//! Ferramenta de Captura para o COSMIC — clone da Ferramenta de Captura do W11.

use i18n::{tr, trf};

mod capture;
mod clip;
mod config;
mod draw;
mod editor;
mod font;
mod i18n;
mod icons;
mod image;
mod notify;
mod paths;
mod shortcuts;
mod snip;
mod theme;
mod wlimpl;

fn help() -> &'static str {
    tr("\
Snipping Tool (recorte) — W11-style screenshots for COSMIC

Usage:
  recorte                         Start a snip (shortcut: Print or Super+Shift+S)
  recorte --mode MODE             rectangle | freeform | window | screen
  recorte --delay SECONDS         Wait before freezing the screen
  recorte edit [FILE]             Open the editor (empty or with an image)
  recorte setup-shortcuts         Bind Print and Super+Shift+S to this program in COSMIC
  recorte --help                  Show this help
")
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = match args.first().map(String::as_str) {
        Some("editar" | "editor" | "--editor" | "edit") => editor::run(args[1..].to_vec()),
        Some("__clip") => match args.get(1) {
            Some(p) => clip::serve(std::path::Path::new(p)),
            None => 2,
        },
        Some("setup-shortcuts" | "configurar-atalhos") => match shortcuts::install() {
            Ok(msg) => {
                println!("{msg}");
                0
            }
            Err(e) => {
                eprintln!("recorte: {e}");
                1
            }
        },
        Some("-h" | "--help" | "--ajuda") => {
            print!("{}", help());
            0
        }
        _ => {
            let mut a = capture::Args { mode: None, delay: 0, for_editor: false };
            let mut it = args.iter();
            while let Some(arg) = it.next() {
                match arg.as_str() {
                    "--mode" | "--modo" | "-m" => match it.next().and_then(|m| config::Mode::parse(m)) {
                        Some(m) => a.mode = Some(m),
                        None => {
                            eprintln!("recorte: {}", tr("invalid mode (use rectangle, freeform, window or screen)"));
                            std::process::exit(2);
                        }
                    },
                    "--delay" | "--atraso" | "-d" | "-a" => a.delay = it.next().and_then(|d| d.parse().ok()).unwrap_or(0).min(60),
                    "--for-editor" | "--para-editor" => a.for_editor = true,
                    other => {
                        eprintln!("recorte: {}\n\n{}", trf("unknown argument: {}", &[&other]), help());
                        std::process::exit(2);
                    }
                }
            }
            capture::run(a)
        }
    };
    std::process::exit(code);
}
