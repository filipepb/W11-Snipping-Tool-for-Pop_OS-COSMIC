//! Editor de recortes (caneta, marca-texto, borracha, formas, texto, recorte,
//! desfazer/refazer, zoom, salvar e copiar), no estilo da Ferramenta de Captura.

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use adw::prelude::*;
use gtk::{cairo, gdk, gio, glib, graphene, pango};

use crate::config::{color_hex, Config, Mode};
use crate::notify::APP_ID;

const MARGIN: f64 = 32.0;
const PEN_COLORS: &[[u8; 3]] = &[
    [0x00, 0x00, 0x00],
    [0xff, 0xff, 0xff],
    [0x7a, 0x7a, 0x7a],
    [0xe8, 0x11, 0x23],
    [0xf7, 0x63, 0x0c],
    [0xff, 0xb9, 0x00],
    [0x16, 0xc6, 0x0c],
    [0x00, 0xb7, 0xc3],
    [0x00, 0x63, 0xb1],
    [0x88, 0x6c, 0xe4],
    [0xe3, 0x00, 0x8c],
    [0x8e, 0x56, 0x2e],
];
const HL_COLORS: &[[u8; 3]] = &[
    [0xff, 0xeb, 0x3b],
    [0x76, 0xff, 0x03],
    [0x18, 0xff, 0xff],
    [0xff, 0x40, 0xff],
    [0xff, 0x9e, 0x22],
    [0xff, 0x6b, 0x6b],
];

// ---------------------------------------------------------------------------
// Documento

#[derive(Clone, Copy, Debug, PartialEq)]
struct Rect {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

impl Rect {
    fn from_points(a: (f64, f64), b: (f64, f64)) -> Rect {
        Rect { x: a.0.min(b.0), y: a.1.min(b.1), w: (a.0 - b.0).abs(), h: (a.1 - b.1).abs() }
    }
    fn contains(&self, p: (f64, f64)) -> bool {
        p.0 >= self.x && p.1 >= self.y && p.0 <= self.x + self.w && p.1 <= self.y + self.h
    }
}

type Rgba = [f64; 4];

fn rgba(c: [u8; 3]) -> Rgba {
    [c[0] as f64 / 255.0, c[1] as f64 / 255.0, c[2] as f64 / 255.0, 1.0]
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ShapeKind {
    Rect,
    Ellipse,
    Line,
    Arrow,
}

impl ShapeKind {
    const ALL: [ShapeKind; 4] = [ShapeKind::Rect, ShapeKind::Ellipse, ShapeKind::Line, ShapeKind::Arrow];
    fn from_index(i: u32) -> ShapeKind {
        Self::ALL[(i as usize).min(3)]
    }
    fn index(self) -> u32 {
        Self::ALL.iter().position(|k| *k == self).unwrap_or(0) as u32
    }
    fn icon(self) -> &'static str {
        match self {
            ShapeKind::Rect => "recorte-rectangle-symbolic",
            ShapeKind::Ellipse => "recorte-ellipse-symbolic",
            ShapeKind::Line => "recorte-line-symbolic",
            ShapeKind::Arrow => "recorte-arrow-symbolic",
        }
    }
    fn label(self) -> &'static str {
        match self {
            ShapeKind::Rect => "Retângulo",
            ShapeKind::Ellipse => "Elipse",
            ShapeKind::Line => "Linha",
            ShapeKind::Arrow => "Seta",
        }
    }
}

#[derive(Clone, Debug)]
enum Item {
    Stroke { pts: Vec<(f64, f64)>, color: Rgba, width: f64, hl: bool },
    Shape { kind: ShapeKind, a: (f64, f64), b: (f64, f64), color: Rgba, width: f64, fill: bool },
    Text { x: f64, y: f64, text: String, color: Rgba, size: f64, w: f64, h: f64 },
}

#[derive(Clone)]
struct DocState {
    items: Vec<Item>,
    crop: Rect,
}

struct Doc {
    base: cairo::ImageSurface,
    w: f64,
    h: f64,
    state: DocState,
    undo: Vec<DocState>,
    redo: Vec<DocState>,
    dirty: bool,
}

impl Doc {
    fn push_undo(&mut self) {
        self.undo.push(self.state.clone());
        self.redo.clear();
        self.dirty = true;
    }
}

fn load_surface(path: &Path) -> Result<cairo::ImageSurface, String> {
    let texture = gdk::Texture::from_filename(path).map_err(|e| e.to_string())?;
    let (w, h) = (texture.width(), texture.height());
    let stride = cairo::Format::ARgb32.stride_for_width(w as u32).map_err(|e| e.to_string())?;
    let mut data = vec![0u8; stride as usize * h as usize];
    // O formato padrão do download é BGRA pré-multiplicado — o mesmo do cairo ARGB32.
    texture.download(&mut data, stride as usize);
    cairo::ImageSurface::create_for_data(data, cairo::Format::ARgb32, w, h, stride).map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// Desenho dos itens

fn set_color(cr: &cairo::Context, c: Rgba) {
    cr.set_source_rgba(c[0], c[1], c[2], c[3]);
}

/// Traço suavizado por curvas entre os pontos médios.
fn trace_stroke(cr: &cairo::Context, pts: &[(f64, f64)]) {
    if pts.is_empty() {
        return;
    }
    cr.move_to(pts[0].0, pts[0].1);
    if pts.len() == 1 {
        cr.line_to(pts[0].0 + 0.01, pts[0].1);
        return;
    }
    let mut cur = pts[0];
    for i in 1..pts.len() - 1 {
        let q = pts[i];
        let mid = ((q.0 + pts[i + 1].0) / 2.0, (q.1 + pts[i + 1].1) / 2.0);
        let c1 = (cur.0 + 2.0 / 3.0 * (q.0 - cur.0), cur.1 + 2.0 / 3.0 * (q.1 - cur.1));
        let c2 = (mid.0 + 2.0 / 3.0 * (q.0 - mid.0), mid.1 + 2.0 / 3.0 * (q.1 - mid.1));
        cr.curve_to(c1.0, c1.1, c2.0, c2.1, mid.0, mid.1);
        cur = mid;
    }
    let last = pts[pts.len() - 1];
    cr.line_to(last.0, last.1);
}

fn text_layout(cr: &cairo::Context, text: &str, size: f64) -> pango::Layout {
    let layout = pangocairo::functions::create_layout(cr);
    let mut fd = pango::FontDescription::from_string("Sans");
    fd.set_absolute_size(size * pango::SCALE as f64);
    layout.set_font_description(Some(&fd));
    layout.set_text(text);
    layout
}

fn measure_text(text: &str, size: f64) -> (f64, f64) {
    let Ok(surf) = cairo::ImageSurface::create(cairo::Format::ARgb32, 1, 1) else { return (0.0, 0.0) };
    let Ok(cr) = cairo::Context::new(&surf) else { return (0.0, 0.0) };
    let (w, h) = text_layout(&cr, text, size).pixel_size();
    (w as f64, h as f64)
}

fn draw_item(cr: &cairo::Context, item: &Item) {
    let _ = cr.save();
    match item {
        Item::Stroke { pts, color, width, hl } => {
            cr.set_line_width(*width);
            cr.set_line_join(cairo::LineJoin::Round);
            if *hl {
                // Marca-texto: cor multiplicada, o texto por baixo continua legível.
                cr.push_group();
                cr.set_line_cap(cairo::LineCap::Square);
                set_color(cr, *color);
                trace_stroke(cr, pts);
                let _ = cr.stroke();
                let _ = cr.pop_group_to_source();
                cr.set_operator(cairo::Operator::Multiply);
                let _ = cr.paint();
            } else {
                cr.set_line_cap(cairo::LineCap::Round);
                set_color(cr, *color);
                trace_stroke(cr, pts);
                let _ = cr.stroke();
            }
        }
        Item::Shape { kind, a, b, color, width, fill } => {
            set_color(cr, *color);
            cr.set_line_width(*width);
            cr.set_line_join(cairo::LineJoin::Round);
            cr.set_line_cap(cairo::LineCap::Round);
            let r = Rect::from_points(*a, *b);
            match kind {
                ShapeKind::Rect => {
                    cr.rectangle(r.x, r.y, r.w, r.h);
                    stroke_or_fill(cr, *fill);
                }
                ShapeKind::Ellipse => {
                    if r.w > 0.0 && r.h > 0.0 {
                        let _ = cr.save();
                        cr.translate(r.x + r.w / 2.0, r.y + r.h / 2.0);
                        cr.scale(r.w / 2.0, r.h / 2.0);
                        cr.arc(0.0, 0.0, 1.0, 0.0, std::f64::consts::TAU);
                        let _ = cr.restore();
                        stroke_or_fill(cr, *fill);
                    }
                }
                ShapeKind::Line => {
                    cr.move_to(a.0, a.1);
                    cr.line_to(b.0, b.1);
                    let _ = cr.stroke();
                }
                ShapeKind::Arrow => {
                    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
                    let len = dx.hypot(dy);
                    if len > 0.5 {
                        let head = (width * 3.5 + 6.0).min(len * 0.6);
                        let (ux, uy) = (dx / len, dy / len);
                        let base = (b.0 - ux * head * 0.8, b.1 - uy * head * 0.8);
                        cr.move_to(a.0, a.1);
                        cr.line_to(base.0, base.1);
                        let _ = cr.stroke();
                        let (px, py) = (-uy, ux);
                        let half = head * 0.55;
                        cr.move_to(b.0, b.1);
                        cr.line_to(b.0 - ux * head + px * half, b.1 - uy * head + py * half);
                        cr.line_to(b.0 - ux * head - px * half, b.1 - uy * head - py * half);
                        cr.close_path();
                        let _ = cr.fill();
                    }
                }
            }
        }
        Item::Text { x, y, text, color, size, .. } => {
            set_color(cr, *color);
            let layout = text_layout(cr, text, *size);
            cr.move_to(*x, *y);
            pangocairo::functions::show_layout(cr, &layout);
        }
    }
    let _ = cr.restore();
}

fn stroke_or_fill(cr: &cairo::Context, fill: bool) {
    if fill {
        let _ = cr.fill();
    } else {
        let _ = cr.stroke();
    }
}

fn seg_dist(p: (f64, f64), a: (f64, f64), b: (f64, f64)) -> f64 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let l2 = dx * dx + dy * dy;
    let t = if l2 == 0.0 { 0.0 } else { (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / l2).clamp(0.0, 1.0) };
    (p.0 - a.0 - t * dx).hypot(p.1 - a.1 - t * dy)
}

fn poly_hit(pts: &[(f64, f64)], p: (f64, f64), r: f64) -> bool {
    if pts.len() == 1 {
        return (pts[0].0 - p.0).hypot(pts[0].1 - p.1) <= r;
    }
    pts.windows(2).any(|w| seg_dist(p, w[0], w[1]) <= r)
}

