//! A evidência a mais da análise final de código sem uso: o aviso de
//! declaração sem uso que o compilador ou o lint escreveu na saída dos
//! comandos do fechamento. A prova da análise é do mapa, em qualquer
//! linguagem; este aviso só acrescenta, quando existe: ele confirma, para a
//! declaração cujo nome ainda aparece noutro lugar, que o texto não é um uso.
//!
//! Cada ferramenta escreve o aviso do seu jeito, e cada jeito tem a sua
//! leitura: o diagnóstico em JSON do rustc e do cargo, o texto do rustc e do
//! clippy, o TS6133 e o TS6196 do tsc (com ou sem `--pretty`), o
//! `no-unused-vars` do eslint e do typescript-eslint, e o CS8321 e o IDE0051
//! do compilador do C#. Nenhuma leitura depende da linguagem do arquivo: o
//! aviso só conta quando cita o nome da declaração, no arquivo dela, numa
//! linha dela, e a linha de agora ainda traz o nome onde a ferramenta disse.
//! O caminho relativo do aviso se lê a partir da pasta de onde a ferramenta
//! rodou: a raiz do projeto, de onde os comandos rodam, ou a pasta do pacote
//! que o pnpm, o npm, o turbo ou o yarn dizem, cada um do seu jeito, que
//! rodou o script ([`tool_lines`]). Outro formato não acrescenta nada.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use mustard_core::domain::project_map::{MapDecl, MapModule};
use regex::Regex;
use serde_json::Value;

use crate::commands::flow::validation::{Packages, tool_lines};

/// Um aviso de declaração sem uso, como a ferramenta o escreveu: o arquivo,
/// a linha, os nomes que ele cita e por onde se confere que ele fala da
/// linha de agora.
struct Citation {
    file: PathBuf,
    line: u64,
    names: Vec<String>,
    anchor: Anchor,
}

/// Por onde se confere que o aviso fala da linha de agora, e não de um texto
/// que já mudou.
enum Anchor {
    /// O texto da linha que a ferramenta mostrou, que tem de ser o de agora.
    Shown(Vec<String>),
    /// A coluna, em caracteres contados a partir de 1, onde o nome começa.
    Column(usize),
}

/// As leituras dos formatos de texto, compiladas uma vez.
struct Patterns {
    /// A sequência de cor do terminal, que o `--pretty` do tsc escreve.
    color: Regex,
    /// O título do aviso de código morto do rustc e do clippy, com o que
    /// sobra e os nomes entre crases.
    rust_header: Regex,
    /// O arquivo de um trecho do rustc (`-->` ou `:::`).
    rust_location: Regex,
    /// Uma linha do código que o rustc mostra, com o número dela.
    rust_source: Regex,
    /// A linha das marcas debaixo do código; o `^` marca o trecho principal.
    rust_marks: Regex,
    /// `arquivo(linha,coluna): nível CÓDIGO: texto`, do tsc e do compilador do C#.
    located: Regex,
    /// `arquivo:linha:coluna - nível CÓDIGO: texto`, do `--pretty` do tsc.
    pretty: Regex,
    /// A linha de um aviso do formato padrão do eslint, debaixo do arquivo.
    eslint: Regex,
    /// O nome entre aspas simples.
    quoted: Regex,
    /// O nome entre crases.
    ticked: Regex,
}

fn patterns() -> Option<&'static Patterns> {
    static PATTERNS: OnceLock<Option<Patterns>> = OnceLock::new();
    PATTERNS
        .get_or_init(|| {
            Some(Patterns {
                color: Regex::new(r"\x1b\[[0-9;]*[A-Za-z]").ok()?,
                rust_header: Regex::new(
                    r"^(?:warning|error): (?P<what>[a-z ]+?) (?P<names>`[^`]+`(?:(?:, |,? and )`[^`]+`)*) (?:is|are) never (?:used|constructed|read)$",
                )
                .ok()?,
                rust_location: Regex::new(r"^\s*(?:-->|:::) (?P<file>.+):\d+:\d+$").ok()?,
                rust_source: Regex::new(r"^\s*(?P<line>\d+) \|(?: (?P<text>.*))?$").ok()?,
                rust_marks: Regex::new(r"^\s*\|(?P<marks>.*)$").ok()?,
                located: Regex::new(
                    r"^\s*(?P<file>.+?)\((?P<line>\d+),(?P<column>\d+)\): (?:error|warning) (?:TS6133|TS6196|CS8321|IDE0051): (?P<message>.+)$",
                )
                .ok()?,
                pretty: Regex::new(r"^(?P<file>.+?):(?P<line>\d+):(?P<column>\d+) - (?:error|warning) (?:TS6133|TS6196): (?P<message>.+)$").ok()?,
                eslint: Regex::new(r"^\s+(?P<line>\d+):(?P<column>\d+)\s+(?:error|warning)\s+(?P<message>.+?)\s+(?:\S+/)?no-unused-vars$").ok()?,
                quoted: Regex::new(r"'([^']+)'").ok()?,
                ticked: Regex::new(r"`([^`]+)`").ok()?,
            })
        })
        .as_ref()
}

