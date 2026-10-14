//! Fluxo completo de um recorte: congela, seleciona, salva, copia e notifica.

use std::os::fd::AsRawFd;
use std::time::Duration;

use crate::config::{Config, Mode};
use crate::draw::color;
use crate::snip::{self, App, Outcome};

pub struct Args {
    pub mode: Option<Mode>,
    pub delay: u32,
    pub for_editor: bool,
}

/// Impede duas sobreposições simultâneas (tecla pressionada duas vezes).
fn lock() -> Option<std::fs::File> {
    let f = std::fs::File::create(crate::paths::runtime_dir().join("captura.lock")).ok()?;
    let r = unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    (r == 0).then_some(f)
}

fn add_border(img: &mut crate::image::Image, rgb: [u8; 3], width: u32) {
    let w = width as f32;
    let (iw, ih) = (img.w as f32, img.h as f32);
    if let Some(mut pm) = img.pixmap_mut() {
        crate::draw::stroke_rrect(&mut pm, w / 2.0, w / 2.0, iw - w, ih - w, 0.0, w, color(rgb[0], rgb[1], rgb[2], 255));
    }
}

pub fn run(args: Args) -> i32 {
    let Some(lock) = lock() else { return 0 };
    crate::paths::prune_runtime_dir();
    let cfg = Config::load();
    let mode = args.mode.unwrap_or(cfg.mode);
    if args.delay > 0 {
        std::thread::sleep(Duration::from_secs(args.delay as u64));
    }

    let (conn, mut queue, mut app) = match App::connect(mode, cfg.dim) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("recorte: {e}");
            crate::notify::error(&e);
            return 2;
        }
    };
    if let Err(e) = app.capture_outputs(&mut queue) {
        eprintln!("recorte: {e}");
        crate::notify::error(&e);
        return 2;
    }
    let outcome = if mode == Mode::Screen && app.outputs.len() == 1 {
        Outcome::Output(0)
    } else {
        match snip::run_overlay(&conn, &mut queue, &mut app) {
            Ok(o) => o,
            Err(e) => {
                eprintln!("recorte: {e}");
                return 2;
            }
        }
    };
    app.destroy_surfaces();
    let _ = conn.flush();
    // A trava só cobre a sobreposição: um novo recorte pode começar enquanto
    // este processo ainda salva, copia ou espera o clique na notificação.
    drop(lock);

    // Lembra o modo escolhido na barra, como o Windows (não o de --modo).
    if app.mode != mode {
        let mut fresh = Config::load();
        fresh.mode = app.mode;
        fresh.save();
    }
    let Some(mut img) = app.result_image(&outcome) else { return 1 };
    drop(app);
    drop(queue);
    drop(conn);

    if cfg.border {
        add_border(&mut img, cfg.border_color, cfg.border_width);
    }
    let png = match img.encode_png() {
        Ok(p) => p,
        Err(e) => {
            crate::notify::error(&format!("falha ao codificar PNG: {e}"));
            return 2;
        }
    };
    drop(img);
    let name = crate::paths::screenshot_file_name();
    let mut path = if cfg.auto_save {
        crate::paths::unique_path(&cfg.save_dir, &name)
    } else {
        crate::paths::runtime_dir().join(&name)
    };
    if let Err(e) = crate::image::write_atomic(&path, &png) {
        // Pasta inacessível: mantém a captura no diretório temporário.
        eprintln!("recorte: não foi possível salvar em {}: {e}", path.display());
        path = crate::paths::runtime_dir().join(&name);
        if crate::image::write_atomic(&path, &png).is_err() {
            return 2;
        }
    }
    drop(png);
    if let Err(e) = crate::clip::copy_file(&path) {
        eprintln!("recorte: falha ao copiar: {e}");
    }
    if args.for_editor {
        println!("{}", path.display());
        return 0;
    }
    if cfg.open_editor {
        crate::notify::spawn_editor(Some(&path), None);
    } else if cfg.notify {
        crate::notify::show_and_wait(&path);
    }
    0
}
