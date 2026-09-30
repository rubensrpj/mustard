//! O cabeçalho YAML de um `SKILL.md`, lido no que o Mustard usa dele.
//!
//! Toda skill — as de base que o instalador leva e as que o scan escreve em
//! `{subprojeto}/.claude/skills/` — abre com um bloco de cabeçalho entre duas
//! linhas `---`. O Mustard lê dele a descrição, que a busca de skill, o pedido
//! da onda e a lista do agente mostram, e o campo `source`, que o censo da
//! branch usa para distinguir a skill que o scan escreveu. Qualquer outra chave
//! cai em [`SkillFrontmatter::extra`] e não é conferida: uma skill escrita para
//! um Mustard futuro não quebra o leitor de hoje.
//!
//! A leitura é tolerante e o analisador de YAML é um subconjunto pequeno, o
//! bastante para `chave: valor`, texto entre aspas e a descrição em várias
//! linhas. Trazer um leitor de YAML completo pesaria no projeto para nada.

use serde::{Deserialize, Serialize};

/// O cabeçalho de um `SKILL.md`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillFrontmatter {
    /// A descrição da skill, que a busca de skill e os pedidos mostram.
    #[serde(default)]
    pub description: String,
    /// Toda outra chave de nível raiz que traz valor na mesma linha (`name`,
    /// `source`, `license`, `version`, ...), como texto.
    #[serde(flatten)]
    pub extra: serde_json::Value,
}

impl SkillFrontmatter {
    /// O campo `source:` do cabeçalho (`scan`, `manual`, ...), quando ele traz
    /// um. Não é um campo tipado, então mora em [`Self::extra`]; este leitor é
    /// o caminho único para quem precisa dele, como o censo da branch que
    /// separa a skill escrita pelo scan, sem reler o bloco.
    #[must_use]
    pub fn source(&self) -> Option<String> {
        self.extra
            .get("source")
            .and_then(|v| v.as_str())
            .map(str::to_string)
    }
}

/// O erro de ler um cabeçalho.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SkillFrontmatterError {
    /// O bloco de cabeçalho não existe (falta o par de linhas `---`).
    #[error("missing YAML frontmatter")]
    MissingFrontmatter,
}

/// Lê o texto de um `SKILL.md` (ou só o corpo do cabeçalho) num
/// [`SkillFrontmatter`]. Tolerante: a chave desconhecida cai em `extra`.
///
/// # Errors
///
/// Devolve [`SkillFrontmatterError::MissingFrontmatter`] quando não acha o par
/// de linhas `---`.
pub fn parse(raw: &str) -> Result<SkillFrontmatter, SkillFrontmatterError> {
    let yaml = extract_frontmatter(raw).ok_or(SkillFrontmatterError::MissingFrontmatter)?;
    Ok(parse_yaml(&yaml))
}

/// Extract the YAML body between leading `---\n` and the next `\n---` fence.
/// Tolerates a leading UTF-8 BOM and CRLF line endings.
#[must_use]
pub fn extract_frontmatter(raw: &str) -> Option<String> {
    // Some editors prepend a UTF-8 BOM; strip it before the opening fence so a
    // BOM-first SKILL.md still parses (matches the scan-pattern origin reader).
    let raw = raw.strip_prefix('\u{feff}').unwrap_or(raw);
    let normalized = raw.replace("\r\n", "\n");
    let rest = normalized.strip_prefix("---\n")?;
    let end = rest.find("\n---")?;
    Some(rest[..end].to_string())
}

// ---------------------------------------------------------------------------
// Internals
// ---------------------------------------------------------------------------

