//! A leitura das regras do projeto que todo pedido ao revisor leva: os
//! arquivos de regras da raiz e das pastas onde a obra mexeu, como o Claude
//! Code os lê na conversa principal, com o texto que cada um manda incluir.
//! Nada aqui conhece pasta, linguagem ou arquivo de um projeto em particular.

use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};

use globset::GlobBuilder;

use crate::domain::skill::extract_frontmatter;
use crate::domain::spec_events::SpecLog;
use crate::domain::wave_prompt::{touched_by, RulesFile, ROOT_RULES_FILE};

/// Os arquivos de regras de uma pasta, na ordem em que entram. O
/// `CLAUDE.local.md`, que é pessoal, fica de fora.
const FOLDER_RULES: [&str; 2] = [ROOT_RULES_FILE, ".claude/CLAUDE.md"];

/// A pasta das regras avulsas, só na raiz: cada `.md` dela, em qualquer
/// nível, entra, se vale para a obra ([`in_scope`]).
const ROOT_RULES_DIR: &str = ".claude/rules";

/// Quantos níveis de inclusão a leitura segue a partir de um arquivo de
/// regras.
const INCLUDE_DEPTH: usize = 5;

/// Os arquivos de regras que valem para os arquivos `touched`, com caminhos
/// relativos a `root`: os da raiz primeiro, depois os de cada pasta entre a
/// raiz e a pasta de cada arquivo mexido, em ordem de caminho e sem repetir.
/// A pasta com regras e sem arquivo mexido fica de fora, e também a regra
/// avulsa cujos caminhos nenhum arquivo mexido casa. Cada texto vem sem
/// os brancos das pontas, com as inclusões no lugar ([`expanded`]); o arquivo
/// ilegível, vazio ou já incluído por outro não entra.
#[must_use]
pub fn project_rules(root: &Path, touched: &[String]) -> Vec<RulesFile> {
    let Ok(home) = root.canonicalize() else { return Vec::new() };
    let mut folders = BTreeSet::from([String::new()]);
    for path in touched {
        folders.extend(folders_of(root, path));
    }
    let mut seen: BTreeSet<PathBuf> = BTreeSet::new();
    let mut out = Vec::new();
    for folder in &folders {
        let mut paths: Vec<String> = FOLDER_RULES.iter().map(|name| joined(folder, name)).collect();
        if folder.is_empty() {
            paths.extend(markdown_under(root, ROOT_RULES_DIR).into_iter().filter(|path| in_scope(root, path, touched)));
        }
        for path in paths {
            let Ok(file) = root.join(&path).canonicalize() else { continue };
            if !file.starts_with(&home) || !file.is_file() || !seen.insert(file.clone()) {
                continue;
            }
            let Some(text) = expanded(&home, &file, 0, &mut seen) else { continue };
            let text = text.trim();
            if !text.is_empty() {
                out.push(RulesFile { path, text: text.to_string() });
            }
        }
    }
    out
}

/// Os arquivos que a obra mexe, para a leitura das regras: os que as tarefas e
/// as entregas da spec declaram ([`touched_by`]) e, sem spec ou sem nenhum
/// declarado, os que a branch mudou contra a base do projeto. A revisão final
/// e o despacho ao revisor leem por aqui. Sem git ou sem base, nenhum.
#[must_use]
pub fn touched_files(root: &Path, log: Option<&SpecLog>) -> Vec<String> {
    let declared = log.map(touched_by).unwrap_or_default();
    if !declared.is_empty() {
        return declared;
    }
    let base = crate::io::project_map::base_of(root);
    if base.tip.is_empty() {
        return Vec::new();
    }
    let range = format!("{}...HEAD", base.tip);
    let listed = crate::platform::git::run(root, &["-c", "core.quotePath=false", "diff", "--name-only", "--relative", &range]);
    listed.out().unwrap_or_default().lines().map(str::trim).filter(|line| !line.is_empty()).map(str::to_string).collect()
}

