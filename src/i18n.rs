//! Translations. Source strings are English; each `po/<lang>.po` catalog is
//! embedded in the binary and chosen from LANGUAGE / LC_ALL / LC_MESSAGES / LANG.
//! To add a language, copy `po/pt_BR.po`, translate it and list it in `CATALOGS`.

use std::collections::HashMap;
use std::fmt::Display;
use std::sync::OnceLock;

const CATALOGS: &[(&str, &str)] = &[("pt_BR", include_str!("../po/pt_BR.po"))];

static ACTIVE: OnceLock<HashMap<String, String>> = OnceLock::new();

/// Translates `msgid` into the user's language (or returns it unchanged).
pub fn tr(msgid: &'static str) -> &'static str {
    ACTIVE
        .get_or_init(load)
        .get(msgid)
        .map(String::as_str)
        .unwrap_or(msgid)
}

/// `tr` followed by replacing each `{}` with the next argument.
pub fn trf(msgid: &'static str, args: &[&dyn Display]) -> String {
    let mut out = String::new();
    let mut parts = tr(msgid).split("{}");
    out.push_str(parts.next().unwrap_or(""));
    for (i, part) in parts.enumerate() {
        if let Some(a) = args.get(i) {
            out.push_str(&a.to_string());
        }
        out.push_str(part);
    }
    out
}

fn load() -> HashMap<String, String> {
    let env = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
    // LANGUAGE is only honored when the locale itself is not "C", as in gettext.
    let locale = env("LC_ALL").or_else(|| env("LC_MESSAGES")).or_else(|| env("LANG"));
    let mut wanted: Vec<String> = Vec::new();
    if let Some(list) = env("LANGUAGE").filter(|_| locale.as_deref().is_some_and(|l| l != "C" && l != "POSIX")) {
        wanted.extend(list.split(':').map(str::to_string));
    }
    wanted.extend(locale);
    match pick(&wanted) {
        Some(po) => parse_po(po),
        None => HashMap::new(),
    }
}

/// First catalog matching the preferred languages; "en"/"C" means no translation.
fn pick(wanted: &[String]) -> Option<&'static str> {
    for w in wanted {
        let tag = w.split(['.', '@']).next().unwrap_or("");
        let lang = tag.split('_').next().unwrap_or("");
        if lang.is_empty() || lang == "en" || tag == "C" || tag == "POSIX" {
            return None;
        }
        let exact = CATALOGS.iter().find(|(l, _)| *l == tag);
        let same_lang = CATALOGS.iter().find(|(l, _)| l.split('_').next() == Some(lang));
        if let Some((_, po)) = exact.or(same_lang) {
            return Some(po);
        }
    }
    None
}

/// Minimal .po parser: msgid/msgstr pairs with continuation lines; fuzzy and
/// untranslated entries are skipped.
pub fn parse_po(text: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let (mut id, mut s, mut fuzzy, mut next_fuzzy) = (String::new(), String::new(), false, false);
    let mut field: Option<bool> = None; // Some(true) = msgid, Some(false) = msgstr
    let mut flush = |id: &mut String, s: &mut String, fuzzy: &mut bool| {
        if !id.is_empty() && !s.is_empty() && !*fuzzy {
            map.insert(std::mem::take(id), std::mem::take(s));
        }
        id.clear();
        s.clear();
        *fuzzy = false;
    };
    for line in text.lines().map(str::trim) {
        if let Some(rest) = line.strip_prefix("msgid ") {
            flush(&mut id, &mut s, &mut fuzzy);
            fuzzy = std::mem::take(&mut next_fuzzy);
            id = unquote(rest);
            field = Some(true);
        } else if let Some(rest) = line.strip_prefix("msgstr ") {
            s = unquote(rest);
            field = Some(false);
        } else if line.starts_with('"') {
            match field {
                Some(true) => id.push_str(&unquote(line)),
                Some(false) => s.push_str(&unquote(line)),
                None => {}
            }
        } else if line.starts_with("#,") && line.contains("fuzzy") {
            flush(&mut id, &mut s, &mut fuzzy);
            next_fuzzy = true;
            field = None;
        }
    }
    flush(&mut id, &mut s, &mut fuzzy);
    map
}

