//! Configurações persistentes em `~/.config/recorte/recorte.conf` (chave=valor).

use std::path::PathBuf;

use crate::i18n::tr;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Rect,
    Free,
    Window,
    Screen,
}

impl Mode {
    pub const ALL: [Mode; 4] = [Mode::Rect, Mode::Free, Mode::Window, Mode::Screen];

    pub fn key(self) -> &'static str {
        match self {
            Mode::Rect => "retangulo",
            Mode::Free => "livre",
            Mode::Window => "janela",
            Mode::Screen => "tela",
        }
    }

    pub fn parse(s: &str) -> Option<Mode> {
        match s {
            "retangulo" | "retângulo" | "rect" | "rectangle" => Some(Mode::Rect),
            "livre" | "forma-livre" | "free" | "freeform" => Some(Mode::Free),
            "janela" | "window" => Some(Mode::Window),
            "tela" | "tela-inteira" | "screen" | "fullscreen" => Some(Mode::Screen),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Mode::Rect => tr("Rectangle"),
            Mode::Free => tr("Freeform"),
            Mode::Window => tr("Window"),
            Mode::Screen => tr("Full screen"),
        }
    }

    pub fn tooltip(self) -> &'static str {
        match self {
            Mode::Rect => tr("Rectangle mode"),
            Mode::Free => tr("Freeform mode"),
            Mode::Window => tr("Window mode"),
            Mode::Screen => tr("Full screen mode"),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Config {
    pub mode: Mode,
    pub delay: u32,
    pub auto_save: bool,
    pub save_dir: PathBuf,
    pub auto_copy_edits: bool,
    pub open_editor: bool,
    pub notify: bool,
    pub dim: bool,
    pub ask_save: bool,
    pub border: bool,
    pub border_color: [u8; 3],
    pub border_width: u32,
    pub pen_color: [u8; 3],
    pub pen_size: f64,
    pub hl_color: [u8; 3],
    pub hl_size: f64,
    pub shape_color: [u8; 3],
    pub shape_size: f64,
    pub shape_kind: u32,
    pub shape_fill: bool,
    pub text_color: [u8; 3],
    pub text_size: f64,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            mode: Mode::Rect,
            delay: 0,
            auto_save: true,
            save_dir: crate::paths::default_save_dir(),
            auto_copy_edits: true,
            open_editor: false,
            notify: true,
            dim: true,
            ask_save: true,
            border: false,
            border_color: [0x3b, 0x7a, 0x9f],
            border_width: 2,
            pen_color: [0xe8, 0x11, 0x23],
            pen_size: 4.0,
            hl_color: [0xff, 0xeb, 0x3b],
            hl_size: 18.0,
            shape_color: [0xe8, 0x11, 0x23],
            shape_size: 4.0,
            shape_kind: 0,
            shape_fill: false,
            text_color: [0xe8, 0x11, 0x23],
            text_size: 24.0,
        }
    }
}

fn path() -> PathBuf {
    crate::paths::config_dir().join("recorte").join("recorte.conf")
}

fn parse_bool(v: &str) -> Option<bool> {
    match v {
        "true" | "1" | "sim" => Some(true),
        "false" | "0" | "nao" | "não" => Some(false),
        _ => None,
    }
}

pub fn parse_color(v: &str) -> Option<[u8; 3]> {
    let h = v.trim().trim_start_matches('#');
    if h.len() < 6 || !h.is_ascii() {
        return None;
    }
    let c = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok();
    Some([c(0)?, c(2)?, c(4)?])
}

pub fn color_hex(c: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}

