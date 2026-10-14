//! Despachantes de eventos Wayland da sobreposição.

use wayland_client::protocol::{
    wl_buffer, wl_callback, wl_compositor, wl_keyboard, wl_output, wl_pointer, wl_registry, wl_seat, wl_shm,
    wl_shm_pool, wl_surface,
};
use wayland_client::{event_created_child, Connection, Dispatch, Proxy, QueueHandle, WEnum};
use wayland_protocols::ext::foreign_toplevel_list::v1::client::{
    ext_foreign_toplevel_handle_v1::{self, ExtForeignToplevelHandleV1},
    ext_foreign_toplevel_list_v1::{self, ExtForeignToplevelListV1},
};
use wayland_protocols::ext::image_capture_source::v1::client::{
    ext_foreign_toplevel_image_capture_source_manager_v1::ExtForeignToplevelImageCaptureSourceManagerV1,
    ext_image_capture_source_v1::ExtImageCaptureSourceV1,
    ext_output_image_capture_source_manager_v1::ExtOutputImageCaptureSourceManagerV1,
};
use wayland_protocols::ext::image_copy_capture::v1::client::{
    ext_image_copy_capture_frame_v1::{self, ExtImageCopyCaptureFrameV1},
    ext_image_copy_capture_manager_v1::ExtImageCopyCaptureManagerV1,
    ext_image_copy_capture_session_v1::{self, ExtImageCopyCaptureSessionV1},
};
use wayland_protocols::ext::workspace::v1::client::{
    ext_workspace_group_handle_v1::ExtWorkspaceGroupHandleV1,
    ext_workspace_handle_v1::{self, ExtWorkspaceHandleV1},
    ext_workspace_manager_v1::{self, ExtWorkspaceManagerV1},
};
use wayland_protocols::wp::cursor_shape::v1::client::{
    wp_cursor_shape_device_v1::WpCursorShapeDeviceV1, wp_cursor_shape_manager_v1::WpCursorShapeManagerV1,
};
use wayland_protocols::wp::viewporter::client::{wp_viewport::WpViewport, wp_viewporter::WpViewporter};
use wayland_protocols::xdg::xdg_output::zv1::client::{
    zxdg_output_manager_v1::ZxdgOutputManagerV1,
    zxdg_output_v1::{self, ZxdgOutputV1},
};
use wayland_protocols_wlr::layer_shell::v1::client::{
    zwlr_layer_shell_v1::ZwlrLayerShellV1,
    zwlr_layer_surface_v1::{self, ZwlrLayerSurfaceV1},
};
use cosmic_protocols::toplevel_info::v1::client::{
    zcosmic_toplevel_handle_v1::{self, ZcosmicToplevelHandleV1},
    zcosmic_toplevel_info_v1::{self, ZcosmicToplevelInfoV1},
};

use crate::snip::{App, BufTag, CapState, Outcome, Toplevel, Workspace};

