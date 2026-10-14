//! Diretórios XDG, nomes de arquivo e horário local.

use std::path::PathBuf;

use crate::i18n::{tr, trf};

pub fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/tmp"))
}

pub fn config_dir() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| home().join(".config"))
}

pub fn runtime_dir() -> PathBuf {
    let base = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let dir = base.join("recorte");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// Pasta "Imagens" do usuário, lida de `user-dirs.dirs`.
pub fn pictures_dir() -> PathBuf {
    let home = home();
    if let Ok(text) = std::fs::read_to_string(config_dir().join("user-dirs.dirs")) {
        for line in text.lines() {
            let line = line.trim();
            if let Some(rest) = line.strip_prefix("XDG_PICTURES_DIR=") {
                let value = rest.trim_matches('"').replace("$HOME", &home.to_string_lossy());
                let path = PathBuf::from(value);
                if path.is_absolute() && path != home {
                    return path;
                }
            }
        }
    }
    for name in ["Imagens", "Pictures"] {
        let p = home.join(name);
        if p.is_dir() {
            return p;
        }
    }
    home.join(tr("Pictures"))
}

pub fn default_save_dir() -> PathBuf {
    pictures_dir().join(tr("Screenshots"))
}

pub struct LocalTime {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub min: u32,
    pub sec: u32,
}

pub fn now() -> LocalTime {
    unsafe {
        let t = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&t, &mut tm);
        LocalTime {
            year: tm.tm_year + 1900,
            month: (tm.tm_mon + 1) as u32,
            day: tm.tm_mday as u32,
            hour: tm.tm_hour as u32,
            min: tm.tm_min as u32,
            sec: tm.tm_sec as u32,
        }
    }
}

/// "Captura de tela 2026-10-13 143005.png", como o Windows nomeia.
pub fn screenshot_file_name() -> String {
    let t = now();
    let stamp = format!("{:04}-{:02}-{:02} {:02}{:02}{:02}", t.year, t.month, t.day, t.hour, t.min, t.sec);
    trf("Screenshot {}.png", &[&stamp])
}

/// Caminho livre em `dir` para um novo arquivo de captura.
pub fn unique_path(dir: &std::path::Path, name: &str) -> PathBuf {
    let candidate = dir.join(name);
    if !candidate.exists() {
        return candidate;
    }
    let stem = name.trim_end_matches(".png");
    for i in 2.. {
        let p = dir.join(format!("{stem} ({i}).png"));
        if !p.exists() {
            return p;
        }
    }
    unreachable!()
}

/// Remove capturas temporárias antigas (mais de um dia).
pub fn prune_runtime_dir() {
    let Ok(entries) = std::fs::read_dir(runtime_dir()) else { return };
    let limit = std::time::Duration::from_secs(24 * 3600);
    for e in entries.flatten() {
        if let Ok(meta) = e.metadata() {
            if let Ok(age) = meta.modified().and_then(|m| m.elapsed().map_err(std::io::Error::other)) {
                if age > limit && meta.is_file() {
                    let _ = std::fs::remove_file(e.path());
                }
            }
        }
    }
}

/// Caminho deste executável, mesmo que ele tenha sido substituído por uma
/// atualização enquanto o processo roda (o Linux anexa " (deleted)").
pub fn self_exe() -> std::io::Result<PathBuf> {
    let exe = std::env::current_exe()?;
    if exe.exists() {
        return Ok(exe);
    }
    let stripped = PathBuf::from(exe.to_string_lossy().trim_end_matches(" (deleted)"));
    if stripped.exists() {
        return Ok(stripped);
    }
    Err(std::io::Error::new(std::io::ErrorKind::NotFound, trf("executable not found: {}", &[&exe.display()])))
}
