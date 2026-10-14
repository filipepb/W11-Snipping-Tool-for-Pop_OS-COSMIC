//! Ferramenta de Captura para o COSMIC — clone da Ferramenta de Captura do Windows 11.

mod capture;
mod clip;
mod config;
mod draw;
mod editor;
mod font;
mod icons;
mod image;
mod notify;
mod paths;
mod shortcuts;
mod snip;
mod theme;
mod wlimpl;

const HELP: &str = "\
Ferramenta de Captura (recorte) — capturas de tela no estilo do Windows 11

Uso:
  recorte                         Inicia um recorte (atalho: Print ou Super+Shift+S)
  recorte --modo MODO             retangulo | livre | janela | tela
  recorte --atraso SEGUNDOS       Espera antes de congelar a tela
  recorte editar [ARQUIVO]        Abre o editor (vazio ou com uma imagem)
  recorte configurar-atalhos      Liga Print e Super+Shift+S a este programa no COSMIC
  recorte --ajuda                 Mostra esta ajuda
";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = match args.first().map(String::as_str) {
        Some("editar" | "editor" | "--editor" | "edit") => editor::run(args[1..].to_vec()),
        Some("__clip") => match args.get(1) {
            Some(p) => clip::serve(std::path::Path::new(p)),
            None => 2,
        },
        Some("configurar-atalhos") => match shortcuts::install() {
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
            print!("{HELP}");
            0
        }
        _ => {
            let mut a = capture::Args { mode: None, delay: 0, for_editor: false };
            let mut it = args.iter();
            while let Some(arg) = it.next() {
                match arg.as_str() {
                    "--modo" | "-m" => match it.next().and_then(|m| config::Mode::parse(m)) {
                        Some(m) => a.mode = Some(m),
                        None => {
                            eprintln!("recorte: modo inválido (use retangulo, livre, janela ou tela)");
                            std::process::exit(2);
                        }
                    },
                    "--atraso" | "-a" => a.delay = it.next().and_then(|d| d.parse().ok()).unwrap_or(0).min(60),
                    "--para-editor" => a.for_editor = true,
                    other => {
                        eprintln!("recorte: argumento desconhecido: {other}\n\n{HELP}");
                        std::process::exit(2);
                    }
                }
            }
            capture::run(a)
        }
    };
    std::process::exit(code);
}
