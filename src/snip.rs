//! Sobreposição de captura: congela a tela, escurece, mostra a barra flutuante
//! e devolve a região escolhida (retângulo, forma livre, janela ou tela).

use std::fs::File;
use std::os::fd::{AsFd, AsRawFd, FromRawFd};
use std::time::{Duration, Instant};

use memmap2::MmapMut;
use tiny_skia::{Color, FillRule, Mask, PathBuilder, PixmapMut, PixmapPaint, Transform};
use wayland_client::protocol::{
    wl_buffer::WlBuffer, wl_compositor::WlCompositor, wl_keyboard::WlKeyboard, wl_output, wl_output::WlOutput,
    wl_pointer::WlPointer, wl_seat::WlSeat, wl_shm, wl_shm::WlShm, wl_shm_pool::WlShmPool, wl_surface::WlSurface,
};
use wayland_client::{Connection, EventQueue, QueueHandle};
use wayland_protocols::ext::foreign_toplevel_list::v1::client::{
    ext_foreign_toplevel_handle_v1::ExtForeignToplevelHandleV1, ext_foreign_toplevel_list_v1::ExtForeignToplevelListV1,
};
use wayland_protocols::ext::image_capture_source::v1::client::{
    ext_foreign_toplevel_image_capture_source_manager_v1::ExtForeignToplevelImageCaptureSourceManagerV1,
    ext_output_image_capture_source_manager_v1::ExtOutputImageCaptureSourceManagerV1,
};
use wayland_protocols::ext::image_copy_capture::v1::client::{
    ext_image_copy_capture_frame_v1::ExtImageCopyCaptureFrameV1,
    ext_image_copy_capture_manager_v1::{self, ExtImageCopyCaptureManagerV1},
    ext_image_copy_capture_session_v1::ExtImageCopyCaptureSessionV1,
};
use wayland_protocols::ext::workspace::v1::client::{
    ext_workspace_handle_v1::ExtWorkspaceHandleV1, ext_workspace_manager_v1::ExtWorkspaceManagerV1,
};
use wayland_protocols::wp::cursor_shape::v1::client::{
    wp_cursor_shape_device_v1::{self, WpCursorShapeDeviceV1},
    wp_cursor_shape_manager_v1::WpCursorShapeManagerV1,
};
use wayland_protocols::wp::viewporter::client::{wp_viewport::WpViewport, wp_viewporter::WpViewporter};
use wayland_protocols::xdg::xdg_output::zv1::client::{
    zxdg_output_manager_v1::ZxdgOutputManagerV1, zxdg_output_v1::ZxdgOutputV1,
};
use wayland_protocols_wlr::layer_shell::v1::client::{
    zwlr_layer_shell_v1::{self, ZwlrLayerShellV1},
    zwlr_layer_surface_v1::{self, ZwlrLayerSurfaceV1},
};
use cosmic_protocols::toplevel_info::v1::client::{
    zcosmic_toplevel_handle_v1::ZcosmicToplevelHandleV1, zcosmic_toplevel_info_v1::ZcosmicToplevelInfoV1,
};

use crate::config::Mode;
use crate::draw::{self, color, Icon};
use crate::image::Image;

// ---------------------------------------------------------------------------
// Estruturas

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RectF {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl RectF {
    pub fn from_points(a: (f64, f64), b: (f64, f64)) -> RectF {
        RectF { x: a.0.min(b.0), y: a.1.min(b.1), w: (a.0 - b.0).abs(), h: (a.1 - b.1).abs() }
    }
    pub fn contains(&self, p: (f64, f64)) -> bool {
        p.0 >= self.x && p.1 >= self.y && p.0 < self.x + self.w && p.1 < self.y + self.h
    }
    pub fn intersect(&self, o: &RectF) -> Option<RectF> {
        let x0 = self.x.max(o.x);
        let y0 = self.y.max(o.y);
        let x1 = (self.x + self.w).min(o.x + o.w);
        let y1 = (self.y + self.h).min(o.y + o.h);
        (x1 > x0 && y1 > y0).then(|| RectF { x: x0, y: y0, w: x1 - x0, h: y1 - y0 })
    }
}

/// Retângulo em pixels do buffer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IRect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl IRect {
    fn clamp(self, w: i32, h: i32) -> Option<IRect> {
        let x0 = self.x.clamp(0, w);
        let y0 = self.y.clamp(0, h);
        let x1 = (self.x + self.w).clamp(0, w);
        let y1 = (self.y + self.h).clamp(0, h);
        (x1 > x0 && y1 > y0).then_some(IRect { x: x0, y: y0, w: x1 - x0, h: y1 - y0 })
    }
    fn grow(self, d: i32) -> IRect {
        IRect { x: self.x - d, y: self.y - d, w: self.w + 2 * d, h: self.h + 2 * d }
    }
}

pub struct ShmBuf {
    _file: File,
    pub map: MmapMut,
    pub pool: WlShmPool,
}

pub fn memfd(size: usize) -> std::io::Result<File> {
    let name = c"recorte";
    let fd = unsafe { libc::memfd_create(name.as_ptr(), libc::MFD_CLOEXEC) };
    if fd < 0 {
        return Err(std::io::Error::last_os_error());
    }
    let f = unsafe { File::from_raw_fd(fd) };
    f.set_len(size as u64)?;
    Ok(f)
}

pub struct OvBuf {
    pub buffer: WlBuffer,
    pub offset: usize,
    pub busy: bool,
    pub drawn: bool,
    pub rects: Vec<IRect>,
}

pub struct Surf {
    pub wl: WlSurface,
    pub layer: ZwlrLayerSurfaceV1,
    pub viewport: Option<WpViewport>,
    pub configured: bool,
    pub shm: Option<ShmBuf>,
    pub bufs: Vec<OvBuf>,
    pub frame_pending: bool,
    pub dirty: bool,
}

pub struct Output {
    pub wl: WlOutput,
    pub xdg: Option<ZxdgOutputV1>,
    pub name: String,
    pub lx: i32,
    pub ly: i32,
    pub lw: i32,
    pub lh: i32,
    pub mode_w: i32,
    pub mode_h: i32,
    pub scale: i32,
    pub transform: wl_output::Transform,
    pub img: Option<Image>,
    pub dim: Vec<u8>,
    pub surf: Option<Surf>,
}

impl Output {
    pub fn logical(&self) -> RectF {
        RectF { x: self.lx as f64, y: self.ly as f64, w: self.lw as f64, h: self.lh as f64 }
    }
    /// Pixels físicos por unidade lógica.
    pub fn sx(&self) -> f64 {
        match &self.img {
            Some(i) if self.lw > 0 => i.w as f64 / self.lw as f64,
            _ => self.scale.max(1) as f64,
        }
    }
    pub fn sy(&self) -> f64 {
        match &self.img {
            Some(i) if self.lh > 0 => i.h as f64 / self.lh as f64,
            _ => self.scale.max(1) as f64,
        }
    }
}