/// As pastas entre a raiz e a do arquivo `path`, sem a raiz: `a` e `a/b` para
/// `a/b/c.rs`. O caminho que é uma pasta conta também ela. O caminho absoluto
/// ou que sobe da raiz não dá pasta nenhuma.
fn folders_of(root: &Path, path: &str) -> Vec<String> {
    let path = path.trim().replace('\\', "/");
    let relative = Path::new(&path);
    if relative.components().any(|part| !matches!(part, Component::Normal(_) | Component::CurDir)) {
        return Vec::new();
    }
    let parts: Vec<&str> = path.split('/').filter(|part| !part.is_empty() && *part != ".").collect();
    let own = usize::from(root.join(relative).is_dir());
    let depth = (parts.len() + own).saturating_sub(1);
    (1..=depth).map(|n| parts[..n].join("/")).collect()
}

/// O caminho `name` dentro da pasta `folder`, relativa à raiz.
fn joined(folder: &str, name: &str) -> String {
    if folder.is_empty() {
        name.to_string()
    } else {
        format!("{folder}/{name}")
    }
}

/// Os `.md` da pasta `folder` de `root` e das que ela tem dentro, com o
/// caminho relativo à raiz, em ordem de nome.
fn markdown_under(root: &Path, folder: &str) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(root.join(folder)) else { return Vec::new() };
    let mut names: Vec<String> = entries.flatten().map(|entry| entry.file_name().to_string_lossy().into_owned()).collect();
    names.sort();
    let mut out = Vec::new();
    for name in names {
        let path = format!("{folder}/{name}");
        if root.join(&path).is_dir() {
            out.extend(markdown_under(root, &path));
        } else if name.ends_with(".md") {
            out.push(path);
        }
    }
    out
}

/// `true` quando a regra avulsa `path`, relativa a `root`, vale para os
/// arquivos `touched`: sem caminhos no cabeçalho ([`scoped_paths`]), sempre;
/// com eles, quando algum arquivo mexido casa um dos padrões. O arquivo que não
/// se lê passa, e a leitura das regras o deixa de fora como os outros.
fn in_scope(root: &Path, path: &str, touched: &[String]) -> bool {
    let Ok(text) = std::fs::read_to_string(root.join(path)) else { return true };
    let Some(patterns) = scoped_paths(&text) else { return true };
    let matchers: Vec<_> = patterns
        .iter()
        .filter_map(|pattern| GlobBuilder::new(pattern).literal_separator(true).backslash_escape(true).build().ok())
        .map(|glob| glob.compile_matcher())
        .collect();
    touched.iter().map(|file| file.trim().replace('\\', "/")).any(|file| {
        let file = file.strip_prefix("./").unwrap_or(&file);
        matchers.iter().any(|matcher| matcher.is_match(file))
    })
}

/// Os padrões do campo `paths:` do cabeçalho YAML de uma regra, lidos como o
/// Claude Code os lê: lista em linhas ou entre colchetes, ou um valor de uma
/// linha só, com os padrões separados por vírgula fora das chaves. `None`
/// quando a regra vale sempre: sem cabeçalho, sem `paths:` ou com ele vazio,
/// e também com o cabeçalho que não se lê.
fn scoped_paths(text: &str) -> Option<Vec<String>> {
    let header = extract_frontmatter(text)?;
    let mut lines = header.lines().peekable();
    let mut patterns = Vec::new();
    while let Some(line) = lines.next() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || line.starts_with([' ', '\t']) {
            continue;
        }
        let (key, value) = line.split_once(':')?;
        if key.trim() != "paths" {
            continue;
        }
        let value = value.trim();
        if let Some(flow) = value.strip_prefix('[') {
            let (inner, rest) = flow.rsplit_once(']')?;
            if !(rest.trim().is_empty() || rest.trim().starts_with('#')) {
                return None;
            }
            for item in split_outside(inner) {
                patterns.push(scalar(item)?);
            }
        } else if value.is_empty() || value.starts_with('#') {
            while let Some(item) = lines.peek().map(|next| next.trim()) {
                if let Some(item) = item.strip_prefix('-') {
                    patterns.push(scalar(item)?);
                } else if !(item.is_empty() || item.starts_with('#')) {
                    break;
                }
                lines.next();
            }
        } else {
            patterns.extend(split_outside(&scalar(value)?).into_iter().map(str::to_string));
        }
    }
    let patterns: Vec<String> =
        patterns.iter().map(|pattern| pattern.trim().to_string()).filter(|pattern| !pattern.is_empty()).collect();
    (!patterns.is_empty()).then_some(patterns)
}

