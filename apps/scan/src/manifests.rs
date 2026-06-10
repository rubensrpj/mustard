//! Build-system manifest detection — data-driven, no hardcoded filenames.
//!
//! The registry (which file is which build system, where its deps/scripts live,
//! which directories to skip) is the external `manifests.toml`, parsed once at
//! startup. A small set of GENERIC format readers (json/xml/toml/yaml/lines)
//! does the extraction, parameterized by selectors from the registry. Adding a
//! build system is a data row; this file never names one.

use regex::Regex;
use std::sync::OnceLock;

struct ManifestDef {
    kind: String,
    filename: Option<String>,
    exts: Vec<String>,
    format: String,
    name: String, // "dir" | "stem"
    deps: Vec<String>,
    scripts: Option<String>,
    element: Option<String>,
    attr: Option<String>,
    module_pattern: Option<String>,
    dep_pattern: Option<String>,
}

struct Registry {
    skip_dirs: Vec<String>,
    manifests: Vec<ManifestDef>,
}

fn registry() -> &'static Registry {
    static R: OnceLock<Registry> = OnceLock::new();
    R.get_or_init(|| parse_registry(include_str!("../manifests.toml")))
}

/// Directories to skip while walking (build/dependency output).
pub fn skip_dirs() -> &'static [String] {
    &registry().skip_dirs
}

/// Cheap filename check so we only read files that are manifests.
pub fn is_manifest(filename: &str) -> bool {
    find_def(filename).is_some()
}

pub struct Parsed {
    pub kind: String,
    pub deps: Vec<String>,
    pub scripts: Vec<String>,
    pub module: Option<String>,
    pub name: String,
}

/// Parse a manifest's content into kind + dependencies + scripts (+ this unit's
/// own module path, for languages that declare one) and the project name.
pub fn parse(rel: &str, filename: &str, content: &str) -> Option<Parsed> {
    let def = find_def(filename)?;
    let deps = match def.format.as_str() {
        "json" => json_deps(content, &def.deps),
        "xml-attr" => xml_attr(content, def.element.as_deref()?, def.attr.as_deref()?),
        "xml-text" => xml_text(content, def.element.as_deref()?),
        "toml-sections" => toml_sections(content, &def.deps),
        "yaml-section" => yaml_sections(content, &def.deps),
        "gomod" => def.dep_pattern.as_deref().map(|p| lines_pattern(content, p)).unwrap_or_default(),
        _ => Vec::new(),
    };
    let scripts = match (&def.format, &def.scripts) {
        (f, Some(path)) if f == "json" => json_scripts(content, path),
        _ => Vec::new(),
    };
    let module = def.module_pattern.as_deref().and_then(|p| first_capture(content, p));
    Some(Parsed { kind: def.kind.clone(), deps, scripts, module, name: derive_name(rel, &def.name) })
}

fn find_def(filename: &str) -> Option<&'static ManifestDef> {
    let lower = filename.to_ascii_lowercase();
    registry().manifests.iter().find(|m| {
        m.filename.as_deref().map(|f| f.eq_ignore_ascii_case(filename)).unwrap_or(false)
            || m.exts.iter().any(|e| lower.ends_with(&format!(".{}", e.to_ascii_lowercase())))
    })
}

fn derive_name(rel: &str, rule: &str) -> String {
    let base = rel.rsplit('/').next().unwrap_or(rel);
    match rule {
        "stem" => base.rsplit_once('.').map(|(s, _)| s.to_string()).unwrap_or_else(|| base.to_string()),
        _ => {
            let dir = rel.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
            dir.rsplit('/').next().filter(|s| !s.is_empty()).unwrap_or("(root)").to_string()
        }
    }
}

// --- generic format readers -------------------------------------------------

fn json_deps(txt: &str, paths: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(txt) {
        for key in paths {
            if let Some(map) = v.get(key).and_then(|d| d.as_object()) {
                out.extend(map.keys().cloned());
            }
        }
    }
    out
}

fn json_scripts(txt: &str, path: &str) -> Vec<String> {
    let mut out = Vec::new();
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(txt) {
        if let Some(map) = v.get(path).and_then(|d| d.as_object()) {
            for (k, val) in map {
                out.push(format!("{k}: {}", val.as_str().unwrap_or("")));
            }
        }
    }
    out
}