pub struct Workspace {
    pub handle: ExtWorkspaceHandleV1,
    pub active: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CapState {
    None,
    Pending,
    Done,
    Failed,
}

pub struct Toplevel {
    pub handle: ExtForeignToplevelHandleV1,
    pub cosmic: Option<ZcosmicToplevelHandleV1>,
    pub title: String,
    pub app_id: String,
    pub geoms: Vec<(WlOutput, i32, i32, i32, i32)>,
    pub minimized: bool,
    pub activated: bool,
    pub workspaces: Vec<ExtWorkspaceHandleV1>,
    pub closed: bool,
    pub img: Option<Image>,
    /// Deslocamento do conteúdo (sem sombras) dentro do buffer capturado.
    pub offset: Option<(i32, i32)>,
    pub cap: CapState,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum JobKind {
    Output(usize),
    Toplevel(usize),
}

pub struct Job {
    pub kind: JobKind,
    pub session: Option<ExtImageCopyCaptureSessionV1>,
    pub frame: Option<ExtImageCopyCaptureFrameV1>,
    pub w: u32,
    pub h: u32,
    pub formats: Vec<wl_shm::Format>,
    pub format: wl_shm::Format,
    pub shm: Option<ShmBuf>,
    pub buffer: Option<WlBuffer>,
    pub transform: wl_output::Transform,
    pub finished: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Btn {
    Mode(Mode),
    Close,
}

pub enum Outcome {
    Cancel,
    Rect(RectF),
    Free(Vec<(f64, f64)>),
    Window(usize),
    Output(usize),
}

pub struct Globals {
    pub compositor: Option<WlCompositor>,
    pub shm: Option<WlShm>,
    pub seat: Option<WlSeat>,
    pub layer_shell: Option<ZwlrLayerShellV1>,
    pub viewporter: Option<WpViewporter>,
    pub cursor_mgr: Option<WpCursorShapeManagerV1>,
    pub xdg_output_mgr: Option<ZxdgOutputManagerV1>,
    pub out_src: Option<ExtOutputImageCaptureSourceManagerV1>,
    pub top_src: Option<ExtForeignToplevelImageCaptureSourceManagerV1>,
    pub copy: Option<ExtImageCopyCaptureManagerV1>,
    pub toplevel_list: Option<ExtForeignToplevelListV1>,
    pub cosmic_info: Option<ZcosmicToplevelInfoV1>,
    pub workspace_mgr: Option<ExtWorkspaceManagerV1>,
}

pub struct App {
    pub qh: QueueHandle<App>,
    pub registry: Option<wayland_client::protocol::wl_registry::WlRegistry>,
    pub g: Globals,
    pub pending_globals: Vec<(u32, String, u32)>,
    pub outputs: Vec<Output>,
    pub toplevels: Vec<Toplevel>,
    pub workspaces: Vec<Workspace>,
    pub jobs: Vec<Job>,
    pub pointer: Option<WlPointer>,
    pub keyboard: Option<WlKeyboard>,
    pub cursor_dev: Option<WpCursorShapeDeviceV1>,
    pub cursor_serial: u32,
    pub cursor_shape: Option<wp_cursor_shape_device_v1::Shape>,

    pub mode: Mode,
    pub dim_enabled: bool,
    pub pointer_out: Option<usize>,
    pub pos: Option<(f64, f64)>,
    pub active_out: usize,
    pub drag_start: Option<(f64, f64)>,
    pub free_path: Vec<(f64, f64)>,
    pub hover_btn: Option<Btn>,
    pub pressed_btn: Option<Btn>,
    pub hover_since: Instant,
    pub tooltip_shown: bool,
    pub hover_window: Option<usize>,
    pub hover_output: Option<usize>,
    pub outcome: Option<Outcome>,
    pub accent: [u8; 3],
    pub font: Option<crate::font::Font>,
    pub font_tried: bool,
    pub shown_at: Option<Instant>,
    /// Geometria/estado de janelas mudou (o COSMIC envia isso alguns ms depois).
    pub windows_changed: bool,
}

// ---------------------------------------------------------------------------
// Inicialização e captura

const TOOLBAR_TOP: f64 = 12.0;
const BTN: f64 = 40.0;
const PAD: f64 = 4.0;
const GAP: f64 = 2.0;
const SEP: f64 = 9.0;
const ANIM: Duration = Duration::from_millis(180);
const TOOLTIP_DELAY: Duration = Duration::from_millis(550);

pub fn toolbar_size() -> (f64, f64) {
    (PAD * 2.0 + BTN * 5.0 + GAP * 3.0 + SEP, BTN + PAD * 2.0)
}

/// Botões na ordem do Windows 11 e suas posições relativas à barra.
fn toolbar_buttons() -> [(Btn, f64); 5] {
    let mut x = PAD;
    let mut out = [(Btn::Close, 0.0); 5];
    for (i, m) in Mode::ALL.iter().enumerate() {
        out[i] = (Btn::Mode(*m), x);
        x += BTN + GAP;
    }
    out[4] = (Btn::Close, x - GAP + SEP);
    out
}

impl App {
    pub fn connect(mode: Mode, dim: bool) -> Result<(Connection, EventQueue<App>, App), String> {
        let conn = Connection::connect_to_env().map_err(|e| format!("sem conexão Wayland: {e}"))?;
        let mut queue = conn.new_event_queue::<App>();
        let qh = queue.handle();
        let mut app = App {
            qh: qh.clone(),
            registry: None,
            g: Globals {
                compositor: None,
                shm: None,
                seat: None,
                layer_shell: None,
                viewporter: None,
                cursor_mgr: None,
                xdg_output_mgr: None,
                out_src: None,
                top_src: None,
                copy: None,
                toplevel_list: None,
                cosmic_info: None,
                workspace_mgr: None,
            },
            pending_globals: Vec::new(),
            outputs: Vec::new(),
            toplevels: Vec::new(),
            workspaces: Vec::new(),
            jobs: Vec::new(),
            pointer: None,
            keyboard: None,
            cursor_dev: None,
            cursor_serial: 0,
            cursor_shape: None,
            mode,
            dim_enabled: dim,
            pointer_out: None,
            pos: None,
            active_out: 0,
            drag_start: None,
            free_path: Vec::new(),
            hover_btn: None,
            pressed_btn: None,
            hover_since: Instant::now(),
            tooltip_shown: false,
            hover_window: None,
            hover_output: None,
            outcome: None,
            accent: crate::theme::accent(),
            font: None,
            font_tried: false,
            shown_at: None,
            windows_changed: false,
        };
        app.registry = Some(conn.display().get_registry(&qh, ()));
        queue.roundtrip(&mut app).map_err(|e| e.to_string())?;
        app.bind_globals();
        if app.g.copy.is_none() || app.g.out_src.is_none() {
            return Err("o compositor não oferece ext-image-copy-capture-v1".into());
        }
        if app.g.layer_shell.is_none() || app.g.compositor.is_none() || app.g.shm.is_none() {
            return Err("o compositor não oferece wlr-layer-shell".into());
        }
        // Saídas e áreas de trabalho primeiro; depois as janelas, que fazem referência a elas.
        queue.roundtrip(&mut app).map_err(|e| e.to_string())?;
        queue.roundtrip(&mut app).map_err(|e| e.to_string())?;
        queue.roundtrip(&mut app).map_err(|e| e.to_string())?;
        app.outputs.retain(|o| o.lw > 0 && o.lh > 0);
        if app.outputs.is_empty() {
            return Err("nenhum monitor encontrado".into());
        }
        Ok((conn, queue, app))
    }

    fn bind_globals(&mut self) {
        let globals = std::mem::take(&mut self.pending_globals);
        // A ordem importa: gerenciador de xdg_output e áreas de trabalho antes das janelas.
        let order = |i: &str| match i {
            "zxdg_output_manager_v1" => 0,
            "wl_output" => 1,
            "ext_workspace_manager_v1" => 2,
            "zcosmic_toplevel_info_v1" => 3,
            "ext_foreign_toplevel_list_v1" => 4,
            _ => 1,
        };
        let mut globals = globals;
        globals.sort_by_key(|g| order(&g.1));
        for (name, interface, version) in globals {
            self.bind_global(name, &interface, version);
        }
    }