macro_rules! noop {
    ($($t:ty),* $(,)?) => {$(
        impl Dispatch<$t, ()> for App {
            fn event(_: &mut Self, _: &$t, _: <$t as Proxy>::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
        }
    )*};
}

noop!(
    wl_compositor::WlCompositor,
    wl_shm::WlShm,
    wl_shm_pool::WlShmPool,
    wl_surface::WlSurface,
    ZwlrLayerShellV1,
    WpViewporter,
    WpViewport,
    WpCursorShapeManagerV1,
    WpCursorShapeDeviceV1,
    ZxdgOutputManagerV1,
    ExtOutputImageCaptureSourceManagerV1,
    ExtForeignToplevelImageCaptureSourceManagerV1,
    ExtImageCaptureSourceV1,
    ExtImageCopyCaptureManagerV1,
    ExtWorkspaceGroupHandleV1,
);

impl Dispatch<wl_registry::WlRegistry, ()> for App {
    fn event(app: &mut Self, _: &wl_registry::WlRegistry, ev: wl_registry::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        if let wl_registry::Event::Global { name, interface, version } = ev {
            app.pending_globals.push((name, interface, version));
        }
    }
}

impl Dispatch<wl_output::WlOutput, ()> for App {
    fn event(app: &mut Self, out: &wl_output::WlOutput, ev: wl_output::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        let Some(o) = app.outputs.iter_mut().find(|o| &o.wl == out) else { return };
        match ev {
            wl_output::Event::Geometry { transform: WEnum::Value(t), .. } => o.transform = t,
            wl_output::Event::Mode { flags: WEnum::Value(f), width, height, .. } if f.contains(wl_output::Mode::Current) => {
                o.mode_w = width;
                o.mode_h = height;
            }
            wl_output::Event::Scale { factor } => o.scale = factor,
            wl_output::Event::Name { name } => o.name = name,
            wl_output::Event::Done => {
                // Sem xdg_output, deduz o tamanho lógico do modo e da escala inteira.
                if o.xdg.is_none() && o.lw == 0 {
                    let (w, h) = match o.transform {
                        wl_output::Transform::_90 | wl_output::Transform::_270 | wl_output::Transform::Flipped90 | wl_output::Transform::Flipped270 => (o.mode_h, o.mode_w),
                        _ => (o.mode_w, o.mode_h),
                    };
                    o.lw = w / o.scale.max(1);
                    o.lh = h / o.scale.max(1);
                }
            }
            _ => {}
        }
    }
}

impl Dispatch<ZxdgOutputV1, ()> for App {
    fn event(app: &mut Self, xdg: &ZxdgOutputV1, ev: zxdg_output_v1::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        let Some(o) = app.outputs.iter_mut().find(|o| o.xdg.as_ref() == Some(xdg)) else { return };
        match ev {
            zxdg_output_v1::Event::LogicalPosition { x, y } => {
                o.lx = x;
                o.ly = y;
            }
            zxdg_output_v1::Event::LogicalSize { width, height } => {
                o.lw = width;
                o.lh = height;
            }
            zxdg_output_v1::Event::Name { name } if o.name.is_empty() => o.name = name,
            _ => {}
        }
    }
}

impl Dispatch<wl_seat::WlSeat, ()> for App {
    fn event(app: &mut Self, seat: &wl_seat::WlSeat, ev: wl_seat::Event, _: &(), _: &Connection, qh: &QueueHandle<Self>) {
        if let wl_seat::Event::Capabilities { capabilities: WEnum::Value(caps) } = ev {
            if caps.contains(wl_seat::Capability::Pointer) && app.pointer.is_none() {
                let p = seat.get_pointer(qh, ());
                app.cursor_dev = app.g.cursor_mgr.as_ref().map(|m| m.get_pointer(&p, qh, ()));
                app.pointer = Some(p);
            }
            if caps.contains(wl_seat::Capability::Keyboard) && app.keyboard.is_none() {
                app.keyboard = Some(seat.get_keyboard(qh, ()));
            }
        }
    }
}

const BTN_LEFT: u32 = 0x110;
const BTN_RIGHT: u32 = 0x111;
const KEY_ESC: u32 = 1;

impl Dispatch<wl_pointer::WlPointer, ()> for App {
    fn event(app: &mut Self, _: &wl_pointer::WlPointer, ev: wl_pointer::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        match ev {
            wl_pointer::Event::Enter { serial, surface, surface_x, surface_y } => {
                app.cursor_serial = serial;
                app.cursor_shape = None;
                let idx = app.outputs.iter().position(|o| o.surf.as_ref().is_some_and(|s| s.wl == surface));
                app.pointer_out = idx;
                if let Some(i) = idx {
                    let o = &app.outputs[i];
                    let p = (o.lx as f64 + surface_x, o.ly as f64 + surface_y);
                    app.on_motion(p);
                }
            }
            wl_pointer::Event::Leave { .. } => {
                if app.drag_start.is_none() {
                    app.pointer_out = None;
                }
            }
            wl_pointer::Event::Motion { surface_x, surface_y, .. } => {
                if let Some(i) = app.pointer_out {
                    let o = &app.outputs[i];
                    let p = (o.lx as f64 + surface_x, o.ly as f64 + surface_y);
                    app.on_motion(p);
                }
            }
            wl_pointer::Event::Button { button, state: WEnum::Value(state), .. } => {
                let pressed = state == wl_pointer::ButtonState::Pressed;
                match (button, pressed) {
                    (BTN_LEFT, true) => app.on_press(),
                    (BTN_LEFT, false) => app.on_release(),
                    // Botão direito durante o arraste desiste da seleção atual.
                    (BTN_RIGHT, true) if app.drag_start.is_some() => {
                        app.drag_start = None;
                        app.free_path.clear();
                        app.update_hover();
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
}

impl Dispatch<wl_keyboard::WlKeyboard, ()> for App {
    fn event(app: &mut Self, _: &wl_keyboard::WlKeyboard, ev: wl_keyboard::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        if let wl_keyboard::Event::Key { key, state: WEnum::Value(wl_keyboard::KeyState::Pressed), .. } = ev {
            if key == KEY_ESC {
                if app.drag_start.is_some() {
                    app.drag_start = None;
                    app.free_path.clear();
                    app.update_hover();
                } else {
                    app.outcome = Some(Outcome::Cancel);
                }
            }
        }
    }
}

impl Dispatch<ZwlrLayerSurfaceV1, usize> for App {
    fn event(app: &mut Self, layer: &ZwlrLayerSurfaceV1, ev: zwlr_layer_surface_v1::Event, idx: &usize, _: &Connection, _: &QueueHandle<Self>) {
        match ev {
            zwlr_layer_surface_v1::Event::Configure { serial, .. } => {
                layer.ack_configure(serial);
                if let Some(s) = app.outputs.get_mut(*idx).and_then(|o| o.surf.as_mut()) {
                    s.configured = true;
                    s.dirty = true;
                }
            }
            zwlr_layer_surface_v1::Event::Closed => app.outcome = Some(Outcome::Cancel),
            _ => {}
        }
    }
}

impl Dispatch<wl_callback::WlCallback, usize> for App {
    fn event(app: &mut Self, _: &wl_callback::WlCallback, ev: wl_callback::Event, idx: &usize, _: &Connection, _: &QueueHandle<Self>) {
        if let wl_callback::Event::Done { .. } = ev {
            if let Some(s) = app.outputs.get_mut(*idx).and_then(|o| o.surf.as_mut()) {
                s.frame_pending = false;
            }
        }
    }
}

impl Dispatch<wl_buffer::WlBuffer, BufTag> for App {
    fn event(app: &mut Self, _: &wl_buffer::WlBuffer, ev: wl_buffer::Event, tag: &BufTag, _: &Connection, _: &QueueHandle<Self>) {
        if let (wl_buffer::Event::Release, BufTag::Overlay(oi, bi)) = (ev, tag) {
            if let Some(b) = app.outputs.get_mut(*oi).and_then(|o| o.surf.as_mut()).and_then(|s| s.bufs.get_mut(*bi)) {
                b.busy = false;
            }
        }
    }
}

impl Dispatch<ExtImageCopyCaptureSessionV1, usize> for App {
    fn event(app: &mut Self, _: &ExtImageCopyCaptureSessionV1, ev: ext_image_copy_capture_session_v1::Event, idx: &usize, _: &Connection, _: &QueueHandle<Self>) {
        let idx = *idx;
        use ext_image_copy_capture_session_v1::Event as E;
        match ev {
            E::BufferSize { width, height } => {
                app.jobs[idx].w = width;
                app.jobs[idx].h = height;
            }
            E::ShmFormat { format: WEnum::Value(f) } => app.jobs[idx].formats.push(f),
            E::Done => app.job_constraints_done(idx),
            E::Stopped => {
                if app.jobs[idx].frame.is_none() || !app.jobs[idx].finished {
                    app.finish_job(idx);
                }
            }
            _ => {}
        }
    }
}

impl Dispatch<ExtImageCopyCaptureFrameV1, usize> for App {
    fn event(app: &mut Self, _: &ExtImageCopyCaptureFrameV1, ev: ext_image_copy_capture_frame_v1::Event, idx: &usize, _: &Connection, _: &QueueHandle<Self>) {
        use ext_image_copy_capture_frame_v1::Event as E;
        match ev {
            E::Transform { transform: WEnum::Value(t) } => app.jobs[*idx].transform = t,
            E::Ready => app.job_ready(*idx),
            E::Failed { .. } => app.finish_job(*idx),
            _ => {}
        }
    }
}

impl Dispatch<ExtForeignToplevelListV1, ()> for App {
    fn event(app: &mut Self, _: &ExtForeignToplevelListV1, ev: ext_foreign_toplevel_list_v1::Event, _: &(), _: &Connection, qh: &QueueHandle<Self>) {
        if let ext_foreign_toplevel_list_v1::Event::Toplevel { toplevel } = ev {
            let cosmic = app.g.cosmic_info.as_ref().map(|i| i.get_cosmic_toplevel(&toplevel, qh, ()));
            app.toplevels.push(Toplevel {
                handle: toplevel,
                cosmic,
                title: String::new(),
                app_id: String::new(),
                geoms: Vec::new(),
                minimized: false,
                activated: false,
                workspaces: Vec::new(),
                closed: false,
                img: None,
                offset: None,
                cap: CapState::None,
            });
        }
    }

    event_created_child!(App, ExtForeignToplevelListV1, [
        ext_foreign_toplevel_list_v1::EVT_TOPLEVEL_OPCODE => (ExtForeignToplevelHandleV1, ()),
    ]);
}

impl Dispatch<ExtForeignToplevelHandleV1, ()> for App {
    fn event(app: &mut Self, h: &ExtForeignToplevelHandleV1, ev: ext_foreign_toplevel_handle_v1::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        let Some(t) = app.toplevels.iter_mut().find(|t| &t.handle == h) else { return };
        match ev {
            ext_foreign_toplevel_handle_v1::Event::Title { title } => t.title = title,
            ext_foreign_toplevel_handle_v1::Event::AppId { app_id } => t.app_id = app_id,
            ext_foreign_toplevel_handle_v1::Event::Closed => t.closed = true,
            _ => {}
        }
    }
}

impl Dispatch<ZcosmicToplevelInfoV1, ()> for App {
    fn event(_: &mut Self, _: &ZcosmicToplevelInfoV1, _: zcosmic_toplevel_info_v1::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}

    event_created_child!(App, ZcosmicToplevelInfoV1, [
        zcosmic_toplevel_info_v1::EVT_TOPLEVEL_OPCODE => (ZcosmicToplevelHandleV1, ()),
    ]);
}

impl Dispatch<ZcosmicToplevelHandleV1, ()> for App {
    fn event(app: &mut Self, h: &ZcosmicToplevelHandleV1, ev: zcosmic_toplevel_handle_v1::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        let Some(t) = app.toplevels.iter_mut().find(|t| t.cosmic.as_ref() == Some(h)) else { return };
        app.windows_changed = true;
        use zcosmic_toplevel_handle_v1::Event as E;
        match ev {
            E::State { state } => {
                let states: Vec<u32> = state.chunks_exact(4).map(|c| u32::from_ne_bytes([c[0], c[1], c[2], c[3]])).collect();
                t.minimized = states.contains(&1);
                t.activated = states.contains(&2);
            }
            E::Geometry { output, x, y, width, height } => {
                t.geoms.retain(|g| g.0 != output);
                t.geoms.push((output, x, y, width, height));
            }
            E::OutputLeave { output } => t.geoms.retain(|g| g.0 != output),
            E::ExtWorkspaceEnter { workspace } => {
                if !t.workspaces.contains(&workspace) {
                    t.workspaces.push(workspace);
                }
            }
            E::ExtWorkspaceLeave { workspace } => t.workspaces.retain(|w| w != &workspace),
            _ => {}
        }
    }
}

impl Dispatch<ExtWorkspaceManagerV1, ()> for App {
    fn event(app: &mut Self, _: &ExtWorkspaceManagerV1, ev: ext_workspace_manager_v1::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        if let ext_workspace_manager_v1::Event::Workspace { workspace } = ev {
            app.workspaces.push(Workspace { handle: workspace, active: false });
        }
    }

    event_created_child!(App, ExtWorkspaceManagerV1, [
        ext_workspace_manager_v1::EVT_WORKSPACE_GROUP_OPCODE => (ExtWorkspaceGroupHandleV1, ()),
        ext_workspace_manager_v1::EVT_WORKSPACE_OPCODE => (ExtWorkspaceHandleV1, ()),
    ]);
}

impl Dispatch<ExtWorkspaceHandleV1, ()> for App {
    fn event(app: &mut Self, h: &ExtWorkspaceHandleV1, ev: ext_workspace_handle_v1::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        let Some(w) = app.workspaces.iter_mut().find(|w| &w.handle == h) else { return };
        match ev {
            ext_workspace_handle_v1::Event::State { state: WEnum::Value(s) } => {
                w.active = s.contains(ext_workspace_handle_v1::State::Active);
                app.windows_changed = true;
            }
            ext_workspace_handle_v1::Event::Removed => w.active = false,
            _ => {}
        }
    }
}