fn xml_attr(txt: &str, element: &str, attr: &str) -> Vec<String> {
    let pat = format!(r#"<{}\s+[^>]*{}="([^"]+)""#, regex::escape(element), regex::escape(attr));
    match Regex::new(&pat) {
        Ok(re) => re.captures_iter(txt).map(|c| c[1].to_string()).collect(),
        Err(_) => Vec::new(),
    }
}

fn xml_text(txt: &str, element: &str) -> Vec<String> {
    let e = regex::escape(element);
    match Regex::new(&format!(r#"<{e}>([^<]+)</{e}>"#)) {
        Ok(re) => re.captures_iter(txt).map(|c| c[1].to_string()).collect(),
        Err(_) => Vec::new(),
    }
}

fn toml_sections(txt: &str, sections: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let mut in_deps = false;
    for line in txt.lines() {
        let t = line.trim();
        if t.starts_with('[') {
            let header = t.trim_matches(|c| c == '[' || c == ']').trim();
            in_deps = sections.iter().any(|s| header == s || header.ends_with(&format!(".{s}")));
            continue;
        }
        if in_deps {
            if let Some(idx) = t.find(|c| c == '=' || c == ' ') {
                let name = t[..idx].trim();
                if !name.is_empty() && !name.starts_with('#') {
                    out.push(name.to_string());
                }
            }
        }
    }
    out
}

fn yaml_sections(txt: &str, sections: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let mut in_deps = false;
    for line in txt.lines() {
        let trimmed = line.trim_end();
        if sections.iter().any(|s| trimmed.starts_with(&format!("{s}:"))) {
            in_deps = true;
            continue;
        }
        if in_deps {
            if !trimmed.is_empty() && !trimmed.starts_with(' ') {
                in_deps = false;
                continue;
            }
            if let Some(idx) = trimmed.find(':') {
                let key = trimmed[..idx].trim();
                if !key.is_empty() && key.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-') {
                    out.push(key.to_string());
                }
            }
        }
    }
    out
}

fn lines_pattern(txt: &str, pattern: &str) -> Vec<String> {
    match Regex::new(pattern) {
        Ok(re) => txt.lines().filter_map(|l| re.captures(l).map(|c| c[1].to_string())).collect(),
        Err(_) => Vec::new(),
    }
}

fn first_capture(txt: &str, pattern: &str) -> Option<String> {
    let re = Regex::new(pattern).ok()?;
    txt.lines().find_map(|l| re.captures(l).map(|c| c[1].to_string()))
}

// --- registry parsing -------------------------------------------------------

fn parse_registry(src: &str) -> Registry {
    let v: toml::Value = toml::from_str(src).unwrap_or_else(|e| panic!("manifests.toml invalid: {e}"));
    let strs = |val: Option<&toml::Value>| -> Vec<String> {
        val.and_then(|x| x.as_array())
            .map(|a| a.iter().filter_map(|e| e.as_str().map(String::from)).collect())
            .unwrap_or_default()
    };
    let skip_dirs = strs(v.get("skip_dirs"));
    let mut manifests = Vec::new();
    if let Some(arr) = v.get("manifest").and_then(|x| x.as_array()) {
        for m in arr {
            let g = |k: &str| m.get(k).and_then(|x| x.as_str()).map(String::from);
            let exts = match m.get("ext") {
                Some(toml::Value::String(s)) => vec![s.clone()],
                Some(toml::Value::Array(_)) => strs(m.get("ext")),
                _ => Vec::new(),
            };
            manifests.push(ManifestDef {
                kind: g("kind").unwrap_or_default(),
                filename: g("filename"),
                exts,
                format: g("format").unwrap_or_default(),
                name: g("name").unwrap_or_else(|| "dir".to_string()),
                deps: strs(m.get("deps")),
                scripts: g("scripts"),
                element: g("element"),
                attr: g("attr"),
                module_pattern: g("module_pattern"),
                dep_pattern: g("dep_pattern"),
            });
        }
    }
    Registry { skip_dirs, manifests }
}