/// Subconjunto mínimo de YAML: escalares de nível raiz, texto entre aspas e a
/// descrição em várias linhas (as linhas indentadas que a seguem). A chave que
/// o Mustard não lê, com valor na mesma linha, vai para `extra` como texto; a
/// linha indentada de um bloco que ele não lê é pulada.
fn parse_yaml(yaml: &str) -> SkillFrontmatter {
    let lines: Vec<&str> = yaml.lines().collect();
    let mut fm = SkillFrontmatter::default();
    let mut extra = serde_json::Map::<String, serde_json::Value>::new();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        // Linhas em branco e comentários não contam.
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            i += 1;
            continue;
        }
        // Só a coluna zero é chave. A linha indentada sem dono reconhecido é
        // parte de um bloco que o Mustard não lê, e é pulada.
        if line.starts_with([' ', '\t']) {
            i += 1;
            continue;
        }
        let (key, value) = match line.split_once(':') {
            Some((k, v)) => (k.trim().to_string(), v.trim_start().to_string()),
            None => {
                i += 1;
                continue;
            }
        };
        match key.as_str() {
            "description" => {
                // As linhas indentadas logo abaixo continuam a descrição.
                let mut acc = unquote(value.trim()).to_string();
                let mut j = i + 1;
                while j < lines.len() && lines[j].starts_with([' ', '\t']) {
                    let cont = lines[j].trim();
                    if !cont.is_empty() {
                        acc.push(' ');
                        acc.push_str(cont);
                    }
                    j += 1;
                }
                fm.description = acc;
                i = j;
                continue;
            }
            other => {
                if !value.is_empty() {
                    extra.insert(other.to_string(), serde_json::Value::String(unquote(&value).to_string()));
                }
            }
        }
        i += 1;
    }
    fm.extra = serde_json::Value::Object(extra);
    fm
}

/// Strip a single layer of `"..."` or `'...'` quotes.
fn unquote(s: &str) -> &str {
    let trimmed = s.trim();
    if (trimmed.starts_with('"') && trimmed.ends_with('"') && trimmed.len() >= 2)
        || (trimmed.starts_with('\'') && trimmed.ends_with('\'') && trimmed.len() >= 2)
    {
        &trimmed[1..trimmed.len() - 1]
    } else {
        trimmed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_minimal_legacy_frontmatter() {
        let raw = "---\nname: foo\ndescription: A long enough description to clear the fifty character min.\nsource: manual\n---\nbody";
        let fm = parse(raw).expect("parses");
        assert_eq!(fm.extra.get("name").and_then(|v| v.as_str()), Some("foo"), "a chave que o Mustard não lê cai em extra");
        assert!(fm.description.contains("clear the fifty"));
    }

    #[test]
    fn missing_frontmatter_errors() {
        assert!(matches!(
            parse("just a body").unwrap_err(),
            SkillFrontmatterError::MissingFrontmatter
        ));
    }

    #[test]
    fn extra_preserves_unknown_keys() {
        let raw = "---\nname: x\ndescription: Use when the user wants something with enough characters to clear the limit.\nlicense: MIT\nversion: 1.0.0\n---\n";
        let fm = parse(raw).unwrap();
        let obj = fm.extra.as_object().expect("object");
        assert_eq!(obj.get("license").and_then(|v| v.as_str()), Some("MIT"));
        assert_eq!(obj.get("version").and_then(|v| v.as_str()), Some("1.0.0"));
    }

    #[test]
    fn tolerates_leading_bom() {
        // Um BOM antes da cerca de abertura não impede a extração — a
        // tolerância que o leitor de origem do scan sempre teve e que o núcleo
        // agora compartilha.
        let raw = "\u{feff}---\nname: x\ndescription: Use when the user wants something with enough characters here.\nsource: scan\n---\nbody";
        let fm = parse(raw).expect("BOM-prefixed frontmatter parses");
        assert_eq!(fm.extra.get("name").and_then(|v| v.as_str()), Some("x"));
        assert_eq!(fm.source().as_deref(), Some("scan"));
    }

    #[test]
    fn source_accessor_reads_legacy_field() {
        let raw = "---\nname: x\ndescription: Use when the user wants something with enough characters here.\nsource: manual\n---\n";
        let fm = parse(raw).unwrap();
        assert_eq!(fm.source().as_deref(), Some("manual"));
        // Sem `source:` → None (e não uma string vazia).
        let raw_no_source = "---\nname: x\ndescription: Use when the user wants something with enough characters here.\n---\n";
        assert_eq!(parse(raw_no_source).unwrap().source(), None);
    }

    #[test]
    fn crlf_and_bom_converge_on_source() {
        // Fim de linha CRLF e BOM juntos ainda dão o mesmo `source` — as duas
        // normalizações (tirar o BOM e trocar o CRLF) se compõem.
        let raw = "\u{feff}---\r\nname: x\r\ndescription: Use when the user wants something with enough characters here.\r\nsource: scan\r\n---\r\nbody";
        let fm = parse(raw).expect("CRLF+BOM parses");
        assert_eq!(fm.source().as_deref(), Some("scan"));
    }
}
