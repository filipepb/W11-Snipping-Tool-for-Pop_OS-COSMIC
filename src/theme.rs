//! Cor de destaque do tema do COSMIC.

fn parse_hex_rgba(s: &str) -> Option<[u8; 4]> {
    let h = s.trim().trim_matches('"').trim_start_matches('#');
    if h.len() != 8 || !h.is_ascii() {
        return None;
    }
    let c = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok();
    Some([c(0)?, c(2)?, c(4)?, c(6)?])
}

/// Lê `base` do arquivo `accent` do tema ativo; cai para o azul padrão do COSMIC.
pub fn accent() -> [u8; 3] {
    let root = crate::paths::config_dir().join("cosmic");
    let dark = std::fs::read_to_string(root.join("com.system76.CosmicTheme.Mode/v1/is_dark"))
        .map(|s| s.trim() != "false")
        .unwrap_or(true);
    let theme = if dark { "com.system76.CosmicTheme.Dark" } else { "com.system76.CosmicTheme.Light" };
    for version in ["v2", "v1"] {
        if let Ok(text) = std::fs::read_to_string(root.join(theme).join(version).join("accent")) {
            for line in text.lines() {
                if let Some(v) = line.trim().strip_prefix("base:") {
                    if let Some(c) = parse_hex_rgba(v.trim().trim_end_matches(',')) {
                        return [c[0], c[1], c[2]];
                    }
                }
            }
        }
    }
    [0x63, 0xd0, 0xdf]
}