/// O escalar YAML `raw`, sem as aspas e sem o comentário do fim. `None` quando
/// a aspa não fecha ou deixa texto depois dela, e quando o valor sem aspas
/// começa por `*`, que no YAML abre uma referência e não um texto.
fn scalar(raw: &str) -> Option<String> {
    let raw = raw.trim();
    let Some(quote) = raw.chars().next().filter(|first| matches!(first, '"' | '\'')) else {
        return (!raw.starts_with('*')).then(|| raw.split(" #").next().unwrap_or_default().trim_end().to_string());
    };
    let (body, rest) = raw[1..].split_once(quote)?;
    let rest = rest.trim();
    (rest.is_empty() || rest.starts_with('#')).then(|| body.to_string())
}

/// `text` partido nas vírgulas que ficam fora de aspas, chaves e colchetes.
fn split_outside(text: &str) -> Vec<&str> {
    let (mut depth, mut quote, mut start, mut out) = (0i32, None, 0, Vec::new());
    for (at, c) in text.char_indices() {
        match (quote, c) {
            (Some(open), _) if c == open => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(c),
            (None, '{' | '[') => depth += 1,
            (None, '}' | ']') => depth -= 1,
            (None, ',') if depth <= 0 => {
                out.push(&text[start..at]);
                start = at + 1;
            }
            _ => {}
        }
    }
    out.push(&text[start..]);
    out
}

