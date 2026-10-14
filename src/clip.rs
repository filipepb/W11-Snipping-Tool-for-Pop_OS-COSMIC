//! Área de transferência persistente via `ext_data_control_v1`.
//!
//! No Wayland o conteúdo copiado vive no processo que o oferece. Por isso cada
//! cópia roda um pequeno processo destacado (`recorte __clip ARQUIVO`) que
//! serve o PNG até outro aplicativo assumir a área de transferência.

use std::io::Write;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::Arc;

use crate::i18n::{tr, trf};

use wayland_client::protocol::{wl_registry, wl_seat};
use wayland_client::{event_created_child, Connection, Dispatch, QueueHandle};
use wayland_protocols::ext::data_control::v1::client::{
    ext_data_control_device_v1::{self, ExtDataControlDeviceV1},
    ext_data_control_manager_v1::ExtDataControlManagerV1,
    ext_data_control_offer_v1::{self, ExtDataControlOfferV1},
    ext_data_control_source_v1::{self, ExtDataControlSourceV1},
};

/// Inicia um servidor destacado para o PNG em `path`.
pub fn copy_file(path: &Path) -> std::io::Result<()> {
    let exe = crate::paths::self_exe()?;
    let mut cmd = Command::new(exe);
    cmd.arg("__clip").arg(path).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    cmd.spawn().map(|_| ())
}

struct State {
    data: Arc<Vec<u8>>,
    seat: Option<wl_seat::WlSeat>,
    manager: Option<ExtDataControlManagerV1>,
    done: bool,
}

pub fn serve(path: &Path) -> i32 {
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_IGN);
    }
    let data = match std::fs::read(path) {
        Ok(d) => Arc::new(d),
        Err(e) => {
            eprintln!("recorte: {}", trf("could not read {}: {}", &[&path.display(), &e]));
            return 1;
        }
    };
    let Ok(conn) = Connection::connect_to_env() else {
        eprintln!("recorte: {}", tr("no Wayland connection"));
        return 1;
    };
    let mut queue = conn.new_event_queue();
    let qh = queue.handle();
    conn.display().get_registry(&qh, ());
    let mut st = State { data, seat: None, manager: None, done: false };
    if queue.roundtrip(&mut st).is_err() {
        return 1;
    }
    let (Some(seat), Some(manager)) = (st.seat.clone(), st.manager.clone()) else {
        eprintln!("recorte: {}", tr("the compositor does not offer ext_data_control_v1"));
        return 1;
    };
    let device = manager.get_data_device(&seat, &qh, ());
    let source = manager.create_data_source(&qh, ());
    for mime in ["image/png", "image/x-png"] {
        source.offer(mime.to_string());
    }
    device.set_selection(Some(&source));
    while !st.done {
        if queue.blocking_dispatch(&mut st).is_err() {
            break;
        }
    }
    source.destroy();
    device.destroy();
    let _ = conn.flush();
    0
}

impl Dispatch<wl_registry::WlRegistry, ()> for State {
    fn event(st: &mut Self, reg: &wl_registry::WlRegistry, ev: wl_registry::Event, _: &(), _: &Connection, qh: &QueueHandle<Self>) {
        if let wl_registry::Event::Global { name, interface, version } = ev {
            match interface.as_str() {
                "wl_seat" if st.seat.is_none() => st.seat = Some(reg.bind(name, version.min(5), qh, ())),
                "ext_data_control_manager_v1" => st.manager = Some(reg.bind(name, 1, qh, ())),
                _ => {}
            }
        }
    }
}

impl Dispatch<wl_seat::WlSeat, ()> for State {
    fn event(_: &mut Self, _: &wl_seat::WlSeat, _: wl_seat::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

impl Dispatch<ExtDataControlManagerV1, ()> for State {
    fn event(_: &mut Self, _: &ExtDataControlManagerV1, _: <ExtDataControlManagerV1 as wayland_client::Proxy>::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

impl Dispatch<ExtDataControlDeviceV1, ()> for State {
    fn event(st: &mut Self, _: &ExtDataControlDeviceV1, ev: ext_data_control_device_v1::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        match ev {
            ext_data_control_device_v1::Event::DataOffer { id } => id.destroy(),
            ext_data_control_device_v1::Event::Finished => st.done = true,
            _ => {}
        }
    }

    event_created_child!(State, ExtDataControlDeviceV1, [
        ext_data_control_device_v1::EVT_DATA_OFFER_OPCODE => (ExtDataControlOfferV1, ()),
    ]);
}

impl Dispatch<ExtDataControlOfferV1, ()> for State {
    fn event(_: &mut Self, _: &ExtDataControlOfferV1, _: ext_data_control_offer_v1::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

impl Dispatch<ExtDataControlSourceV1, ()> for State {
    fn event(st: &mut Self, _: &ExtDataControlSourceV1, ev: ext_data_control_source_v1::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        match ev {
            ext_data_control_source_v1::Event::Send { fd, .. } => {
                let data = st.data.clone();
                // Escreve em outra thread: o leitor pode demorar e não deve travar o laço.
                std::thread::spawn(move || {
                    let mut f = std::fs::File::from(fd);
                    let _ = f.write_all(&data);
                });
            }
            ext_data_control_source_v1::Event::Cancelled => st.done = true,
            _ => {}
        }
    }
}
