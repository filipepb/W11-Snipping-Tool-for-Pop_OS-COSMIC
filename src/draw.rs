//! Primitivas de desenho da sobreposição.
//!
//! Os buffers estão em BGRA, e o tiny-skia escreve RGBA; por isso `color()`
//! troca vermelho e azul, mantendo a cópia direta para o wl_shm.

use tiny_skia::{Color, LineCap, LineJoin, Paint, Path, PathBuilder, PixmapMut, Rect, Stroke, Transform};

pub fn color(r: u8, g: u8, b: u8, a: u8) -> Color {
    Color::from_rgba8(b, g, r, a)
}

pub fn paint(c: Color) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color(c);
    p.anti_alias = true;
    p
}

pub fn stroke(width: f32) -> Stroke {
    Stroke { width, line_cap: LineCap::Round, line_join: LineJoin::Round, ..Stroke::default() }
}

pub fn rrect(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<Path> {
    let r = r.min(w / 2.0).min(h / 2.0).max(0.0);
    let k = 0.552_284_8 * r;
    let mut pb = PathBuilder::new();
    pb.move_to(x + r, y);
    pb.line_to(x + w - r, y);
    pb.cubic_to(x + w - r + k, y, x + w, y + r - k, x + w, y + r);
    pb.line_to(x + w, y + h - r);
    pb.cubic_to(x + w, y + h - r + k, x + w - r + k, y + h, x + w - r, y + h);
    pb.line_to(x + r, y + h);
    pb.cubic_to(x + r - k, y + h, x, y + h - r + k, x, y + h - r);
    pb.line_to(x, y + r);
    pb.cubic_to(x, y + r - k, x + r - k, y, x + r, y);
    pb.close();
    pb.finish()
}

pub fn fill_rrect(pm: &mut PixmapMut, x: f32, y: f32, w: f32, h: f32, r: f32, c: Color) {
    if let Some(p) = rrect(x, y, w, h, r) {
        pm.fill_path(&p, &paint(c), tiny_skia::FillRule::Winding, Transform::identity(), None);
    }
}

pub fn stroke_rrect(pm: &mut PixmapMut, x: f32, y: f32, w: f32, h: f32, r: f32, width: f32, c: Color) {
    if let Some(p) = rrect(x, y, w, h, r) {
        pm.stroke_path(&p, &paint(c), &stroke(width), Transform::identity(), None);
    }
}

pub fn fill_rect(pm: &mut PixmapMut, x: f32, y: f32, w: f32, h: f32, c: Color) {
    if let Some(r) = Rect::from_xywh(x, y, w, h) {
        pm.fill_rect(r, &paint(c), Transform::identity(), None);
    }
}

/// Sombra suave aproximada por camadas concêntricas.
pub fn shadow(pm: &mut PixmapMut, x: f32, y: f32, w: f32, h: f32, r: f32, s: f32) {
    let layers = 8;
    for i in 0..layers {
        let grow = (layers - i) as f32 * 1.6 * s;
        let a = (3 + i * 2) as u8;
        fill_rrect(pm, x - grow, y - grow + 3.0 * s, w + 2.0 * grow, h + 2.0 * grow, r + grow, color(0, 0, 0, a));
    }
}

/// Ícones da barra, desenhados num quadrado de 20×20 unidades.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Rect,
    Free,
    Window,
    Screen,
    Close,
}

pub fn icon(pm: &mut PixmapMut, which: Icon, cx: f32, cy: f32, s: f32, c: Color) {
    let t = Transform::from_row(s, 0.0, 0.0, s, cx - 10.0 * s, cy - 10.0 * s);
    let st = stroke(1.5);
    let p = paint(c);
    let mut pb = PathBuilder::new();
    match which {
        Icon::Rect => {
            // Retângulo tracejado com cantos marcados.
            for (x0, y0, dx, dy) in [(3.0, 4.0, 1.0, 1.0), (17.0, 4.0, -1.0, 1.0), (3.0, 16.0, 1.0, -1.0), (17.0, 16.0, -1.0, -1.0)] {
                pb.move_to(x0, y0 + 3.0 * dy);
                pb.line_to(x0, y0);
                pb.line_to(x0 + 3.0 * dx, y0);
            }
            for (a, b) in [((8.5, 4.0), (11.5, 4.0)), ((8.5, 16.0), (11.5, 16.0)), ((3.0, 8.8), (3.0, 11.2)), ((17.0, 8.8), (17.0, 11.2))] {
                pb.move_to(a.0, a.1);
                pb.line_to(b.0, b.1);
            }
        }
        Icon::Free => {
            // Laço de forma livre.
            pb.move_to(6.0, 15.5);
            pb.cubic_to(2.0, 13.0, 2.0, 6.0, 7.5, 4.2);
            pb.cubic_to(12.5, 2.6, 18.0, 4.5, 17.6, 8.6);
            pb.cubic_to(17.2, 12.4, 12.0, 13.6, 8.6, 12.6);
            pb.cubic_to(6.6, 12.0, 6.0, 10.2, 7.4, 9.4);
            pb.move_to(6.0, 15.5);
            pb.cubic_to(7.2, 16.6, 6.4, 18.0, 5.0, 17.6);
        }
        Icon::Window => {
            if let Some(r) = rrect(2.5, 3.5, 15.0, 13.0, 2.0) {
                pm.stroke_path(&r, &p, &st, t, None);
            }
            pb.move_to(2.5, 7.0);
            pb.line_to(17.5, 7.0);
            pb.move_to(13.0, 5.25);
            pb.line_to(13.01, 5.25);
            pb.move_to(15.2, 5.25);
            pb.line_to(15.21, 5.25);
        }
        Icon::Screen => {
            if let Some(r) = rrect(2.0, 3.0, 16.0, 11.0, 1.8) {
                pm.stroke_path(&r, &p, &st, t, None);
            }
            pb.move_to(10.0, 14.0);
            pb.line_to(10.0, 17.0);
            pb.move_to(6.5, 17.0);
            pb.line_to(13.5, 17.0);
        }
        Icon::Close => {
            pb.move_to(5.0, 5.0);
            pb.line_to(15.0, 15.0);
            pb.move_to(15.0, 5.0);
            pb.line_to(5.0, 15.0);
        }
    }
    if let Some(path) = pb.finish() {
        pm.stroke_path(&path, &p, &st, t, None);
    }
}