/// Os avisos de declaração sem uso que as saídas dos comandos escreveram,
/// lidos uma vez para a análise final inteira, com os pacotes do projeto.
#[derive(Default)]
pub(super) struct Evidence(Vec<Citation>);

impl Evidence {
    /// Os avisos das saídas `outputs` dos comandos que rodaram no projeto em
    /// `root`, cada um com o arquivo a partir da raiz.
    pub(super) fn read(root: &Path, outputs: &[String]) -> Self {
        let Some(patterns) = patterns() else {
            return Self::default();
        };
        let packages = Packages::of(root);
        Self(outputs.iter().flat_map(|output| citations(patterns, output, &packages)).collect())
    }
}

/// Algum aviso de `evidence` diz que `decl`, declarada em `module`, sobra:
/// um aviso que cita o nome dela, no arquivo dela dentro de `root`, numa
/// linha dela, e que ainda fala do texto de agora.
pub(super) fn proves(root: &Path, module: &MapModule, decl: &MapDecl, evidence: &Evidence) -> bool {
    let Ok(current) = root.join(&module.path).canonicalize() else {
        return false;
    };
    let Ok(project) = root.canonicalize() else {
        return false;
    };
    if !current.starts_with(&project) {
        return false;
    }
    let Ok(source) = std::fs::read_to_string(&current) else {
        return false;
    };
    evidence.0.iter().any(|citation| lands(root, &current, &source, decl, citation))
}

/// Os avisos de declaração sem uso de `output`, em cada formato que se lê,
/// com o arquivo a partir da raiz. O que uma ferramenta escreveu dentro de um
/// dos `packages` se lê junto, sem as linhas de outro pacote no meio, e o
/// arquivo do aviso é o da pasta dele; o caminho relativo escrito num pacote
/// que não se sabe qual é não leva a arquivo nenhum.
fn citations(patterns: &Patterns, output: &str, packages: &Packages) -> Vec<Citation> {
    let plain = patterns.color.replace_all(output, "");
    let mut out = Vec::new();
    for (dir, text) in by_dir(&plain, packages) {
        let mut found = rustc_json(patterns, &text);
        found.extend(rustc_text(patterns, &text));
        found.extend(located(patterns, &text));
        found.extend(eslint_stylish(patterns, &text));
        for citation in found {
            let file = match dir {
                Some(dir) => Path::new(dir).join(&citation.file),
                None if citation.file.is_absolute() => citation.file.clone(),
                None => continue,
            };
            out.push(Citation { file, ..citation });
        }
    }
    out
}

/// O texto de `output` junto pela pasta de onde rodou a ferramenta que o
/// escreveu, na ordem em que cada pasta aparece; `None` junta o dos pacotes
/// que não se sabe quais são.
fn by_dir<'a>(output: &'a str, packages: &'a Packages) -> Vec<(Option<&'a str>, String)> {
    let mut out: Vec<(Option<&str>, String)> = Vec::new();
    for (dir, text) in tool_lines(output, packages) {
        let at = out.iter().position(|(seen, _)| *seen == dir).unwrap_or_else(|| {
            out.push((dir, String::new()));
            out.len() - 1
        });
        out[at].1.push_str(text);
        out[at].1.push('\n');
    }
    out
}

/// O aviso `citation` fala de `decl`, declarada no arquivo `current` de
/// texto `source`: cita o nome dela, no arquivo dela, numa linha dela, e a
/// linha de agora traz o nome onde a ferramenta disse. O aviso velho, ou o de
/// outra declaração, não conta só porque o caminho bate.
fn lands(root: &Path, current: &Path, source: &str, decl: &MapDecl, citation: &Citation) -> bool {
    if !citation.names.contains(&decl.name) || citation.line < decl.line || citation.line > decl.end_line.max(decl.line) {
        return false;
    }
    if root.join(&citation.file).canonicalize().ok().as_deref() != Some(current) {
        return false;
    }
    let Some(actual) = citation.line.checked_sub(1).and_then(|n| usize::try_from(n).ok()).and_then(|n| source.lines().nth(n)) else {
        return false;
    };
    match &citation.anchor {
        // O texto do rustc mostra a tabulação como quatro espaços; o JSON
        // traz a linha como está.
        Anchor::Shown(texts) => {
            actual.contains(decl.name.as_str()) && texts.iter().any(|text| text == actual || *text == actual.replace('\t', "    "))
        }
        Anchor::Column(column) => column
            .checked_sub(1)
            .and_then(|skip| actual.char_indices().nth(skip))
            .is_some_and(|(at, _)| actual[at..].starts_with(decl.name.as_str())),
    }
}

