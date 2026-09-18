//! A leitura da seção `## Files` / `## Arquivos` de uma spec.
//!
//! Um leitor só: a porta do pull request entrega ao revisor os mesmos arquivos
//! que a spec declara, e uma segunda leitura da seção seria uma segunda
//! grafia dela.
//!
//! O resumo estrutural que este arquivo montava — assinaturas públicas e
//! entidades, pela árvore de sintaxe — saiu com o renderizador antigo do pedido
//! de onda, o único que o pedia.

use crate::commands::spec::spec_sections::{is_heading, section_end};

/// Extract the file paths listed under a spec's `## Files` / `## Arquivos`
/// section. Each line's first backtick-quoted token (or, failing that, the
/// first path-ish token) is taken as the path. Stops at the next `## ` heading.
pub(crate) fn files_section_paths(spec_text: &str) -> Vec<String> {
    let lines: Vec<&str> = spec_text.lines().collect();
    let Some(start) = lines.iter().position(|l| is_heading(l, "files")) else {
        return Vec::new();
    };
    let end = section_end(&lines, start);
    let mut out: Vec<String> = Vec::new();
    for line in &lines[start + 1..end] {
        if let Some(path) = first_path_token(line)
            && !out.contains(&path) {
                out.push(path);
            }
    }
    out
}

/// First path-like token in a `## Files` bullet: the content of the first
/// backtick pair when present, else the first whitespace-delimited token that
/// looks like a path (contains `/` or a dotted extension).
fn first_path_token(line: &str) -> Option<String> {
    if let Some(open) = line.find('`')
        && let Some(close_rel) = line[open + 1..].find('`') {
            let inner = line[open + 1..open + 1 + close_rel].trim();
            if !inner.is_empty() {
                return Some(inner.replace('\\', "/"));
            }
        }
    let stripped = line
        .trim_start()
        .trim_start_matches(['-', '*', ' '])
        .trim_start_matches(['[', 'x', ' ', ']'])
        .trim_start();
    let first = stripped.split_whitespace().next()?;
    let looks_pathy = first.contains('/')
        || first
            .rsplit_once('.')
            .is_some_and(|(_, ext)| !ext.is_empty() && ext.chars().all(|c| c.is_ascii_alphanumeric()));
    if looks_pathy {
        Some(first.trim_matches(['(', ')', ',']).replace('\\', "/"))
    } else {
        None
    }
}