fn unquote(s: &str) -> String {
    let s = s.trim().strip_prefix('"').and_then(|s| s.strip_suffix('"')).unwrap_or("");
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some(o) => out.push(o),
            None => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_catalog_by_locale() {
        let v = |l: &[&str]| l.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(pick(&v(&["pt_BR.UTF-8"])).is_some());
        assert!(pick(&v(&["pt_PT.UTF-8"])).is_some());
        assert!(pick(&v(&["en_US.UTF-8", "pt_BR.UTF-8"])).is_none());
        assert!(pick(&v(&["ja_JP.UTF-8", "pt_BR"])).is_some());
        assert!(pick(&v(&["ja_JP.UTF-8"])).is_none());
        assert!(pick(&v(&["C"])).is_none());
    }

    #[test]
    fn parses_po_entries() {
        let po = "msgid \"\"\nmsgstr \"Header\\n\"\n\n#, fuzzy\nmsgid \"Old\"\nmsgstr \"Velho\"\n\nmsgid \"A\\n\"\n\"B \\\"q\\\"\"\nmsgstr \"\"\n\"C\\n\"\n\"D\"\n\nmsgid \"Empty\"\nmsgstr \"\"\n";
        let m = parse_po(po);
        assert_eq!(m.get("A\nB \"q\"").map(String::as_str), Some("C\nD"));
        assert!(!m.contains_key("Old") && !m.contains_key("Empty") && !m.contains_key(""));
    }

    /// Every `tr("…")`/`trf("…")` in the sources has a Portuguese translation
    /// with the same number of `{}` placeholders, and the catalog has no stale entries.
    #[test]
    fn portuguese_catalog_is_complete() {
        let po = parse_po(include_str!("../po/pt_BR.po"));
        let mut used = std::collections::BTreeSet::new();
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.file_name().is_some_and(|n| n == "i18n.rs") {
                continue;
            }
            let src = std::fs::read_to_string(path).unwrap();
            used.extend(extract_msgids(&src));
        }
        let missing: Vec<_> = used.iter().filter(|id| !po.contains_key(*id)).collect();
        assert!(missing.is_empty(), "missing from po/pt_BR.po: {missing:#?}");
        let stale: Vec<_> = po.keys().filter(|id| !used.contains(*id)).collect();
        assert!(stale.is_empty(), "unused in po/pt_BR.po: {stale:#?}");
        for (id, s) in &po {
            assert_eq!(id.matches("{}").count(), s.matches("{}").count(), "placeholders: {id}");
        }
    }

    fn extract_msgids(src: &str) -> Vec<String> {
        let mut out = Vec::new();
        for pat in ["tr(\"", "trf(\""] {
            let mut rest = src;
            while let Some(i) = rest.find(pat) {
                let before = rest[..i].chars().last();
                rest = &rest[i + pat.len()..];
                if before.is_some_and(|c| c.is_alphanumeric() || c == '_') {
                    continue;
                }
                let mut s = String::new();
                let mut chars = rest.char_indices();
                while let Some((j, c)) = chars.next() {
                    match c {
                        '"' => {
                            rest = &rest[j..];
                            break;
                        }
                        '\\' => match chars.next().map(|(_, c)| c) {
                            Some('n') => s.push('\n'),
                            Some('\n') => {
                                // Line continuation: skip leading whitespace.
                                while let Some((_, c)) = chars.clone().next() {
                                    if !c.is_whitespace() {
                                        break;
                                    }
                                    chars.next();
                                }
                            }
                            Some(o) => s.push(o),
                            None => {}
                        },
                        c => s.push(c),
                    }
                }
                out.push(s);
            }
        }
        out
    }
}