/// O diagnóstico `dead_code` em JSON do rustc (`--error-format=json`) e do
/// cargo (`--message-format=json`): um aviso por trecho principal, com o
/// texto da linha que ele traz.
fn rustc_json(patterns: &Patterns, output: &str) -> Vec<Citation> {
    let mut out = Vec::new();
    for document in output.lines().filter_map(|line| serde_json::from_str::<Value>(line).ok()) {
        let diagnostic = if document["reason"] == "compiler-message" {
            &document["message"]
        } else if document["$message_type"] == "diagnostic" {
            &document
        } else {
            continue;
        };
        if diagnostic["code"]["code"] != "dead_code" || !matches!(diagnostic["level"].as_str(), Some("warning" | "error")) {
            continue;
        }
        let names = captured(&patterns.ticked, diagnostic["message"].as_str().unwrap_or_default());
        for span in diagnostic["spans"].as_array().into_iter().flatten().filter(|span| span["is_primary"] == true) {
            let (Some(file), Some(line)) = (span["file_name"].as_str(), span["line_start"].as_u64()) else {
                continue;
            };
            let shown = span["text"].as_array().into_iter().flatten().filter_map(|text| text["text"].as_str()).map(str::to_string).collect();
            out.push(Citation { file: file.into(), line, names: names.clone(), anchor: Anchor::Shown(shown) });
        }
    }
    out
}

/// O aviso de código morto no texto do rustc e do clippy: o título diz o que
/// sobra e os nomes, o `-->` diz o arquivo, e cada linha de código marcada
/// com `^` é um trecho principal. O valor atribuído e nunca lido
/// (`unused_assignments`) fala de uma variável, e não de uma declaração.
fn rustc_text(patterns: &Patterns, output: &str) -> Vec<Citation> {
    let mut out = Vec::new();
    let mut names: Vec<String> = Vec::new();
    let mut file: Option<&str> = None;
    let mut shown: Option<(u64, &str)> = None;
    for line in output.lines() {
        if let Some(header) = patterns.rust_header.captures(line) {
            names = if &header["what"] == "value assigned to" { Vec::new() } else { captured(&patterns.ticked, &header["names"]) };
            (file, shown) = (None, None);
            continue;
        }
        if names.is_empty() {
            continue;
        }
        if line.trim().is_empty() || line.starts_with("warning") || line.starts_with("error") {
            names.clear();
        } else if let Some(at) = patterns.rust_location.captures(line) {
            (file, shown) = (at.name("file").map(|m| m.as_str()), None);
        } else if let Some(code) = patterns.rust_source.captures(line) {
            shown = code["line"].parse().ok().map(|n| (n, code.name("text").map_or("", |m| m.as_str())));
        } else if let Some(marks) = patterns.rust_marks.captures(line)
            && let (Some(file), Some((n, text))) = (file, shown.take())
            && marks["marks"].contains('^')
        {
            out.push(Citation { file: file.into(), line: n, names: names.clone(), anchor: Anchor::Shown(vec![text.to_string()]) });
        }
    }
    out
}

/// O aviso de uma linha com arquivo, linha e coluna: o TS6133 e o TS6196 do
/// tsc, nas duas formas, e o CS8321 e o IDE0051 do compilador do C#, que
/// qualifica o membro pelo tipo (`'Calc.Total'`).
fn located(patterns: &Patterns, output: &str) -> Vec<Citation> {
    output
        .lines()
        .filter_map(|line| {
            let found = patterns.located.captures(line).or_else(|| patterns.pretty.captures(line))?;
            let names = captured(&patterns.quoted, &found["message"]).into_iter().map(|name| last_segment(&name).to_string()).collect();
            Some(Citation {
                file: PathBuf::from(&found["file"]),
                line: found["line"].parse().ok()?,
                names,
                anchor: Anchor::Column(found["column"].parse().ok()?),
            })
        })
        .collect()
}

/// O `no-unused-vars` no formato padrão do eslint (e o do
/// typescript-eslint): o arquivo numa linha sem recuo, e cada aviso dele nas
/// linhas recuadas de baixo.
fn eslint_stylish(patterns: &Patterns, output: &str) -> Vec<Citation> {
    let mut out = Vec::new();
    let mut file: Option<&str> = None;
    for line in output.lines() {
        if !line.is_empty() && !line.starts_with(char::is_whitespace) {
            file = Some(line.trim_end());
            continue;
        }
        let (Some(file), Some(found)) = (file, patterns.eslint.captures(line)) else {
            continue;
        };
        let (Ok(at), Ok(column)) = (found["line"].parse(), found["column"].parse()) else {
            continue;
        };
        out.push(Citation { file: file.into(), line: at, names: captured(&patterns.quoted, &found["message"]), anchor: Anchor::Column(column) });
    }
    out
}

