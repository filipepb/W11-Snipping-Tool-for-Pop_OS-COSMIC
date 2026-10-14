//! Texto simples com ttf-parser + tiny-skia (usado só nas dicas da sobreposição).

use tiny_skia::{FillRule, Paint, PathBuilder, PixmapMut, Transform};

pub struct Font {
    data: Vec<u8>,
}

fn find_font_file() -> Option<std::path::PathBuf> {
    for family in ["Open Sans", "Noto Sans", "sans-serif"] {
        let out = std::process::Command::new("fc-match")
            .args(["-f", "%{file}", family])
            .output()
            .ok()?;
        let path = std::path::PathBuf::from(String::from_utf8_lossy(&out.stdout).to_string());
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
        if path.is_file() && (ext == "ttf" || ext == "otf") {
            return Some(path);
        }
    }
    None
}

struct Outline<'a> {
    pb: &'a mut PathBuilder,
    scale: f32,
    x: f32,
    y: f32,
}

impl ttf_parser::OutlineBuilder for Outline<'_> {
    fn move_to(&mut self, x: f32, y: f32) {
        self.pb.move_to(self.x + x * self.scale, self.y - y * self.scale);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.pb.line_to(self.x + x * self.scale, self.y - y * self.scale);
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let s = self.scale;
        self.pb.quad_to(self.x + x1 * s, self.y - y1 * s, self.x + x * s, self.y - y * s);
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let s = self.scale;
        self.pb.cubic_to(
            self.x + x1 * s,
            self.y - y1 * s,
            self.x + x2 * s,
            self.y - y2 * s,
            self.x + x * s,
            self.y - y * s,
        );
    }
    fn close(&mut self) {
        self.pb.close();
    }
}

impl Font {
    pub fn load() -> Option<Font> {
        let data = std::fs::read(find_font_file()?).ok()?;
        ttf_parser::Face::parse(&data, 0).ok()?;
        Some(Font { data })
    }

    fn face(&self) -> ttf_parser::Face<'_> {
        ttf_parser::Face::parse(&self.data, 0).expect("fonte validada em load")
    }

    pub fn width(&self, text: &str, px: f32) -> f32 {
        let face = self.face();
        let scale = px / face.units_per_em() as f32;
        text.chars()
            .filter_map(|c| face.glyph_index(c))
            .map(|g| face.glyph_hor_advance(g).unwrap_or(0) as f32 * scale)
            .sum()
    }

    /// Desenha `text` com a linha de base em `y`.
    pub fn draw(&self, pm: &mut PixmapMut, text: &str, x: f32, y: f32, px: f32, paint: &Paint) {
        let face = self.face();
        let scale = px / face.units_per_em() as f32;
        let mut pb = PathBuilder::new();
        let mut pen = x;
        for c in text.chars() {
            let Some(g) = face.glyph_index(c) else { continue };
            face.outline_glyph(g, &mut Outline { pb: &mut pb, scale, x: pen, y });
            pen += face.glyph_hor_advance(g).unwrap_or(0) as f32 * scale;
        }
        if let Some(path) = pb.finish() {
            pm.fill_path(&path, paint, FillRule::Winding, Transform::identity(), None);
        }
    }

    pub fn ascent(&self, px: f32) -> f32 {
        let face = self.face();
        face.ascender() as f32 * px / face.units_per_em() as f32
    }
}