/// O texto do arquivo `file`, com cada linha de inclusão trocada pelo texto
/// do arquivo que ela aponta. A linha de inclusão é `@caminho`, sozinha e fora
/// de bloco de código, com o caminho relativo à pasta de `file`. Ela fica como
/// está quando o arquivo apontado não existe, fica fora de `home`, a raiz do
/// projeto, ou já entrou (`seen`), e também depois de [`INCLUDE_DEPTH`]
/// níveis. Sem nenhuma troca, o texto sai byte a byte como está no disco.
fn expanded(home: &Path, file: &Path, depth: usize, seen: &mut BTreeSet<PathBuf>) -> Option<String> {
    let text = std::fs::read_to_string(file).ok()?;
    let folder = file.parent().unwrap_or(home);
    let mut out = String::with_capacity(text.len());
    let mut fenced = false;
    for segment in text.split_inclusive('\n') {
        let line = segment.trim_end_matches(['\r', '\n']);
        let trimmed = line.trim();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            fenced = !fenced;
        }
        let target = trimmed.strip_prefix('@').filter(|rest| !rest.is_empty() && !rest.contains(char::is_whitespace));
        let included = target
            .filter(|_| !fenced && depth < INCLUDE_DEPTH)
            .and_then(|target| folder.join(target).canonicalize().ok())
            .filter(|target| target.starts_with(home) && target.is_file() && seen.insert(target.clone()))
            .and_then(|target| expanded(home, &target, depth + 1, seen));
        match included {
            Some(inner) => {
                out.push_str(inner.trim());
                out.push_str(&segment[line.len()..]);
            }
            None => out.push_str(segment),
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use serde_json::json;
    use tempfile::tempdir;

    use super::*;
    use crate::domain::spec_events::{normalize, parse_log, render_line, stamp};

    /// Grava `text` em `path`, dentro de `root`, com as pastas do caminho.
    fn put(root: &Path, path: &str, text: &str) {
        let file = root.join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, text).unwrap();
    }

    /// Cada caso: os arquivos do projeto, os arquivos mexidos e o que a
    /// leitura devolve, pelo caminho e o texto. Sem arquivo de regras, ou com
    /// ele só com brancos, não há regras; só a raiz dá o texto dela sem os
    /// brancos das pontas. A raiz vem primeiro, com o `.claude/CLAUDE.md` e
    /// cada `.md` de `.claude/rules/`; depois, em ordem de caminho, as pastas
    /// entre a raiz e cada arquivo mexido, a mexida inclusive. A pasta com
    /// regras e sem arquivo mexido, o `CLAUDE.local.md` e o caminho que sai
    /// da raiz ficam fora.
    #[test]
    fn the_rules_files_are_those_of_the_root_and_of_each_folder_on_the_way_to_a_touched_file() {
        let root_crlf = ("CLAUDE.md", "\n# Regras\r\n\r\n- Nunca grave no git.\r\n\n");
        let nested = [
            ("CLAUDE.md", "- Raiz."),
            ("libs/CLAUDE.md", "- Bibliotecas."),
            ("libs/core/src/.claude/CLAUDE.md", "- Fontes do núcleo."),
            ("libs/other/CLAUDE.md", "- Outra biblioteca."),
            ("tools/CLAUDE.md", "- Ferramentas."),
            ("tools/gen/CLAUDE.md", "- Gerador."),
        ];
        let claude_folder = [
            ("CLAUDE.md", "- Raiz."),
            ("CLAUDE.local.md", "- Pessoal."),
            (".claude/CLAUDE.md", "- Pasta do Claude."),
            (".claude/rules/testes.md", "- Testes."),
            (".claude/rules/api/rotas.md", "- Rotas."),
            (".claude/rules/notas.txt", "- Fora."),
            ("web/CLAUDE.local.md", "- Pessoal da web."),
        ];
        /// Um arquivo da tabela: o caminho e o texto.
        type Written<'a> = (&'a str, &'a str);
        /// Uma linha da tabela: o nome do caso, os arquivos do projeto, os
        /// arquivos mexidos e o que a leitura devolve.
        type Case<'a> = (&'a str, &'a [Written<'a>], &'a [&'a str], &'a [Written<'a>]);
        let cases: [Case<'_>; 6] = [
            ("no rules file", &[("src/lib.rs", "")], &["src/lib.rs"], &[]),
            ("a blank file", &[("CLAUDE.md", " \n\n\t\n")], &["src/lib.rs"], &[]),
            ("the root alone", &[root_crlf], &["src/lib.rs"], &[("CLAUDE.md", "# Regras\r\n\r\n- Nunca grave no git.")]),
            (
                "folders on the way",
                &nested,
                &["libs/core/src/lib.rs", "./tools/gen", "libs/core/src/main.rs", "../fora/x.rs", "/etc/x.rs"],
                &[
                    ("CLAUDE.md", "- Raiz."),
                    ("libs/CLAUDE.md", "- Bibliotecas."),
                    ("libs/core/src/.claude/CLAUDE.md", "- Fontes do núcleo."),
                    ("tools/CLAUDE.md", "- Ferramentas."),
                    ("tools/gen/CLAUDE.md", "- Gerador."),
                ],
            ),
            ("nothing touched", &nested, &[], &[("CLAUDE.md", "- Raiz.")]),
            (
                "the claude folder of the root",
                &claude_folder,
                &["web/app.ts"],
                &[
                    ("CLAUDE.md", "- Raiz."),
                    (".claude/CLAUDE.md", "- Pasta do Claude."),
                    (".claude/rules/api/rotas.md", "- Rotas."),
                    (".claude/rules/testes.md", "- Testes."),
                ],
            ),
        ];
        for (case, files, touched, expected) in cases {
            let dir = tempdir().unwrap();
            for (path, text) in files {
                put(dir.path(), path, text);
            }
            let touched: Vec<String> = touched.iter().map(|path| (*path).to_string()).collect();
            let found: Vec<(String, String)> =
                project_rules(dir.path(), &touched).into_iter().map(|file| (file.path, file.text)).collect();
            let expected: Vec<(String, String)> =
                expected.iter().map(|(path, text)| ((*path).to_string(), (*text).to_string())).collect();
            assert_eq!(found, expected, "{case}");
        }
    }

    /// Cada caso: os arquivos mexidos e as regras avulsas que entram. A regra
    /// sem cabeçalho, ou com cabeçalho sem `paths:`, entra sempre, e também a
    /// de cabeçalho que não se lê. A de `paths:` entra só quando um arquivo
    /// mexido casa um padrão: `*` não passa de uma pasta, `**` passa de
    /// várias, `{a,b}` vale por cada opção, a lista vale por cada padrão e o
    /// valor de uma linha só se parte nas vírgulas fora das chaves. O padrão
    /// que não se lê não casa nada, e o outro da mesma regra segue valendo.
    #[test]
    fn a_rule_with_paths_enters_only_when_a_touched_file_matches_one_of_them() {
        let dir = tempdir().unwrap();
        let rules = [
            ("always", "- Sempre."),
            ("other-key", "---\ndescription: sem caminhos\n---\n- Outra chave."),
            ("api", "---\npaths:\n  - \"src/api/**/*.ts\"\n---\n- Api."),
            ("front", "---\r\npaths:\r\n- \"web/**/*.{ts,tsx}\" # telas\r\n- 'lib/**/*.ts'\r\n---\r\n- Front."),
            ("one-line", "---\npaths: docs/*.md, tools/**/*.{sh,ps1}\n---\n- Uma linha."),
            ("flow", "---\npaths: [\"Makefile\", 'ci/*.yml']\n---\n- Colchetes."),
            ("bad-glob", "---\npaths:\n  - \"photos [2024/**\"\n  - \"assets/**\"\n---\n- Fotos."),
            ("bad-quote", "---\npaths:\n  - \"src/**\n---\n- Aspa aberta."),
            ("alias", "---\npaths: **/*.go\n---\n- Referência."),
        ];
        for (name, text) in rules {
            put(dir.path(), &format!(".claude/rules/{name}.md"), text);
        }
        let always = ["alias", "always", "bad-quote", "other-key"];
        let cases: [(&[&str], &[&str]); 11] = [
            (&[], &[]),
            (&["src/api/v1/user.ts"], &["api"]),
            (&["src\\api\\user.ts", "./web/page.tsx"], &["api", "front"]),
            (&["src/api/user.js", "src/web/x.ts"], &[]),
            (&["lib/a/b.ts"], &["front"]),
            (&["docs/guide.md"], &["one-line"]),
            (&["docs/deep/guide.md"], &[]),
            (&["tools/x/run.ps1"], &["one-line"]),
            (&["Makefile", "ci/build.yml"], &["flow"]),
            (&["assets/logo.png"], &["bad-glob"]),
            (&["photos [2024/a.png"], &[]),
        ];
        for (touched, scoped) in cases {
            let touched: Vec<String> = touched.iter().map(|path| (*path).to_string()).collect();
            let found: Vec<String> = project_rules(dir.path(), &touched).into_iter().map(|file| file.path).collect();
            let mut expected: Vec<&str> = always.iter().chain(scoped).copied().collect();
            expected.sort_unstable();
            let expected: Vec<String> = expected.iter().map(|name| format!(".claude/rules/{name}.md")).collect();
            assert_eq!(found, expected, "{touched:?}");
        }
    }

    /// A linha `@caminho` traz o texto do arquivo apontado, relativo ao que a
    /// contém, uma vez só: a segunda menção e o ciclo deixam a linha, e o
    /// arquivo de regras já trazido não volta na lista. A que aponta para fora
    /// do projeto, para arquivo que não existe ou que está num bloco de código
    /// fica como está, e a inclusão para no quinto nível.
    #[test]
    fn an_include_line_brings_the_file_once_and_stays_when_it_leaves_the_project_or_is_missing() {
        let outer = tempdir().unwrap();
        let root = outer.path().join("projeto");
        put(outer.path(), "segredo.md", "- Fora do projeto.");
        put(&root, "CLAUDE.md", "- Raiz.\n@docs/estilo.md\n@docs/estilo.md\n@../segredo.md\n@docs/falta.md\n```\n@docs/estilo.md\n```\n@.claude/CLAUDE.md\n");
        put(&root, ".claude/CLAUDE.md", "- Pasta do Claude.");
        put(&root, "docs/estilo.md", "\n- Estilo.\n@ciclo.md\n");
        put(&root, "docs/ciclo.md", "- Ciclo.\n@estilo.md\n@../CLAUDE.md");
        let read = |root: &Path| -> Vec<(String, String)> {
            project_rules(root, &[]).into_iter().map(|file| (file.path, file.text)).collect()
        };
        let expected = "- Raiz.\n- Estilo.\n- Ciclo.\n@estilo.md\n@../CLAUDE.md\n@docs/estilo.md\n@../segredo.md\n@docs/falta.md\n```\n@docs/estilo.md\n```\n- Pasta do Claude.";
        assert_eq!(read(&root), [("CLAUDE.md".to_string(), expected.to_string())]);

        let deep = tempdir().unwrap();
        put(deep.path(), "CLAUDE.md", "@n1.md");
        for n in 1..=6 {
            put(deep.path(), &format!("n{n}.md"), &format!("- Nível {n}.\n@n{}.md", n + 1));
        }
        let text = read(deep.path()).remove(0).1;
        assert_eq!(text, "- Nível 1.\n- Nível 2.\n- Nível 3.\n- Nível 4.\n- Nível 5.\n@n6.md");
    }

    /// Os arquivos mexidos são os que as tarefas e as entregas da spec
    /// declaram; sem spec, ou com ela sem arquivo declarado, os que a branch
    /// mudou contra a base declarada do projeto.
    #[test]
    fn the_touched_files_are_the_declared_ones_or_else_what_the_branch_changed() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let git = |args: &[&str]| {
            let out = Command::new("git")
                .args(["-c", "user.email=t@t", "-c", "user.name=t", "-c", "commit.gpgsign=false"])
                .args(args)
                .current_dir(root)
                .output()
                .expect("spawn git");
            assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        };
        git(&["init", "-q", "-b", "main"]);
        put(root, "mustard.json", r#"{"git": {"flow": {"*": "main"}}}"#);
        put(root, "README.md", "x");
        git(&["add", "-A"]);
        git(&["commit", "-q", "-m", "base"]);
        git(&["checkout", "-q", "-b", "feature/y"]);
        put(root, "lib/sub/a.rs", "x");
        git(&["add", "-A"]);
        git(&["commit", "-q", "-m", "obra"]);
        assert_eq!(touched_files(root, None), ["lib/sub/a.rs"], "no spec: what the branch changed");

        let line = |id: u64, event_type: &str, body: serde_json::Value| {
            let mut map = normalize(body.as_object().cloned().unwrap_or_default(), event_type);
            map.insert("type".into(), json!(event_type));
            render_line(&stamp(map, id, None, "2026-10-04T10:00:00-03:00"))
        };
        let empty = parse_log(&line(1, "task", json!({"text": "T.", "files": [], "depends_on": []})));
        assert_eq!(touched_files(root, Some(&empty)), ["lib/sub/a.rs"], "no declared file: what the branch changed");
        let log = parse_log(
            &[
                line(1, "task", json!({"wave": 1, "text": "T.", "files": [{"path": "web/app.ts"}, {"path": "api/x.cs"}], "depends_on": []})),
                line(2, "delivered", json!({"wave": 1, "text": "E.", "files": ["api/x.cs", "docs/a.md"], "commit": "c"})),
            ]
            .join("\n"),
        );
        assert_eq!(touched_files(root, Some(&log)), ["web/app.ts", "api/x.cs", "docs/a.md"]);
    }
}