/// Cada nome que `pattern` captura em `text`.
fn captured(pattern: &Regex, text: &str) -> Vec<String> {
    pattern.captures_iter(text).filter_map(|found| found.get(1)).map(|name| name.as_str().to_string()).collect()
}

/// O último pedaço do nome qualificado por ponto (`Calc.Total` dá `Total`).
fn last_segment(name: &str) -> &str {
    name.rsplit('.').next().unwrap_or(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_compiler_diagnostic_counts_only_for_the_current_source_of_the_declaration() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("math.rs"), "fn unused_sum() {}\n").unwrap();
        let module: MapModule = serde_json::from_value(json!({"path":"math.rs","language":"rust",
            "analysis":{"parse_complete":true,"relations":"syntactic-candidates"}}))
        .unwrap();
        let decl: MapDecl = serde_json::from_value(json!({"name":"unused_sum","kind":"function","line":1,"end_line":1})).unwrap();
        assert!(!proves(dir.path(), &module, &decl, &Evidence::default()));
        let diagnostic = json!({"$message_type":"diagnostic","level":"warning","code":{"code":"dead_code"},
            "message":"function `unused_sum` is never used","spans":[{"file_name":"math.rs","line_start":1,
            "is_primary":true,"text":[{"text":"fn unused_sum() {}"}]}]})
        .to_string();
        assert!(proves(dir.path(), &module, &decl, &Evidence::read(dir.path(), std::slice::from_ref(&diagnostic))));
        std::fs::write(dir.path().join("math.rs"), "fn unused_sum() { runtime_registration(); }\n").unwrap();
        assert!(!proves(dir.path(), &module, &decl, &Evidence::read(dir.path(), &[diagnostic])));
    }
    #[test]
    fn actual_compiler_evidence_distinguishes_private_dead_code_from_a_public_contract() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        // `noted_sum` tem o nome escrito noutro arquivo, o que o mapa não liga
        // e pode ser um uso; `unused_sum` não aparece em lugar nenhum.
        std::fs::write(root.join("math.rs"), "fn unused_sum() {}\nfn noted_sum() {}\npub fn public_contract() {}\n").unwrap();
        std::fs::write(root.join("notes.txt"), "noted_sum fica para a próxima versão\n").unwrap();
        for args in [&["init", "-q"][..], &["add", "-A"]] {
            assert!(std::process::Command::new("git").args(args).current_dir(root).status().unwrap().success());
        }
        let ran = std::process::Command::new("rustc").args(["--crate-type", "lib", "--error-format=json", "math.rs"]).current_dir(root).output().unwrap();
        assert!(ran.status.success(), "{}", String::from_utf8_lossy(&ran.stderr));
        let output = String::from_utf8_lossy(&ran.stderr).into_owned();
        let module: MapModule = serde_json::from_value(json!({"path":"math.rs","language":"rust"})).unwrap();
        let private: MapDecl = serde_json::from_value(json!({"name":"unused_sum","kind":"function","line":1,"end_line":1})).unwrap();
        let public: MapDecl = serde_json::from_value(json!({"name":"public_contract","kind":"function","line":3,"end_line":3})).unwrap();
        let evidence = Evidence::read(root, std::slice::from_ref(&output));
        assert!(proves(root, &module, &private, &evidence));
        assert!(!proves(root, &module, &public, &evidence));
        let after = serde_json::from_value(json!({"modules":[{"path":"math.rs","language":"rust","declarations":[
            {"name":"unused_sum","kind":"function","line":1,"end_line":1},
            {"name":"noted_sum","kind":"function","line":2,"end_line":2},
            {"name":"public_contract","kind":"function","line":3,"end_line":3,"syntax":{"exported":true}},
        ]}]}))
        .unwrap();
        let maps = super::super::commit::AfterWave { base: Default::default(), after, changed: vec![(1, vec!["math.rs".into()])] };
        let refused = |outputs: &[String]| -> Vec<String> {
            let findings = super::super::removed_check::final_findings(root, &maps, mustard_core::platform::i18n::Locale::PtBr, outputs);
            ["unused_sum", "noted_sum", "public_contract"]
                .into_iter()
                .filter(|name| findings.iter().any(|finding| finding.refuses && finding.text.contains(&format!("`{name}`"))))
                .map(str::to_string)
                .collect()
        };
        // Sem o compilador, o mapa já recusa a privada sem uso; com ele, a de
        // nome escrito noutro lugar também recusa. A exportada só avisa.
        assert_eq!(refused(&[]), ["unused_sum"]);
        assert_eq!(refused(&[output]), ["unused_sum", "noted_sum"]);
    }

    /// O código de cada saída abaixo, tal como a ferramenta o leu.
    const RUST_DEAD: &str = "fn unused_sum() -> u32 {\n    1\n}\n\nstruct Lonely;\n\nimpl Lonely {\n\tfn tabbed_total(&self) {}\n}\n\npub fn kept() {}\n";
    const RUST_PAIR: &str = "struct Pair;\n\nimpl Pair {\n    fn first_total(&self) {}\n    fn second_total(&self) {}\n}\n\nfn assigned() {\n    let mut assigned = 1;\n    assigned = 2;\n}\n\npub fn kept() { assigned(); let _ = Pair; }\n";
    const TYPESCRIPT: &str = "function lonelyTotal(): number {\n  return 2;\n}\n\nclass LonelyBox {\n\tprivate unusedField = 1;\n\tprivate hiddenSum(): number {\n\t\treturn 1;\n\t}\n}\n\ninterface LonelyShape {\n  n: number;\n}\n\ntype LonelyAlias = number;\n\nexport function runSum(): number {\n  const spare = 3;\n  return 1;\n}\n";
    const JAVASCRIPT: &str = "function lonelyTotal() {\n  return 2;\n}\n\nclass LonelyBox {}\n\nexport function runSum() {\n\tconst spare = 3;\n  return 1;\n}\n";
    const CSHARP: &str = "namespace App {\n  public class Calc {\n    public int RunSum() {\n      int Helper() => 2;\n      return 1;\n    }\n\n\tprivate int LonelyTotal() { return 2; }\n  }\n}\n";

    /// A saída de `cargo clippy` (1.9x) sobre [`RUST_DEAD`].
    const CLIPPY_DEAD: &str = r"    Checking prova v0.1.0 ({root})