impl Config {
    pub fn load() -> Config {
        let mut cfg = Config::default();
        let Ok(text) = std::fs::read_to_string(path()) else { return cfg };
        for line in text.lines() {
            let Some((k, v)) = line.split_once('=') else { continue };
            let (k, v) = (k.trim(), v.trim());
            let b = parse_bool(v);
            let f = v.parse::<f64>().ok().filter(|x| x.is_finite() && *x > 0.0);
            let c = parse_color(v);
            match k {
                "modo" => cfg.mode = Mode::parse(v).unwrap_or(cfg.mode),
                "atraso" => cfg.delay = v.parse().unwrap_or(0).min(60),
                "salvar_automaticamente" => cfg.auto_save = b.unwrap_or(cfg.auto_save),
                "pasta" if !v.is_empty() => cfg.save_dir = PathBuf::from(v),
                "copiar_alteracoes" => cfg.auto_copy_edits = b.unwrap_or(cfg.auto_copy_edits),
                "abrir_editor" => cfg.open_editor = b.unwrap_or(cfg.open_editor),
                "notificacao" => cfg.notify = b.unwrap_or(cfg.notify),
                "escurecer" => cfg.dim = b.unwrap_or(cfg.dim),
                "perguntar_salvar" => cfg.ask_save = b.unwrap_or(cfg.ask_save),
                "borda" => cfg.border = b.unwrap_or(cfg.border),
                "borda_cor" => cfg.border_color = c.unwrap_or(cfg.border_color),
                "borda_largura" => cfg.border_width = v.parse().unwrap_or(cfg.border_width).clamp(1, 32),
                "caneta_cor" => cfg.pen_color = c.unwrap_or(cfg.pen_color),
                "caneta_tamanho" => cfg.pen_size = f.unwrap_or(cfg.pen_size),
                "marca_texto_cor" => cfg.hl_color = c.unwrap_or(cfg.hl_color),
                "marca_texto_tamanho" => cfg.hl_size = f.unwrap_or(cfg.hl_size),
                "forma_cor" => cfg.shape_color = c.unwrap_or(cfg.shape_color),
                "forma_tamanho" => cfg.shape_size = f.unwrap_or(cfg.shape_size),
                "forma_tipo" => cfg.shape_kind = v.parse().unwrap_or(0).min(3),
                "forma_preencher" => cfg.shape_fill = b.unwrap_or(cfg.shape_fill),
                "texto_cor" => cfg.text_color = c.unwrap_or(cfg.text_color),
                "texto_tamanho" => cfg.text_size = f.unwrap_or(cfg.text_size),
                _ => {}
            }
        }
        cfg
    }

    pub fn save(&self) {
        let p = path();
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let text = format!(
            "# Ferramenta de Captura (recorte)\n\
             modo={}\natraso={}\nsalvar_automaticamente={}\npasta={}\ncopiar_alteracoes={}\n\
             abrir_editor={}\nnotificacao={}\nescurecer={}\nperguntar_salvar={}\nborda={}\n\
             borda_cor={}\nborda_largura={}\ncaneta_cor={}\ncaneta_tamanho={}\nmarca_texto_cor={}\n\
             marca_texto_tamanho={}\nforma_cor={}\nforma_tamanho={}\nforma_tipo={}\nforma_preencher={}\n\
             texto_cor={}\ntexto_tamanho={}\n",
            self.mode.key(),
            self.delay,
            self.auto_save,
            self.save_dir.display(),
            self.auto_copy_edits,
            self.open_editor,
            self.notify,
            self.dim,
            self.ask_save,
            self.border,
            color_hex(self.border_color),
            self.border_width,
            color_hex(self.pen_color),
            self.pen_size,
            color_hex(self.hl_color),
            self.hl_size,
            color_hex(self.shape_color),
            self.shape_size,
            self.shape_kind,
            self.shape_fill,
            color_hex(self.text_color),
            self.text_size,
        );
        // Escrita atômica: nunca deixa um arquivo pela metade.
        let tmp = p.with_extension("conf.tmp");
        if std::fs::write(&tmp, text).is_ok() {
            let _ = std::fs::rename(&tmp, &p);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modos_e_cores() {
        for m in Mode::ALL {
            assert_eq!(Mode::parse(m.key()), Some(m));
        }
        assert_eq!(parse_color("#3B7A9F"), Some([0x3b, 0x7a, 0x9f]));
        assert_eq!(parse_color("zz"), None);
        assert_eq!(color_hex([1, 2, 255]), "#0102ff");
    }
}
