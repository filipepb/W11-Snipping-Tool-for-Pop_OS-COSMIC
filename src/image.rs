//! Imagem em memória no formato do wl_shm ARGB8888 (bytes BGRA, alfa pré-multiplicado).

use std::io::Write;
use std::path::Path;

#[derive(Clone)]
pub struct Image {
    pub w: u32,
    pub h: u32,
    pub data: Vec<u8>,
}

impl Image {
    pub fn new(w: u32, h: u32) -> Image {
        Image { w, h, data: vec![0; (w as usize) * (h as usize) * 4] }
    }

    #[inline]
    pub fn pixel(&self, x: i32, y: i32) -> Option<[u8; 4]> {
        if x < 0 || y < 0 || x as u32 >= self.w || y as u32 >= self.h {
            return None;
        }
        let i = (y as usize * self.w as usize + x as usize) * 4;
        Some([self.data[i], self.data[i + 1], self.data[i + 2], self.data[i + 3]])
    }

    pub fn pixmap(&self) -> Option<tiny_skia::PixmapRef<'_>> {
        tiny_skia::PixmapRef::from_bytes(&self.data, self.w, self.h)
    }

    pub fn pixmap_mut(&mut self) -> Option<tiny_skia::PixmapMut<'_>> {
        tiny_skia::PixmapMut::from_bytes(&mut self.data, self.w, self.h)
    }

    /// Converte para RGBA não pré-multiplicado.
    pub fn to_rgba(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.data.len());
        for p in self.data.chunks_exact(4) {
            let (b, g, r, a) = (p[0] as u32, p[1] as u32, p[2] as u32, p[3] as u32);
            if a == 255 {
                out.extend_from_slice(&[r as u8, g as u8, b as u8, 255]);
            } else if a == 0 {
                out.extend_from_slice(&[0, 0, 0, 0]);
            } else {
                let un = |c: u32| ((c * 255 + a / 2) / a).min(255) as u8;
                out.extend_from_slice(&[un(r), un(g), un(b), a as u8]);
            }
        }
        out
    }

    pub fn has_alpha(&self) -> bool {
        self.data.chunks_exact(4).any(|p| p[3] != 255)
    }

    pub fn encode_png(&self) -> Result<Vec<u8>, png::EncodingError> {
        let mut out = Vec::with_capacity(self.data.len() / 3);
        {
            let mut enc = png::Encoder::new(&mut out, self.w, self.h);
            let alpha = self.has_alpha();
            enc.set_color(if alpha { png::ColorType::Rgba } else { png::ColorType::Rgb });
            enc.set_depth(png::BitDepth::Eight);
            enc.set_compression(png::Compression::Fast);
            let mut writer = enc.write_header()?;
            let rgba = self.to_rgba();
            if alpha {
                writer.write_image_data(&rgba)?;
            } else {
                let rgb: Vec<u8> = rgba.chunks_exact(4).flat_map(|p| [p[0], p[1], p[2]]).collect();
                writer.write_image_data(&rgb)?;
            }
        }
        Ok(out)
    }
}

/// Grava bytes de forma atômica (arquivo temporário + rename).
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("png.part");
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_data()?;
    }
    std::fs::rename(&tmp, path)
}