warning: function `unused_sum` is never used
 --> src/lib.rs:1:4
  |
1 | fn unused_sum() -> u32 {
  |    ^^^^^^^^^^
  |
  = note: `#[warn(dead_code)]` (part of `#[warn(unused)]`) on by default

warning: struct `Lonely` is never constructed
 --> src/lib.rs:5:8
  |
5 | struct Lonely;
  |        ^^^^^^

warning: method `tabbed_total` is never used
 --> src/lib.rs:8:5
  |
7 | impl Lonely {
  | ----------- method in this implementation
8 |     fn tabbed_total(&self) {}
  |        ^^^^^^^^^^^^

warning: `prova` (lib) generated 3 warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 8.54s
";

    /// A saída de `cargo clippy` sobre [`RUST_PAIR`]: dois nomes num aviso
    /// só, e a variável de nome igual ao da função que a declara.
    const CLIPPY_PAIR: &str = r"    Checking prova v0.1.0 ({root})
warning: variable `assigned` is assigned to, but never used
 --> src/lib.rs:9:9
  |
9 |     let mut assigned = 1;
  |         ^^^^^^^^^^^^
  |
  = note: consider using `_assigned` instead
  = note: `#[warn(unused_variables)]` (part of `#[warn(unused)]`) on by default

warning: value assigned to `assigned` is never read
  --> src/lib.rs:10:5
   |
10 |     assigned = 2;
   |     ^^^^^^^^^^^^
   |
   = help: maybe it is overwritten before being read?
   = note: `#[warn(unused_assignments)]` (part of `#[warn(unused)]`) on by default

warning: methods `first_total` and `second_total` are never used
 --> src/lib.rs:4:8
  |
3 | impl Pair {
  | --------- methods in this implementation
4 |     fn first_total(&self) {}
  |        ^^^^^^^^^^^
5 |     fn second_total(&self) {}
  |        ^^^^^^^^^^^^
  |
  = note: `#[warn(dead_code)]` (part of `#[warn(unused)]`) on by default

warning: `prova` (lib) generated 3 warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 2.02s
";

    /// A saída de `tsc -p .` (7.0) sobre [`TYPESCRIPT`], com `noUnusedLocals`.
    const TSC: &str = r"web/math.ts(1,10): error TS6133: 'lonelyTotal' is declared but its value is never read.
web/math.ts(5,7): error TS6196: 'LonelyBox' is declared but never used.
web/math.ts(6,10): error TS6133: 'unusedField' is declared but its value is never read.
web/math.ts(7,10): error TS6133: 'hiddenSum' is declared but its value is never read.
web/math.ts(12,11): error TS6196: 'LonelyShape' is declared but never used.
web/math.ts(16,6): error TS6196: 'LonelyAlias' is declared but never used.
web/math.ts(19,9): error TS6133: 'spare' is declared but its value is never read.
";

    /// O começo e o fim da saída de `tsc -p . --pretty` sobre [`TYPESCRIPT`],
    /// com as cores do terminal.
    const TSC_PRETTY: &str = "\x1b[96mweb/math.ts\x1b[0m:\x1b[93m1\x1b[0m:\x1b[93m10\x1b[0m - \x1b[91merror\x1b[0m\x1b[90m TS6133: \x1b[0m'lonelyTotal' is declared but its value is never read.\n\
\n\
\x1b[7m1\x1b[0m function lonelyTotal(): number {\n\
\x1b[7m \x1b[0m \x1b[91m         ~~~~~~~~~~~\x1b[0m\n\
\n\
\x1b[96mweb/math.ts\x1b[0m:\x1b[93m5\x1b[0m:\x1b[93m7\x1b[0m - \x1b[91merror\x1b[0m\x1b[90m TS6196: \x1b[0m'LonelyBox' is declared but never used.\n\
\n\
\x1b[7m5\x1b[0m class LonelyBox {\n\
\x1b[7m \x1b[0m \x1b[91m      ~~~~~~~~~\x1b[0m\n\
\n\
\n\
Found 7 errors in the same file, starting at: web/math.ts\x1b[90m:1\x1b[0m\n\
\n";

    /// A saída de `eslint web` (10.9) sobre [`JAVASCRIPT`], com
    /// `no-unused-vars` em aviso.
    const ESLINT: &str = "
{root}/web/math.js
  1:10  warning  'lonelyTotal' is defined but never used     no-unused-vars
  5:7   warning  'LonelyBox' is defined but never used       no-unused-vars
  8:8   warning  'spare' is assigned a value but never used  no-unused-vars

\u{2716} 3 problems (0 errors, 3 warnings)

";

    /// A saída de `eslint web` sobre [`TYPESCRIPT`], com o
    /// `@typescript-eslint/no-unused-vars` (8.69) em aviso.
    const TYPESCRIPT_ESLINT: &str = "
{root}/web/math.ts
   1:10  warning  'lonelyTotal' is defined but never used     @typescript-eslint/no-unused-vars
   5:7   warning  'LonelyBox' is defined but never used       @typescript-eslint/no-unused-vars
  12:11  warning  'LonelyShape' is defined but never used     @typescript-eslint/no-unused-vars
  16:6   warning  'LonelyAlias' is defined but never used     @typescript-eslint/no-unused-vars
  19:9   warning  'spare' is assigned a value but never used  @typescript-eslint/no-unused-vars

\u{2716} 5 problems (0 errors, 5 warnings)

";

    /// A saída de `dotnet build` (9.0) sobre [`CSHARP`], com o IDE0051 em
    /// aviso no `.editorconfig` e `EnforceCodeStyleInBuild`.
    const DOTNET: &str = "  Determining projects to restore...
  All projects are up-to-date for restore.
{root}/App/Calc.cs(4,11): warning CS8321: The local function 'Helper' is declared but never used [{root}/App/App.csproj]
{root}/App/Calc.cs(8,14): warning IDE0051: Private member 'Calc.LonelyTotal' is unused (https://learn.microsoft.com/dotnet/fundamentals/code-analysis/style-rules/ide0051) [{root}/App/App.csproj]
  App -> {root}/App/bin/Debug/net9.0/App.dll

Build succeeded.

{root}/App/Calc.cs(4,11): warning CS8321: The local function 'Helper' is declared but never used [{root}/App/App.csproj]
{root}/App/Calc.cs(8,14): warning IDE0051: Private member 'Calc.LonelyTotal' is unused (https://learn.microsoft.com/dotnet/fundamentals/code-analysis/style-rules/ide0051) [{root}/App/App.csproj]
    2 Warning(s)
    0 Error(s)

Time Elapsed 00:00:04.57
";

    /// A declaração `name`, das linhas `line` a `end_line`.
    fn declared(name: &str, line: u64, end_line: u64) -> MapDecl {
        serde_json::from_value(json!({"name": name, "kind": "function", "line": line, "end_line": end_line})).unwrap()
    }

    /// Cada ferramenta, com o arquivo e o código que ela leu, a saída que ela
    /// escreveu, as declarações que a saída diz que sobram (nome, primeira e
    /// última linha) e as que ela não diz, mesmo com o aviso de outra coisa
    /// dentro delas.
    type ToolCase = (&'static str, &'static str, &'static str, &'static str, &'static [(&'static str, u64, u64)], &'static [(&'static str, u64, u64)]);
    const TOOLS: [ToolCase; 7] = [
        ("clippy", "src/lib.rs", RUST_DEAD, CLIPPY_DEAD, &[("unused_sum", 1, 3), ("Lonely", 5, 5), ("tabbed_total", 8, 8)], &[("kept", 11, 11)]),
        ("clippy", "src/lib.rs", RUST_PAIR, CLIPPY_PAIR, &[("first_total", 4, 4), ("second_total", 5, 5)], &[("assigned", 8, 11), ("kept", 13, 13)]),
        (
            "tsc",
            "web/math.ts",
            TYPESCRIPT,
            TSC,
            &[("lonelyTotal", 1, 3), ("LonelyBox", 5, 10), ("hiddenSum", 7, 9), ("LonelyShape", 12, 14), ("LonelyAlias", 16, 16)],
            &[("runSum", 18, 21)],
        ),
        ("tsc --pretty", "web/math.ts", TYPESCRIPT, TSC_PRETTY, &[("lonelyTotal", 1, 3), ("LonelyBox", 5, 10)], &[("runSum", 18, 21)]),
        ("eslint", "web/math.js", JAVASCRIPT, ESLINT, &[("lonelyTotal", 1, 3), ("LonelyBox", 5, 5)], &[("runSum", 7, 10)]),
        ("typescript-eslint", "web/math.ts", TYPESCRIPT, TYPESCRIPT_ESLINT, &[("lonelyTotal", 1, 3), ("LonelyShape", 12, 14), ("LonelyAlias", 16, 16)], &[("runSum", 18, 21)]),
        ("dotnet", "App/Calc.cs", CSHARP, DOTNET, &[("Helper", 4, 4), ("LonelyTotal", 8, 8)], &[("RunSum", 3, 6)]),
    ];

    /// Cada ferramenta escreve o aviso do seu jeito, e a saída de verdade de
    /// cada uma prova só a declaração que ela cita, na linha que ela diz, e
    /// nada dentro da declaração de outro nome. Com o código mudado depois do
    /// aviso (cada linha um espaço adiante), a mesma saída não prova mais nada.
    #[test]
    fn each_tool_output_proves_only_the_declaration_it_cites_on_its_current_line() {
        for (tool, file, source, output, unused, kept) in TOOLS {
            let dir = tempfile::tempdir().unwrap();
            let root = dir.path();
            std::fs::create_dir_all(root.join(file).parent().unwrap()).unwrap();
            std::fs::write(root.join(file), source).unwrap();
            let module: MapModule = serde_json::from_value(json!({"path": file})).unwrap();
            let evidence = Evidence::read(root, &[output.replace("{root}", &root.to_string_lossy())]);
            for &(name, line, end_line) in unused {
                assert!(proves(root, &module, &declared(name, line, end_line), &evidence), "{tool}: `{name}`");
            }
            for &(name, line, end_line) in kept {
                assert!(!proves(root, &module, &declared(name, line, end_line), &evidence), "{tool}: `{name}`");
            }
            let moved: String = source.lines().flat_map(|line| [" ", line, "\n"]).collect();
            std::fs::write(root.join(file), moved).unwrap();
            for &(name, line, end_line) in unused {
                assert!(!proves(root, &module, &declared(name, line, end_line), &evidence), "{tool}, moved: `{name}`");
            }
        }
    }

    /// No TypeScript, no JavaScript e no C#, com o mapa do scan de verdade: a
    /// declaração privada que o mapa não vê usada, mas cujo nome uma nota
    /// cita, só avisa na análise final; com a saída da ferramenta da
    /// linguagem dizendo que ela sobra, recusa.
    #[test]
    fn a_tool_output_makes_a_cited_private_declaration_refuse_in_other_languages() {
        let scan = mustard_core::Scan::locate();
        assert!(scan.is_compiled_alongside(), "rode `cargo build -p scan` antes de `cargo test -p mustard-rt`");
        for (file, source, name, output) in
            [("web/math.ts", TYPESCRIPT, "lonelyTotal", TSC), ("web/math.js", JAVASCRIPT, "lonelyTotal", ESLINT), ("App/Calc.cs", CSHARP, "LonelyTotal", DOTNET)]
        {
            let dir = tempfile::tempdir().unwrap();
            let root = dir.path();
            std::fs::create_dir_all(root.join(file).parent().unwrap()).unwrap();
            std::fs::write(root.join(file), source).unwrap();
            std::fs::write(root.join("notes.txt"), format!("{name} fica para a próxima versão\n")).unwrap();
            super::super::tests::git_at(root, &["init", "-q"]);
            super::super::tests::git_at(root, &["add", "-A"]);
            let maps = tempfile::tempdir().unwrap();
            let out = maps.path().join("after.db");
            scan.scan(root, &out).expect("the scan writes the map");
            let after = mustard_core::io::project_map::read_at(&out).expect("the map reads back");
            let maps = super::super::commit::AfterWave { base: Default::default(), after, changed: vec![(1, vec![file.into()])] };
            let refuses = |outputs: &[String]| {
                let findings = super::super::removed_check::final_findings(root, &maps, mustard_core::platform::i18n::Locale::PtBr, outputs);
                let cited: Vec<bool> = findings.iter().filter(|finding| finding.text.contains(&format!("`{name}` em `{file}`"))).map(|finding| finding.refuses).collect();
                assert_eq!(cited.len(), 1, "{file}: {:?}", findings.iter().map(|finding| &finding.text).collect::<Vec<_>>());
                cited[0]
            };
            assert!(!refuses(&[]), "{file}: sem a ferramenta, só avisa");
            assert!(refuses(&[output.replace("{root}", &root.to_string_lossy())]), "{file}: com a ferramenta, recusa");
        }
    }

    /// O `src/a.ts` da raiz e dos dois pacotes do monorepo de onde saiu a
    /// saída do `pnpm -r lint`.
    const WORKSPACE_SOURCE: &str = "function lonelyTotal(): number {\n  return 2;\n}\n\nexport function runSum(): number {\n  return 1;\n}\n";

    /// Num monorepo do pnpm com o mesmo `src/a.ts` na raiz e em dois pacotes,
    /// com o mapa do scan de verdade e uma nota que cita `lonelyTotal`: o tsc
    /// que o `pnpm -r lint` rodou dentro de `packages/web` diz que ela sobra,
    /// com o caminho a partir do pacote. Só a declaração desse pacote recusa;
    /// a da raiz e a do outro pacote, de mesmo nome na mesma linha, só avisam.
    /// Na saída do pnpm de um pacote só, do npm, do turbo e do yarn, recusa a
    /// declaração de cada pacote em que o tsc rodou e disse que ela sobra, e
    /// só ela; o aviso escrito no bloco de um pacote que não se sabe qual é
    /// não recusa nenhuma, nem a da raiz.
    #[test]
    fn a_warning_from_a_tool_run_inside_a_workspace_package_lands_only_on_that_package() {
        let scan = mustard_core::Scan::locate();
        assert!(scan.is_compiled_alongside(), "rode `cargo build -p scan` antes de `cargo test -p mustard-rt`");
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let files = ["src/a.ts", "packages/web/src/a.ts", "packages/api/src/a.ts"];
        std::fs::write(root.join("pnpm-workspace.yaml"), "packages:\n  - \"packages/*\"\n").unwrap();
        for (name, file) in ["mono", "@mono/web", "@mono/api"].into_iter().zip(files) {
            std::fs::create_dir_all(root.join(file).parent().unwrap()).unwrap();
            std::fs::write(root.join(file), WORKSPACE_SOURCE).unwrap();
            let manifest = json!({"name": name, "version": "1.0.0", "private": true, "scripts": {"lint": "tsc --noEmit -p ."}});
            std::fs::write(root.join(file).parent().unwrap().parent().unwrap().join("package.json"), manifest.to_string()).unwrap();
        }
        std::fs::write(root.join("notes.txt"), "lonelyTotal fica para a próxima versão\n").unwrap();
        super::super::tests::git_at(root, &["init", "-q"]);
        super::super::tests::git_at(root, &["add", "-A"]);
        let maps = tempfile::tempdir().unwrap();
        let out = maps.path().join("after.db");
        scan.scan(root, &out).expect("the scan writes the map");
        let after = mustard_core::io::project_map::read_at(&out).expect("the map reads back");
        let maps = super::super::commit::AfterWave { base: Default::default(), after, changed: vec![(1, files.map(str::to_string).to_vec())] };
        let refuses = |outputs: &[String]| -> Vec<bool> {
            let findings = super::super::removed_check::final_findings(root, &maps, mustard_core::platform::i18n::Locale::PtBr, outputs);
            files
                .iter()
                .map(|file| {
                    let cited: Vec<bool> = findings.iter().filter(|finding| finding.text.contains(&format!("`lonelyTotal` em `{file}`"))).map(|finding| finding.refuses).collect();
                    assert_eq!(cited.len(), 1, "{file}: {:?}", findings.iter().map(|finding| &finding.text).collect::<Vec<_>>());
                    cited[0]
                })
                .collect()
        };
        assert_eq!(refuses(&[]), [false, false, false], "sem a ferramenta, as três só avisam");
        let output = crate::commands::flow::validation::PNPM_RECURSIVE_LINT.replace("{root}", &root.to_string_lossy());
        assert_eq!(refuses(&[output]), [false, true, false], "o aviso do tsc de `packages/web` recusa só a de lá");
        let tools = crate::commands::flow::validation::MONOREPO_LINT.into_iter().chain(crate::commands::flow::validation::PACKAGE_BLOCKS);
        for (tool, output, dirs) in tools {
            let ran = ["", "packages/web", "packages/api"].map(|dir| dirs.contains(&dir));
            assert_eq!(refuses(&[output.replace("{root}", &root.to_string_lossy())]), ran, "{tool}");
        }
    }
}