fn item_hit(item: &Item, p: (f64, f64), r: f64) -> bool {
    match item {
        Item::Stroke { pts, width, .. } => poly_hit(pts, p, r + width / 2.0),
        Item::Shape { kind, a, b, width, fill, .. } => {
            let rr = r + width / 2.0;
            let rect = Rect::from_points(*a, *b);
            match kind {
                ShapeKind::Line | ShapeKind::Arrow => seg_dist(p, *a, *b) <= rr,
                ShapeKind::Rect => {
                    if *fill && rect.contains(p) {
                        return true;
                    }
                    let c = [(rect.x, rect.y), (rect.x + rect.w, rect.y), (rect.x + rect.w, rect.y + rect.h), (rect.x, rect.y + rect.h), (rect.x, rect.y)];
                    poly_hit(&c, p, rr)
                }
                ShapeKind::Ellipse => {
                    let (cx, cy, rx, ry) = (rect.x + rect.w / 2.0, rect.y + rect.h / 2.0, rect.w / 2.0, rect.h / 2.0);
                    if *fill && rx > 0.0 && ry > 0.0 && ((p.0 - cx) / rx).powi(2) + ((p.1 - cy) / ry).powi(2) <= 1.0 {
                        return true;
                    }
                    let pts: Vec<(f64, f64)> = (0..=48)
                        .map(|i| {
                            let t = i as f64 / 48.0 * std::f64::consts::TAU;
                            (cx + rx * t.cos(), cy + ry * t.sin())
                        })
                        .collect();
                    poly_hit(&pts, p, rr)
                }
            }
        }
        Item::Text { x, y, w, h, .. } => Rect { x: x - r, y: y - r, w: w + 2.0 * r, h: h + 2.0 * r }.contains(p),
    }
}

