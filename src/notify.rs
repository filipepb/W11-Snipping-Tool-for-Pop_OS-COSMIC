//! Notificação "Recorte copiado para a área de transferência", como no Windows.
//! Clicar nela abre o editor.

use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::rc::Rc;
use std::cell::RefCell;

use gtk::glib;
use gtk::prelude::*;
use gtk::gio;

use crate::i18n::tr;

pub const APP_ID: &str = "io.github.recorte.Recorte";
const DEST: &str = "org.freedesktop.Notifications";
const PATH: &str = "/org/freedesktop/Notifications";

/// Abre o editor como processo independente.
pub fn spawn_editor(path: Option<&Path>, activation_token: Option<&str>) {
    let Ok(exe) = crate::paths::self_exe() else { return };
    let mut cmd = Command::new(exe);
    cmd.arg("edit");
    if let Some(p) = path {
        cmd.arg(p);
    }
    if let Some(t) = activation_token {
        cmd.env("XDG_ACTIVATION_TOKEN", t);
    }
    cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    let _ = cmd.spawn();
}

/// Mostra a notificação e espera o clique (ou o fechamento) dela.
pub fn show_and_wait(path: &Path) {
    let Ok(bus) = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE) else { return };
    let uri = gio::File::for_path(path).uri();
    let hints = glib::VariantDict::new(None);
    hints.insert_value("image-path", &uri.as_str().to_variant());
    hints.insert_value("desktop-entry", &APP_ID.to_variant());
    hints.insert_value("category", &"transfer.complete".to_variant());
    let actions: Vec<String> = vec!["default".into(), tr("Mark up and share").into()];
    let params = glib::Variant::tuple_from_iter([
        tr("Snipping Tool").to_variant(),
        0u32.to_variant(),
        APP_ID.to_variant(),
        tr("Snip copied to clipboard").to_variant(),
        tr("Select here to mark up and share.").to_variant(),
        actions.to_variant(),
        hints.end(),
        (-1i32).to_variant(),
    ]);
    let reply = bus.call_sync(
        Some(DEST),
        PATH,
        DEST,
        "Notify",
        Some(&params),
        glib::VariantTy::new("(u)").ok(),
        gio::DBusCallFlags::NONE,
        3000,
        gio::Cancellable::NONE,
    );
    let Ok(reply) = reply else { return };
    let Some(id) = reply.child_value(0).get::<u32>() else { return };
    // Fecha a notificação do recorte anterior; o processo que a esperava sai.
    let last = crate::paths::runtime_dir().join("notificacao");
    if let Some(prev) = std::fs::read_to_string(&last).ok().and_then(|s| s.trim().parse::<u32>().ok()) {
        if prev != id {
            let _ = bus.call_sync(Some(DEST), PATH, DEST, "CloseNotification", Some(&(prev,).to_variant()), None, gio::DBusCallFlags::NONE, 1000, gio::Cancellable::NONE);
        }
    }
    let _ = std::fs::write(&last, id.to_string());

    let main = glib::MainLoop::new(None, false);
    let token: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
    let path = path.to_path_buf();
    let main2 = main.clone();
    #[allow(deprecated)]
    let sub = bus.signal_subscribe(Some(DEST), Some(DEST), None, Some(PATH), None, gio::DBusSignalFlags::NONE, move |_, _, _, _, signal, params| {
        match signal {
            "ActivationToken" => {
                if let Some((nid, t)) = params.get::<(u32, String)>() {
                    if nid == id {
                        *token.borrow_mut() = Some(t);
                    }
                }
            }
            "ActionInvoked" => {
                if let Some((nid, _key)) = params.get::<(u32, String)>() {
                    if nid == id {
                        spawn_editor(Some(&path), token.borrow().as_deref());
                        main2.quit();
                    }
                }
            }
            "NotificationClosed" => {
                if let Some((nid, _reason)) = params.get::<(u32, u32)>() {
                    if nid == id {
                        main2.quit();
                    }
                }
            }
            _ => {}
        }
    });
    // A notificação continua clicável na central enquanto este processo espera.
    let main3 = main.clone();
    glib::timeout_add_seconds_local_once(30 * 60, move || main3.quit());
    main.run();
    #[allow(deprecated)]
    bus.signal_unsubscribe(sub);
}

/// Mensagem de erro curta para o usuário.
pub fn error(body: &str) {
    let Ok(bus) = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE) else { return };
    let params = glib::Variant::tuple_from_iter([
        tr("Snipping Tool").to_variant(),
        0u32.to_variant(),
        APP_ID.to_variant(),
        tr("Could not capture the screen").to_variant(),
        body.to_variant(),
        Vec::<String>::new().to_variant(),
        glib::VariantDict::new(None).end(),
        (-1i32).to_variant(),
    ]);
    let _ = bus.call_sync(Some(DEST), PATH, DEST, "Notify", Some(&params), None, gio::DBusCallFlags::NONE, 3000, gio::Cancellable::NONE);
}
