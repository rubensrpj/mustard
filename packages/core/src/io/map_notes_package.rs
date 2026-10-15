//! `map_notes_package` — o pacote das funções sem frase em dia, que o
//! condutor entrega ao agente de análise para ele devolver uma frase por
//! função.
//!
//! O agente de análise só lê: quem grava as frases que ele devolve é o
//! condutor, por [`crate::io::map_notes::write_all`]. Aqui só se monta o
//! pacote: as funções e os métodos do mapa sem nota em dia pela regra que a
//! busca lê ([`crate::io::map_notes_fresh::FRESH`]) — os de nota velha
//! primeiro, depois os dos arquivos mudados, depois os outros —, cada um com
//! o texto dele lido do disco, até o teto de caracteres de um pedido ao
//! agente.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use serde::Serialize;

use crate::domain::project_map::{MapRefusal, clean_path, lines_of};
use crate::io::map_db::table_exists;
use crate::io::map_notes_fresh::FRESH;
use crate::io::project_map::{DECLS, FILES, NOTES, open_reading, unreadable};

/// O teto de caracteres de um pedido ao agente de análise: o gancho recusa o
/// pedido maior (`plugin/hooks/analysis-policy.js`).
pub const PROMPT_CHARS: usize = 32_000;

/// O que o pacote padrão deixa livre no mesmo pedido para o texto do condutor
/// que pede as frases.
pub const ASK_CHARS: usize = 2_000;

/// O teto padrão do pacote: o pedido inteiro, com o texto do condutor, cabe
/// no teto do agente de análise.
pub const PACKAGE_CHARS: usize = PROMPT_CHARS - ASK_CHARS;

/// Uma função do pacote: o arquivo, o nome, as linhas e o texto dela, o
/// mesmo trecho que o `slice` do mapa mostra.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Entry {
    pub file: String,
    pub name: String,
    pub line: u64,
    pub end_line: u64,
    pub text: String,
}

/// A função que sozinha passa do teto do pacote, e quantos caracteres ela
/// ocupa: ela fica de fora de todo pacote com esse teto.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TooLarge {
    pub file: String,
    pub name: String,
    pub chars: usize,
}

/// O pacote montado.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Package {
    /// As funções do pacote, na ordem em que pedem frase.
    pub entries: Vec<Entry>,
    /// O tamanho de `entries` escrito como JSON, contado como o gancho conta
    /// o pedido (unidades de UTF-16, o `length` do JavaScript).
    pub chars: usize,
    /// Quantas funções e métodos do mapa estão sem nota em dia.
    pub waiting: usize,
    /// As que passam sozinhas do teto, achadas antes de o pacote encher.
    pub too_large: Vec<TooLarge>,
}

/// Uma função sem nota em dia, como o mapa a guarda.
struct Wanted {
    file: String,
    name: String,
    line: u64,
    end_line: u64,
    /// Ela tem nota, mas a nota envelheceu.
    stale: bool,
}

/// O pacote do mapa em `model`, de projeto em `root`, até `limit`
/// caracteres: as funções e os métodos sem nota em dia, uma vez por arquivo
/// e nome (a nota vale para o nome no arquivo), os de nota velha primeiro,
/// depois os dos arquivos de `changed`, depois os outros; em cada grupo, por
/// arquivo e linha. O pacote para na primeira função que não cabe no que
/// sobra, e a próxima chamada começa por ela; a que passa sozinha do teto vai
/// para `too_large` e não segura as seguintes. A função cujo arquivo não se lê
/// do disco fica de fora.
///
/// # Errors
///
/// Sem o mapa ou com ele ilegível.
pub fn package(root: &Path, model: &Path, changed: &[String], limit: usize) -> Result<Package, MapRefusal> {
    let mut wanted = without_note(model)?;
    let changed: HashSet<String> = changed.iter().map(|file| clean_path(file)).collect();
    wanted.sort_by_key(|item| u8::from(!item.stale) + u8::from(!item.stale && !changed.contains(&item.file)));
    let mut out = Package { waiting: wanted.len(), chars: 2, ..Package::default() };
    let mut sources: HashMap<String, Option<String>> = HashMap::new();
    for item in wanted {
        let source = sources.entry(item.file.clone()).or_insert_with(|| std::fs::read_to_string(root.join(&item.file)).ok());
        let Some(source) = source else {
            continue;
        };
        let entry = Entry { text: lines_of(source, item.line, item.end_line), file: item.file, name: item.name, line: item.line, end_line: item.end_line };
        let alone = serde_json::to_string(&entry).map_or(usize::MAX, |json| json.encode_utf16().count());
        if alone.saturating_add(2) > limit {
            out.too_large.push(TooLarge { file: entry.file, name: entry.name, chars: alone });
            continue;
        }
        let size = alone + usize::from(!out.entries.is_empty());
        if out.chars + size > limit {
            break;
        }
        out.chars += size;
        out.entries.push(entry);
    }
    if out.entries.is_empty() {
        out.chars = 0;
    }
    Ok(out)
}