// ---------------------------------------------------------------------------
// Editor

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Tool {
    Pen,
    Highlighter,
    Eraser,
    Shape,
    Text,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Handle {
    N,
    S,
    E,
    W,
    NE,
    NW,
    SE,
    SW,
    Move,
    New,
}

struct CropEdit {
    rect: Rect,
    drag: Option<(Handle, Rect, (f64, f64))>,
}

struct TextEdit {
    view: gtk::TextView,
    x: f64,
    y: f64,
}

struct Editor {
    window: adw::ApplicationWindow,
    toasts: adw::ToastOverlay,
    stack: gtk::Stack,
    title_stack: gtk::Stack,
    overlay: gtk::Overlay,
    scroller: gtk::ScrolledWindow,
    area: gtk::DrawingArea,
    zoom_label: gtk::Label,
    mode_icon: gtk::Image,
    delay_label: gtk::Label,
    tool_buttons: RefCell<Vec<(Tool, gtk::Button, gtk::Box)>>,
    shape_kind_buttons: RefCell<Vec<(ShapeKind, gtk::ToggleButton)>>,
    shape_button_icon: gtk::Image,
    text_css: gtk::CssProvider,

    doc: RefCell<Option<Doc>>,
    path: RefCell<Option<PathBuf>>,
    cfg: RefCell<Config>,
    tool: Cell<Tool>,
    zoom: Cell<f64>,
    fit: Cell<bool>,
    last_viewport: Cell<(i32, i32)>,
    current: RefCell<Option<Item>>,
    drag_origin: Cell<(f64, f64)>,
    erase_pushed: Cell<bool>,
    crop: RefCell<Option<CropEdit>>,
    text: RefCell<Option<TextEdit>>,
    hover: Cell<Option<(f64, f64)>>,
    copy_timer: RefCell<Option<glib::SourceId>>,
    pending_scroll: Cell<Option<(f64, f64)>>,
    sized_for_doc: Cell<bool>,
}

thread_local! {
    static EDITOR: RefCell<Option<Rc<Editor>>> = const { RefCell::new(None) };
}

pub fn run(files: Vec<String>) -> i32 {
    let app = adw::Application::builder()
        .application_id(APP_ID)
        .flags(gio::ApplicationFlags::HANDLES_OPEN)
        .build();
    app.connect_startup(|app| {
        adw::StyleManager::default().set_color_scheme(adw::ColorScheme::PreferDark);
        setup_style();
        setup_accels(app);
    });
    app.connect_activate(|app| {
        Editor::get(app).window.present();
    });
    app.connect_open(|app, files, _| {
        let ed = Editor::get(app);
        if let Some(path) = files.first().and_then(|f| f.path()) {
            ed.load_path(&path);
        }
        ed.window.present();
    });
    let mut argv = vec!["recorte".to_string()];
    argv.extend(files);
    let code = app.run_with_args(&argv);
    if code == glib::ExitCode::SUCCESS {
        0
    } else {
        1
    }
}

fn setup_style() {
    let Some(display) = gdk::Display::default() else { return };
    let theme = gtk::IconTheme::for_display(&display);
    theme.add_search_path(crate::icons::ensure_installed());
    let accent = crate::theme::accent();
    let css = gtk::CssProvider::new();
    css.load_from_string(&format!(
        "
        .canvas {{ background: alpha(black, 0.18); }}
        .tool-button {{ min-width: 36px; min-height: 30px; padding: 2px 6px; border-radius: 6px; }}
        .tool-button.active {{ background: alpha(currentColor, 0.12); }}
        .swatch-bar {{ min-height: 3px; min-width: 16px; border-radius: 2px; margin-top: 2px; }}
        .color-swatch {{ min-width: 22px; min-height: 22px; padding: 0; border-radius: 999px;
                         border: 1px solid alpha(white, 0.25); }}
        .color-swatch.selected {{ box-shadow: 0 0 0 2px {accent}; }}
        .zoom-label {{ font-feature-settings: 'tnum'; min-width: 48px; }}
        .empty-hint {{ font-size: 15px; }}
        .new-button {{ padding-left: 10px; padding-right: 12px; }}
        ",
        accent = color_hex(accent)
    ));
    gtk::style_context_add_provider_for_display(&display, &css, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);
}

fn setup_accels(app: &adw::Application) {
    for (action, keys) in [
        ("win.new", &["<Ctrl>n"][..]),
        ("win.open", &["<Ctrl>o"]),
        ("win.save", &["<Ctrl>s"]),
        ("win.copy", &["<Ctrl>c"]),
        ("win.undo", &["<Ctrl>z"]),
        ("win.redo", &["<Ctrl>y", "<Ctrl><Shift>z"]),
        ("win.zoom-in", &["<Ctrl>plus", "<Ctrl>equal", "<Ctrl>KP_Add"]),
        ("win.zoom-out", &["<Ctrl>minus", "<Ctrl>KP_Subtract"]),
        ("win.zoom-fit", &["<Ctrl>0"]),
        ("win.settings", &["<Ctrl>comma"]),
    ] {
        app.set_accels_for_action(action, keys);
    }
}

fn icon_button(icon: &str, tooltip: &str) -> gtk::Button {
    let b = gtk::Button::from_icon_name(icon);
    b.set_tooltip_text(Some(tooltip));
    b.add_css_class("flat");
    b
}

impl Editor {
    fn get(app: &adw::Application) -> Rc<Editor> {
        if let Some(ed) = EDITOR.with(|e| e.borrow().clone()) {
            return ed;
        }
        let ed = Editor::build(app);
        EDITOR.with(|e| *e.borrow_mut() = Some(ed.clone()));
        ed
    }

    fn build(app: &adw::Application) -> Rc<Editor> {
        let cfg = Config::load();
        let window = adw::ApplicationWindow::builder()
            .application(app)
            .title("Ferramenta de Captura")
            .icon_name(APP_ID)
            .default_width(760)
            .default_height(420)
            .build();

        let header = adw::HeaderBar::new();
        // "+ Novo", modo e atraso: à esquerda, como no W11.
        let new_btn = gtk::Button::new();
        let nb = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        nb.append(&gtk::Image::from_icon_name("list-add-symbolic"));
        nb.append(&gtk::Label::new(Some("Novo")));
        new_btn.set_child(Some(&nb));
        new_btn.add_css_class("suggested-action");
        new_btn.add_css_class("new-button");
        new_btn.set_action_name(Some("win.new"));
        new_btn.set_tooltip_text(Some("Novo recorte (Ctrl+N)"));
        header.pack_start(&new_btn);

        let mode_icon = gtk::Image::from_icon_name(mode_icon_name(cfg.mode));
        let mode_btn = gtk::MenuButton::new();
        let mb = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        mb.append(&mode_icon);
        mb.append(&gtk::Image::from_icon_name("pan-down-symbolic"));
        mode_btn.set_child(Some(&mb));
        mode_btn.set_tooltip_text(Some("Modo de recorte"));
        mode_btn.add_css_class("flat");
        header.pack_start(&mode_btn);

        let delay_label = gtk::Label::new(Some(&delay_text(cfg.delay)));
        let delay_btn = gtk::MenuButton::new();
        let db = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        db.append(&gtk::Image::from_icon_name("recorte-timer-symbolic"));
        db.append(&delay_label);
        db.append(&gtk::Image::from_icon_name("pan-down-symbolic"));
        delay_btn.set_child(Some(&db));
        delay_btn.set_tooltip_text(Some("Atraso do recorte"));
        delay_btn.add_css_class("flat");
        header.pack_start(&delay_btn);

        // Ações à direita.
        let menu = gio::Menu::new();
        menu.append(Some("Abrir arquivo…"), Some("win.open"));
        menu.append(Some("Salvar como…"), Some("win.save"));
        let sec = gio::Menu::new();
        sec.append(Some("Configurações"), Some("win.settings"));
        sec.append(Some("Sobre a Ferramenta de Captura"), Some("win.about"));
        menu.append_section(None, &sec);
        let more = gtk::MenuButton::builder().icon_name("view-more-symbolic").menu_model(&menu).tooltip_text("Ver mais").build();
        more.add_css_class("flat");
        header.pack_end(&more);
        let copy_btn = icon_button("edit-copy-symbolic", "Copiar (Ctrl+C)");
        copy_btn.set_action_name(Some("win.copy"));
        header.pack_end(&copy_btn);
        let save_btn = icon_button("document-save-symbolic", "Salvar como (Ctrl+S)");
        save_btn.set_action_name(Some("win.save"));
        header.pack_end(&save_btn);

        let zoom_label = gtk::Label::new(Some("100%"));
        zoom_label.add_css_class("zoom-label");
        let zoom_btn = gtk::MenuButton::new();
        let zb = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        zb.append(&gtk::Image::from_icon_name("zoom-in-symbolic"));
        zoom_btn.set_child(Some(&zb));
        zoom_btn.set_tooltip_text(Some("Zoom"));
        zoom_btn.add_css_class("flat");
        let zpop = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        zpop.set_margin_start(6);
        zpop.set_margin_end(6);
        zpop.set_margin_top(6);
        zpop.set_margin_bottom(6);
        let zout = icon_button("zoom-out-symbolic", "Reduzir (Ctrl+−)");
        zout.set_action_name(Some("win.zoom-out"));
        let zin = icon_button("zoom-in-symbolic", "Ampliar (Ctrl++)");
        zin.set_action_name(Some("win.zoom-in"));
        let zfit = gtk::Button::with_label("Ajustar");
        zfit.add_css_class("flat");
        zfit.set_action_name(Some("win.zoom-fit"));
        zfit.set_tooltip_text(Some("Ajustar à janela (Ctrl+0)"));
        let z100 = gtk::Button::with_label("100%");
        z100.add_css_class("flat");
        z100.set_action_name(Some("win.zoom-100"));
        zpop.append(&zout);
        zpop.append(&zoom_label);
        zpop.append(&zin);
        zpop.append(&gtk::Separator::new(gtk::Orientation::Vertical));
        zpop.append(&z100);
        zpop.append(&zfit);
        zoom_btn.set_popover(Some(&gtk::Popover::builder().child(&zpop).build()));
        header.pack_end(&zoom_btn);

        // Ferramentas de desenho no centro da barra de título.
        let tools = gtk::Box::new(gtk::Orientation::Horizontal, 2);
        let crop_bar = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        let title_stack = gtk::Stack::new();
        title_stack.set_transition_type(gtk::StackTransitionType::Crossfade);
        title_stack.add_named(&adw::WindowTitle::new("Ferramenta de Captura", ""), Some("title"));
        title_stack.add_named(&tools, Some("tools"));
        title_stack.add_named(&crop_bar, Some("crop"));
        header.set_title_widget(Some(&title_stack));

        // Área de desenho.
        let area = gtk::DrawingArea::new();
        area.set_focusable(true);
        let scroller = gtk::ScrolledWindow::new();
        scroller.set_child(Some(&area));
        scroller.add_css_class("canvas");
        scroller.set_vexpand(true);
        scroller.set_hexpand(true);
        let overlay = gtk::Overlay::new();
        overlay.set_child(Some(&scroller));

        let empty = gtk::Box::new(gtk::Orientation::Vertical, 18);
        empty.set_valign(gtk::Align::Center);
        empty.set_halign(gtk::Align::Center);
        let hint = gtk::Label::new(Some("Pressione Print Screen ou Super + Shift + S para iniciar um recorte."));
        hint.add_css_class("empty-hint");
        hint.add_css_class("dim-label");
        hint.set_wrap(true);
        hint.set_justify(gtk::Justification::Center);
        empty.append(&hint);

        let stack = gtk::Stack::new();
        stack.add_named(&empty, Some("empty"));
        stack.add_named(&overlay, Some("doc"));
        stack.set_visible_child_name("empty");

        let toasts = adw::ToastOverlay::new();
        toasts.set_child(Some(&stack));
        let view = adw::ToolbarView::new();
        view.add_top_bar(&header);
        view.set_content(Some(&toasts));
        window.set_content(Some(&view));

        let shape_button_icon = gtk::Image::from_icon_name(ShapeKind::from_index(cfg.shape_kind).icon());
        let text_css = gtk::CssProvider::new();
        if let Some(display) = gdk::Display::default() {
            gtk::style_context_add_provider_for_display(&display, &text_css, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION + 1);
        }

        let ed = Rc::new(Editor {
            window,
            toasts,
            stack,
            title_stack,
            overlay,
            scroller,
            area,
            zoom_label,
            mode_icon,
            delay_label,
            tool_buttons: RefCell::new(Vec::new()),
            shape_kind_buttons: RefCell::new(Vec::new()),
            shape_button_icon,
            text_css,
            doc: RefCell::new(None),
            path: RefCell::new(None),
            cfg: RefCell::new(cfg),
            tool: Cell::new(Tool::Pen),
            zoom: Cell::new(1.0),
            fit: Cell::new(true),
            last_viewport: Cell::new((0, 0)),
            current: RefCell::new(None),
            drag_origin: Cell::new((0.0, 0.0)),
            erase_pushed: Cell::new(false),
            crop: RefCell::new(None),
            text: RefCell::new(None),
            hover: Cell::new(None),
            copy_timer: RefCell::new(None),
            pending_scroll: Cell::new(None),
            sized_for_doc: Cell::new(false),
        });
        ed.build_mode_menu(&mode_btn);
        ed.build_delay_menu(&delay_btn);
        ed.build_tools(&tools);
        ed.build_crop_bar(&crop_bar);
        ed.setup_actions();
        ed.setup_canvas();
        ed.update_sensitivity();
        ed
    }

    // -- Barra de título ------------------------------------------------------

    fn build_mode_menu(self: &Rc<Self>, btn: &gtk::MenuButton) {
        let list = gtk::Box::new(gtk::Orientation::Vertical, 2);
        let pop = gtk::Popover::builder().child(&list).build();
        for m in Mode::ALL {
            let row = gtk::Button::new();
            row.add_css_class("flat");
            let b = gtk::Box::new(gtk::Orientation::Horizontal, 10);
            b.append(&gtk::Image::from_icon_name(mode_icon_name(m)));
            let l = gtk::Label::new(Some(m.label()));
            l.set_xalign(0.0);
            b.append(&l);
            row.set_child(Some(&b));
            let ed = self.clone();
            let pop2 = pop.clone();
            row.connect_clicked(move |_| {
                ed.update_cfg(|c| c.mode = m);
                ed.mode_icon.set_icon_name(Some(mode_icon_name(m)));
                pop2.popdown();
            });
            list.append(&row);
        }
        btn.set_popover(Some(&pop));
    }

    fn build_delay_menu(self: &Rc<Self>, btn: &gtk::MenuButton) {
        let list = gtk::Box::new(gtk::Orientation::Vertical, 2);
        let pop = gtk::Popover::builder().child(&list).build();
        for (d, label) in [(0, "Sem atraso"), (3, "Atraso de 3 segundos"), (5, "Atraso de 5 segundos"), (10, "Atraso de 10 segundos")] {
            let row = gtk::Button::with_label(label);
            row.add_css_class("flat");
            if let Some(l) = row.child().and_downcast::<gtk::Label>() {
                l.set_xalign(0.0);
            }
            let ed = self.clone();
            let pop2 = pop.clone();
            row.connect_clicked(move |_| {
                ed.update_cfg(|c| c.delay = d);
                ed.delay_label.set_text(&delay_text(d));
                pop2.popdown();
            });
            list.append(&row);
        }
        btn.set_popover(Some(&pop));
    }

    fn build_tools(self: &Rc<Self>, tools: &gtk::Box) {
        let defs: [(Tool, &str, &str); 5] = [
            (Tool::Pen, "recorte-pen-symbolic", "Caneta esferográfica"),
            (Tool::Highlighter, "recorte-highlighter-symbolic", "Marca-texto"),
            (Tool::Eraser, "recorte-eraser-symbolic", "Borracha"),
            (Tool::Shape, "", "Formas"),
            (Tool::Text, "recorte-text-symbolic", "Texto"),
        ];
        for (tool, icon, tip) in defs {
            let b = gtk::Button::new();
            b.add_css_class("flat");
            b.add_css_class("tool-button");
            b.set_tooltip_text(Some(tip));
            let col = gtk::Box::new(gtk::Orientation::Vertical, 0);
            col.set_halign(gtk::Align::Center);
            col.set_valign(gtk::Align::Center);
            if tool == Tool::Shape {
                col.append(&self.shape_button_icon);
            } else {
                col.append(&gtk::Image::from_icon_name(icon));
            }
            let bar = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            bar.add_css_class("swatch-bar");
            col.append(&bar);
            b.set_child(Some(&col));
            let pop = self.tool_popover(tool);
            pop.set_parent(&b);
            let ed = self.clone();
            b.connect_clicked(move |_| {
                if ed.tool.get() == tool {
                    pop.popup();
                } else {
                    ed.set_tool(tool);
                }
            });
            tools.append(&b);
            self.tool_buttons.borrow_mut().push((tool, b, bar));
        }
        let crop = icon_button("recorte-crop-symbolic", "Recortar imagem");
        crop.add_css_class("tool-button");
        crop.set_action_name(Some("win.crop"));
        tools.append(&crop);
        tools.append(&gtk::Separator::new(gtk::Orientation::Vertical));
        let undo = icon_button("edit-undo-symbolic", "Desfazer (Ctrl+Z)");
        undo.set_action_name(Some("win.undo"));
        let redo = icon_button("edit-redo-symbolic", "Refazer (Ctrl+Y)");
        redo.set_action_name(Some("win.redo"));
        tools.append(&undo);
        tools.append(&redo);
        self.refresh_tool_buttons();
    }

    fn build_crop_bar(self: &Rc<Self>, bar: &gtk::Box) {
        let label = gtk::Label::new(Some("Recortar imagem"));
        label.add_css_class("heading");
        label.set_margin_end(12);
        let apply = gtk::Button::from_icon_name("object-select-symbolic");
        apply.set_tooltip_text(Some("Aplicar (Enter)"));
        apply.add_css_class("suggested-action");
        apply.set_action_name(Some("win.crop-apply"));
        let cancel = gtk::Button::from_icon_name("window-close-symbolic");
        cancel.set_tooltip_text(Some("Cancelar (Esc)"));
        cancel.add_css_class("flat");
        cancel.set_action_name(Some("win.crop-cancel"));
        bar.append(&label);
        bar.append(&apply);
        bar.append(&cancel);
    }

    /// Painel de opções de cada ferramenta (aparece ao clicar de novo nela).
    fn tool_popover(self: &Rc<Self>, tool: Tool) -> gtk::Popover {
        let content = gtk::Box::new(gtk::Orientation::Vertical, 10);
        content.set_margin_start(10);
        content.set_margin_end(10);
        content.set_margin_top(10);
        content.set_margin_bottom(10);
        let pop = gtk::Popover::builder().child(&content).build();
        if tool == Tool::Eraser {
            let b = gtk::Button::with_label("Apagar toda a tinta");
            b.add_css_class("flat");
            let ed = self.clone();
            let pop2 = pop.clone();
            b.connect_clicked(move |_| {
                pop2.popdown();
                ed.clear_ink();
            });
            content.append(&b);
            return pop;
        }
        if tool == Tool::Shape {
            let kinds = gtk::Box::new(gtk::Orientation::Horizontal, 4);
            kinds.add_css_class("linked");
            let current = ShapeKind::from_index(self.cfg.borrow().shape_kind);
            let mut group: Option<gtk::ToggleButton> = None;
            for k in ShapeKind::ALL {
                let tb = gtk::ToggleButton::new();
                tb.set_icon_name(k.icon());
                tb.set_tooltip_text(Some(k.label()));
                tb.set_active(k == current);
                if let Some(g) = &group {
                    tb.set_group(Some(g));
                } else {
                    group = Some(tb.clone());
                }
                let ed = self.clone();
                tb.connect_toggled(move |tb| {
                    if tb.is_active() {
                        ed.update_cfg(|c| c.shape_kind = k.index());
                        ed.shape_button_icon.set_icon_name(Some(k.icon()));
                    }
                });
                kinds.append(&tb);
                self.shape_kind_buttons.borrow_mut().push((k, tb));
            }
            content.append(&kinds);
        }
        let palette: &[[u8; 3]] = if tool == Tool::Highlighter { HL_COLORS } else { PEN_COLORS };
        let grid = gtk::FlowBox::new();
        grid.set_max_children_per_line(6);
        grid.set_min_children_per_line(6);
        grid.set_selection_mode(gtk::SelectionMode::None);
        grid.set_column_spacing(6);
        grid.set_row_spacing(6);
        let swatches: Rc<RefCell<Vec<(gtk::Button, [u8; 3])>>> = Rc::new(RefCell::new(Vec::new()));
        for &c in palette {
            let b = gtk::Button::new();
            b.add_css_class("color-swatch");
            b.set_tooltip_text(Some(&color_hex(c)));
            let provider = gtk::CssProvider::new();
            provider.load_from_string(&format!("button {{ background: {}; }}", color_hex(c)));
            #[allow(deprecated)]
            b.style_context().add_provider(&provider, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION + 2);
            let ed = self.clone();
            let sw = swatches.clone();
            b.connect_clicked(move |_| {
                ed.update_cfg(|cfg| match tool {
                    Tool::Pen => cfg.pen_color = c,
                    Tool::Highlighter => cfg.hl_color = c,
                    Tool::Shape => cfg.shape_color = c,
                    Tool::Text => cfg.text_color = c,
                    Tool::Eraser => {}
                });
                for (s, sc) in sw.borrow().iter() {
                    if *sc == c {
                        s.add_css_class("selected");
                    } else {
                        s.remove_css_class("selected");
                    }
                }
                ed.refresh_tool_buttons();
            });
            grid.insert(&b, -1);
            swatches.borrow_mut().push((b, c));
        }
        let selected = self.tool_color(tool);
        for (s, c) in swatches.borrow().iter() {
            if *c == selected {
                s.add_css_class("selected");
            }
        }
        content.append(&grid);

        let (min, max, value, label) = {
            let cfg = self.cfg.borrow();
            match tool {
                Tool::Pen => (1.0, 24.0, cfg.pen_size, "Espessura"),
                Tool::Highlighter => (6.0, 48.0, cfg.hl_size, "Espessura"),
                Tool::Shape => (1.0, 24.0, cfg.shape_size, "Espessura"),
                _ => (10.0, 96.0, cfg.text_size, "Tamanho do texto"),
            }
        };
        let l = gtk::Label::new(Some(label));
        l.set_xalign(0.0);
        l.add_css_class("dim-label");
        content.append(&l);
        let scale = gtk::Scale::with_range(gtk::Orientation::Horizontal, min, max, 1.0);
        scale.set_value(value);
        scale.set_draw_value(true);
        scale.set_digits(0);
        scale.set_width_request(220);
        let ed = self.clone();
        scale.connect_value_changed(move |s| {
            let v = s.value().round();
            ed.update_cfg(|cfg| match tool {
                Tool::Pen => cfg.pen_size = v,
                Tool::Highlighter => cfg.hl_size = v,
                Tool::Shape => cfg.shape_size = v,
                Tool::Text => cfg.text_size = v,
                Tool::Eraser => {}
            });
        });
        content.append(&scale);
        if tool == Tool::Shape {
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
            let l = gtk::Label::new(Some("Preencher forma"));
            l.set_hexpand(true);
            l.set_xalign(0.0);
            let sw = gtk::Switch::new();
            sw.set_active(self.cfg.borrow().shape_fill);
            let ed = self.clone();
            sw.connect_active_notify(move |s| ed.update_cfg(|c| c.shape_fill = s.is_active()));
            row.append(&l);
            row.append(&sw);
            content.append(&row);
        }
        pop
    }

    fn tool_color(&self, tool: Tool) -> [u8; 3] {
        let cfg = self.cfg.borrow();
        match tool {
            Tool::Pen => cfg.pen_color,
            Tool::Highlighter => cfg.hl_color,
            Tool::Shape => cfg.shape_color,
            Tool::Text => cfg.text_color,
            Tool::Eraser => [0x80, 0x80, 0x80],
        }
    }

    fn refresh_tool_buttons(&self) {
        let current = self.tool.get();
        for (tool, button, bar) in self.tool_buttons.borrow().iter() {
            if *tool == current {
                button.add_css_class("active");
            } else {
                button.remove_css_class("active");
            }
            let visible = *tool != Tool::Eraser;
            bar.set_opacity(if visible { 1.0 } else { 0.0 });
            let provider = gtk::CssProvider::new();
            provider.load_from_string(&format!("box {{ background: {}; }}", color_hex(self.tool_color(*tool))));
            #[allow(deprecated)]
            bar.style_context().add_provider(&provider, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION + 2);
        }
    }

    fn set_tool(&self, tool: Tool) {
        self.commit_text();
        self.tool.set(tool);
        self.refresh_tool_buttons();
        self.update_cursor();
        self.area.queue_draw();
    }

    /// Lê a configuração atual do disco, aplica a mudança e grava.
    fn update_cfg(&self, f: impl FnOnce(&mut Config)) {
        let mut fresh = Config::load();
        f(&mut fresh);
        fresh.save();
        *self.cfg.borrow_mut() = fresh;
    }

    // -- Ações ----------------------------------------------------------------

    fn setup_actions(self: &Rc<Self>) {
        let add = |name: &str, f: Box<dyn Fn(&Rc<Editor>)>| {
            let a = gio::SimpleAction::new(name, None);
            let ed = self.clone();
            a.connect_activate(move |_, _| f(&ed));
            self.window.add_action(&a);
        };
        add("new", Box::new(|ed| ed.confirm_discard(Box::new(|ed| ed.new_capture()))));
        add("open", Box::new(|ed| ed.confirm_discard(Box::new(|ed| ed.open_dialog()))));
        add("save", Box::new(|ed| ed.save_dialog(None)));
        add("copy", Box::new(|ed| ed.copy(true)));
        add("undo", Box::new(|ed| ed.undo()));
        add("redo", Box::new(|ed| ed.redo()));
        add("crop", Box::new(|ed| ed.begin_crop()));
        add("crop-apply", Box::new(|ed| ed.end_crop(true)));
        add("crop-cancel", Box::new(|ed| ed.end_crop(false)));
        add("zoom-in", Box::new(|ed| ed.zoom_by(1.25, None)));
        add("zoom-out", Box::new(|ed| ed.zoom_by(0.8, None)));
        add("zoom-fit", Box::new(|ed| {
            ed.fit.set(true);
            ed.apply_fit();
        }));
        add("zoom-100", Box::new(|ed| ed.set_zoom(1.0, None)));
        add("settings", Box::new(|ed| ed.show_settings()));
        add("about", Box::new(|ed| ed.show_about()));

        let ed = self.clone();
        self.window.connect_close_request(move |w| {
            if ed.needs_save_prompt() {
                let ed2 = ed.clone();
                let w = w.clone();
                ed.ask_save(Box::new(move |ed| {
                    if let Some(d) = ed.doc.borrow_mut().as_mut() {
                        d.dirty = false;
                    }
                    let _ = &ed2;
                    w.close();
                }));
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });

        let keys = gtk::EventControllerKey::new();
        let ed = self.clone();
        keys.connect_key_pressed(move |_, key, _, state| {
            let plain = !state.intersects(gdk::ModifierType::CONTROL_MASK | gdk::ModifierType::ALT_MASK);
            if ed.crop.borrow().is_some() && plain {
                match key {
                    gdk::Key::Escape => {
                        ed.end_crop(false);
                        return glib::Propagation::Stop;
                    }
                    gdk::Key::Return | gdk::Key::KP_Enter => {
                        ed.end_crop(true);
                        return glib::Propagation::Stop;
                    }
                    _ => {}
                }
            }
            glib::Propagation::Proceed
        });
        self.window.add_controller(keys);
    }

    fn action(&self, name: &str) -> Option<gio::SimpleAction> {
        self.window.lookup_action(name).and_downcast::<gio::SimpleAction>()
    }

    fn update_sensitivity(&self) {
        let doc = self.doc.borrow();
        let has = doc.is_some();
        let cropping = self.crop.borrow().is_some();
        let editing_text = self.text.borrow().is_some();
        let (can_undo, can_redo) = doc.as_ref().map(|d| (!d.undo.is_empty(), !d.redo.is_empty())).unwrap_or((false, false));
        let set = |n: &str, v: bool| {
            if let Some(a) = self.action(n) {
                a.set_enabled(v);
            }
        };
        set("save", has && !cropping);
        // Enquanto se digita texto, Ctrl+C/Ctrl+Z ficam com a caixa de texto.
        set("copy", has && !cropping && !editing_text);
        set("undo", can_undo && !cropping && !editing_text);
        set("redo", can_redo && !cropping && !editing_text);
        set("crop", has && !cropping);
        set("crop-apply", cropping);
        set("crop-cancel", cropping);
        for z in ["zoom-in", "zoom-out", "zoom-fit", "zoom-100"] {
            set(z, has);
        }
        self.title_stack.set_visible_child_name(if cropping {
            "crop"
        } else if has {
            "tools"
        } else {
            "title"
        });
    }

    // -- Documento ------------------------------------------------------------

    fn load_path(self: &Rc<Self>, path: &Path) {
        self.commit_text();
        *self.crop.borrow_mut() = None;
        match load_surface(path) {
            Ok(base) => {
                let (w, h) = (base.width() as f64, base.height() as f64);
                *self.doc.borrow_mut() = Some(Doc {
                    base,
                    w,
                    h,
                    state: DocState { items: Vec::new(), crop: Rect { x: 0.0, y: 0.0, w, h } },
                    undo: Vec::new(),
                    redo: Vec::new(),
                    dirty: false,
                });
                *self.path.borrow_mut() = Some(path.to_path_buf());
                self.stack.set_visible_child_name("doc");
                if !self.window.is_visible() || !self.sized_for_doc.get() {
                    self.size_window_for(w, h);
                }
                self.fit.set(true);
                self.last_viewport.set((0, 0));
                self.apply_fit();
                self.update_sensitivity();
                self.area.queue_draw();
            }
            Err(e) => self.toast(&format!("Não foi possível abrir a imagem: {e}")),
        }
    }

    /// Ajusta a janela ao tamanho do recorte, como o Windows faz.
    fn size_window_for(&self, w: f64, h: f64) {
        let (mut mw, mut mh) = (1920.0, 1080.0);
        if let Some(display) = gdk::Display::default() {
            if let Some(m) = display.monitors().item(0).and_downcast::<gdk::Monitor>() {
                let g = m.geometry();
                mw = g.width() as f64;
                mh = g.height() as f64;
            }
        }
        let ww = (w + 2.0 * MARGIN + 24.0).clamp(760.0, mw * 0.85);
        let wh = (h + 2.0 * MARGIN + 70.0).clamp(420.0, mh * 0.85);
        self.window.set_default_size(ww as i32, wh as i32);
        self.sized_for_doc.set(true);
    }

    fn view_rect(&self) -> Rect {
        let doc = self.doc.borrow();
        let Some(d) = doc.as_ref() else { return Rect { x: 0.0, y: 0.0, w: 1.0, h: 1.0 } };
        if self.crop.borrow().is_some() {
            Rect { x: 0.0, y: 0.0, w: d.w, h: d.h }
        } else {
            d.state.crop
        }
    }

    fn offset(&self) -> (f64, f64) {
        let v = self.view_rect();
        let z = self.zoom.get();
        let ox = ((self.area.width() as f64 - v.w * z) / 2.0).max(MARGIN);
        let oy = ((self.area.height() as f64 - v.h * z) / 2.0).max(MARGIN);
        (ox, oy)
    }

    fn to_img(&self, p: (f64, f64)) -> (f64, f64) {
        let v = self.view_rect();
        let (ox, oy) = self.offset();
        let z = self.zoom.get();
        (v.x + (p.0 - ox) / z, v.y + (p.1 - oy) / z)
    }

    fn to_widget(&self, p: (f64, f64)) -> (f64, f64) {
        let v = self.view_rect();
        let (ox, oy) = self.offset();
        let z = self.zoom.get();
        (ox + (p.0 - v.x) * z, oy + (p.1 - v.y) * z)
    }

    fn update_content_size(&self) {
        let v = self.view_rect();
        let z = self.zoom.get();
        self.area.set_content_width((v.w * z + 2.0 * MARGIN).ceil() as i32);
        self.area.set_content_height((v.h * z + 2.0 * MARGIN).ceil() as i32);
        self.zoom_label.set_text(&format!("{:.0}%", z * 100.0));
        self.reposition_text();
        self.area.queue_draw();
    }

    fn fit_zoom(&self) -> f64 {
        let v = self.view_rect();
        let (sw, sh) = (self.scroller.width() as f64, self.scroller.height() as f64);
        if sw < 10.0 || sh < 10.0 {
            return 1.0;
        }
        ((sw - 2.0 * MARGIN) / v.w).min((sh - 2.0 * MARGIN) / v.h).clamp(0.05, 1.0)
    }

    fn apply_fit(&self) {
        if self.fit.get() {
            self.zoom.set(self.fit_zoom());
        }
        self.update_content_size();
    }

    /// Altera o zoom mantendo fixo o ponto sob o cursor (coordenadas da área).
    fn set_zoom(&self, z: f64, anchor: Option<(f64, f64)>) {
        if self.doc.borrow().is_none() {
            return;
        }
        let z = z.clamp(0.05, 8.0);
        let (hadj, vadj) = (self.scroller.hadjustment(), self.scroller.vadjustment());
        let anchor = anchor.unwrap_or((hadj.value() + hadj.page_size() / 2.0, vadj.value() + vadj.page_size() / 2.0));
        let img_pt = self.to_img(anchor);
        let screen = (anchor.0 - hadj.value(), anchor.1 - vadj.value());
        self.fit.set(false);
        self.zoom.set(z);
        self.update_content_size();
        // Valor de rolagem desejado é aplicado quando a nova alocação chegar.
        let v = self.view_rect();
        // Só há rolagem se o conteúdo passar da área visível; senão nenhuma
        // realocação acontece e o valor ficaria pendente para depois.
        let overflows = v.w * z + 2.0 * MARGIN > hadj.page_size() || v.h * z + 2.0 * MARGIN > vadj.page_size();
        let new_w = (img_pt.0 - v.x) * z + MARGIN;
        let new_h = (img_pt.1 - v.y) * z + MARGIN;
        self.pending_scroll.set(overflows.then_some((new_w - screen.0, new_h - screen.1)));
    }

    fn zoom_by(&self, f: f64, anchor: Option<(f64, f64)>) {
        self.set_zoom(self.zoom.get() * f, anchor);
    }

    fn setup_canvas(self: &Rc<Self>) {
        let ed = self.clone();
        self.area.set_draw_func(move |_, cr, w, h| ed.draw(cr, w, h));

        let ed = self.clone();
        self.area.connect_resize(move |_, _, _| {
            if let Some((x, y)) = ed.pending_scroll.take() {
                let ed2 = ed.clone();
                glib::idle_add_local_once(move || {
                    ed2.scroller.hadjustment().set_value(x);
                    ed2.scroller.vadjustment().set_value(y);
                });
            }
        });

        // Reajusta o zoom "Ajustar" quando a janela muda de tamanho.
        let ed = self.clone();
        self.scroller.add_tick_callback(move |s, _| {
            let size = (s.width(), s.height());
            if size != ed.last_viewport.get() {
                ed.last_viewport.set(size);
                if ed.fit.get() && ed.doc.borrow().is_some() {
                    ed.apply_fit();
                }
            }
            glib::ControlFlow::Continue
        });

        let ed = self.clone();
        self.scroller.hadjustment().connect_value_changed(move |_| ed.reposition_text());
        let ed = self.clone();
        self.scroller.vadjustment().connect_value_changed(move |_| ed.reposition_text());

        let drag = gtk::GestureDrag::new();
        drag.set_button(gdk::BUTTON_PRIMARY);
        let ed = self.clone();
        drag.connect_drag_begin(move |g, x, y| ed.drag_begin(g, (x, y)));
        let ed = self.clone();
        drag.connect_drag_update(move |g, dx, dy| ed.drag_update(g, dx, dy));
        let ed = self.clone();
        drag.connect_drag_end(move |g, dx, dy| {
            ed.drag_update(g, dx, dy);
            ed.drag_end();
        });
        self.area.add_controller(drag);

        let motion = gtk::EventControllerMotion::new();
        let ed = self.clone();
        motion.connect_motion(move |_, x, y| {
            ed.hover.set(Some((x, y)));
            if ed.tool.get() == Tool::Eraser || ed.crop.borrow().is_some() {
                ed.update_cursor();
                ed.area.queue_draw();
            }
        });
        let ed = self.clone();
        motion.connect_leave(move |_| {
            ed.hover.set(None);
            ed.area.queue_draw();
        });
        self.area.add_controller(motion);

        let scroll = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::VERTICAL);
        let ed = self.clone();
        scroll.connect_scroll(move |c, _, dy| {
            if c.current_event_state().contains(gdk::ModifierType::CONTROL_MASK) {
                let anchor = ed.hover.get();
                ed.zoom_by(if dy < 0.0 { 1.1 } else { 1.0 / 1.1 }, anchor);
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        self.area.add_controller(scroll);

        // Arrastar uma imagem para a janela abre-a.
        let drop = gtk::DropTarget::new(gio::File::static_type(), gdk::DragAction::COPY);
        let ed = self.clone();
        drop.connect_drop(move |_, value, _, _| {
            if let Ok(file) = value.get::<gio::File>() {
                if let Some(p) = file.path() {
                    let ed2 = ed.clone();
                    ed.confirm_discard(Box::new(move |_| ed2.load_path(&p)));
                    return true;
                }
            }
            false
        });
        self.window.add_controller(drop);
        self.update_cursor();
    }

    fn update_cursor(&self) {
        let name = if let Some(c) = self.crop.borrow().as_ref() {
            match self.hover.get().map(|p| self.crop_handle_at(&c.rect, p)) {
                Some(Handle::N | Handle::S) => "ns-resize",
                Some(Handle::E | Handle::W) => "ew-resize",
                Some(Handle::NE | Handle::SW) => "nesw-resize",
                Some(Handle::NW | Handle::SE) => "nwse-resize",
                Some(Handle::Move) => "move",
                _ => "crosshair",
            }
        } else {
            match self.tool.get() {
                Tool::Text => "text",
                Tool::Eraser => "none",
                _ => "crosshair",
            }
        };
        self.area.set_cursor_from_name(Some(name));
    }

    // -- Desenho na tela ------------------------------------------------------

    fn draw(&self, cr: &cairo::Context, _w: i32, _h: i32) {
        let doc = self.doc.borrow();
        let Some(d) = doc.as_ref() else { return };
        let v = self.view_rect();
        let z = self.zoom.get();
        let (ox, oy) = self.offset();
        let (iw, ih) = (v.w * z, v.h * z);

        // Sombra da "folha".
        for i in 0..6 {
            let g = (6 - i) as f64 * 1.5;
            cr.set_source_rgba(0.0, 0.0, 0.0, 0.035);
            cr.rectangle(ox - g, oy - g + 2.0, iw + 2.0 * g, ih + 2.0 * g);
            let _ = cr.fill();
        }
        // Xadrez para áreas transparentes (recorte de forma livre).
        let _ = cr.save();
        cr.rectangle(ox, oy, iw, ih);
        cr.clip();
        cr.set_source_rgb(0.85, 0.85, 0.85);
        let _ = cr.paint();
        cr.set_source_rgb(0.75, 0.75, 0.75);
        let cell = 8.0;
        let mut y = 0.0;
        while y < ih {
            let mut x = if ((y / cell) as i64) % 2 == 0 { 0.0 } else { cell };
            while x < iw {
                cr.rectangle(ox + x, oy + y, cell, cell);
                x += 2.0 * cell;
            }
            y += cell;
        }
        let _ = cr.fill();

        cr.translate(ox, oy);
        cr.scale(z, z);
        cr.translate(-v.x, -v.y);
        let _ = cr.set_source_surface(&d.base, 0.0, 0.0);
        let filter = if z >= 2.0 { cairo::Filter::Nearest } else { cairo::Filter::Good };
        cr.source().set_filter(filter);
        let _ = cr.paint();
        for item in &d.state.items {
            draw_item(cr, item);
        }
        if let Some(item) = self.current.borrow().as_ref() {
            draw_item(cr, item);
        }
        let _ = cr.restore();

        if let Some(c) = self.crop.borrow().as_ref() {
            self.draw_crop(cr, &c.rect, (ox, oy, iw, ih));
        } else if self.tool.get() == Tool::Eraser {
            if let Some(p) = self.hover.get() {
                cr.set_line_width(1.0);
                cr.arc(p.0, p.1, 8.0, 0.0, std::f64::consts::TAU);
                cr.set_source_rgba(1.0, 1.0, 1.0, 0.95);
                let _ = cr.stroke_preserve();
                cr.set_source_rgba(0.0, 0.0, 0.0, 0.6);
                cr.set_dash(&[2.0, 2.0], 0.0);
                let _ = cr.stroke();
                cr.set_dash(&[], 0.0);
            }
        }
    }

    fn draw_crop(&self, cr: &cairo::Context, r: &Rect, view: (f64, f64, f64, f64)) {
        let a = self.to_widget((r.x, r.y));
        let b = self.to_widget((r.x + r.w, r.y + r.h));
        let (x, y, w, h) = (a.0, a.1, b.0 - a.0, b.1 - a.1);
        cr.set_fill_rule(cairo::FillRule::EvenOdd);
        cr.rectangle(view.0, view.1, view.2, view.3);
        cr.rectangle(x, y, w, h);
        cr.set_source_rgba(0.0, 0.0, 0.0, 0.55);
        let _ = cr.fill();
        cr.set_fill_rule(cairo::FillRule::Winding);
        // Grade de terços.
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.35);
        cr.set_line_width(1.0);
        for i in 1..3 {
            let fx = x + w * i as f64 / 3.0;
            let fy = y + h * i as f64 / 3.0;
            cr.move_to(fx.round() + 0.5, y);
            cr.line_to(fx.round() + 0.5, y + h);
            cr.move_to(x, fy.round() + 0.5);
            cr.line_to(x + w, fy.round() + 0.5);
        }
        let _ = cr.stroke();
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.9);
        cr.rectangle(x.round() + 0.5, y.round() + 0.5, w.round() - 1.0, h.round() - 1.0);
        let _ = cr.stroke();
        // Alças em "L" nos cantos e barras no meio das bordas.
        cr.set_source_rgb(1.0, 1.0, 1.0);
        cr.set_line_width(4.0);
        cr.set_line_cap(cairo::LineCap::Square);
        let l = 18.0_f64.min(w / 2.0).min(h / 2.0);
        for (cx, cy, dx, dy) in [(x, y, 1.0, 1.0), (x + w, y, -1.0, 1.0), (x, y + h, 1.0, -1.0), (x + w, y + h, -1.0, -1.0)] {
            cr.move_to(cx + dx * l, cy);
            cr.line_to(cx, cy);
            cr.line_to(cx, cy + dy * l);
        }
        let m = 10.0_f64.min(w / 4.0).min(h / 4.0);
        cr.move_to(x + w / 2.0 - m, y);
        cr.line_to(x + w / 2.0 + m, y);
        cr.move_to(x + w / 2.0 - m, y + h);
        cr.line_to(x + w / 2.0 + m, y + h);
        cr.move_to(x, y + h / 2.0 - m);
        cr.line_to(x, y + h / 2.0 + m);
        cr.move_to(x + w, y + h / 2.0 - m);
        cr.line_to(x + w, y + h / 2.0 + m);
        let _ = cr.stroke();
    }

    // -- Interação ------------------------------------------------------------

    fn drag_begin(self: &Rc<Self>, g: &gtk::GestureDrag, p: (f64, f64)) {
        self.area.grab_focus();
        if self.text.borrow().is_some() {
            self.commit_text();
            if self.tool.get() == Tool::Text {
                g.set_state(gtk::EventSequenceState::Denied);
                return;
            }
        }
        if self.doc.borrow().is_none() {
            return;
        }
        self.drag_origin.set(p);
        let ip = self.to_img(p);
        if let Some(c) = self.crop.borrow_mut().as_mut() {
            let handle = self.crop_handle_at(&c.rect, p);
            c.drag = Some((handle, c.rect, ip));
            return;
        }
        let cfg = self.cfg.borrow().clone();
        let z = self.zoom.get();
        match self.tool.get() {
            Tool::Pen => {
                *self.current.borrow_mut() = Some(Item::Stroke { pts: vec![ip], color: rgba(cfg.pen_color), width: cfg.pen_size, hl: false });
            }
            Tool::Highlighter => {
                *self.current.borrow_mut() = Some(Item::Stroke { pts: vec![ip], color: rgba(cfg.hl_color), width: cfg.hl_size, hl: true });
            }
            Tool::Shape => {
                *self.current.borrow_mut() = Some(Item::Shape {
                    kind: ShapeKind::from_index(cfg.shape_kind),
                    a: ip,
                    b: ip,
                    color: rgba(cfg.shape_color),
                    width: cfg.shape_size,
                    fill: cfg.shape_fill,
                });
            }
            Tool::Eraser => {
                self.erase_pushed.set(false);
                self.erase_at(ip, 8.0 / z);
            }
            Tool::Text => {
                g.set_state(gtk::EventSequenceState::Denied);
                self.start_text(ip);
            }
        }
        self.area.queue_draw();
    }

    fn drag_update(&self, g: &gtk::GestureDrag, dx: f64, dy: f64) {
        let o = self.drag_origin.get();
        let wp = (o.0 + dx, o.1 + dy);
        self.hover.set(Some(wp));
        let ip = self.to_img(wp);
        let z = self.zoom.get();
        if self.crop.borrow().is_some() {
            self.crop_drag(ip);
            self.area.queue_draw();
            return;
        }
        let shift = g.current_event_state().contains(gdk::ModifierType::SHIFT_MASK);
        let mut cur = self.current.borrow_mut();
        match cur.as_mut() {
            Some(Item::Stroke { pts, .. }) => {
                if pts.last().is_none_or(|l| (l.0 - ip.0).hypot(l.1 - ip.1) * z >= 1.0) {
                    pts.push(ip);
                }
            }
            Some(Item::Shape { kind, a, b, .. }) => {
                *b = if shift { constrain(*kind, *a, ip) } else { ip };
            }
            _ => {
                drop(cur);
                if self.tool.get() == Tool::Eraser {
                    self.erase_at(ip, 8.0 / z);
                }
                self.area.queue_draw();
                return;
            }
        }
        drop(cur);
        self.area.queue_draw();
    }

    fn drag_end(self: &Rc<Self>) {
        if let Some(c) = self.crop.borrow_mut().as_mut() {
            c.drag = None;
            return;
        }
        let Some(item) = self.current.borrow_mut().take() else {
            if self.erase_pushed.get() {
                self.changed();
            }
            return;
        };
        let keep = match &item {
            Item::Shape { kind, a, b, .. } => {
                let d = (a.0 - b.0).hypot(a.1 - b.1);
                if matches!(kind, ShapeKind::Line | ShapeKind::Arrow) { d >= 2.0 } else { (a.0 - b.0).abs() >= 2.0 && (a.1 - b.1).abs() >= 2.0 }
            }
            _ => true,
        };
        if keep {
            if let Some(d) = self.doc.borrow_mut().as_mut() {
                d.push_undo();
                d.state.items.push(item);
            }
            self.changed();
        }
        self.area.queue_draw();
    }

    fn erase_at(&self, p: (f64, f64), r: f64) {
        let mut doc = self.doc.borrow_mut();
        let Some(d) = doc.as_mut() else { return };
        if !d.state.items.iter().any(|i| item_hit(i, p, r)) {
            return;
        }
        if !self.erase_pushed.get() {
            d.push_undo();
            self.erase_pushed.set(true);
        }
        d.state.items.retain(|i| !item_hit(i, p, r));
        drop(doc);
        self.area.queue_draw();
    }

    fn clear_ink(self: &Rc<Self>) {
        let mut doc = self.doc.borrow_mut();
        let Some(d) = doc.as_mut() else { return };
        if d.state.items.is_empty() {
            return;
        }
        d.push_undo();
        d.state.items.clear();
        drop(doc);
        self.changed();
        self.area.queue_draw();
    }

    fn undo(self: &Rc<Self>) {
        self.commit_text();
        let mut doc = self.doc.borrow_mut();
        let Some(d) = doc.as_mut() else { return };
        let Some(prev) = d.undo.pop() else { return };
        let cur = std::mem::replace(&mut d.state, prev);
        d.redo.push(cur);
        d.dirty = true;
        drop(doc);
        self.after_history_change();
    }

    fn redo(self: &Rc<Self>) {
        let mut doc = self.doc.borrow_mut();
        let Some(d) = doc.as_mut() else { return };
        let Some(next) = d.redo.pop() else { return };
        let cur = std::mem::replace(&mut d.state, next);
        d.undo.push(cur);
        d.dirty = true;
        drop(doc);
        self.after_history_change();
    }

    fn after_history_change(self: &Rc<Self>) {
        if self.fit.get() {
            self.apply_fit();
        } else {
            self.update_content_size();
        }
        self.changed();
    }

    /// Após cada alteração: atualiza botões e copia automaticamente (como o Windows).
    fn changed(self: &Rc<Self>) {
        self.update_sensitivity();
        self.area.queue_draw();
        if !self.cfg.borrow().auto_copy_edits {
            return;
        }
        if let Some(id) = self.copy_timer.borrow_mut().take() {
            id.remove();
        }
        let ed = self.clone();
        let id = glib::timeout_add_local_once(Duration::from_millis(350), move || {
            ed.copy_timer.borrow_mut().take();
            ed.copy(false);
        });
        *self.copy_timer.borrow_mut() = Some(id);
    }

    // -- Texto ----------------------------------------------------------------

    fn start_text(self: &Rc<Self>, ip: (f64, f64)) {
        let cfg = self.cfg.borrow().clone();
        let z = self.zoom.get();
        self.text_css.load_from_string(&format!(
            "textview.recorte-text, textview.recorte-text text {{ background: transparent; color: {}; \
             font-family: Sans; font-size: {}px; padding: 0; caret-color: {}; }}\n\
             textview.recorte-text {{ outline: 1px dashed alpha(white, 0.8); }}",
            color_hex(cfg.text_color),
            cfg.text_size * z,
            color_hex(cfg.text_color),
        ));
        let view = gtk::TextView::new();
        view.add_css_class("recorte-text");
        view.set_halign(gtk::Align::Start);
        view.set_valign(gtk::Align::Start);
        view.set_wrap_mode(gtk::WrapMode::None);
        view.set_accepts_tab(false);
        view.set_size_request(40, -1);
        let keys = gtk::EventControllerKey::new();
        let ed = self.clone();
        keys.connect_key_pressed(move |_, key, _, state| match key {
            gdk::Key::Escape => {
                ed.commit_text();
                glib::Propagation::Stop
            }
            gdk::Key::Return | gdk::Key::KP_Enter if !state.contains(gdk::ModifierType::SHIFT_MASK) => {
                ed.commit_text();
                glib::Propagation::Stop
            }
            _ => glib::Propagation::Proceed,
        });
        view.add_controller(keys);
        self.overlay.add_overlay(&view);
        *self.text.borrow_mut() = Some(TextEdit { view: view.clone(), x: ip.0, y: ip.1 });
        self.reposition_text();
        view.grab_focus();
        self.update_sensitivity();
    }

    fn reposition_text(&self) {
        let text = self.text.borrow();
        let Some(t) = text.as_ref() else { return };
        let wp = self.to_widget((t.x, t.y));
        let pt = self.area.compute_point(&self.overlay, &graphene::Point::new(wp.0 as f32, wp.1 as f32));
        if let Some(pt) = pt {
            t.view.set_margin_start(pt.x().max(0.0) as i32);
            t.view.set_margin_top(pt.y().max(0.0) as i32);
        }
    }

    fn commit_text(&self) {
        let Some(t) = self.text.borrow_mut().take() else { return };
        let buf = t.view.buffer();
        let text = buf.text(&buf.start_iter(), &buf.end_iter(), false).to_string();
        self.overlay.remove_overlay(&t.view);
        if !text.trim().is_empty() {
            let cfg = self.cfg.borrow().clone();
            let (w, h) = measure_text(&text, cfg.text_size);
            if let Some(d) = self.doc.borrow_mut().as_mut() {
                d.push_undo();
                d.state.items.push(Item::Text { x: t.x, y: t.y, text, color: rgba(cfg.text_color), size: cfg.text_size, w, h });
            }
            EDITOR.with(|e| {
                if let Some(ed) = e.borrow().clone() {
                    ed.changed();
                }
            });
        }
        self.update_sensitivity();
        self.area.queue_draw();
    }

    // -- Recorte da imagem ----------------------------------------------------

    fn begin_crop(self: &Rc<Self>) {
        self.commit_text();
        let Some(crop) = self.doc.borrow().as_ref().map(|d| d.state.crop) else { return };
        *self.crop.borrow_mut() = Some(CropEdit { rect: crop, drag: None });
        self.fit.set(true);
        self.apply_fit();
        self.update_sensitivity();
        self.update_cursor();
    }

    fn end_crop(self: &Rc<Self>, apply: bool) {
        let Some(c) = self.crop.borrow_mut().take() else { return };
        if apply {
            let r = Rect { x: c.rect.x.round(), y: c.rect.y.round(), w: c.rect.w.round().max(1.0), h: c.rect.h.round().max(1.0) };
            let mut changed = false;
            if let Some(d) = self.doc.borrow_mut().as_mut() {
                if r != d.state.crop {
                    d.push_undo();
                    d.state.crop = r;
                    changed = true;
                }
            }
            if changed {
                self.changed();
            }
        }
        self.fit.set(true);
        self.apply_fit();
        self.update_sensitivity();
        self.update_cursor();
    }

    fn crop_handle_at(&self, r: &Rect, p: (f64, f64)) -> Handle {
        let a = self.to_widget((r.x, r.y));
        let b = self.to_widget((r.x + r.w, r.y + r.h));
        let tol = 12.0;
        let near = |v: f64, t: f64| (v - t).abs() <= tol;
        let inx = p.0 >= a.0 - tol && p.0 <= b.0 + tol;
        let iny = p.1 >= a.1 - tol && p.1 <= b.1 + tol;
        let (l, rr, t, bt) = (near(p.0, a.0) && iny, near(p.0, b.0) && iny, near(p.1, a.1) && inx, near(p.1, b.1) && inx);
        match (l, rr, t, bt) {
            (true, _, true, _) => Handle::NW,
            (_, true, true, _) => Handle::NE,
            (true, _, _, true) => Handle::SW,
            (_, true, _, true) => Handle::SE,
            (true, _, _, _) => Handle::W,
            (_, true, _, _) => Handle::E,
            (_, _, true, _) => Handle::N,
            (_, _, _, true) => Handle::S,
            _ if p.0 > a.0 && p.0 < b.0 && p.1 > a.1 && p.1 < b.1 => Handle::Move,
            _ => Handle::New,
        }
    }

    fn crop_drag(&self, ip: (f64, f64)) {
        let (dw, dh) = match self.doc.borrow().as_ref() {
            Some(d) => (d.w, d.h),
            None => return,
        };
        let mut crop = self.crop.borrow_mut();
        let Some(c) = crop.as_mut() else { return };
        let Some((handle, start, origin)) = c.drag else { return };
        let ip = (ip.0.clamp(0.0, dw), ip.1.clamp(0.0, dh));
        let (dx, dy) = (ip.0 - origin.0, ip.1 - origin.1);
        let (mut x0, mut y0, mut x1, mut y1) = (start.x, start.y, start.x + start.w, start.y + start.h);
        match handle {
            Handle::Move => {
                let dx = dx.clamp(-x0, dw - x1);
                let dy = dy.clamp(-y0, dh - y1);
                x0 += dx;
                x1 += dx;
                y0 += dy;
                y1 += dy;
            }
            Handle::New => {
                let r = Rect::from_points(origin, ip);
                (x0, y0, x1, y1) = (r.x, r.y, r.x + r.w, r.y + r.h);
            }
            _ => {
                if matches!(handle, Handle::W | Handle::NW | Handle::SW) {
                    x0 = (x0 + dx).clamp(0.0, x1 - 1.0);
                }
                if matches!(handle, Handle::E | Handle::NE | Handle::SE) {
                    x1 = (x1 + dx).clamp(x0 + 1.0, dw);
                }
                if matches!(handle, Handle::N | Handle::NW | Handle::NE) {
                    y0 = (y0 + dy).clamp(0.0, y1 - 1.0);
                }
                if matches!(handle, Handle::S | Handle::SW | Handle::SE) {
                    y1 = (y1 + dy).clamp(y0 + 1.0, dh);
                }
            }
        }
        if x1 - x0 >= 1.0 && y1 - y0 >= 1.0 {
            c.rect = Rect { x: x0, y: y0, w: x1 - x0, h: y1 - y0 };
        }
    }

    // -- Exportação -----------------------------------------------------------

    fn export_png(&self, path: &Path) -> Result<(), String> {
        let doc = self.doc.borrow();
        let d = doc.as_ref().ok_or("nenhuma imagem")?;
        let c = d.state.crop;
        let surf = cairo::ImageSurface::create(cairo::Format::ARgb32, c.w.round() as i32, c.h.round() as i32).map_err(|e| e.to_string())?;
        {
            let cr = cairo::Context::new(&surf).map_err(|e| e.to_string())?;
            cr.translate(-c.x, -c.y);
            cr.set_source_surface(&d.base, 0.0, 0.0).map_err(|e| e.to_string())?;
            cr.paint().map_err(|e| e.to_string())?;
            for item in &d.state.items {
                draw_item(&cr, item);
            }
        }
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let tmp = path.with_extension("png.part");
        {
            let mut f = std::fs::File::create(&tmp).map_err(|e| e.to_string())?;
            surf.write_to_png(&mut f).map_err(|e| e.to_string())?;
        }
        std::fs::rename(&tmp, path).map_err(|e| e.to_string())
    }

    fn copy(&self, explicit: bool) {
        if self.doc.borrow().is_none() {
            return;
        }
        let t = crate::paths::now();
        let path = crate::paths::runtime_dir().join(format!("editado-{:02}{:02}{:02}-{}.png", t.hour, t.min, t.sec, std::process::id()));
        let result = self.export_png(&path).and_then(|_| crate::clip::copy_file(&path).map_err(|e| e.to_string()));
        match result {
            Ok(()) if explicit => self.toast("Copiado para a área de transferência"),
            Ok(()) => {}
            Err(e) => self.toast(&format!("Não foi possível copiar: {e}")),
        }
    }

    fn save_dialog(self: &Rc<Self>, then: Option<Box<dyn FnOnce(&Rc<Editor>)>>) {
        self.commit_text();
        if self.doc.borrow().is_none() {
            return;
        }
        let dir = self.cfg.borrow().save_dir.clone();
        let _ = std::fs::create_dir_all(&dir);
        let filter = gtk::FileFilter::new();
        filter.set_name(Some("Imagem PNG"));
        filter.add_mime_type("image/png");
        let filters = gio::ListStore::new::<gtk::FileFilter>();
        filters.append(&filter);
        let dialog = gtk::FileDialog::builder()
            .title("Salvar como")
            .initial_folder(&gio::File::for_path(&dir))
            .initial_name(crate::paths::screenshot_file_name())
            .filters(&filters)
            .modal(true)
            .build();
        let ed = self.clone();
        glib::spawn_future_local(async move {
            let Ok(file) = dialog.save_future(Some(&ed.window)).await else { return };
            let Some(mut path) = file.path() else { return };
            if path.extension().is_none() {
                path.set_extension("png");
            }
            match ed.export_png(&path) {
                Ok(()) => {
                    if let Some(d) = ed.doc.borrow_mut().as_mut() {
                        d.dirty = false;
                    }
                    *ed.path.borrow_mut() = Some(path.clone());
                    ed.toast(&format!("Salvo em {}", path.display()));
                    if let Some(f) = then {
                        f(&ed);
                    }
                }
                Err(e) => ed.toast(&format!("Não foi possível salvar: {e}")),
            }
        });
    }

    fn open_dialog(self: &Rc<Self>) {
        let filter = gtk::FileFilter::new();
        filter.set_name(Some("Imagens"));
        filter.add_pixbuf_formats();
        let filters = gio::ListStore::new::<gtk::FileFilter>();
        filters.append(&filter);
        let dialog = gtk::FileDialog::builder()
            .title("Abrir")
            .initial_folder(&gio::File::for_path(&self.cfg.borrow().save_dir))
            .filters(&filters)
            .modal(true)
            .build();
        let ed = self.clone();
        glib::spawn_future_local(async move {
            if let Ok(file) = dialog.open_future(Some(&ed.window)).await {
                if let Some(p) = file.path() {
                    ed.load_path(&p);
                }
            }
        });
    }

    fn needs_save_prompt(&self) -> bool {
        self.cfg.borrow().ask_save && self.doc.borrow().as_ref().is_some_and(|d| d.dirty)
    }

    /// "Deseja salvar as alterações?" — e então continua com `then`.
    fn ask_save(self: &Rc<Self>, then: Box<dyn FnOnce(&Rc<Editor>)>) {
        let dialog = adw::AlertDialog::new(Some("Deseja salvar as alterações?"), Some("Este recorte foi editado e as alterações ainda não foram salvas."));
        dialog.add_responses(&[("cancel", "Cancelar"), ("discard", "Não salvar"), ("save", "Salvar")]);
        dialog.set_response_appearance("save", adw::ResponseAppearance::Suggested);
        dialog.set_response_appearance("discard", adw::ResponseAppearance::Destructive);
        dialog.set_default_response(Some("save"));
        dialog.set_close_response("cancel");
        let ed = self.clone();
        glib::spawn_future_local(async move {
            let response = dialog.choose_future(Some(&ed.window)).await;
            match response.as_str() {
                "save" => ed.save_dialog(Some(then)),
                "discard" => {
                    if let Some(d) = ed.doc.borrow_mut().as_mut() {
                        d.dirty = false;
                    }
                    then(&ed);
                }
                _ => {}
            }
        });
    }

    fn confirm_discard(self: &Rc<Self>, then: Box<dyn FnOnce(&Rc<Editor>)>) {
        self.commit_text();
        if self.needs_save_prompt() {
            self.ask_save(then);
        } else {
            then(self);
        }
    }

    // -- Novo recorte ---------------------------------------------------------

    fn new_capture(self: &Rc<Self>) {
        self.commit_text();
        let (mode, delay) = {
            let c = self.cfg.borrow();
            (c.mode, c.delay)
        };
        let was_visible = self.window.is_visible();
        // Esconde o editor para que ele não apareça na captura.
        self.window.set_visible(false);
        let ed = self.clone();
        glib::spawn_future_local(async move {
            glib::timeout_future(Duration::from_millis(if was_visible { 300 } else { 50 })).await;
            let Ok(exe) = crate::paths::self_exe() else { return };
            let delay_s = delay.to_string();
            let argv: [&std::ffi::OsStr; 6] = [
                exe.as_os_str(),
                "--modo".as_ref(),
                mode.key().as_ref(),
                "--atraso".as_ref(),
                delay_s.as_ref(),
                "--para-editor".as_ref(),
            ];
            let result = match gio::Subprocess::newv(&argv, gio::SubprocessFlags::STDOUT_PIPE) {
                Ok(p) => p.communicate_utf8_future(None).await.ok().map(|(out, _)| out),
                Err(_) => None,
            };
            if let Some(Some(out)) = result {
                let path = out.trim();
                if !path.is_empty() {
                    ed.sized_for_doc.set(false);
                    ed.load_path(Path::new(path));
                }
            }
            ed.window.present();
        });
    }

    // -- Diálogos -------------------------------------------------------------

    fn toast(&self, msg: &str) {
        let t = adw::Toast::new(msg);
        t.set_timeout(3);
        self.toasts.add_toast(t);
    }

    fn show_about(&self) {
        let about = adw::AboutDialog::builder()
            .application_name("Ferramenta de Captura")
            .application_icon(APP_ID)
            .version(env!("CARGO_PKG_VERSION"))
            .comments("Capturas de tela no estilo do W11, integradas ao COSMIC.")
            .license_type(gtk::License::MitX11)
            .build();
        about.present(Some(&self.window));
    }

    fn show_settings(self: &Rc<Self>) {
        let cfg = self.cfg.borrow().clone();
        let dialog = adw::PreferencesDialog::new();
        dialog.set_title("Configurações");
        let page = adw::PreferencesPage::new();

        let switch = |title: &str, subtitle: &str, active: bool, f: fn(&mut Config, bool)| {
            let row = adw::SwitchRow::builder().title(title).active(active).build();
            if !subtitle.is_empty() {
                row.set_subtitle(subtitle);
            }
            let ed = self.clone();
            row.connect_active_notify(move |r| {
                let v = r.is_active();
                ed.update_cfg(|c| f(c, v));
            });
            row
        };

        let cap = adw::PreferencesGroup::builder().title("Recorte").build();
        cap.add(&switch("Escurecer a tela durante o recorte", "", cfg.dim, |c, v| c.dim = v));
        cap.add(&switch("Mostrar notificação", "Clique na notificação para marcar e compartilhar", cfg.notify, |c, v| c.notify = v));
        cap.add(&switch("Abrir o editor após cada recorte", "", cfg.open_editor, |c, v| c.open_editor = v));
        let border = switch("Adicionar borda a cada captura de tela", "", cfg.border, |c, v| c.border = v);
        cap.add(&border);
        let color_row = adw::ActionRow::builder().title("Cor e espessura da borda").build();
        let color_btn = gtk::ColorDialogButton::new(Some(gtk::ColorDialog::new()));
        let bc = cfg.border_color;
        color_btn.set_rgba(&gdk::RGBA::new(bc[0] as f32 / 255.0, bc[1] as f32 / 255.0, bc[2] as f32 / 255.0, 1.0));
        color_btn.set_valign(gtk::Align::Center);
        let ed = self.clone();
        color_btn.connect_rgba_notify(move |b| {
            let c = b.rgba();
            let v = [(c.red() * 255.0).round() as u8, (c.green() * 255.0).round() as u8, (c.blue() * 255.0).round() as u8];
            ed.update_cfg(|cfg| cfg.border_color = v);
        });
        let spin = gtk::SpinButton::with_range(1.0, 32.0, 1.0);
        spin.set_value(cfg.border_width as f64);
        spin.set_valign(gtk::Align::Center);
        let ed = self.clone();
        spin.connect_value_changed(move |s| {
            let v = s.value() as u32;
            ed.update_cfg(|c| c.border_width = v);
        });
        color_row.add_suffix(&color_btn);
        color_row.add_suffix(&spin);
        border.bind_property("active", &color_row, "sensitive").sync_create().build();
        cap.add(&color_row);
        page.add(&cap);

        let save = adw::PreferencesGroup::builder().title("Salvar").build();
        save.add(&switch("Salvar capturas automaticamente", "As capturas originais são salvas em PNG", cfg.auto_save, |c, v| c.auto_save = v));
        let folder = adw::ActionRow::builder().title("Pasta de capturas de tela").subtitle(cfg.save_dir.display().to_string()).build();
        let change = gtk::Button::with_label("Alterar…");
        change.set_valign(gtk::Align::Center);
        let ed = self.clone();
        let folder2 = folder.clone();
        change.connect_clicked(move |_| {
            let dlg = gtk::FileDialog::builder().title("Pasta de capturas de tela").modal(true).build();
            let ed = ed.clone();
            let folder = folder2.clone();
            glib::spawn_future_local(async move {
                if let Ok(f) = dlg.select_folder_future(Some(&ed.window)).await {
                    if let Some(p) = f.path() {
                        folder.set_subtitle(&p.display().to_string());
                        ed.update_cfg(|c| c.save_dir = p);
                    }
                }
            });
        });
        folder.add_suffix(&change);
        save.add(&folder);
        save.add(&switch("Perguntar se deseja salvar recortes editados", "", cfg.ask_save, |c, v| c.ask_save = v));
        save.add(&switch("Copiar alterações automaticamente", "Cada edição é copiada para a área de transferência", cfg.auto_copy_edits, |c, v| c.auto_copy_edits = v));
        page.add(&save);

        let keys = adw::PreferencesGroup::builder()
            .title("Atalho de teclado")
            .description("Também é possível criar o atalho em Configurações do COSMIC › Teclado › Atalhos personalizados, com o comando “recorte”.")
            .build();
        let row = adw::ActionRow::builder().title("Print Screen e Super + Shift + S").subtitle("Abrem a Ferramenta de Captura").build();
        let btn = gtk::Button::with_label("Configurar");
        btn.set_valign(gtk::Align::Center);
        btn.add_css_class("suggested-action");
        let ed = self.clone();
        btn.connect_clicked(move |_| match crate::shortcuts::install() {
            Ok(msg) => ed.toast(&msg),
            Err(e) => ed.toast(&format!("Não foi possível configurar: {e}")),
        });
        row.add_suffix(&btn);
        keys.add(&row);
        page.add(&keys);

        dialog.add(&page);
        dialog.present(Some(&self.window));
    }
}

fn constrain(kind: ShapeKind, a: (f64, f64), p: (f64, f64)) -> (f64, f64) {
    let (dx, dy) = (p.0 - a.0, p.1 - a.1);
    match kind {
        ShapeKind::Rect | ShapeKind::Ellipse => {
            let m = dx.abs().max(dy.abs());
            (a.0 + m * dx.signum(), a.1 + m * dy.signum())
        }
        ShapeKind::Line | ShapeKind::Arrow => {
            let len = dx.hypot(dy);
            let ang = (dy.atan2(dx) / (std::f64::consts::PI / 4.0)).round() * (std::f64::consts::PI / 4.0);
            (a.0 + len * ang.cos(), a.1 + len * ang.sin())
        }
    }
}

fn mode_icon_name(m: Mode) -> &'static str {
    match m {
        Mode::Rect => "recorte-rect-symbolic",
        Mode::Free => "recorte-free-symbolic",
        Mode::Window => "recorte-window-symbolic",
        Mode::Screen => "recorte-screen-symbolic",
    }
}

fn delay_text(d: u32) -> String {
    if d == 0 {
        "Sem atraso".into()
    } else {
        format!("{d} s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn canvas(rgb: (f64, f64, f64)) -> cairo::ImageSurface {
        let s = cairo::ImageSurface::create(cairo::Format::ARgb32, 100, 100).unwrap();
        let cr = cairo::Context::new(&s).unwrap();
        cr.set_source_rgb(rgb.0, rgb.1, rgb.2);
        cr.paint().unwrap();
        s
    }

    /// Pixel (r, g, b) do ARGB32 (bytes BGRA).
    fn px(s: &mut cairo::ImageSurface, x: usize, y: usize) -> (u8, u8, u8) {
        s.flush();
        let stride = s.stride() as usize;
        let d = s.data().unwrap();
        let i = y * stride + x * 4;
        (d[i + 2], d[i + 1], d[i])
    }

    fn render(s: &cairo::ImageSurface, item: &Item) {
        let cr = cairo::Context::new(s).unwrap();
        draw_item(&cr, item);
    }

    #[test]
    fn caneta_desenha_na_cor_escolhida() {
        let mut s = canvas((1.0, 1.0, 1.0));
        render(&s, &Item::Stroke { pts: vec![(10.0, 50.0), (50.0, 50.0), (90.0, 50.0)], color: rgba([0xe8, 0x11, 0x23]), width: 6.0, hl: false });
        assert_eq!(px(&mut s, 50, 50), (0xe8, 0x11, 0x23));
        assert_eq!(px(&mut s, 50, 10), (255, 255, 255));
    }

    #[test]
    fn marca_texto_multiplica_e_mantem_o_texto_legivel() {
        let mut white = canvas((1.0, 1.0, 1.0));
        let mut black = canvas((0.0, 0.0, 0.0));
        let hl = Item::Stroke { pts: vec![(0.0, 50.0), (100.0, 50.0)], color: rgba([0xff, 0xeb, 0x3b]), width: 20.0, hl: true };
        render(&white, &hl);
        render(&black, &hl);
        assert_eq!(px(&mut white, 50, 50), (0xff, 0xeb, 0x3b));
        assert_eq!(px(&mut black, 50, 50), (0, 0, 0));
    }

    #[test]
    fn seta_tem_ponta_preenchida() {
        let mut s = canvas((1.0, 1.0, 1.0));
        render(&s, &Item::Shape { kind: ShapeKind::Arrow, a: (10.0, 50.0), b: (90.0, 50.0), color: rgba([0, 0, 0]), width: 3.0, fill: false });
        // Ponto dentro do triângulo da ponta, fora da espessura da haste.
        assert_eq!(px(&mut s, 80, 53), (0, 0, 0));
        assert_eq!(px(&mut s, 30, 60), (255, 255, 255));
    }

    #[test]
    fn retangulo_vazado_e_preenchido() {
        let mut s = canvas((1.0, 1.0, 1.0));
        render(&s, &Item::Shape { kind: ShapeKind::Rect, a: (20.0, 20.0), b: (80.0, 80.0), color: rgba([0, 0, 255]), width: 4.0, fill: false });
        assert_eq!(px(&mut s, 20, 50), (0, 0, 255));
        assert_eq!(px(&mut s, 50, 50), (255, 255, 255));
        render(&s, &Item::Shape { kind: ShapeKind::Rect, a: (20.0, 20.0), b: (80.0, 80.0), color: rgba([0, 0, 255]), width: 4.0, fill: true });
        assert_eq!(px(&mut s, 50, 50), (0, 0, 255));
    }

    #[test]
    fn texto_e_medido_e_desenhado() {
        let (w, h) = measure_text("Olá", 24.0);
        assert!(w > 20.0 && h > 15.0, "{w}x{h}");
        let mut s = canvas((1.0, 1.0, 1.0));
        render(&s, &Item::Text { x: 5.0, y: 5.0, text: "████".into(), color: rgba([0, 0, 0]), size: 40.0, w, h });
        let mut dark = 0;
        for y in 0..100 {
            for x in 0..100 {
                if px(&mut s, x, y).0 < 64 {
                    dark += 1;
                }
            }
        }
        assert!(dark > 300, "{dark}");
    }

    #[test]
    fn borracha_acerta_cada_tipo_de_item() {
        let stroke = Item::Stroke { pts: vec![(0.0, 0.0), (100.0, 0.0)], color: rgba([0, 0, 0]), width: 4.0, hl: false };
        assert!(item_hit(&stroke, (50.0, 5.0), 4.0));
        assert!(!item_hit(&stroke, (50.0, 20.0), 4.0));
        let rect = Item::Shape { kind: ShapeKind::Rect, a: (0.0, 0.0), b: (100.0, 100.0), color: rgba([0, 0, 0]), width: 2.0, fill: false };
        assert!(item_hit(&rect, (100.0, 50.0), 3.0));
        assert!(!item_hit(&rect, (50.0, 50.0), 3.0), "o interior de um retângulo vazado não é tinta");
        let ell = Item::Shape { kind: ShapeKind::Ellipse, a: (0.0, 0.0), b: (100.0, 50.0), color: rgba([0, 0, 0]), width: 2.0, fill: true };
        assert!(item_hit(&ell, (50.0, 25.0), 1.0));
        let text = Item::Text { x: 10.0, y: 10.0, text: "a".into(), color: rgba([0, 0, 0]), size: 20.0, w: 30.0, h: 20.0 };
        assert!(item_hit(&text, (20.0, 20.0), 1.0));
        assert!(!item_hit(&text, (80.0, 80.0), 1.0));
    }

    #[test]
    fn shift_restringe_formas() {
        assert_eq!(constrain(ShapeKind::Rect, (0.0, 0.0), (30.0, -10.0)), (30.0, -30.0));
        let (x, y) = constrain(ShapeKind::Line, (0.0, 0.0), (100.0, 10.0));
        assert!((x - 100.498).abs() < 0.01 && y.abs() < 1e-9);
        let (x, y) = constrain(ShapeKind::Arrow, (0.0, 0.0), (50.0, 45.0));
        assert!((x - y).abs() < 1e-9);
    }
}
