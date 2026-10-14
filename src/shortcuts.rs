//! Registra os atalhos personalizados do COSMIC (Print e Super+Shift+S).

use std::path::PathBuf;

use crate::i18n::tr;

fn custom_file() -> PathBuf {
    crate::paths::config_dir().join("cosmic/com.system76.CosmicSettings.Shortcuts/v1/custom")
}

/// Divide o mapa RON de nível superior em entradas, respeitando aspas e parênteses.
fn split_entries(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    let (mut depth, mut in_str, mut esc) = (0i32, false, false);
    let mut cur = String::new();
    for ch in body.chars() {
        if in_str {
            cur.push(ch);
            if esc {
                esc = false;
            } else if ch == '\\' {
                esc = true;
            } else if ch == '"' {
                in_str = false;
            }
            continue;
        }
        match ch {
            '"' => in_str = true,
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth -= 1,
            ',' if depth == 0 => {
                if !cur.trim().is_empty() {
                    out.push(cur.trim().to_string());
                }
                cur.clear();
                continue;
            }
            _ => {}
        }
        cur.push(ch);
    }
    if !cur.trim().is_empty() {
        out.push(cur.trim().to_string());
    }
    out
}

/// (modificadores ordenados, tecla em minúsculas) de uma entrada.
fn binding_of(entry: &str) -> Option<(Vec<String>, String)> {
    let m0 = entry.find("modifiers:")? + "modifiers:".len();
    let lb = entry[m0..].find('[')? + m0;
    let rb = entry[lb..].find(']')? + lb;
    let mut mods: Vec<String> = entry[lb + 1..rb]
        .split(',')
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty())
        .collect();
    mods.sort();
    let k0 = entry.find("key:")? + "key:".len();
    let q0 = entry[k0..].find('"')? + k0 + 1;
    let q1 = entry[q0..].find('"')? + q0;
    Some((mods, entry[q0..q1].to_ascii_lowercase()))
}

/// Mescla nossos dois atalhos num arquivo `custom` existente, substituindo só
/// entradas com as mesmas teclas. Devolve o novo conteúdo.
pub fn merge(old: &str, exe: &str) -> Result<String, String> {
    let exe = exe.replace('\\', "\\\\").replace('"', "\\\"");
    let body = old.trim();
    if !body.is_empty() && !(body.starts_with('{') && body.ends_with('}')) {
        return Err(tr("unexpected format in the shortcuts file; nothing was changed").into());
    }
    let inner = body.strip_prefix('{').and_then(|b| b.strip_suffix('}')).unwrap_or("");
    let ours = [(vec![], "print".to_string()), (vec!["shift".to_string(), "super".to_string()], "s".to_string())];
    let mut kept: Vec<String> = split_entries(inner)
        .into_iter()
        .filter(|e| binding_of(e).is_none_or(|b| !ours.contains(&b)))
        .collect();
    let name = tr("Snipping Tool");
    kept.push(format!("(modifiers: [], key: \"Print\", description: Some(\"{name}\")): Spawn(\"{exe}\")"));
    kept.push(format!("(modifiers: [Super, Shift], key: \"s\", description: Some(\"{name}\")): Spawn(\"{exe}\")"));
    Ok(format!("{{\n{}\n}}\n", kept.iter().map(|e| format!("    {e},")).collect::<Vec<_>>().join("\n")))
}

pub fn install() -> Result<String, String> {
    let exe = crate::paths::self_exe().map_err(|e| e.to_string())?;
    let file = custom_file();
    let old = std::fs::read_to_string(&file).unwrap_or_default();
    let text = merge(&old, &exe.to_string_lossy()).map_err(|e| format!("{e} ({})", file.display()))?;
    if text.trim() == old.trim() {
        return Ok(tr("The shortcuts were already set up.").into());
    }
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    if !old.is_empty() {
        let t = crate::paths::now();
        let backup = file.with_file_name(format!(
            "custom.bak-{:04}{:02}{:02}-{:02}{:02}{:02}",
            t.year, t.month, t.day, t.hour, t.min, t.sec
        ));
        std::fs::write(&backup, &old).map_err(|e| e.to_string())?;
    }
    let tmp = file.with_file_name("custom.recorte-tmp");
    std::fs::write(&tmp, text).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &file).map_err(|e| e.to_string())?;
    Ok(tr("Shortcuts set up: Print Screen and Super + Shift + S.").into())
}

#[cfg(test)]
mod tests {
    use super::*;

    const REAL: &str = r#"{
    (
        modifiers: [],
        key: "Print",
        description: Some("Print Wayshot"),
    ): Spawn("systemd-run --user /home/usuario/.local/bin/meu-screenshot.sh"),
}"#;

    #[test]
    fn substitui_print_e_preserva_o_resto() {
        let extra = REAL.replace("}\n", "").trim_end_matches('}').to_string()
            + "    (modifiers: [Super], key: \"t\"): Spawn(\"cosmic-term\"),\n}";
        let out = merge(&extra, "/home/u/.local/bin/recorte").unwrap();
        assert!(!out.contains("meu-screenshot"), "{out}");
        assert!(out.contains("cosmic-term"), "{out}");
        assert_eq!(out.matches("Spawn(\"/home/u/.local/bin/recorte\")").count(), 2, "{out}");
        assert!(out.contains("(modifiers: [Super, Shift], key: \"s\""));
        // Idempotente.
        assert_eq!(merge(&out, "/home/u/.local/bin/recorte").unwrap(), out);
    }

    #[test]
    fn arquivo_vazio_e_formato_invalido() {
        assert!(merge("", "/x/recorte").unwrap().contains("key: \"Print\""));
        assert!(merge("Spawn(\"x\")", "/x/recorte").is_err());
    }

    #[test]
    fn entrada_com_virgulas_e_parenteses_em_strings() {
        let old = "{\n    (modifiers: [Ctrl], key: \"p\"): Spawn(\"echo 'a, (b)' \\\"q\\\"\"),\n}";
        let out = merge(old, "/x/recorte").unwrap();
        assert!(out.contains("echo 'a, (b)'"), "{out}");
    }
}