/// As funções e os métodos do mapa em `model` sem nota em dia, uma vez por
/// arquivo e nome, por arquivo e linha.
fn without_note(model: &Path) -> Result<Vec<Wanted>, MapRefusal> {
    let db = open_reading(model, &[&FILES, &DECLS, &NOTES])?;
    let conn = db.conn();
    let sql = if table_exists(conn, "notes").map_err(unreadable)? {
        format!(
            "SELECT d.file, d.name, d.line, d.end_line, EXISTS (SELECT 1 FROM notes n WHERE n.file = d.file AND n.name = d.name) \
             FROM decls d WHERE d.kind IN ('function', 'method') \
             AND NOT EXISTS (SELECT 1 FROM notes n WHERE n.file = d.file AND n.name = d.name AND {FRESH}) \
             ORDER BY d.file, d.line, d.rowid"
        )
    } else {
        "SELECT d.file, d.name, d.line, d.end_line, 0 FROM decls d WHERE d.kind IN ('function', 'method') ORDER BY d.file, d.line, d.rowid".to_string()
    };
    let read = || -> rusqlite::Result<Vec<Wanted>> {
        let mut statement = conn.prepare(&sql)?;
        let rows = statement.query_map([], |row| {
            let at = |index: usize| row.get::<_, Option<i64>>(index).map(|n| u64::try_from(n.unwrap_or(0)).unwrap_or(0));
            Ok(Wanted { file: row.get(0)?, name: row.get(1)?, line: at(2)?, end_line: at(3)?, stale: row.get(4)? })
        })?;
        rows.collect()
    };
    let rows = read().map_err(|err| unreadable(err.into()))?;
    let mut seen: HashSet<(String, String)> = HashSet::new();
    Ok(rows.into_iter().filter(|item| seen.insert((item.file.clone(), item.name.clone()))).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::normalize::Languages;
    use crate::io::map_notes::{NewNote, declaration_digests, write};
    use crate::io::project_map::{model_path, save_at};
    use serde_json::{Value, json};
    use tempfile::TempDir;

    /// Um arquivo de `names.len()` funções de três linhas cada, gravado no
    /// disco, como o mapa o guarda.
    fn module(dir: &Path, path: &str, names: &[&str], body: &str) -> Value {
        let content: String = names.iter().map(|name| format!("function {name}() {{\n  return {body};\n}}\n")).collect();
        std::fs::create_dir_all(dir.join(path).parent().unwrap()).unwrap();
        std::fs::write(dir.join(path), &content).unwrap();
        let declarations: Vec<Value> = names
            .iter()
            .enumerate()
            .map(|(at, name)| json!({"kind": "function", "name": name, "line": at * 3 + 1, "end_line": at * 3 + 3}))
            .collect();
        let spans = declarations.iter().map(|d| (d["name"].as_str().unwrap(), d["line"].as_u64().unwrap() as usize, d["end_line"].as_u64().unwrap() as usize));
        let mut hash = crate::io::sha256::Sha256::new();
        hash.update(content.as_bytes());
        json!({"path": path, "blob": format!("{path}-{body}"), "declarations": declarations,
               "analysis": {"content_sha256": hash.hex_digest(), "declaration_sha256": declaration_digests(&content, spans)}})
    }

    /// O projeto em TypeScript de três arquivos, com a função de pedidos no
    /// corpo `orders`: `src/a_billing.ts` com `charge`, `src/b_orders.ts`
    /// com `place_order` e `src/c_stock.ts` com `reserve` e `release`.
    fn saved(dir: &TempDir, orders: &str) {
        let modules = json!({"modules": [
            module(dir.path(), "src/a_billing.ts", &["charge"], "1"),
            module(dir.path(), "src/b_orders.ts", &["place_order"], orders),
            module(dir.path(), "src/c_stock.ts", &["reserve", "release"], "3"),
        ]});
        save_at(&model_path(dir.path()), &modules, "scan", &Languages::new(["en-US"])).unwrap();
    }

    fn noted(dir: &TempDir, file: &str, name: &str) {
        write(&model_path(dir.path()), dir.path(), &NewNote { file, name, text: "Reserves the stock of an order.", spec: "notes" }).unwrap();
    }

    fn names(package: &Package) -> Vec<&str> {
        package.entries.iter().map(|entry| entry.name.as_str()).collect()
    }

    /// Três funções sem frase em dia — uma de nota velha, uma de arquivo
    /// mudado e uma qualquer — saem nessa ordem; a de nota em dia fica de
    /// fora. Com o teto de duas, a terceira espera o próximo pacote, e o
    /// tamanho do pacote é o do JSON das funções.
    #[test]
    fn the_package_puts_stale_notes_first_then_changed_files_and_stops_at_the_limit() {
        let dir = tempfile::tempdir().unwrap();
        saved(&dir, "2");
        noted(&dir, "src/c_stock.ts", "release");
        noted(&dir, "src/c_stock.ts", "reserve");
        // O corpo novo de `reserve` envelhece só a nota dela.
        let stock = module(dir.path(), "src/c_stock.ts", &["reserve", "release"], "3");
        let content = std::fs::read_to_string(dir.path().join("src/c_stock.ts")).unwrap().replacen("return 3", "return 4", 1);
        std::fs::write(dir.path().join("src/c_stock.ts"), &content).unwrap();
        let mut stock = stock;
        let mut hash = crate::io::sha256::Sha256::new();
        hash.update(content.as_bytes());
        stock["analysis"] = json!({"content_sha256": hash.hex_digest(),
            "declaration_sha256": declaration_digests(&content, [("reserve", 1, 3), ("release", 4, 6)])});
        let modules = json!({"modules": [
            module(dir.path(), "src/a_billing.ts", &["charge"], "1"),
            module(dir.path(), "src/b_orders.ts", &["place_order"], "2"),
            stock,
        ]});
        save_at(&model_path(dir.path()), &modules, "scan 2", &Languages::new(["en-US"])).unwrap();
        let changed = ["./src/b_orders.ts".to_string()];

        let whole = package(dir.path(), &model_path(dir.path()), &changed, PACKAGE_CHARS).unwrap();
        assert_eq!(names(&whole), ["reserve", "place_order", "charge"], "{whole:?}");
        assert_eq!(whole.waiting, 3);
        assert_eq!(whole.chars, serde_json::to_string(&whole.entries).unwrap().len());
        assert_eq!(whole.entries[0].text, "function reserve() {\n  return 4;\n}");
        assert_eq!((whole.entries[0].line, whole.entries[0].end_line), (1, 3));

        let two = serde_json::to_string(&whole.entries[..2]).unwrap().len();
        let cut = package(dir.path(), &model_path(dir.path()), &changed, two).unwrap();
        assert_eq!(names(&cut), ["reserve", "place_order"]);
        assert_eq!((cut.chars, cut.waiting), (two, 3));
        let short = package(dir.path(), &model_path(dir.path()), &changed, two - 1).unwrap();
        assert_eq!(names(&short), ["reserve"], "the second one waits for the next package");
    }

    /// A função que passa sozinha do teto vai para a lista das grandes demais
    /// e não segura as seguintes.
    #[test]
    fn a_function_larger_than_the_limit_is_listed_apart_and_does_not_hold_the_others() {
        let dir = tempfile::tempdir().unwrap();
        saved(&dir, &"9".repeat(500));
        let package = package(dir.path(), &model_path(dir.path()), &["src/b_orders.ts".to_string()], 400).unwrap();
        assert_eq!(package.too_large.iter().map(|big| big.name.as_str()).collect::<Vec<_>>(), ["place_order"]);
        assert!(package.too_large[0].chars > 400);
        assert_eq!(names(&package), ["charge", "reserve", "release"]);
        assert!(package.chars <= 400, "{package:?}");
    }
}