    pub fn bind_global(&mut self, name: u32, interface: &str, version: u32) {
        let qh = self.qh.clone();
        let reg = self.registry();
        let Some(reg) = reg else { return };
        match interface {
            "wl_compositor" => self.g.compositor = Some(reg.bind(name, version.min(5), &qh, ())),
            "wl_shm" => self.g.shm = Some(reg.bind(name, 1, &qh, ())),
            "wl_seat" if self.g.seat.is_none() => self.g.seat = Some(reg.bind(name, version.min(7), &qh, ())),
            "zwlr_layer_shell_v1" => self.g.layer_shell = Some(reg.bind(name, version.min(4), &qh, ())),
            "wp_viewporter" => self.g.viewporter = Some(reg.bind(name, 1, &qh, ())),
            "wp_cursor_shape_manager_v1" => self.g.cursor_mgr = Some(reg.bind(name, 1, &qh, ())),
            "zxdg_output_manager_v1" => self.g.xdg_output_mgr = Some(reg.bind(name, version.min(3), &qh, ())),
            "ext_output_image_capture_source_manager_v1" => self.g.out_src = Some(reg.bind(name, 1, &qh, ())),
            "ext_foreign_toplevel_image_capture_source_manager_v1" => self.g.top_src = Some(reg.bind(name, 1, &qh, ())),
            "ext_image_copy_capture_manager_v1" => self.g.copy = Some(reg.bind(name, 1, &qh, ())),
            "ext_workspace_manager_v1" => self.g.workspace_mgr = Some(reg.bind(name, 1, &qh, ())),
            "zcosmic_toplevel_info_v1" if version >= 2 => {
                self.g.cosmic_info = Some(reg.bind(name, version.min(3), &qh, ()))
            }
            "ext_foreign_toplevel_list_v1" => self.g.toplevel_list = Some(reg.bind(name, 1, &qh, ())),
            "wl_output" => {
                let wl: WlOutput = reg.bind(name, version.min(4), &qh, ());
                let xdg = self.g.xdg_output_mgr.as_ref().map(|m| m.get_xdg_output(&wl, &qh, ()));
                self.outputs.push(Output {
                    wl,
                    xdg,
                    name: String::new(),
                    lx: 0,
                    ly: 0,
                    lw: 0,
                    lh: 0,
                    mode_w: 0,
                    mode_h: 0,
                    scale: 1,
                    transform: wl_output::Transform::Normal,
                    img: None,
                    dim: Vec::new(),
                    surf: None,
                });
            }
            _ => {}
        }
    }

    fn registry(&self) -> Option<wayland_client::protocol::wl_registry::WlRegistry> {
        self.registry.clone()
    }

    /// Captura todas as saídas (tela congelada). Bloqueia até terminar.
    pub fn capture_outputs(&mut self, queue: &mut EventQueue<App>) -> Result<(), String> {
        for i in 0..self.outputs.len() {
            let src = self.g.out_src.as_ref().unwrap().create_source(&self.outputs[i].wl, &self.qh, ());
            self.start_job(JobKind::Output(i), &src);
            src.destroy();
        }
        let deadline = Instant::now() + Duration::from_secs(5);
        while self.jobs.iter().any(|j| matches!(j.kind, JobKind::Output(_)) && !j.finished) {
            if Instant::now() > deadline {
                return Err("tempo esgotado ao capturar a tela".into());
            }
            queue.blocking_dispatch(self).map_err(|e| e.to_string())?;
        }
        self.jobs.retain(|j| !j.finished);
        if self.outputs.iter().all(|o| o.img.is_none()) {
            return Err("o compositor recusou a captura de tela".into());
        }
        self.outputs.retain(|o| o.img.is_some());
        Ok(())
    }

    pub fn start_job(&mut self, kind: JobKind, src: &wayland_protocols::ext::image_capture_source::v1::client::ext_image_capture_source_v1::ExtImageCaptureSourceV1) {
        let idx = self.jobs.len();
        let session = self.g.copy.as_ref().unwrap().create_session(
            src,
            ext_image_copy_capture_manager_v1::Options::empty(),
            &self.qh,
            idx,
        );
        self.jobs.push(Job {
            kind,
            session: Some(session),
            frame: None,
            w: 0,
            h: 0,
            formats: Vec::new(),
            format: wl_shm::Format::Argb8888,
            shm: None,
            buffer: None,
            transform: wl_output::Transform::Normal,
            finished: false,
        });
    }

    /// Restrições conhecidas (evento `done` da sessão): aloca o buffer e captura.
    pub fn job_constraints_done(&mut self, idx: usize) {
        let qh = self.qh.clone();
        let shm = self.g.shm.clone().unwrap();
        let job = &mut self.jobs[idx];
        if job.frame.is_some() || job.finished {
            return;
        }
        let pref = [wl_shm::Format::Argb8888, wl_shm::Format::Xrgb8888, wl_shm::Format::Abgr8888, wl_shm::Format::Xbgr8888];
        let Some(format) = pref.iter().copied().find(|f| job.formats.contains(f)) else {
            job.finished = true;
            return;
        };
        if job.w == 0 || job.h == 0 {
            job.finished = true;
            return;
        }
        let stride = job.w as usize * 4;
        let size = stride * job.h as usize;
        let Ok(file) = memfd(size) else {
            job.finished = true;
            return;
        };
        let Ok(map) = (unsafe { MmapMut::map_mut(&file) }) else {
            job.finished = true;
            return;
        };
        let pool = shm.create_pool(file.as_fd(), size as i32, &qh, ());
        let buffer = pool.create_buffer(0, job.w as i32, job.h as i32, stride as i32, format, &qh, BufTag::Capture);
        let frame = job.session.as_ref().unwrap().create_frame(&qh, idx);
        frame.attach_buffer(&buffer);
        frame.damage_buffer(0, 0, job.w as i32, job.h as i32);
        frame.capture();
        job.format = format;
        job.shm = Some(ShmBuf { _file: file, map, pool });
        job.buffer = Some(buffer);
        job.frame = Some(frame);
    }

    pub fn job_ready(&mut self, idx: usize) {
        let job = &mut self.jobs[idx];
        let Some(shm) = job.shm.as_ref() else { return };
        let mut data = shm.map[..(job.w * job.h * 4) as usize].to_vec();
        let swap = matches!(job.format, wl_shm::Format::Abgr8888 | wl_shm::Format::Xbgr8888);
        let opaque = matches!(job.format, wl_shm::Format::Xrgb8888 | wl_shm::Format::Xbgr8888);
        for p in data.chunks_exact_mut(4) {
            if swap {
                p.swap(0, 2);
            }
            if opaque {
                p[3] = 255;
            }
        }
        let img = apply_transform(Image { w: job.w, h: job.h, data }, job.transform);
        let kind = job.kind;
        self.finish_job(idx);
        match kind {
            JobKind::Output(i) => {
                if let Some(o) = self.outputs.get_mut(i) {
                    o.img = Some(img);
                }
            }
            JobKind::Toplevel(i) => {
                if let Some(t) = self.toplevels.get_mut(i) {
                    t.img = Some(img);
                    t.cap = CapState::Done;
                }
                self.estimate_offset(i);
                self.update_hover();
            }
        }
    }

