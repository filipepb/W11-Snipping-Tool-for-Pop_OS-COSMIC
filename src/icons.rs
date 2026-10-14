//! Ícones simbólicos próprios (recoloridos pelo GTK conforme o tema).

pub const ICONS: &[(&str, &str)] = &[
    ("recorte-pen-symbolic", r##"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 16 16"><path fill="none" stroke="#222" stroke-width="1.3" stroke-linejoin="round" d="M10.6 2.2l3.2 3.2-7.9 7.9-4 .8.8-4z M9.2 3.6l3.2 3.2"/></svg>"##),
    ("recorte-highlighter-symbolic", r##"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 16 16"><path fill="none" stroke="#222" stroke-width="1.3" stroke-linejoin="round" d="M9.8 1.8l4.4 4.4-5.6 5.6-4.4-4.4z M4.2 7.4l-.9 3.4 1.9 1.9 3.4-.9 M3.3 10.8L1.5 13.6h3.2l.5-.9"/></svg>"##),
    ("recorte-eraser-symbolic", r##"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 16 16"><path fill="none" stroke="#222" stroke-width="1.3" stroke-linejoin="round" d="M9.4 2.2l4.4 4.4-6.6 6.6H4.6L2.2 10.8a1 1 0 010-1.4z M5.6 6l4.4 4.4 M7.2 13.2H14"/></svg>"##),
    ("recorte-shapes-symbolic", r##"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 16 16"><g fill="none" stroke="#222" stroke-width="1.3"><rect x="1.8" y="1.8" width="7" height="7" rx="1"/><circle cx="10.6" cy="10.6" r="3.8"/></g></svg>"##),
    ("recorte-text-symbolic", r##"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 16 16"><path fill="none" stroke="#222" stroke-width="1.4" stroke-linecap="round" d="M2.5 3.5h11 M8 3.5v10 M6 13.5h4"/></svg>"##),
    ("recorte-crop-symbolic", r##"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 16 16"><path fill="none" stroke="#222" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round" d="M4 1v11h11 M1 4h11v11"/></svg>"##),
    ("recorte-rect-symbolic", r##"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 16 16"><path fill="none" stroke="#222" stroke-width="1.3" stroke-linecap="round" d="M2 5V2.5h2.5 M11.5 2.5H14V5 M14 11v2.5h-2.5 M4.5 13.5H2V11 M7 2.5h2 M7 13.5h2 M2 7.2v1.6 M14 7.2v1.6"/></svg>"##),
    ("recorte-free-symbolic", r##"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 16 16"><path fill="none" stroke="#222" stroke-width="1.3" stroke-linecap="round" d="M4.8 12.4C1.6 10.4 1.6 4.8 6 3.4c4-1.3 8.4.2 8.1 3.5-.3 3-4.5 4-7.2 3.2-1.6-.5-2-1.9-.9-2.6 M4.8 12.4c1 .9.3 2-.8 1.7"/></svg>"##),
    ("recorte-window-symbolic", r##"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 16 16"><g fill="none" stroke="#222" stroke-width="1.3"><rect x="1.8" y="2.6" width="12.4" height="10.8" rx="1.6"/><path d="M1.8 5.6h12.4"/></g></svg>"##),
    ("recorte-screen-symbolic", r##"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 16 16"><g fill="none" stroke="#222" stroke-width="1.3" stroke-linecap="round"><rect x="1.4" y="2.2" width="13.2" height="9" rx="1.4"/><path d="M8 11.2v2.6 M5.2 13.8h5.6"/></g></svg>"##),
    ("recorte-timer-symbolic", r##"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 16 16"><g fill="none" stroke="#222" stroke-width="1.3" stroke-linecap="round"><circle cx="8" cy="9" r="5.6"/><path d="M8 5.8V9l2 1.6 M6.4 1.6h3.2"/></g></svg>"##),
    ("recorte-rectangle-symbolic", r##"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 16 16"><rect x="2" y="3.5" width="12" height="9" rx=".6" fill="none" stroke="#222" stroke-width="1.3"/></svg>"##),
    ("recorte-ellipse-symbolic", r##"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 16 16"><ellipse cx="8" cy="8" rx="6.2" ry="4.8" fill="none" stroke="#222" stroke-width="1.3"/></svg>"##),
    ("recorte-line-symbolic", r##"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 16 16"><path d="M2.5 13.5l11-11" stroke="#222" stroke-width="1.4" stroke-linecap="round"/></svg>"##),
    ("recorte-arrow-symbolic", r##"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 16 16"><path d="M2.5 13.5l10-10 M6.5 3h6.5v6.5" fill="none" stroke="#222" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"/></svg>"##),
];

/// Grava os ícones em `~/.local/share/recorte/icons` e devolve a pasta base.
pub fn ensure_installed() -> std::path::PathBuf {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(std::path::PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| crate::paths::home().join(".local/share"))
        .join("recorte/icons");
    let actions = base.join("hicolor/scalable/actions");
    let _ = std::fs::create_dir_all(&actions);
    for (name, svg) in ICONS {
        let p = actions.join(format!("{name}.svg"));
        if std::fs::read_to_string(&p).ok().as_deref() != Some(svg) {
            let _ = std::fs::write(&p, svg);
        }
    }
    base
}