    pub fn finish_job(&mut self, idx: usize) {
        let job = &mut self.jobs[idx];
        if let Some(f) = job.frame.take() {
            f.destroy();
        }
        if let Some(s) = job.session.take() {
            s.destroy();
        }
        if let Some(b) = job.buffer.take() {
            b.destroy();
        }
        if let Some(shm) = job.shm.take() {
            shm.pool.destroy();
        }
        if !job.finished {
            job.finished = true;
            if let JobKind::Toplevel(i) = job.kind {
                if let Some(t) = self.toplevels.get_mut(i) {
                    if t.cap == CapState::Pending {
                        t.cap = CapState::Failed;
                    }
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // Janelas

    pub fn toplevel_rect(&self, i: usize) -> Option<RectF> {
        let t = &self.toplevels[i];
        for (wl, x, y, w, h) in &t.geoms {
            if let Some(o) = self.outputs.iter().find(|o| &o.wl == wl) {
                if *w > 0 && *h > 0 {
                    return Some(RectF { x: (o.lx + x) as f64, y: (o.ly + y) as f64, w: *w as f64, h: *h as f64 });
                }
            }
        }
        None
    }

    fn toplevel_visible(&self, i: usize) -> bool {
        let t = &self.toplevels[i];
        if t.closed || t.minimized || self.toplevel_rect(i).is_none() {
            return false;
        }
        if t.workspaces.is_empty() {
            return true;
        }
        t.workspaces.iter().any(|w| self.workspaces.iter().any(|ws| &ws.handle == w && ws.active))
    }

    pub fn start_toplevel_captures(&mut self) {
        if self.g.top_src.is_none() {
            return;
        }
        for i in 0..self.toplevels.len() {
            if self.toplevels[i].cap != CapState::None || !self.toplevel_visible(i) {
                continue;
            }
            self.toplevels[i].cap = CapState::Pending;
            let handle = self.toplevels[i].handle.clone();
            let src = self.g.top_src.as_ref().unwrap().create_source(&handle, &self.qh, ());
            self.start_job(JobKind::Toplevel(i), &src);
            src.destroy();
        }
    }

    fn output_at(&self, p: (f64, f64)) -> Option<usize> {
        self.outputs.iter().position(|o| o.logical().contains(p))
    }

    pub fn screen_pixel(&self, p: (f64, f64)) -> Option<[u8; 4]> {
        let o = &self.outputs[self.output_at(p)?];
        let img = o.img.as_ref()?;
        img.pixel(((p.0 - o.lx as f64) * o.sx()) as i32, ((p.1 - o.ly as f64) * o.sy()) as i32)
    }

    fn window_scale(&self, r: &RectF) -> f64 {
        let c = (r.x + r.w / 2.0, r.y + r.h / 2.0);
        self.output_at(c).or_else(|| self.output_at((r.x, r.y))).map(|i| self.outputs[i].sx()).unwrap_or(1.0)
    }

    /// Acha onde o conteúdo da janela está no buffer capturado (sombras do
    /// cliente aumentam o buffer) comparando com a tela congelada.
    fn estimate_offset(&mut self, i: usize) {
        let Some(r) = self.toplevel_rect(i) else { return };
        let s = self.window_scale(&r);
        let Some(img) = self.toplevels[i].img.as_ref() else { return };
        let ew = (r.w * s).round() as i32;
        let eh = (r.h * s).round() as i32;
        let maxdx = img.w as i32 - ew;
        let maxdy = img.h as i32 - eh;
        if maxdx < -2 || maxdy < -2 {
            self.toplevels[i].offset = None;
            return;
        }
        let (maxdx, maxdy) = (maxdx.clamp(0, 256), maxdy.clamp(0, 256));
        let mut samples = Vec::new();
        for gy in 0..9 {
            for gx in 0..9 {
                let p = (r.x + r.w * (0.12 + 0.76 * gx as f64 / 8.0), r.y + r.h * (0.12 + 0.76 * gy as f64 / 8.0));
                if let Some(px) = self.screen_pixel(p) {
                    samples.push((((p.0 - r.x) * s) as i32, ((p.1 - r.y) * s) as i32, px));
                }
            }
        }
        let mut best = ((maxdx / 2, maxdy / 2), -1i32);
        if !samples.is_empty() && (maxdx > 0 || maxdy > 0) {
            for dy in 0..=maxdy {
                for dx in 0..=maxdx {
                    let score = samples
                        .iter()
                        .filter(|(x, y, px)| img.pixel(x + dx, y + dy).is_some_and(|q| close(q, *px)))
                        .count() as i32;
                    if score > best.1 {
                        best = ((dx, dy), score);
                    }
                }
            }
        }
        self.toplevels[i].offset = Some(if maxdx == 0 && maxdy == 0 { (0, 0) } else { best.0 });
    }

    /// Fração dos pontos ao redor de `p` em que a janela capturada coincide com a tela.
    fn match_score(&self, i: usize, p: (f64, f64)) -> Option<f64> {
        let t = &self.toplevels[i];
        let img = t.img.as_ref()?;
        let (dx, dy) = t.offset?;
        let r = self.toplevel_rect(i)?;
        let s = self.window_scale(&r);
        let (mut hits, mut total) = (0, 0);
        for oy in -2..=2 {
            for ox in -2..=2 {
                let q = (p.0 + ox as f64 * 3.0, p.1 + oy as f64 * 3.0);
                if !r.contains(q) {
                    continue;
                }
                let Some(sp) = self.screen_pixel(q) else { continue };
                total += 1;
                let x = ((q.0 - r.x) * s) as i32 + dx;
                let y = ((q.1 - r.y) * s) as i32 + dy;
                if img.pixel(x, y).is_some_and(|wp| close(wp, sp)) {
                    hits += 1;
                }
            }
        }
        (total > 0).then(|| hits as f64 / total as f64)
    }

    fn window_at(&self, p: (f64, f64)) -> Option<usize> {
        let mut cands: Vec<usize> = (0..self.toplevels.len())
            .filter(|&i| self.toplevel_visible(i) && self.toplevel_rect(i).is_some_and(|r| r.contains(p)))
            .collect();
        if cands.len() <= 1 {
            return cands.pop();
        }
        let area = |i: usize| self.toplevel_rect(i).map(|r| r.w * r.h).unwrap_or(f64::MAX);
        let key = |i: usize| {
            let score = self.match_score(i, p).unwrap_or(-1.0);
            (score, self.toplevels[i].activated, -area(i))
        };
        cands.into_iter().max_by(|&a, &b| {
            let (ka, kb) = (key(a), key(b));
            ka.0.partial_cmp(&kb.0)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(ka.1.cmp(&kb.1))
                .then(ka.2.partial_cmp(&kb.2).unwrap_or(std::cmp::Ordering::Equal))
        })
    }

    // -----------------------------------------------------------------------
    // Sobreposição

    pub fn create_surfaces(&mut self) {
        let qh = self.qh.clone();
        for i in 0..self.outputs.len() {
            let wl = self.g.compositor.as_ref().unwrap().create_surface(&qh, ());
            let layer = self.g.layer_shell.as_ref().unwrap().get_layer_surface(
                &wl,
                Some(&self.outputs[i].wl),
                zwlr_layer_shell_v1::Layer::Overlay,
                "recorte".into(),
                &qh,
                i,
            );
            use zwlr_layer_surface_v1::Anchor;
            layer.set_anchor(Anchor::Top | Anchor::Bottom | Anchor::Left | Anchor::Right);
            layer.set_exclusive_zone(-1);
            layer.set_keyboard_interactivity(zwlr_layer_surface_v1::KeyboardInteractivity::Exclusive);
            layer.set_size(0, 0);
            let viewport = self.g.viewporter.as_ref().map(|v| v.get_viewport(&wl, &qh, ()));
            wl.commit();
            self.outputs[i].surf = Some(Surf {
                wl,
                layer,
                viewport,
                configured: false,
                shm: None,
                bufs: Vec::new(),
                frame_pending: false,
                dirty: true,
            });
        }
    }

    pub fn destroy_surfaces(&mut self) {
        for o in &mut self.outputs {
            if let Some(s) = o.surf.take() {
                for b in s.bufs {
                    b.buffer.destroy();
                }
                if let Some(shm) = s.shm {
                    shm.pool.destroy();
                }
                if let Some(v) = s.viewport {
                    v.destroy();
                }
                s.layer.destroy();
                s.wl.destroy();
            }
        }
    }

    pub fn compute_dim(&mut self) {
        let factor: u32 = if self.dim_enabled { 128 } else { 256 };
        for o in &mut self.outputs {
            let Some(img) = &o.img else { continue };
            o.dim = img
                .data
                .chunks_exact(4)
                .flat_map(|p| {
                    [
                        ((p[0] as u32 * factor) >> 8) as u8,
                        ((p[1] as u32 * factor) >> 8) as u8,
                        ((p[2] as u32 * factor) >> 8) as u8,
                        p[3],
                    ]
                })
                .collect();
        }
    }

    pub fn mark_all_dirty(&mut self) {
        for o in &mut self.outputs {
            if let Some(s) = &mut o.surf {
                s.dirty = true;
            }
        }
    }

    pub fn toolbar_rect(&self) -> Option<RectF> {
        let o = self.outputs.get(self.active_out)?;
        let (tw, th) = toolbar_size();
        let t = self.anim_progress();
        let y = TOOLBAR_TOP - (1.0 - t) * (th + TOOLBAR_TOP + 6.0);
        Some(RectF { x: o.lx as f64 + ((o.lw as f64 - tw) / 2.0).round(), y: o.ly as f64 + y, w: tw, h: th })
    }

    fn anim_progress(&self) -> f64 {
        let Some(t0) = self.shown_at else { return 0.0 };
        let t = (t0.elapsed().as_secs_f64() / ANIM.as_secs_f64()).min(1.0);
        1.0 - (1.0 - t).powi(3)
    }

    fn button_at(&self, p: (f64, f64)) -> Option<Btn> {
        let tb = self.toolbar_rect()?;
        if !tb.contains(p) || self.drag_start.is_some() {
            return None;
        }
        toolbar_buttons()
            .into_iter()
            .find(|(_, bx)| RectF { x: tb.x + bx, y: tb.y + PAD, w: BTN, h: BTN }.contains(p))
            .map(|(b, _)| b)
    }

    pub fn update_hover(&mut self) {
        let Some(p) = self.pos else { return };
        let btn = self.button_at(p);
        if btn != self.hover_btn {
            self.hover_btn = btn;
            self.hover_since = Instant::now();
            self.tooltip_shown = false;
        }
        let on_toolbar = self.toolbar_rect().is_some_and(|r| r.contains(p)) && self.drag_start.is_none();
        self.hover_window = None;
        self.hover_output = None;
        if !on_toolbar && self.drag_start.is_none() {
            match self.mode {
                Mode::Window => self.hover_window = self.window_at(p),
                Mode::Screen => self.hover_output = self.output_at(p),
                _ => {}
            }
        }
        let shape = if on_toolbar || matches!(self.mode, Mode::Window | Mode::Screen) && self.drag_start.is_none() {
            wp_cursor_shape_device_v1::Shape::Default
        } else {
            wp_cursor_shape_device_v1::Shape::Crosshair
        };
        if self.cursor_shape != Some(shape) {
            if let Some(dev) = &self.cursor_dev {
                dev.set_shape(self.cursor_serial, shape);
            }
            self.cursor_shape = Some(shape);
        }
        self.mark_all_dirty();
    }

    pub fn set_mode(&mut self, m: Mode) {
        self.mode = m;
        self.drag_start = None;
        self.free_path.clear();
        if m == Mode::Window {
            self.start_toplevel_captures();
        }
        if m == Mode::Screen && self.outputs.len() == 1 {
            self.outcome = Some(Outcome::Output(0));
            return;
        }
        self.update_hover();
    }

    pub fn on_press(&mut self) {
        let Some(p) = self.pos else { return };
        if let Some(b) = self.button_at(p) {
            self.pressed_btn = Some(b);
            self.mark_all_dirty();
            return;
        }
        match self.mode {
            Mode::Rect => self.drag_start = Some(p),
            Mode::Free => {
                self.drag_start = Some(p);
                self.free_path = vec![p];
            }
            Mode::Window => {
                if let Some(w) = self.window_at(p) {
                    self.outcome = Some(Outcome::Window(w));
                }
            }
            Mode::Screen => {
                if let Some(o) = self.output_at(p) {
                    self.outcome = Some(Outcome::Output(o));
                }
            }
        }
        self.update_hover();
    }

    pub fn on_release(&mut self) {
        let p = self.pos.unwrap_or((0.0, 0.0));
        if let Some(b) = self.pressed_btn.take() {
            if self.button_at(p) == Some(b) {
                match b {
                    Btn::Close => self.outcome = Some(Outcome::Cancel),
                    Btn::Mode(m) => self.set_mode(m),
                }
            }
            self.mark_all_dirty();
            return;
        }
        let Some(start) = self.drag_start.take() else { return };
        match self.mode {
            Mode::Rect => {
                let r = RectF::from_points(start, p);
                if r.w >= 2.0 && r.h >= 2.0 {
                    self.outcome = Some(Outcome::Rect(r));
                }
            }
            Mode::Free => {
                let path = std::mem::take(&mut self.free_path);
                let bb = bbox(&path);
                if path.len() > 2 && bb.w >= 2.0 && bb.h >= 2.0 {
                    self.outcome = Some(Outcome::Free(path));
                }
            }
            _ => {}
        }
        self.update_hover();
    }

    pub fn on_motion(&mut self, p: (f64, f64)) {
        self.pos = Some(p);
        if self.drag_start.is_none() {
            if let Some(o) = self.pointer_out {
                if o != self.active_out && self.pressed_btn.is_none() {
                    self.active_out = o;
                }
            }
        }
        if self.mode == Mode::Free && self.drag_start.is_some() {
            if self.free_path.last().is_none_or(|l| (l.0 - p.0).hypot(l.1 - p.1) >= 1.0) {
                self.free_path.push(p);
            }
        }
        self.update_hover();
    }

    /// Tempo até o próximo evento interno (animação ou dica).
    pub fn next_timeout(&self) -> Option<Duration> {
        if self.anim_progress() < 1.0 {
            return Some(Duration::from_millis(8));
        }
        if self.hover_btn.is_some() && !self.tooltip_shown {
            return Some(TOOLTIP_DELAY.saturating_sub(self.hover_since.elapsed()));
        }
        None
    }

    pub fn tick(&mut self) {
        if std::mem::take(&mut self.windows_changed) && self.mode == Mode::Window {
            self.start_toplevel_captures();
            self.update_hover();
        }
        if self.anim_progress() < 1.0 {
            if let Some(s) = self.outputs.get_mut(self.active_out).and_then(|o| o.surf.as_mut()) {
                s.dirty = true;
            }
        }
        if self.hover_btn.is_some() && !self.tooltip_shown && self.hover_since.elapsed() >= TOOLTIP_DELAY {
            self.tooltip_shown = true;
            self.mark_all_dirty();
        }
        for i in 0..self.outputs.len() {
            self.render(i);
        }
    }

    fn ensure_buffers(&mut self, oi: usize) -> bool {
        let qh = self.qh.clone();
        let shm = self.g.shm.clone().unwrap();
        let o = &mut self.outputs[oi];
        let Some(img) = &o.img else { return false };
        let (w, h) = (img.w as usize, img.h as usize);
        let s = o.surf.as_mut().unwrap();
        if s.shm.is_some() {
            return true;
        }
        let size = w * h * 4;
        let Ok(file) = memfd(size * 2) else { return false };
        let Ok(map) = (unsafe { MmapMut::map_mut(&file) }) else { return false };
        let pool = shm.create_pool(file.as_fd(), (size * 2) as i32, &qh, ());
        for b in 0..2 {
            let buffer = pool.create_buffer(
                (b * size) as i32,
                w as i32,
                h as i32,
                (w * 4) as i32,
                wl_shm::Format::Argb8888,
                &qh,
                BufTag::Overlay(oi, b),
            );
            s.bufs.push(OvBuf { buffer, offset: b * size, busy: false, drawn: false, rects: Vec::new() });
        }
        s.shm = Some(ShmBuf { _file: file, map, pool });
        true
    }

    fn bright_region(&self, oi: usize) -> Option<RectF> {
        let o = &self.outputs[oi];
        let r = match self.mode {
            Mode::Rect => RectF::from_points(self.drag_start?, self.pos?),
            Mode::Window => self.toplevel_rect(self.hover_window?)?,
            Mode::Screen if self.hover_output == Some(oi) => o.logical(),
            _ => return None,
        };
        r.intersect(&o.logical())
    }

    fn render(&mut self, oi: usize) {
        {
            let Some(s) = self.outputs[oi].surf.as_ref() else { return };
            if !s.configured || !s.dirty || s.frame_pending {
                return;
            }
        }
        if !self.ensure_buffers(oi) {
            return;
        }
        if !self.font_tried && self.tooltip_shown {
            self.font_tried = true;
            self.font = crate::font::Font::load();
        }
        let bright = self.bright_region(oi);
        let toolbar = (oi == self.active_out).then(|| self.toolbar_rect()).flatten();
        let mode = self.mode;
        let accent = self.accent;
        let hover_btn = self.hover_btn;
        let pressed_btn = self.pressed_btn;
        let tooltip = self.tooltip_shown;
        let free_path = if mode == Mode::Free && self.drag_start.is_some() { self.free_path.clone() } else { Vec::new() };
        let dim_enabled = self.dim_enabled;
        let font = self.font.take();

        let o = &mut self.outputs[oi];
        let (sx, sy) = (o.sx(), o.sy());
        let img = o.img.as_ref().unwrap();
        let (w, h) = (img.w as i32, img.h as i32);
        let s = o.surf.as_mut().unwrap();
        let Some(bi) = s.bufs.iter().position(|b| !b.busy) else {
            self.font = font;
            return;
        };

        // Conteúdo novo deste quadro, em pixels do buffer.
        let mut rects: Vec<IRect> = Vec::new();
        let bright_px = bright.map(|r| Self::to_phys_static(o.lx, o.ly, sx, sy, &r));
        if let Some(b) = bright_px {
            rects.push(b.grow((3.0 * sx).ceil() as i32));
        }
        let tb_px = toolbar.map(|r| Self::to_phys_static(o.lx, o.ly, sx, sy, &r).grow((16.0 * sx) as i32));
        let tip_px = tb_px.map(|r| IRect { x: r.x - (80.0 * sx) as i32, y: r.y, w: r.w + (160.0 * sx) as i32, h: r.h + (40.0 * sy) as i32 });
        if let Some(r) = tip_px.filter(|_| tooltip && hover_btn.is_some()).or(tb_px) {
            rects.push(r);
        }
        let path_px = if free_path.len() > 1 {
            let bb = bbox(&free_path);
            Some(Self::to_phys_static(o.lx, o.ly, sx, sy, &bb).grow((6.0 * sx).ceil() as i32))
        } else {
            None
        };
        if let Some(r) = path_px {
            rects.push(r);
        }
        let rects: Vec<IRect> = rects.into_iter().filter_map(|r| r.clamp(w, h)).collect();

        let shm = s.shm.as_mut().unwrap();
        let buf = &mut s.bufs[bi];
        let size = (w * h * 4) as usize;
        let mem = &mut shm.map[buf.offset..buf.offset + size];
        let stride = w as usize * 4;

        // 1. Restaura o fundo escurecido onde havia ou haverá conteúdo.
        let mut damage: Vec<IRect> = Vec::new();
        if !buf.drawn {
            mem.copy_from_slice(&o.dim);
            damage.push(IRect { x: 0, y: 0, w, h });
            buf.drawn = true;
        } else {
            for r in buf.rects.iter().chain(rects.iter()) {
                copy_rect(mem, &o.dim, stride, r);
                damage.push(*r);
            }
        }
        // 2. Região clara (seleção, janela ou monitor).
        if let Some(b) = bright_px.and_then(|b| b.clamp(w, h)) {
            copy_rect(mem, &img.data, stride, &b);
        }
        // 3. Vetores: borda, traço da forma livre e barra.
        let mut pm = PixmapMut::from_bytes(mem, w as u32, h as u32).unwrap();
        if let Some(b) = bright_px {
            let (x, y, bw, bh) = (b.x as f32, b.y as f32, b.w as f32, b.h as f32);
            match mode {
                Mode::Rect => {
                    let lw = (1.0 * sx as f32).max(1.0);
                    draw::stroke_rrect(&mut pm, x - lw / 2.0, y - lw / 2.0, bw + lw, bh + lw, 0.0, lw, color(255, 255, 255, 200));
                }
                _ if !dim_enabled => {
                    let lw = (3.0 * sx as f32).max(2.0);
                    let c = color(accent[0], accent[1], accent[2], 255);
                    draw::stroke_rrect(&mut pm, x + lw / 2.0, y + lw / 2.0, bw - lw, bh - lw, 0.0, lw, c);
                }
                _ => {}
            }
        }
        if free_path.len() > 1 {
            let mut pb = PathBuilder::new();
            pb.move_to(free_path[0].0 as f32, free_path[0].1 as f32);
            for p in &free_path[1..] {
                pb.line_to(p.0 as f32, p.1 as f32);
            }
            if let Some(path) = pb.finish() {
                let t = Transform::from_row(sx as f32, 0.0, 0.0, sy as f32, (-o.lx as f64 * sx) as f32, (-o.ly as f64 * sy) as f32);
                let mut st = draw::stroke(3.0 / sx as f32);
                pm.stroke_path(&path, &draw::paint(color(0, 0, 0, 160)), &st, t, None);
                st.width = 1.6 / sx as f32;
                pm.stroke_path(&path, &draw::paint(color(255, 255, 255, 255)), &st, t, None);
            }
        }
        if let Some(tb) = toolbar {
            let px = Self::to_phys_static(o.lx, o.ly, sx, sy, &tb);
            draw_toolbar(&mut pm, px.x as f32, px.y as f32, sx as f32, mode, hover_btn, pressed_btn, accent);
            if tooltip {
                if let (Some(btn), Some(font)) = (hover_btn, font.as_ref()) {
                    draw_tooltip(&mut pm, font, px.x as f32, px.y as f32, sx as f32, btn);
                }
            }
        }

        buf.rects = rects;
        buf.busy = true;
        s.wl.attach(Some(&buf.buffer), 0, 0);
        for d in damage.iter().filter_map(|d| d.clamp(w, h)) {
            s.wl.damage_buffer(d.x, d.y, d.w, d.h);
        }
        if let Some(v) = &s.viewport {
            v.set_destination(o.lw, o.lh);
        }
        s.wl.frame(&self.qh, oi);
        s.frame_pending = true;
        s.dirty = false;
        s.wl.commit();
        self.font = font;
        if self.shown_at.is_none() {
            self.shown_at = Some(Instant::now());
        }
    }

    fn to_phys_static(lx: i32, ly: i32, sx: f64, sy: f64, r: &RectF) -> IRect {
        let x0 = ((r.x - lx as f64) * sx).floor() as i32;
        let y0 = ((r.y - ly as f64) * sy).floor() as i32;
        let x1 = ((r.x + r.w - lx as f64) * sx).ceil() as i32;
        let y1 = ((r.y + r.h - ly as f64) * sy).ceil() as i32;
        IRect { x: x0, y: y0, w: x1 - x0, h: y1 - y0 }
    }

    // -----------------------------------------------------------------------
    // Resultado

    fn compose(&self, r: RectF, path: Option<&[(f64, f64)]>) -> Option<Image> {
        let screens: Vec<(RectF, &Image)> = self.outputs.iter().filter_map(|o| Some((o.logical(), o.img.as_ref()?))).collect();
        compose(&screens, r, path)
    }

    pub fn result_image(&self, outcome: &Outcome) -> Option<Image> {
        match outcome {
            Outcome::Cancel => None,
            Outcome::Rect(r) => self.compose(*r, None),
            Outcome::Free(path) => self.compose(bbox(path), Some(path)),
            Outcome::Output(i) => self.outputs.get(*i)?.img.clone(),
            // Como no Windows: o que está na tela na área da janela (cantos e
            // transparência incluídos), limitado aos monitores.
            Outcome::Window(i) => {
                let r = self.toplevel_rect(*i)?;
                let screens = self.outputs.iter().map(|o| o.logical()).reduce(|a, b| {
                    let x0 = a.x.min(b.x);
                    let y0 = a.y.min(b.y);
                    RectF { x: x0, y: y0, w: (a.x + a.w).max(b.x + b.w) - x0, h: (a.y + a.h).max(b.y + b.h) - y0 }
                })?;
                self.compose(r.intersect(&screens)?, None)
            }
        }
    }
}

/// Monta a imagem final de uma região lógica global, na maior escala envolvida.
pub fn compose(screens: &[(RectF, &Image)], r: RectF, path: Option<&[(f64, f64)]>) -> Option<Image> {
    let hits: Vec<&(RectF, &Image)> = screens.iter().filter(|(l, _)| l.intersect(&r).is_some()).collect();
    let sx = |l: &RectF, img: &Image| img.w as f64 / l.w;
    let scale = hits.iter().map(|(l, img)| sx(l, img)).fold(0.0, f64::max);
    if hits.is_empty() || scale <= 0.0 {
        return None;
    }
    let px0 = (r.x * scale).round();
    let py0 = (r.y * scale).round();
    let w = ((r.x + r.w) * scale).round() - px0;
    let h = ((r.y + r.h) * scale).round() - py0;
    if w < 1.0 || h < 1.0 {
        return None;
    }
    let mut out = Image::new(w as u32, h as u32);
    for (l, img) in hits {
        let k = scale / sx(l, img);
        let tx = l.x * scale - px0;
        let ty = l.y * scale - py0;
        if (k - 1.0).abs() < 1e-9 && tx.fract() == 0.0 && ty.fract() == 0.0 {
            // Cópia direta de linhas.
            let (tx, ty) = (tx as i64, ty as i64);
            let x0 = tx.max(0);
            let x1 = (tx + img.w as i64).min(out.w as i64);
            let y0 = ty.max(0);
            let y1 = (ty + img.h as i64).min(out.h as i64);
            if x1 <= x0 || y1 <= y0 {
                continue;
            }
            let n = ((x1 - x0) * 4) as usize;
            for y in y0..y1 {
                let d = ((y as usize) * out.w as usize + x0 as usize) * 4;
                let s = (((y - ty) as usize) * img.w as usize + (x0 - tx) as usize) * 4;
                out.data[d..d + n].copy_from_slice(&img.data[s..s + n]);
            }
        } else {
            let src = img.pixmap()?;
            let mut pm = out.pixmap_mut()?;
            let paint = PixmapPaint { quality: tiny_skia::FilterQuality::Bicubic, ..Default::default() };
            pm.draw_pixmap(0, 0, src, &paint, Transform::from_row(k as f32, 0.0, 0.0, k as f32, tx as f32, ty as f32), None);
        }
    }
    if let Some(path) = path {
        let mut pb = PathBuilder::new();
        pb.move_to(path[0].0 as f32, path[0].1 as f32);
        for p in &path[1..] {
            pb.line_to(p.0 as f32, p.1 as f32);
        }
        pb.close();
        if let (Some(p), Some(mut mask)) = (pb.finish(), Mask::new(out.w, out.h)) {
            let t = Transform::from_row(scale as f32, 0.0, 0.0, scale as f32, -px0 as f32, -py0 as f32);
            mask.fill_path(&p, FillRule::Winding, true, t);
            for (px, m) in out.data.chunks_exact_mut(4).zip(mask.data()) {
                if *m != 255 {
                    for c in px.iter_mut() {
                        *c = ((*c as u32 * *m as u32 + 127) / 255) as u8;
                    }
                }
            }
        }
    }
    Some(out)
}

fn close(a: [u8; 4], b: [u8; 4]) -> bool {
    let d = |i: usize| (a[i] as i32 - b[i] as i32).abs();
    d(0) + d(1) + d(2) <= 24
}

pub fn bbox(path: &[(f64, f64)]) -> RectF {
    let mut r = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for p in path {
        r = (r.0.min(p.0), r.1.min(p.1), r.2.max(p.0), r.3.max(p.1));
    }
    if path.is_empty() {
        return RectF { x: 0.0, y: 0.0, w: 0.0, h: 0.0 };
    }
    RectF { x: r.0, y: r.1, w: r.2 - r.0, h: r.3 - r.1 }
}

fn copy_rect(dst: &mut [u8], src: &[u8], stride: usize, r: &IRect) {
    let n = r.w as usize * 4;
    for y in r.y as usize..(r.y + r.h) as usize {
        let i = y * stride + r.x as usize * 4;
        dst[i..i + n].copy_from_slice(&src[i..i + n]);
    }
}

/// Coloca a imagem capturada na orientação lógica da saída.
fn apply_transform(img: Image, t: wl_output::Transform) -> Image {
    use wl_output::Transform as T;
    let (w, h) = (img.w as usize, img.h as usize);
    let map: fn(usize, usize, usize, usize) -> (usize, usize) = match t {
        T::Normal => return img,
        T::_90 => |x, y, _w, h| (h - 1 - y, x),
        T::_180 => |x, y, w, h| (w - 1 - x, h - 1 - y),
        T::_270 => |x, y, w, _h| (y, w - 1 - x),
        T::Flipped => |x, y, w, _h| (w - 1 - x, y),
        T::Flipped90 => |x, y, _w, _h| (y, x),
        T::Flipped180 => |x, y, _w, h| (x, h - 1 - y),
        T::Flipped270 => |x, y, w, h| (h - 1 - y, w - 1 - x),
        _ => return img,
    };
    let swap = matches!(t, T::_90 | T::_270 | T::Flipped90 | T::Flipped270);
    let (nw, nh) = if swap { (h, w) } else { (w, h) };
    let mut out = Image::new(nw as u32, nh as u32);
    for y in 0..h {
        for x in 0..w {
            let (nx, ny) = map(x, y, w, h);
            let s = (y * w + x) * 4;
            let d = (ny * nw + nx) * 4;
            out.data[d..d + 4].copy_from_slice(&img.data[s..s + 4]);
        }
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn draw_toolbar(pm: &mut PixmapMut, x: f32, y: f32, s: f32, mode: Mode, hover: Option<Btn>, pressed: Option<Btn>, accent: [u8; 3]) {
    let (tw, th) = toolbar_size();
    let (w, h) = (tw as f32 * s, th as f32 * s);
    let r = 8.0 * s;
    draw::shadow(pm, x, y, w, h, r, s);
    draw::fill_rrect(pm, x, y, w, h, r, color(0x2b, 0x2b, 0x2b, 250));
    draw::stroke_rrect(pm, x + 0.5, y + 0.5, w - 1.0, h - 1.0, r, 1.0, color(255, 255, 255, 26));
    let buttons = toolbar_buttons();
    // Separador antes do botão Fechar.
    let sep_x = x + (buttons[4].1 as f32 - SEP as f32 / 2.0 - 0.5) * s;
    draw::fill_rect(pm, sep_x, y + 12.0 * s, (1.0 * s).max(1.0), h - 24.0 * s, color(255, 255, 255, 40));
    for (btn, bx) in buttons {
        let bx = x + bx as f32 * s;
        let by = y + PAD as f32 * s;
        let b = BTN as f32 * s;
        let selected = matches!(btn, Btn::Mode(m) if m == mode);
        let bg = if pressed == Some(btn) {
            Some(color(255, 255, 255, 12))
        } else if hover == Some(btn) {
            Some(color(255, 255, 255, if btn == Btn::Close { 22 } else { 20 }))
        } else if selected {
            Some(color(255, 255, 255, 16))
        } else {
            None
        };
        if let Some(bg) = bg {
            draw::fill_rrect(pm, bx, by, b, b, 5.0 * s, bg);
        }
        let icon = match btn {
            Btn::Mode(Mode::Rect) => Icon::Rect,
            Btn::Mode(Mode::Free) => Icon::Free,
            Btn::Mode(Mode::Window) => Icon::Window,
            Btn::Mode(Mode::Screen) => Icon::Screen,
            Btn::Close => Icon::Close,
        };
        let fg: Color = if selected { color(accent[0], accent[1], accent[2], 255) } else { color(255, 255, 255, 235) };
        let fg = if selected { lighten(fg) } else { fg };
        draw::icon(pm, icon, bx + b / 2.0, by + b / 2.0, s, fg);
        if selected {
            draw::fill_rrect(pm, bx + b / 2.0 - 8.0 * s, by + b - 4.0 * s, 16.0 * s, 3.0 * s, 1.5 * s, color(accent[0], accent[1], accent[2], 255));
        }
    }
}

/// Clareia a cor de destaque para contraste sobre o fundo escuro.
fn lighten(c: Color) -> Color {
    let f = |v: f32| (v + (1.0 - v) * 0.35).min(1.0);
    Color::from_rgba(f(c.red()), f(c.green()), f(c.blue()), c.alpha()).unwrap_or(c)
}

fn draw_tooltip(pm: &mut PixmapMut, font: &crate::font::Font, x: f32, y: f32, s: f32, btn: Btn) {
    let text = match btn {
        Btn::Mode(m) => m.tooltip(),
        Btn::Close => "Fechar",
    };
    let Some((_, bx)) = toolbar_buttons().into_iter().find(|(b, _)| *b == btn) else { return };
    let px = 12.0 * s;
    let tw = font.width(text, px);
    let (w, h) = (tw + 16.0 * s, 26.0 * s);
    let cx = x + (bx as f32 + BTN as f32 / 2.0) * s;
    let tx = (cx - w / 2.0).max(1.0);
    let ty = y + (toolbar_size().1 as f32 + 6.0) * s;
    draw::shadow(pm, tx, ty, w, h, 4.0 * s, s * 0.5);
    draw::fill_rrect(pm, tx, ty, w, h, 4.0 * s, color(0x2b, 0x2b, 0x2b, 252));
    draw::stroke_rrect(pm, tx + 0.5, ty + 0.5, w - 1.0, h - 1.0, 4.0 * s, 1.0, color(255, 255, 255, 30));
    let baseline = ty + (h + font.ascent(px) * 0.72) / 2.0;
    font.draw(pm, text, tx + 8.0 * s, baseline, px, &draw::paint(color(255, 255, 255, 240)));
}

pub enum BufTag {
    Capture,
    Overlay(usize, usize),
}

/// Laço principal da sobreposição; devolve a escolha do usuário.
pub fn run_overlay(conn: &Connection, queue: &mut EventQueue<App>, app: &mut App) -> Result<Outcome, String> {
    app.compute_dim();
    app.create_surfaces();
    if app.mode == Mode::Window {
        app.start_toplevel_captures();
    }
    loop {
        queue.dispatch_pending(app).map_err(|e| e.to_string())?;
        if let Some(o) = app.outcome.take() {
            return Ok(o);
        }
        app.tick();
        // EAGAIN não é erro: o socket só está temporariamente cheio ou vazio.
        if let Err(e) = conn.flush() {
            if !would_block(&e) {
                return Err(e.to_string());
            }
        }
        let Some(guard) = queue.prepare_read() else { continue };
        let fd = guard.connection_fd().as_raw_fd();
        let timeout = app.next_timeout().map(|d| d.as_millis().min(i32::MAX as u128) as i32).unwrap_or(-1);
        let mut pfd = libc::pollfd { fd, events: libc::POLLIN, revents: 0 };
        let n = unsafe { libc::poll(&mut pfd, 1, timeout) };
        if n > 0 {
            if let Err(e) = guard.read() {
                if !would_block(&e) {
                    return Err(e.to_string());
                }
            }
        } else {
            drop(guard);
        }
    }
}

fn would_block(e: &wayland_client::backend::WaylandError) -> bool {
    matches!(e, wayland_client::backend::WaylandError::Io(io) if io.kind() == std::io::ErrorKind::WouldBlock)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(w: u32, h: u32, bgra: [u8; 4]) -> Image {
        Image { w, h, data: bgra.iter().copied().cycle().take((w * h * 4) as usize).collect() }
    }

    #[test]
    fn selecao_atravessa_monitores_com_escalas_diferentes() {
        // Esquerda: 100×100 lógicos em escala 1. Direita: 100×100 lógicos em escala 2.
        let a = solid(100, 100, [255, 0, 0, 255]);
        let b = solid(200, 200, [0, 0, 255, 255]);
        let screens = [
            (RectF { x: 0.0, y: 0.0, w: 100.0, h: 100.0 }, &a),
            (RectF { x: 100.0, y: 0.0, w: 100.0, h: 100.0 }, &b),
        ];
        let out = compose(&screens, RectF { x: 50.0, y: 10.0, w: 100.0, h: 50.0 }, None).unwrap();
        // Usa a maior escala envolvida: 100×50 lógicos viram 200×100 pixels.
        assert_eq!((out.w, out.h), (200, 100));
        assert_eq!(out.pixel(10, 50).unwrap(), [255, 0, 0, 255]);
        assert_eq!(out.pixel(190, 50).unwrap(), [0, 0, 255, 255]);
    }

    #[test]
    fn mesma_escala_copia_pixels_exatos() {
        let mut img = solid(10, 10, [0, 0, 0, 255]);
        let i = (3 * 10 + 4) * 4;
        img.data[i..i + 4].copy_from_slice(&[1, 2, 3, 255]);
        let screens = [(RectF { x: 0.0, y: 0.0, w: 10.0, h: 10.0 }, &img)];
        let out = compose(&screens, RectF { x: 2.0, y: 2.0, w: 5.0, h: 5.0 }, None).unwrap();
        assert_eq!((out.w, out.h), (5, 5));
        assert_eq!(out.pixel(2, 1).unwrap(), [1, 2, 3, 255]);
    }

    #[test]
    fn forma_livre_deixa_o_exterior_transparente() {
        let img = solid(100, 100, [9, 9, 9, 255]);
        let screens = [(RectF { x: 0.0, y: 0.0, w: 100.0, h: 100.0 }, &img)];
        let tri = [(10.0, 10.0), (90.0, 10.0), (10.0, 90.0)];
        let out = compose(&screens, bbox(&tri), Some(&tri)).unwrap();
        assert_eq!(out.pixel(5, 5).unwrap(), [9, 9, 9, 255]);
        assert_eq!(out.pixel(75, 75).unwrap()[3], 0);
    }

    #[test]
    fn rotacao_da_saida() {
        let mut img = Image::new(3, 2);
        img.data[0..4].copy_from_slice(&[1, 1, 1, 255]); // canto (0,0)
        let r = apply_transform(img, wl_output::Transform::_90);
        assert_eq!((r.w, r.h), (2, 3));
        assert_eq!(r.pixel(1, 0).unwrap(), [1, 1, 1, 255]);
    }
}
