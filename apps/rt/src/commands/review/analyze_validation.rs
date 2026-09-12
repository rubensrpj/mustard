//! `mustard-rt run analyze-validation` — a port of `scripts/analyze-validation.js`.
//!
//! WARN-level spec validator (never blocks the pipeline). Checks layer
//! coverage, file-reference resolvability, task-count sanity, and the
//! extended-light scope ↔ model constraint. Emits one JSON line:
//! `{ "ok": bool, "issues": [{ severity, type, message, file? }] }`.
//!
//! The project root is a PARAMETER of [`validate`], never re-derived from the
//! process working directory. This tool cuts a worktree per work unit, so the
//! validator runs off-root as a matter of course; a hidden `current_dir()`
//! would quietly answer about a different project (and could not be tested
//! without mutating the process).
//!
//! # What counts as a file reference
//!
//! `## Files` entries are read as backtick-wrapped tokens. A token is a
//! reference to ONE concrete file — and so gets existence-checked — when:
//!
//! 1. every character is a path character: `[A-Za-z0-9./_-]` plus the routing
//!    punctuation `(`, `)`, `[`, `]`, `{`, `}` and `*`;
//! 2. it carries no PATTERN metacharacter (`*`, `{`, `}`), no `//`, and no
//!    elision segment (`...`) — `plugin/**/*.md`, `wave-N-{role}/spec.md` and
//!    `.../spec.md` name a SET, a shape or an omission, not a file. Brackets
//!    are NOT patterns: `app/[slug]` is a literal directory a routing
//!    convention puts on disk;
//! 3. its last segment splits into a non-empty stem and a clean extension — a
//!    stem-less token (`.tsx`) names an extension, not a file;
//! 4. that extension is known (`KNOWN_FILE_EXTS`) or the token carries a `/`.
//!
//! Rules 2-4 are the price of rule 1: widening the character set admits more
//! prose, so the reference test tightened in the same pass. A token the rule
//! rejects is simply not checked — the validator stays silent about it instead
//! of warning about a file nobody wrote.

use crate::commands::review::{ac_negative_check, qa_run};
use crate::commands::spec::spec_sections::{self, is_heading};
use mustard_core::io::fs;
use std::collections::{BTreeMap, BTreeSet};
use mustard_core::platform::i18n;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// File extensions expected per declared agent layer.
fn layer_extensions(layer: &str) -> &'static [&'static str] {
    match layer {
        "Backend" => &[".ts", ".cs", ".py", ".go", ".rs"],
        "Frontend" => &[".tsx", ".jsx", ".vue", ".svelte", ".html", ".css"],
        "Database" => &[".sql", ".prisma", "schema.ts"],
        "Mobile" => &[".swift", ".kt", ".dart"],
        _ => &[],
    }
}

/// Emit a fatal `validator-crash` result and exit 1.
fn crash(message: &str) -> ! {
    let out = json!({
        "ok": false,
        "issues": [{ "severity": "ERROR", "type": "validator-crash", "message": message }],
    });
    println!("{out}");
    std::process::exit(1);
}

/// Extract the body lines of the `## Files` section.
fn files_section_lines(lines: &[&str]) -> Vec<String> {
    let mut in_files = false;
    let mut out = Vec::new();
    for line in lines {
        if is_heading(line, "files") {
            in_files = true;
            continue;
        }
        if in_files && line.starts_with("##") {
            in_files = false;
        }
        if in_files {
            out.push((*line).to_string());
        }
    }
    out
}

/// Find every `### {Word} Agent` header and return the agent name + the body
/// up to the next `##`/`###` heading.
fn agent_blocks(content: &str) -> Vec<(String, String)> {
    let mut blocks = Vec::new();
    let lines: Vec<&str> = content.split('\n').collect();
    for (i, line) in lines.iter().enumerate() {
        // `###\s+(\S.*?)\s+Agent`
        let Some(rest) = line.strip_prefix("###") else {
            continue;
        };
        if !rest.starts_with([' ', '\t']) {
            continue;
        }
        let rest = rest.trim_start();
        let Some(agent_pos) = rest.find(" Agent") else {
            continue;
        };
        let name = rest[..agent_pos].trim();
        if name.is_empty() {
            continue;
        }
        let mut body = String::new();
        for next in lines.iter().skip(i + 1) {
            let t = next.trim_start();
            if t.starts_with("## ") || t.starts_with("### ") {
                break;
            }
            body.push_str(next);
            body.push('\n');
        }
        blocks.push((name.to_string(), body));
    }
    blocks
}

/// Extract the first capture of a simple `key:\s*["']?value["']?` pattern,
/// case-insensitive. `value_chars` controls which chars belong to the value.
fn extract_kv<'a>(content: &'a str, key: &str) -> Option<&'a str> {
    let lower = content.to_lowercase();
    let key_lower = key.to_lowercase();
    let mut search = 0;
    while let Some(rel) = lower[search..].find(&key_lower) {
        let at = search + rel;
        let after = &content[at + key.len()..];
        let after_t = after.trim_start_matches([' ', '\t']);
        if !after_t.starts_with(':') {
            search = at + key.len();
            continue;
        }
        let mut val = after_t[1..].trim_start_matches([' ', '\t']);
        val = val.strip_prefix(['"', '\'']).unwrap_or(val);
        let end = val
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_'))
            .unwrap_or(val.len());
        let token = &val[..end];
        if !token.is_empty() {
            return Some(token);
        }
        search = at + key.len();
    }
    None
}

/// Count `- [ ]` / `- [x]` checkbox markers in a block.
fn count_tasks(block: &str) -> usize {
    let mut count = 0;
    let bytes = block.as_bytes();
    let needle = b"- [";
    let mut i = 0;
    while i + 5 <= bytes.len() {
        if &bytes[i..i + 3] == needle {
            let c = bytes[i + 3];
            if (c == b' ' || c == b'x') && bytes[i + 4] == b']' {
                count += 1;
                i += 5;
                continue;
            }
        }
        i += 1;
    }
    count
}

/// Common source/config/doc file extensions — the "is this token a real file?"
/// allowlist for [`backtick_file_refs`]. A backtick token without a path
/// separator must end in one of these to count as a file ref; otherwise dotted
/// prose (`extensions.code`, `err.message`) reads as a path to the char-class
/// check and is wrongly flagged as a missing file.
const KNOWN_FILE_EXTS: &[&str] = &[
    "rs", "ts", "tsx", "js", "jsx", "mjs", "cjs", "vue", "svelte", "py", "go", "cs",
    "java", "kt", "swift", "dart", "rb", "php", "c", "h", "cpp", "hpp", "scala",
    "ex", "exs", "html", "css", "scss", "sass", "less", "json", "jsonc", "toml",
    "yaml", "yml", "xml", "ini", "env", "lock", "sql", "prisma", "graphql", "proto",
    "md", "mdx", "txt", "sh", "bash", "ps1", "bat",
];

/// `true` when `ext` (no leading dot) is a recognised file extension.
fn is_known_file_ext(ext: &str) -> bool {
    KNOWN_FILE_EXTS.contains(&ext.to_ascii_lowercase().as_str())
}

/// Characters allowed inside a backtick-wrapped path token.
///
/// Beyond the plain `[A-Za-z0-9./_-]` set this admits routing punctuation:
/// `(`/`)` (route groups), `[`/`]` (dynamic segments), `{`/`}` (template
/// placeholders) and `*` (glob wildcards). They occur in the paths specs
/// genuinely list, so one of them must not disqualify — and silently drop —
/// the whole token; whether the token names one concrete file is decided
/// afterwards by [`is_file_reference`].
fn is_path_token_char(c: char) -> bool {
    c.is_ascii_alphanumeric()
        || matches!(c, '.' | '/' | '-' | '_' | '(' | ')' | '[' | ']' | '{' | '}' | '*')
}

/// THE REFERENCE RULE (stated in full in the module docs): `true` when `token`
/// names ONE concrete file, so its absence on disk is worth a WARN.
///
/// Widening [`is_path_token_char`] admits more prose, so the two changes were
/// designed together: a PATTERN (`plugin/**/*.md`), a TEMPLATE
/// (`wave-N-{role}/spec.md`), an ELISION (`.../spec.md`) and a bare EXTENSION
/// (`.tsx`) all read as paths to a character class, yet none of them is a file
/// anybody wrote. Brackets are deliberately not pattern characters — a dynamic
/// segment such as `app/[slug]` is a literal directory on disk. Pure, total.
fn is_file_reference(token: &str) -> bool {
    if token.is_empty() || !token.chars().all(is_path_token_char) {
        return false;
    }
    // A glob or a `{placeholder}` names a set or a shape, never one file.
    if token.contains(['*', '{', '}']) {
        return false;
    }
    // `//` is malformed; a run of three-or-more dots is a documentation elision
    // (`.../spec.md`), not a directory name.
    if token.contains("//")
        || token
            .split('/')
            .any(|seg| seg.len() >= 3 && seg.chars().all(|c| c == '.'))
    {
        return false;
    }
    // The file name must split into a non-empty stem and a clean extension.
    let name = token.rsplit('/').next().unwrap_or(token);
    let Some((stem, ext)) = name.rsplit_once('.') else {
        return false;
    };
    if stem.is_empty()
        || ext.is_empty()
        || !ext.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        return false;
    }
    // A known extension, or a path separator — a path shape speaks for itself.
    is_known_file_ext(ext) || token.contains('/')
}

/// Scan a string for `` `path.ext` `` tokens (backtick-wrapped file refs),
/// keeping the ones [`is_file_reference`] accepts.
fn backtick_file_refs(text: &str) -> Vec<String> {
    let mut refs = Vec::new();
    let mut rest = text;
    while let Some(open) = rest.find('`') {
        let after = &rest[open + 1..];
        if let Some(close) = after.find('`') {
            let token = &after[..close];
            if is_file_reference(token) {
                refs.push(token.to_string());
            }
            rest = &after[close + 1..];
        } else {
            break;
        }
    }
    refs
}

/// Whether a `## Files` ref resolves on disk: under the spec dir, the PROJECT
/// ROOT, or any subproject root. The subproject roots quiet false "missing"
/// WARNs for existing-but-extended files declared with a subproject-relative or
/// abbreviated path (e.g. a git-submodule backend).
///
/// `root` is the caller's project root, never the process working directory —
/// off-root (a worktree, a nested cwd) the two differ and the bare relative
/// path would be tested against the wrong tree.
fn ref_resolves(r: &str, spec_dir: &Path, root: &Path, project_roots: &[PathBuf]) -> bool {
    let r = normalise_ref(r);
    fs::exists(spec_dir.join(r))
        || fs::exists(root.join(r))
        || project_roots.iter().any(|sub| fs::exists(sub.join(r)))
}

/// A `## Files` reference as a bare relative path: a leading `./` (however many)
/// is dropped. `` `./src/x.rs` `` and `` `src/x.rs` `` name the SAME file, and
/// both readers of a reference — [`ref_resolves`] and the suffix match in
/// [`refs_found_elsewhere`] — go through here so they cannot disagree about it.
/// The suffix match used to compare `found.ends_with("/./src/x.rs")`, which
/// nothing on disk ends with, so a file that existed was reported as missing
/// with the create-marker hint. Pure, total.
fn normalise_ref(r: &str) -> &str {
    let mut r = r.trim();
    while let Some(rest) = r.strip_prefix("./") {
        r = rest;
    }
    r
}

/// Diretórios que a varredura de [`refs_found_elsewhere`] nunca abre: nada que
/// um `## Files` referencie mora ali, e são justamente os que fazem uma
/// varredura honesta custar segundos.
const UNWALKED_DIRS: &[&str] = &[
    ".git", "node_modules", "target", "dist", "build", ".next", ".venv", "venv",
    "__pycache__", "obj", "coverage", ".turbo", ".cache",
];

/// Quantos diretórios a varredura abre antes de desistir. Um teto, não uma
/// otimização: este validador é WARN e não pode custar mais que o passo que o
/// chama, então em repositório grande ele responde "não achei" em vez de
/// procurar para sempre — a mesma resposta que dava antes de existir.
const MAX_WALKED_DIRS: usize = 4000;

/// `true` quando uma referência que não resolveu ainda PODE nomear um arquivo:
/// ela carrega extensão ou uma barra.
///
/// O gatilho da varredura vinha do [`backtick_file_refs`], que recolhe o que
/// está entre crases e não só caminhos — a seção `## Arquivos` de uma spec cita
/// `` `## ACCEPTANCE` `` e o título vira uma "referência ausente" que abre até
/// 4000 diretórios atrás de um arquivo com esse nome. Um título de seção e a
/// prosa entre crases não passam por aqui; `apps/rt/src/x.rs`, `Cargo.toml` e
/// `docs/notas` passam.
///
/// Deliberadamente mais frouxo que [`looks_like_file_path`], que exige extensão
/// CONHECIDA: ali a pergunta é "isto é um caminho?", aqui é "vale a pena abrir o
/// disco por isto?", e recusar `scripts/ac/deps` por não ter extensão custaria a
/// resposta certa num caso real. Pura, total.
fn could_name_a_file(r: &str) -> bool {
    let r = r.trim();
    if r.is_empty() {
        return false;
    }
    r.contains('/')
        || r.contains('\\')
        || Path::new(r).extension().is_some_and(|ext| !ext.is_empty())
}

/// Onde, sob `root`, existe um arquivo com o mesmo NOME-BASE de cada referência
/// que não resolveu — a referência como declarada, mapeada para o caminho de
/// repositório onde o arquivo está de fato.
///
/// O defeito que ela conserta: a mensagem de `missing-file` nomeava UMA causa —
/// esqueceu de marcar como novo — e o caso de campo era o outro, o caminho
/// escrito relativo ao subprojeto ou a um pedaço da árvore
/// (`src/commands/x.rs` quando o arquivo é `apps/rt/src/commands/x.rs`).
/// Mandar marcar como novo ali é mandar criar uma segunda cópia.
///
/// Um candidato cujo caminho TERMINA na referência declarada vence um que só
/// compartilha o nome: o primeiro é exatamente "o mesmo arquivo sob outro
/// prefixo", o segundo é um homônimo. Empate fica com o primeiro encontrado.
///
/// A DISTINÇÃO entre os dois viaja com a resposta ([`FoundElsewhere::suffix`]),
/// porque a mensagem que sai delas não pode ser a mesma. Um sufixo verdadeiro é
/// um endereço; um homônimo de nome-base é uma possibilidade — e nomes como
/// `mod.rs`, `index.ts` e `cli.rs` fazem do homônimo o caso comum.
///
/// Determinística: as entradas de cada diretório são ordenadas pelo nome antes
/// de descer, e a varredura é em LARGURA, então a resposta não depende da ordem
/// que o sistema de arquivos devolve — a mensagem sai num JSON comparado byte a
/// byte. Uma passada só para todas as referências, e nada é aberto quando
/// nenhuma delas pode ser um arquivo ([`could_name_a_file`]).
/// Onde um arquivo com o nome-base de uma referência foi encontrado, e SE o
/// achado é o mesmo arquivo sob outro prefixo (`suffix`) ou apenas um homônimo.
struct FoundElsewhere {
    /// O caminho relativo ao repositório onde o arquivo está.
    path: String,
    /// `true` quando o caminho encontrado TERMINA na referência declarada — o
    /// caso "mesmo arquivo, outro prefixo". `false` é homonímia de nome-base.
    suffix: bool,
}

fn refs_found_elsewhere(root: &Path, refs: &[String]) -> BTreeMap<String, FoundElsewhere> {
    // O que o `backtick_file_refs` recolhe não é só caminho: um `## ACCEPTANCE`
    // entre crases na própria seção `## Arquivos` chega aqui como referência que
    // não resolveu, e abrir 4000 diretórios atrás de um TÍTULO de seção é o
    // custo inteiro deste passo pago por nada.
    let refs: Vec<&str> =
        refs.iter().map(String::as_str).filter(|r| could_name_a_file(r)).collect();
    if refs.is_empty() {
        return BTreeMap::new();
    }
    let mut best: BTreeMap<String, (u8, String)> = BTreeMap::new();
    let mut frontier = vec![root.to_path_buf()];
    let mut walked = 0usize;
    while !frontier.is_empty() && walked < MAX_WALKED_DIRS {
        let mut next = Vec::new();
        for dir in frontier {
            walked += 1;
            if walked > MAX_WALKED_DIRS {
                break;
            }
            let Ok(mut entries) = fs::read_dir(&dir) else {
                continue;
            };
            entries.sort_by(|a, b| a.file_name.cmp(&b.file_name));
            for entry in entries {
                if entry.is_dir {
                    if !UNWALKED_DIRS.contains(&entry.file_name.as_str()) {
                        next.push(entry.path);
                    }
                    continue;
                }
                for &r in &refs {
                    if r.rsplit('/').next().unwrap_or(r) != entry.file_name {
                        continue;
                    }
                    let found = ac_negative_check::repo_relative(root, &entry.path);
                    // A mesma normalização que o `ref_resolves` aplica: uma
                    // referência escrita `./src/x.rs` é sufixo de
                    // `apps/rt/src/x.rs` tanto quanto `src/x.rs` é.
                    let bare = normalise_ref(r);
                    let score = u8::from(found.ends_with(&format!("/{bare}")) || found == bare) + 1;
                    if best.get(r).is_none_or(|(previous, _)| *previous < score) {
                        best.insert(r.to_string(), (score, found));
                    }
                }
            }
        }
        frontier = next;
    }
    best.into_iter()
        .map(|(r, (score, path))| (r, FoundElsewhere { path, suffix: score == 2 }))
        .collect()
}

/// `true` when a bare (un-backticked) token names ONE concrete file: it survives
/// THE REFERENCE RULE ([`is_file_reference`]) and ends in a recognised source
/// extension.
///
/// Both halves are load-bearing and neither is redundant. The known-extension
/// requirement is what keeps prose out — "3.5", "e.g." and
/// "https://example.com" all fail, while `src/list.rs` and `Cargo.toml` pass.
/// [`is_file_reference`] is what keeps SETS out: this recogniser used to accept
/// `src/*.rs` and `wave-N-{role}/spec.md`, and its consumer then compared them
/// BYTE-LITERALLY against declared paths — a guaranteed warning on a correct
/// plan. The strict sibling already lived in this file; it simply was not the
/// one wired in.
///
/// Backslashes are normalised first, so a Windows-spelled `src\list.rs` reads
/// as the path it is rather than as prose.
///
/// `pub(crate)` so the wave traceability pass decides "does this criterion's
/// command name a repository path" with the SAME recogniser — one definition of
/// what reads as a path, tuned in one place.
pub(crate) fn looks_like_file_path(token: &str) -> bool {
    let token = token.trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '.' && c != '/'
        && c != '\\' && c != '-' && c != '_');
    let token = token.replace('\\', "/");
    if !is_file_reference(&token) {
        return false;
    }
    let ext = token.rsplit('.').next().unwrap_or("");
    is_known_file_ext(ext)
}

/// The prose-only violations in the PRD `## Context` section, as human-readable
/// fragments. Empty when the section is absent or clean.
///
/// The shipped spec law (`plugin/refs/feature/spec-language.md`, "Contexto
/// rules") makes the PRD layer prose-only: `## Context` briefs a human
/// rediscovering the work next week, so file paths, line numbers, identifiers
/// and bullet lists belong to `## Root cause` / `## Files` / `## Tasks` — and a
/// VERIFIED FINDING to `## Evidence`, the section the conversation channel
/// (`spec-draft --material`) materialises. The rule is unchanged; what changed
/// is that the message now names where a finding actually goes, because a rule
/// that rejects without naming the destination is how the material ends up
/// nowhere. The law shipped but nothing enforced it, and the drafter itself
/// violated it — it spliced the scan digest's anchors into Context as a bullet
/// list of paths. Checked here so the violation is caught wherever it comes from.
fn context_prose_violations(content: &str) -> Vec<String> {
    let Some(block) = crate::commands::spec::spec_sections::section_block(content, "context")
    else {
        return Vec::new();
    };
    let mut violations = Vec::new();
    // Skip the heading line itself; a `##` heading is not section body.
    for line in block.lines().skip(1) {
        let trimmed = line.trim_start();
        if trimmed.starts_with("- ") || trimmed.starts_with("* ") || trimmed.starts_with("+ ") {
            violations.push(format!("bullet list (`{}`)", truncate_for_message(trimmed)));
        }
        for token in trimmed.split_whitespace() {
            if looks_like_file_path(token) {
                violations.push(format!("file path (`{}`)", truncate_for_message(token)));
            }
        }
    }
    violations
}

/// Clip a quoted fragment so one long line cannot flood the issue message.
fn truncate_for_message(s: &str) -> String {
    const MAX: usize = 48;
    if s.chars().count() <= MAX {
        return s.to_string();
    }
    let head: String = s.chars().take(MAX).collect();
    format!("{head}…")
}

/// Whether an AC `command` is a TAUTOLOGY — it exits 0 whether or not the
/// feature was actually built, so it verifies nothing. These are the rubber
/// stamps this linter catches: a bare `cargo build`/`cargo check`, a `cargo test` with no
/// test-name filter (it just re-runs the pre-existing suite), `npm test`/
/// `npm run build`, or a source `grep`/`rg` (asserts textual presence OR
/// absence — neither is runtime behaviour).
///
/// A COMPOUND command is judged BY ITS PARTS: the whole is weak only when EVERY
/// part is weak, so `cargo test -p x foo && ./verify.sh` stays strong while
/// `rg -q 'literal' src/lib.rs && echo OK` — a presence search wearing a
/// compound coat — is weak, because a bare `echo`/`true`/`:` asserts nothing.
/// The blanket "the author combined steps on purpose" exemption this replaces
/// was walked straight through by that shape in the field.
///
/// A leading `rtk ` wrapper is transparent, and any positional test-name /
/// assertion target makes a part strong. Pure, total, never panics.
fn is_weak_ac_command(command: &str) -> bool {
    let cmd = command.trim();
    if cmd.is_empty() {
        return false;
    }
    let parts = split_command_parts(cmd);
    // A part that asserts nothing cannot rescue its neighbours, and a single
    // strong part is enough to make the whole a real verification.
    parts.iter().all(|part| is_weak_command_part(part))
}

/// Split a shell command on its top-level operators (`&&`, `||`, `;`, `|`),
/// ignoring any that sit inside quotes so a pattern like `rg 'a|b' src` stays
/// ONE part. An unterminated quote yields the whole command as a single part —
/// when the split cannot be trusted, the judgement falls back to the whole
/// string rather than to a fabricated part list. Pure, total.
fn split_command_parts(cmd: &str) -> Vec<&str> {
    let bytes = cmd.as_bytes();
    let mut parts = Vec::new();
    let mut start = 0;
    let mut i = 0;
    let mut quote: Option<u8> = None;
    while i < bytes.len() {
        let b = bytes[i];
        match quote {
            Some(q) => {
                if b == q {
                    quote = None;
                }
                i += 1;
            }
            None => {
                if b == b'\'' || b == b'"' || b == b'`' {
                    quote = Some(b);
                    i += 1;
                    continue;
                }
                let width = match b {
                    b'&' if bytes.get(i + 1) == Some(&b'&') => 2,
                    b'|' if bytes.get(i + 1) == Some(&b'|') => 2,
                    b';' | b'|' => 1,
                    _ => 0,
                };
                if width == 0 {
                    i += 1;
                    continue;
                }
                parts.push(cmd[start..i].trim());
                i += width;
                start = i;
            }
        }
    }
    if quote.is_some() {
        return vec![cmd];
    }
    parts.push(cmd[start..].trim());
    parts.retain(|p| !p.is_empty());
    if parts.is_empty() {
        return vec![cmd];
    }
    parts
}

/// Whether ONE part of a command is weak — a tautology, or a step that asserts
/// nothing at all. The per-part half of [`is_weak_ac_command`]: it never looks
/// at operators, so the two concerns stay separable and testable apart.
///
/// A search (`grep`/`rg`/…) is ALWAYS weak here, in either direction. An
/// absence search used to be exempt as a "genuine post-condition", but
/// `--files-without-match` exits 0 precisely when the pattern matches nothing
/// and `-v` exits 0 when any single line fails to match — whether such a search
/// CAN fail is a fact about the repository, which only the negative test
/// (`ac-negative-check`) can establish.
///
/// This function is the SHAPE half of that judgement and stays deliberately
/// blind to the repository. The MEASUREMENT half now sits beside it: [`validate`]
/// reads the proof ledger the negative test writes into the same directory, and
/// a criterion the measurement already settled is never reported weak on shape
/// alone. Pure, total.
fn is_weak_command_part(part: &str) -> bool {
    // `rtk` is a transparent RTK passthrough — the weakness (if any) lives in
    // the wrapped command.
    let cmd = part.trim();
    let cmd = cmd.strip_prefix("rtk ").map_or(cmd, str::trim_start);
    let tokens: Vec<&str> = cmd.split_whitespace().collect();
    let Some(&first) = tokens.first() else {
        return false;
    };
    match first {
        // Asserts NOTHING: a step whose only job is to exit 0. Chained after a
        // real command it also swallows nothing — it just re-states success.
        "echo" | "true" | ":" => true,
        // A source search asserts textual presence or absence, not behaviour.
        "grep" | "egrep" | "fgrep" | "rg" | "ag" | "ack" => true,
        // A bare build word / whole-project type-check with no target.
        "build" | "tsc" | "make" if tokens.len() == 1 => true,
        "cargo" => match tokens.get(1).copied() {
            Some("build" | "b" | "check" | "c") => true,
            // O MESMO leitor que [`test_runner_has_selector`] usa para o cargo:
            // "tem filtro" e "estreita por nome" são a mesma pergunta, e duas
            // varreduras é como as duas portas passariam a responder diferente.
            Some("test" | "t" | "nextest") => !cargo_narrows_by_name(&tokens),
            _ => false,
        },
        "npm" | "pnpm" | "yarn" | "bun" => match tokens.get(1).copied() {
            Some("test" | "t" | "build") => true,
            Some("run") => matches!(
                tokens.get(2).copied(),
                Some("build" | "test" | "lint" | "typecheck" | "check")
            ),
            _ => false,
        },
        _ => false,
    }
}

/// Whether an AC `command` COUNTS PER FILE, and therefore prints `file:count`
/// rather than a bare number: `grep -c` / `git grep -c` and their combined
/// short flags (`-ci`, `-cn`), plus the long `--count`.
///
/// Keyed off the count FLAG, never off any output, so it stays language- and
/// platform-agnostic like every other check here. Pure, total, never panics.
///
/// Used by the V6c lint: an `Expect:` regex anchored at `^` against one of these
/// can never match, so the criterion is red in both directions and says nothing
/// about whether the work was done.
///
/// `pub(crate)` because the AMENDMENT door needs the same predicate: once such a
/// criterion has shipped, the lint can no longer help it, and `ac-amend` has to
/// recognise the impossible pair to let the repair through
/// ([`crate::commands::spec::ac_amend`]). One rule, two readers — a second copy
/// is how the drafting lint and the amendment door would drift into disagreeing
/// about the same criterion.
pub(crate) fn counts_per_file(command: &str) -> bool {
    if !command.contains("grep") {
        return false;
    }
    command.split_whitespace().any(|tok| {
        tok == "--count"
            || (tok.starts_with('-') && !tok.starts_with("--") && tok.contains('c'))
    })
}

/// Whether an AC `command` invokes a TEST RUNNER — one that runs a suite and
/// reports the suite's own verdict: `cargo test|t|nextest`, `dotnet test`,
/// `go test`, `npm|pnpm|yarn|bun test|t` / `… run test`, `pytest`/`py.test`,
/// `vitest`, `jest`. A leading `rtk `, `npx ` or `bunx ` wrapper is transparent;
/// a COMPOUND command (`&&`/`||`/`;`/`|`) is exempt (the author already chained
/// an assertion). Language-agnostic — it keys off the runner verb, never the
/// runner's output. Pure, total, never panics.
///
/// The family is named by ONE fact they share: every one of them exits 0 when
/// its FILTER selects nothing. So a runner AC can be green for a suite that ran
/// nothing of this feature's, and red because the `Expect:` regex missed rather
/// than because the behaviour is absent.
///
/// `pub(crate)` because the readers that ask this about the same criterion must
/// never disagree: the V6b lint (a runner AC with no `Expect:` evidence regex)
/// and the V6d lint (a runner AC with no `Control:`), both WARN-level at
/// drafting. A second copy is how two warnings would come to name different
/// criteria — the same reason [`counts_per_file`] is shared with the amendment
/// door.
pub(crate) fn is_test_runner_command(command: &str) -> bool {
    let cmd = command.trim();
    if cmd.is_empty()
        || cmd.contains("&&")
        || cmd.contains("||")
        || cmd.contains(';')
        || cmd.contains('|')
    {
        return false;
    }
    let tokens = runner_tokens(cmd);
    let Some(&first) = tokens.first() else {
        return false;
    };
    match first {
        // Runners invoked as a program of their own.
        "pytest" | "py.test" | "vitest" | "jest" => true,
        "cargo" => matches!(tokens.get(1).copied(), Some("test" | "t" | "nextest")),
        "dotnet" | "go" => tokens.get(1).copied() == Some("test"),
        "npm" | "pnpm" | "yarn" | "bun" => match tokens.get(1).copied() {
            Some("test" | "t") => true,
            Some("run") => tokens.get(2).copied() == Some("test"),
            _ => false,
        },
        _ => false,
    }
}

/// Os tokens de um comando, sem os invólucros transparentes — `rtk npx vitest …`
/// chega aqui como `vitest …`.
///
/// UMA normalização, lida por [`is_test_runner_command`] e por
/// [`test_runner_has_selector`]: a segunda pergunta é sobre o MESMO comando que
/// a primeira reconheceu, e dois desembrulhadores é como elas passariam a falar
/// de comandos diferentes.
fn runner_tokens(command: &str) -> Vec<&str> {
    let mut cmd = command.trim();
    while let Some(rest) = ["rtk ", "npx ", "bunx "]
        .into_iter()
        .find_map(|w| cmd.strip_prefix(w))
        .map(str::trim_start)
    {
        cmd = rest;
    }
    cmd.split_whitespace().collect()
}

// ---------------------------------------------------------------------------
// Flags de ESCOPO que consomem o token seguinte — uma lista POR FAMÍLIA
// ---------------------------------------------------------------------------
//
// Uma lista só, compartilhada por todas as famílias, mede a família errada em
// quase todas elas: `-v`, `-w`, `-f`, `-c` e `-j` tomam valor no `dotnet` e são
// BOOLEANAS no go, no pytest, no vitest e no jest. Com a lista única,
// `go test ./... -v -run TestNewCase` pulava dois tokens no `-v`, engolia o
// `-run` e lia `TestNewCase` como posicional — o detector devolvia "sem
// seletor" e o aviso `test-ac-no-control` não disparava para a população
// inteira que ele existe para pegar.
//
// A direção do erro continua sendo a segura em cada lista: uma flag de valor
// que a lista da família não conhece faz o valor dela ser lido como posicional,
// e isso faz o aviso DISPARAR — nunca o silencia.

/// cargo: `-p`, `--features`, `--target` … escolhem ONDE rodar.
const CARGO_SCOPE_VALUE_FLAGS: &[&str] = &[
    "-p", "--package", "--test", "--bench", "--example", "--bin", "--features", "-F",
    "--manifest-path", "-j", "--jobs", "--target", "--profile", "--target-dir", "--color",
];

/// `go test`: `-v`, `-race`, `-cover`, `-short` e `-c` são BOOLEANAS aqui.
const GO_SCOPE_VALUE_FLAGS: &[&str] = &[
    "-timeout", "-count", "-parallel", "-cpu", "-tags", "-covermode", "-coverprofile",
    "-coverpkg", "-outputdir", "-o", "-exec", "-gcflags", "-ldflags", "-cpuprofile",
    "-memprofile", "-blockprofile", "-trace",
];

/// `dotnet test`: a única família em que `-v`, `-f`, `-c`, `-l`, `-r`, `-s` e
/// `-o` realmente TOMAM valor — que é de onde a lista única veio.
const DOTNET_SCOPE_VALUE_FLAGS: &[&str] = &[
    "-v", "--verbosity", "-f", "--framework", "-c", "--configuration", "-l", "--logger",
    "-r", "--results-directory", "-s", "--settings", "-o", "--output", "-a",
    "--test-adapter-path", "--runtime", "--collect", "--diag", "--blame-hang-timeout",
];

/// pytest: `-v`, `-q`, `-x`, `-s` e `-l` são BOOLEANAS aqui.
const PYTEST_SCOPE_VALUE_FLAGS: &[&str] = &[
    "-p", "-c", "-o", "-W", "-n", "--rootdir", "--junitxml", "--maxfail", "--tb", "--color",
    "--capture", "--ignore", "--deselect", "--basetemp", "--dist", "--override-ini",
    "--log-level", "--import-mode",
];

/// vitest / jest: `-w`, `-u`, `-v`, `-t` (que é flag de NOME, tratada à parte)
/// não são flags de escopo com valor separado. `-w` é `--watch` no vitest; no
/// jest ele é `--maxWorkers`, e lê-lo como booleano faz `jest -w 4` ver `4`
/// como posicional — o lado seguro.
const JS_SCOPE_VALUE_FLAGS: &[&str] = &[
    "-c", "--config", "--reporter", "--reporters", "--rootDir", "--maxWorkers", "--workers",
    "--environment", "--testEnvironment", "--outputFile", "--shard", "--project",
    "--testTimeout", "--dir", "--mode", "--coverage.reporter",
];

/// `true` quando o comando de um executor de teste NARROWS BY NAME — carrega um
/// filtro ou seletor que pode não casar nada.
///
/// ## O que conta como seletor, e por quê
///
/// A razão declarada do aviso `test-ac-no-control` é UMA: um executor de teste
/// sai com código 0 quando o FILTRO dele não seleciona nada, então o vermelho
/// do critério pode ser a seleção vazia em vez do comportamento ausente. Um
/// comando que roda a SUÍTE INTEIRA (`cargo test -p mustard-rt --lib`,
/// `pytest`, `go test ./...`) não tem filtro que possa selecionar nada — não
/// existe o modo de falha que o aviso endereça, e avisar ali é ruído puro.
///
/// Então o gatilho é NOMEAÇÃO, não escopo:
///
/// * **cargo** — um posicional depois de `test`; `-p`, `--lib` e `--test <alvo>`
///   escolhem ONDE rodar, e um alvo que não existe faz o cargo sair diferente
///   de zero, alto.
/// * **go** — `-run` / `-bench`; `./...` e um caminho de pacote são escopo.
/// * **dotnet** — `--filter`.
/// * **pytest** — `-k` / `-m`, ou um posicional (arquivo, diretório ou node id:
///   ali o caminho É a seleção).
/// * **vitest / jest** — `-t` / `--testNamePattern` / `--testPathPattern`, ou um
///   posicional (o padrão de arquivo).
/// * **npm/pnpm/yarn/bun** — um posicional depois da palavra do script.
///
/// Pura, total. Falso para tudo que [`is_test_runner_command`] não reconhece.
pub(crate) fn test_runner_has_selector(command: &str) -> bool {
    if !is_test_runner_command(command) {
        return false;
    }
    let tokens = runner_tokens(command);
    let Some(&first) = tokens.first() else {
        return false;
    };
    match first {
        // O cargo passa pela MESMA varredura que todas as outras famílias — ver
        // [`cargo_narrows_by_name`] e [`narrows_by_name`]. Ele tinha um scanner
        // próprio (`cargo_test_has_filter`), idêntico a esta chamada e com 13
        // das 15 flags dele repetidas literalmente na lista compartilhada: uma
        // flag acrescentada a uma das duas e não à outra fazia o cargo e o resto
        // discordarem sobre o que é um seletor — que é exatamente a deriva de
        // que a lista única por família era um caso.
        "cargo" => cargo_narrows_by_name(&tokens),
        "go" => {
            narrows_by_name(&tokens, 2, &["-run", "-bench"], false, GO_SCOPE_VALUE_FLAGS, &[])
        }
        "dotnet" => {
            narrows_by_name(&tokens, 2, &["--filter"], false, DOTNET_SCOPE_VALUE_FLAGS, &[])
        }
        "pytest" | "py.test" => {
            narrows_by_name(&tokens, 1, &["-k", "-m"], true, PYTEST_SCOPE_VALUE_FLAGS, &[])
        }
        // `vitest run` é a invocação da suíte inteira fora do modo watch — o
        // `run` é subcomando, não padrão de arquivo.
        "vitest" | "jest" => narrows_by_name(
            &tokens,
            if tokens.get(1).copied() == Some("run") { 2 } else { 1 },
            &["-t", "--testNamePattern", "--testPathPattern", "--testPathPatterns"],
            true,
            JS_SCOPE_VALUE_FLAGS,
            &[],
        ),
        // `npm test x` / `npm run test x`: o posicional vem depois da palavra do
        // script, que está no índice 1 ou 2. Tudo depois dela é do script, não
        // do gerenciador, então nenhuma flag de escopo é conhecida aqui.
        "npm" | "pnpm" | "yarn" | "bun" => {
            let start = if tokens.get(1).copied() == Some("run") { 3 } else { 2 };
            narrows_by_name(&tokens, start, &[], true, &[], &[])
        }
        _ => false,
    }
}

/// A varredura do CARGO, com o subcomando pulado — uma função só, lida pelas
/// duas portas que perguntam "este comando estreita por nome?"
/// ([`test_runner_has_selector`] e [`is_weak_command_part`]).
///
/// `cargo nextest run` é a invocação da SUÍTE INTEIRA: `nextest` é o subcomando
/// do cargo e `run` é o subcomando DELE, não um nome de teste. Lendo a partir do
/// índice 2 fixo, o `run` caía como posicional e a suíte inteira ganhava um
/// `test-ac-no-control` que não tinha defeito atrás — o ruído que o gatilho por
/// NOMEAÇÃO existe para não causar. É a mesma isenção de subcomando que
/// `vitest run` / `jest run` já tinham.
///
/// A isenção alcança SÓ a palavra `run`: `cargo nextest run my_case` e
/// `cargo nextest run -E 'test(my_case)'` continuam estreitando por nome, e
/// `cargo test -p x my_case` nunca passou por aqui. Pura, total.
fn cargo_narrows_by_name(tokens: &[&str]) -> bool {
    let start =
        if tokens.get(1).copied() == Some("nextest") && tokens.get(2).copied() == Some("run") {
            3
        } else {
            2
        };
    narrows_by_name(tokens, start, &[], true, CARGO_SCOPE_VALUE_FLAGS, CARGO_HARNESS_VALUE_FLAGS)
}

/// Flags do HARNESS de teste do Rust (libtest) que TOMAM valor — o que o cargo
/// encaminha depois do `--`. `--test-threads 1` era lido como o nome `1`, e
/// `cargo test -p x --lib -- --test-threads 1` — a suíte inteira — escapava do
/// lint de comando fraco como se fosse filtrado. `--skip` também toma valor, e
/// EXCLUI em vez de selecionar: o que sobra ainda é a suíte, então não é
/// seletor. `--exact`, `--nocapture`, `--ignored`, `--include-ignored`,
/// `--report-time` e `--ensure-time` são booleanas.
const CARGO_HARNESS_VALUE_FLAGS: &[&str] = &[
    "--test-threads", "--skip", "--logfile", "--format", "--color", "-Z", "--shuffle-seed",
];

/// `true` para um token DEPOIS do `--` que é `chave=valor` e não nome de teste:
/// `dotnet test -- RunConfiguration.MaxCpuCount=1` passa um runsettings, e
/// lê-lo como seleção fazia `test-ac-no-control` avisar num comando que não
/// tem filtro nenhum para vir vazio.
///
/// A chave tem a forma de chave — letras, dígitos, `.`, `_`, `-` — e nada
/// mais: um node id do pytest (`tests/x.py::test_y[a=1]`) carrega `/`, `::` e
/// `[` antes do `=`, e continua sendo posicional.
fn is_key_value_token(token: &str) -> bool {
    token.split_once('=').is_some_and(|(key, _)| {
        !key.is_empty()
            && key
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
    })
}

/// A varredura compartilhada de [`test_runner_has_selector`] e de
/// [`is_weak_command_part`]: a partir de `start`, procura uma das `name_flags`
/// COM valor — e, quando `positional_narrows`, também um posicional.
///
/// `--` encaminha o resto ao executor, onde um não-flag é nome de teste — em
/// TODA família, porque é o executor quem o lê, não o gerenciador. Mas a
/// varredura NÃO para de ler flags ali: o mesmo conhecimento de flag-com-valor
/// que vale antes do `--` vale depois (`scope_flags`, e `harness_flags` para o
/// que só existe do lado do executor), e um `chave=valor` solto depois do `--`
/// é valor de configuração, não nome ([`is_key_value_token`]). Ler qualquer
/// não-flag depois do `--` como nome fazia `--test-threads 1` virar o nome `1`
/// e `RunConfiguration.MaxCpuCount=1` virar seleção.
///
/// Um `--flag=valor` é auto-contido; uma flag de escopo conhecida (`scope_flags`,
/// a lista DESTA família) consome o token seguinte; toda outra flag é booleana.
///
/// `scope_flags` é parâmetro e não constante porque a resposta depende da
/// família: `-v` toma valor no `dotnet` e é booleana em todas as outras. Ver as
/// listas acima.
///
/// Um token com `=` só é lido como `--flag=valor` quando começa com `-`. Um
/// POSICIONAL pode carregar `=` — `pytest 'tests/x.py::test_y[a=1]'` é um node
/// id, não uma flag — e lê-lo como flag pulava justamente o seletor.
fn narrows_by_name(
    tokens: &[&str],
    start: usize,
    name_flags: &[&str],
    positional_narrows: bool,
    scope_flags: &[&str],
    harness_flags: &[&str],
) -> bool {
    let mut i = start;
    let mut after_dashes = false;
    while i < tokens.len() {
        let t = tokens[i];
        if !after_dashes && t == "--" {
            after_dashes = true;
            i += 1;
            continue;
        }
        if t.starts_with('-') {
            if let Some((flag, value)) = t.split_once('=') {
                if name_flags.contains(&flag) && !value.is_empty() {
                    return true;
                }
                i += 1;
                continue;
            }
            if name_flags.contains(&t) {
                return tokens.get(i + 1).is_some_and(|v| !v.starts_with('-'));
            }
            if scope_flags.contains(&t) || (after_dashes && harness_flags.contains(&t)) {
                i += 2;
                continue;
            }
            i += 1; // booleana
            continue;
        }
        if after_dashes && is_key_value_token(t) {
            i += 1; // valor de configuração do executor, não nome
            continue;
        }
        if after_dashes || positional_narrows {
            return true;
        }
        // Um posicional que NÃO seleciona por nome (o pacote do `go`, o
        // `.csproj` do `dotnet`) é escopo: segue a varredura, senão uma flag de
        // nome escrita DEPOIS dele nunca seria vista.
        i += 1;
    }
    false
}

/// The ids the negative test has already MEASURED as able to fail — the
/// criteria this linter has nothing left to guess about.
///
/// Read through the producer's own door: [`ac_negative_check::load_ledger`] is
/// the single parser of `ac-proof.json`, [`ac_negative_check::recorded_proof`]
/// the single lookup rule (id AND command AND expect must all still match), and
/// [`ac_negative_check::AcProof::evidenced`] the single reading of what counts
/// as evidence. Restating any of them here is how the gate that produces the
/// proof and the linter that would override it drift apart.
///
/// Fail-open in the direction that keeps the WARN: an absent, unreadable or
/// stale ledger yields an empty set, so every criterion is judged on shape
/// exactly as before. A hand-edited command no longer matches its record and is
/// therefore judged on shape too — which is the correct answer, not a
/// degradation.
fn proven_criteria(spec_dir: &Path, items: &[qa_run::AcItem]) -> BTreeSet<String> {
    let ledger_path = spec_dir.join(ac_negative_check::AC_PROOF_JSON);
    let Some(ledger) = ac_negative_check::load_ledger(&ledger_path) else {
        return BTreeSet::new();
    };
    items
        .iter()
        .filter(|item| {
            ac_negative_check::recorded_proof(
                &ledger,
                &item.id,
                &item.command,
                item.expect.as_deref(),
            )
            .is_some_and(ac_negative_check::AcProof::evidenced)
        })
        .map(|item| item.id.clone())
        .collect()
}

/// Whether `line` names a wave by its NUMBER — `onda 1`, `wave 2`, `ondas 3 e 4`.
///
/// Exige ao menos um espaço entre a palavra e o dígito, e que a palavra comece
/// palavra: `wave-1-rt` e `wave-plan.md` são caminhos, não prescrição, e a
/// coluna `| Wave |` de uma tabela é cabeçalho. Sem regex — o crate não carrega
/// uma para uso genérico (só o matcher `Expect:` do `qa-run` a usa).
fn names_a_wave_by_number(line: &str) -> bool {
    let lower = line.to_lowercase();
    for word in ["ondas", "onda", "waves", "wave"] {
        let mut from = 0usize;
        while let Some(rel) = lower[from..].find(word) {
            let at = from + rel;
            from = at + word.len();
            // Fronteira à esquerda: a palavra não pode ser o fim de outra maior.
            if lower[..at]
                .chars()
                .next_back()
                .is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '-')
            {
                continue;
            }
            let after = &lower[from..];
            let trimmed = after.trim_start_matches([' ', '\t']);
            if trimmed.len() == after.len() {
                continue;
            }
            if trimmed.starts_with(|c: char| c.is_ascii_digit()) {
                return true;
            }
        }
    }
    false
}

/// Os títulos das seções que atribuem trabalho a uma onda PELO NÚMERO e cujo
/// título não resolve para chave canônica nenhuma — a prescrição nominal
/// inalcançável, em ordem do documento e sem repetir.
///
/// O lint NÃO tenta adivinhar se alguma tarefa cobre a prescrição: casar prosa
/// com tarefa é julgamento e daria um aviso ruidoso. Ele checa o que é
/// determinístico e foi o defeito medido em campo — o texto estava sob um título
/// FORA do vocabulário, `## Decisão em aberto`, que leitor nenhum deste
/// repositório reconhece.
///
/// O corte é o vocabulário inteiro ([`spec_sections::canonical_key`]), não as
/// três seções de material: medido nas 103 specs do acervo, olhar só o material
/// acusava 24 seções — `## Arquivos`, `## Contexto`, `## Preocupações`,
/// `## Critérios de Aceitação` —, todas canônicas e todas menções de referência
/// ("a lição da onda 1 chega à onda 3"), nunca prescrição perdida. E o passo 4 do
/// `full-plan` despeja `validation.issues[]` no `## Concerns` de cada spec, então
/// esse ruído nasceria em toda spec futura. Uma decisão pediu um lint
/// determinístico E sem ruído; um que erra num quarto do acervo é ruído.
///
/// Blocos cercados por ``` são pulados (um exemplo de plano não é prescrição), e
/// só um H2 delimita seção: o `### {Role} Agent` é um bloco DENTRO dela.
fn unreachable_wave_prescriptions(content: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut section: Option<String> = None;
    let mut canonical = false;
    let mut fenced = false;
    for line in content.split('\n') {
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
            continue;
        }
        if fenced {
            continue;
        }
        if line.starts_with("##") && !line.starts_with("###") {
            let title = line.trim_start_matches('#').trim();
            if title.is_empty() {
                continue;
            }
            canonical = spec_sections::canonical_key(line).is_some();
            section = Some(format!("## {title}"));
            continue;
        }
        if canonical {
            continue;
        }
        // Antes do primeiro H2 há só o título e o frontmatter da spec.
        let Some(name) = section.as_ref() else {
            continue;
        };
        if names_a_wave_by_number(line) && !out.contains(name) {
            out.push(name.clone());
        }
    }
    out
}

/// Run the validation against an explicit project `root`. Returns the issues
/// list.
///
/// `root` is UNCONDITIONAL on purpose — an `Option` with an internal
/// `current_dir()` fallback would keep the hidden dependency and make the
/// off-root defect conditional on the caller remembering to pass it. Callers
/// hand over `crate::shared::context::project_dir()` (or, in tests, the root
/// they built).
///
/// `pub` so `plan-materialize` composes the same checks in-process (single
/// validator source — no subprocess, no drift) and the acceptance-criteria
/// tests can assert a verdict without shelling out, while the CLI entry [`run`]
/// keeps the stdout/exit contract.
pub fn validate(root: &Path, abs_path: &Path, content: &str) -> Vec<Value> {
    let lines: Vec<&str> = content.split('\n').collect();
    let mut issues: Vec<Value> = Vec::new();

    let file_lines = files_section_lines(&lines);
    let files_text = file_lines.join("\n");

    // Validation 1: layer coverage.
    for layer in ["Backend", "Frontend", "Database", "Mobile"] {
        let header = format!("### {layer} Agent");
        if !content.contains(&header) {
            continue;
        }
        let exts = layer_extensions(layer);
        let has_match = exts.iter().any(|ext| files_text.contains(ext));
        if !has_match {
            issues.push(json!({
                "severity": "WARN",
                "type": "layer-gap",
                "message": format!("Spec declares {layer} Agent but Files has no {layer} extensions"),
            }));
        }
    }

    // Validation 2: file refs resolvable.
    let spec_dir = abs_path.parent().unwrap_or_else(|| Path::new("."));
    // Subproject roots from the scan model: resolve existing-but-extended files
    // declared with a subproject-relative / abbreviated path so they are not
    // reported as false "missing" WARNs. An absent model yields no extra roots,
    // so resolution matches the historical two-path behaviour when no model is
    // present.
    let model = root.join(".claude").join("grain.model.json");
    let project_roots: Vec<PathBuf> = mustard_core::read_projects(&model)
        .into_iter()
        .map(|p| root.join(p.dir))
        .collect();
    let missing: Vec<String> = backtick_file_refs(&files_text)
        .into_iter()
        .filter(|r| {
            let line_with_ref = file_lines
                .iter()
                .find(|l| l.contains(&format!("`{r}`")))
                .map_or("", String::as_str);
            // Localized marker recognition: the drafter writes the create marker
            // in the spec's narrative locale (`(novo)`/`(criar)` in pt-BR), so the
            // check goes through the core i18n catalogue — the single origin of
            // the marker synonyms — instead of the historical EN-only literal
            // (which flagged every pt-BR net-new file as `missing-file`).
            let is_create = i18n::line_has_file_marker(line_with_ref, i18n::FileMarker::Create);
            !is_create && !ref_resolves(r, spec_dir, root, &project_roots)
        })
        .collect();
    // UMA varredura para todas as referências que não resolveram: a mensagem
    // antiga nomeava uma causa só — esqueceu de marcar como novo — e o caso de
    // campo era o outro, o arquivo existindo sob outro prefixo porque o caminho
    // foi escrito relativo ao subprojeto. Perguntar ao disco custa uma passada e
    // troca um palpite errado por um endereço.
    let elsewhere = refs_found_elsewhere(root, &missing);
    for r in missing {
        let accepted = i18n::file_marker_synonyms(i18n::FileMarker::Create).join(" / ");
        let message = match elsewhere.get(&r) {
            // O MESMO arquivo sob outro prefixo: o caminho encontrado termina na
            // referência declarada, então isto é um endereço e não um palpite —
            // e mandar marcar como novo aqui é mandar criar uma segunda cópia.
            Some(found) if found.suffix => format!(
                "File referenced as `{r}` but not found there — the same file exists at `{path}`. \
                 Declare the path relative to the repository root (or to a subproject root), so \
                 every reader resolves it the same way.",
                path = found.path,
            ),
            // Só o NOME-BASE bate. `mod.rs`, `index.ts` e `cli.rs` tornam isso
            // rotina, então a dica do marcador CONTINUA sendo a resposta
            // principal e o homônimo entra como possibilidade — nunca como
            // fato, que é o que mandaria o autor a um arquivo sem relação.
            Some(found) => format!(
                "File referenced but not found and not marked {accepted} — if it is new, mark it. \
                 A DIFFERENT file with the same name exists at `{path}`; if that is the one you \
                 meant, declare the path relative to the repository root (or to a subproject \
                 root).",
                path = found.path,
            ),
            None => format!("File referenced but not found and not marked {accepted}"),
        };
        issues.push(json!({
            "severity": "WARN",
            "type": "missing-file",
            "file": r,
            "message": message,
        }));
    }

    // Validation 3: task decomposition sane.
    for (agent_name, block) in agent_blocks(content) {
        let tasks = count_tasks(&block);
        if !(2..=10).contains(&tasks) {
            issues.push(json!({
                "severity": "WARN",
                "type": "task-count",
                "message": format!("{agent_name} Agent has {tasks} tasks (expected 2-10)"),
            }));
        }
    }

    // Validation 4: extended-light scope requires the entity to already exist in
    // the repo model (grain.model.json declaration names, read via the scan tool —
    // this crate never parses the model's schema itself).
    if let Some(scope) = extract_kv(content, "scope")
        && scope.eq_ignore_ascii_case("extended-light")
            && let Some(entity) = extract_kv(content, "entity") {
                // The SAME model path as validation 2 — one project root, one
                // model, no second notion of "here".
                let known = mustard_core::read_entity_names(&model);
                if !known.iter().any(|k| k.eq_ignore_ascii_case(entity)) {
                    let message = if known.is_empty() {
                        "Extended Light scope requires the entity in grain.model.json, but no model/declarations were found. Reclassify as Full.".to_string()
                    } else {
                        format!("Extended Light scope requires entity \"{entity}\" in grain.model.json, but not found. Reclassify as Full.")
                    };
                    issues.push(json!({ "severity": "WARN", "type": "scope-mismatch", "message": message }));
                }
            }

    // Validation 5: AC format parseability. The AC section heading resolves
    // (EN `## Acceptance Criteria` / PT `## Critérios de Aceitação`, via the
    // shared i18n-aware extractor) but ZERO items survive the exact parser
    // qa-run executes — qa-run would later degrade to `overall: skip`, so the
    // format problem is surfaced here, at ANALYZE time. An absent section is
    // deliberately NOT flagged: behaviour stays unchanged for specs that carry
    // no ACs at this stage.
    let ac_section = crate::commands::review::qa_run::extract_ac_section(content);
    let ac_items = ac_section
        .as_deref()
        .map(crate::commands::review::qa_run::parse_ac_items)
        .unwrap_or_default();
    if ac_section.is_some() && ac_items.is_empty() {
        issues.push(json!({
            "severity": "WARN",
            "type": "unparseable-ac",
            "message": "Acceptance Criteria section found but no parseable AC items. \
                        Expected format: `**AC-N** — title` followed by a line \
                        `Command: `<runnable command>``.",
        }));
    }

    // Validation 6: AC TAUTOLOGY linter. A criterion "verified" by a bare
    // `cargo build` / `cargo test` (no filter) / `npm test` / source `grep`
    // passes whether or not the feature exists — a rubber stamp. Flag
    // each such WEAK AC by id (WARN — analyze-validation never blocks). Two
    // exemptions: the LAST AC is the trailing build-green SAFETY net (kept on
    // purpose), and an unfilled `<…>` skeleton command is not yet a real
    // command. Reuses the exposed `AcItem` `id` + `command`.
    if ac_items.len() > 1 {
        let last = ac_items.len() - 1;
        // The MEASUREMENT, read before the shape is judged. `is_weak_ac_command`
        // asks how a command is SPELLED; whether it can fail is a fact about
        // this repository, and the negative test already established it for
        // every criterion it proved red. Its ledger lives in this very
        // directory, and until now the linter — whose own docstring says only
        // that pass can settle the question — was not among its readers, so a
        // MEASURED search kept being reported as a rubber stamp.
        let proven = proven_criteria(spec_dir, &ac_items);
        let weak: Vec<String> = ac_items
            .iter()
            .enumerate()
            .filter(|(i, item)| {
                *i != last
                    && !qa_run::is_skeleton(&item.command)
                    && !proven.contains(&item.id)
                    && is_weak_ac_command(&item.command)
            })
            .map(|(_, item)| item.id.clone())
            .collect();
        if !weak.is_empty() {
            issues.push(json!({
                "severity": "WARN",
                "type": "weak-ac",
                "message": format!(
                    "Acceptance criteria verified by a tautological build/test/search command \
                     that passes whether or not the feature exists: {}. Replace with a command \
                     that asserts the new behaviour.",
                    weak.join(", ")
                ),
            }));
        }

        // Validation 6b: a TEST-RUNNER AC that declares no `Expect:` evidence
        // regex. A green `cargo test` / `npm test` proves the suite ran, not
        // that THIS feature's behaviour holds; a declared `Expect: `<regex>``
        // (matched by qa-run against the command's own output) turns the pass
        // into evidence. WARN-level, language-agnostic (keyed off the runner
        // verb, never its output). Excludes the trailing safety AC, `<…>`
        // skeletons, and ids already flagged weak (a tautology's fix is
        // replacement, not an Expect line). Reuses the exposed `AcItem.expect`.
        let no_expect: Vec<String> = ac_items
            .iter()
            .enumerate()
            .filter(|(i, item)| {
                *i != last
                    && item.expect.is_none()
                    && !qa_run::is_skeleton(&item.command)
                    && is_test_runner_command(&item.command)
                    && !weak.contains(&item.id)
            })
            .map(|(_, item)| item.id.clone())
            .collect();
        if !no_expect.is_empty() {
            issues.push(json!({
                "severity": "WARN",
                "type": "test-ac-no-expect",
                "message": format!(
                    "Test-runner acceptance criteria with no declared `Expect:` evidence regex: \
                     {}. A passing suite proves the tests ran, not that this feature's behaviour \
                     holds — add an `Expect: `<regex>`` line so qa-run matches the expected \
                     evidence in the command's output.",
                    no_expect.join(", ")
                ),
            }));
        }

        // Validation 6c: an `Expect:` regex anchored at `^` against a command
        // whose output carries a `path:` prefix on every line. `grep -c` and
        // `git grep -c` count PER FILE and print `file:count`, never a bare
        // count — so `^[0-9]+$` cannot match its own command's output in EITHER
        // direction. Such a criterion is red before the work and red after it,
        // which reads as "still not done" forever and is indistinguishable from
        // one nobody satisfied.
        //
        // Why it must be caught HERE. The negative proof clears it happily — it
        // IS red — and that is exactly the trap: the red is real, only its cause
        // is the regex rather than the missing work. By the time the wave has
        // delivered, `ac-amend` refuses the repair, because the corrected regex
        // now passes and a replacement that passes is refused by design. So the
        // defect has no door left once the spec is frozen, and drafting time is
        // the only cheap moment to name it. Found in the field, 2026-08-14.
        let prefixed: Vec<String> = ac_items
            .iter()
            .enumerate()
            .filter(|(i, item)| {
                *i != last
                    && !qa_run::is_skeleton(&item.command)
                    && counts_per_file(&item.command)
                    && item
                        .expect
                        .as_deref()
                        .is_some_and(|e| e.starts_with('^') && !e.contains(':'))
            })
            .map(|(_, item)| item.id.clone())
            .collect();
        if !prefixed.is_empty() {
            issues.push(json!({
                "severity": "WARN",
                "type": "expect-anchored-against-prefixed-output",
                "message": format!(
                    "Acceptance criteria whose `Expect:` regex is anchored at `^` against a \
                     per-file counting command: {}. `grep -c` / `git grep -c` print \
                     `file:count`, not a bare count, so the regex can never match its own \
                     output — the criterion stays red whether or not the work is done. Anchor \
                     it after the prefix instead (`:[0-9]+$`).",
                    prefixed.join(", ")
                ),
            }));
        }

        // Validation 6d: a TEST-RUNNER AC that declares no `Control:` command —
        // o irmão do V6b, e a metade que o `Expect:` não cobre. Todo executor de
        // teste sai com código 0 quando o FILTRO não casa nada, então o vermelho
        // que o critério ganha na prova negativa pode ser a seleção vazia (um
        // nome de teste com erro de digitação, um caminho que não existe) em vez
        // do comportamento ausente. O `Control:` — um comando que precisa vir
        // VERDE contra a árvore como ela está — é o que separa os dois, e ele é
        // pedido aqui, na redação, onde o conserto custa uma linha. Só aqui: a
        // prova negativa NÃO recusa o critério por isso — ela o prova do jeito
        // de sempre e registra `control: not-declared`. Este aviso é o sinal
        // honesto; a recusa é outra unidade.
        //
        // E o predicado é o do FILTRO, não o do verbo. Uma suíte inteira
        // (`cargo test -p x --lib`, `pytest`, `go test ./...`) não tem filtro que
        // possa selecionar nada, então o modo de falha que o aviso endereça não
        // existe ali — ver [`test_runner_has_selector`].
        // Excludes the trailing safety AC, `<…>` skeletons, and ids already
        // flagged weak (a tautology's fix is replacement, not a Control line).
        //
        // A `Control:` still carrying the scaffold placeholder counts as NOT
        // declared — the same reading `ac_negative_check::take_control` gives
        // it (`NotAttempted`) and `ac-amend` gives it (not a declared
        // control). The lint and the gate promise the same criteria; a lint
        // silent on exactly the control the gate refuses breaks that promise.
        let no_control: Vec<String> = ac_items
            .iter()
            .enumerate()
            .filter(|(i, item)| {
                *i != last
                    && item.control.as_deref().is_none_or(qa_run::is_skeleton)
                    && !qa_run::is_skeleton(&item.command)
                    && test_runner_has_selector(&item.command)
                    && !weak.contains(&item.id)
            })
            .map(|(_, item)| item.id.clone())
            .collect();
        if !no_control.is_empty() {
            issues.push(json!({
                "severity": "WARN",
                "type": "test-ac-no-control",
                "message": format!(
                    "Filtered test-runner acceptance criteria with no declared `Control:` \
                     command: {}. A test runner exits 0 when its filter matches nothing, so a red \
                     here can be an empty selection instead of the missing behaviour — add a \
                     `Control: `<command>`` line that comes back GREEN against the tree as it is \
                     (the unfiltered suite, or the file the new test lands in), so the red is \
                     proven to be about the behaviour. Without one, `ac-negative-check` still \
                     takes the proof and records `control: not-declared`.",
                    no_control.join(", ")
                ),
            }));
        }
    }

    // Validation 7: cross-artifact coherence (AC × task × file). Mirrors V5 — it
    // only runs once an AC section EXISTS (a spec whose ACs are not authored yet
    // is left alone, behaviour unchanged) and only when the plan carries
    // `### {Role} Agent` task blocks (a virgin draft has none). A present-but-
    // unparseable AC section with agent work, or ACs+tasks that point at no
    // files, is a gap. The wave↔AC COVERAGE itself is enforced deterministically
    // in `wave-scaffold` (the `satisfies`/`acceptance` traceability). Reuses the
    // folded agent-block + file-ref lists.
    let agents_with_tasks: Vec<String> = agent_blocks(content)
        .into_iter()
        .filter(|(_, body)| count_tasks(body) > 0)
        .map(|(name, _)| name)
        .collect();
    if ac_section.is_some() && !agents_with_tasks.is_empty() {
        if ac_items.is_empty() {
            issues.push(json!({
                "severity": "WARN",
                "type": "ac-task-gap",
                "message": format!(
                    "{} agent task block(s) but no acceptance criteria to verify them — \
                     every wave must satisfy an AC.",
                    agents_with_tasks.len()
                ),
            }));
        } else if backtick_file_refs(&files_text).is_empty() {
            issues.push(json!({
                "severity": "WARN",
                "type": "ac-file-gap",
                "message": "Acceptance criteria and agent tasks present but the Files section \
                            lists no files to implement them.",
            }));
        }
    }

    // Validation 8: the PRD layer is PROSE-ONLY. A `## Context` carrying file
    // paths or a bullet list is agent input pasted into a human briefing — the
    // shipped spec law forbids it, and until now nothing checked.
    let context_violations = context_prose_violations(content);
    if !context_violations.is_empty() {
        issues.push(json!({
            "severity": "WARN",
            "type": "context-not-prose",
            "message": format!(
                "The Context section is prose-only — it briefs a human rediscovering the work. \
                 A VERIFIED FINDING, with the file and line it was checked at, belongs to the \
                 Evidence section (carried in by `spec-draft --material`); other paths and \
                 lists belong to Root cause / Files / Tasks. Found: {}.",
                context_violations.join(", ")
            ),
        }));
    }

    // Validation 9: PRESCRIÇÃO NOMINAL inalcançável. Prosa que atribui trabalho a
    // uma onda pelo número — "a onda 1 mede os três candidatos e escolhe" — sob um
    // título FORA do vocabulário canônico, que leitor nenhum deste repositório
    // resolve. Só `definitions`, `decisions` e `evidence` são coladas no prompt da
    // onda, e o template despachado não manda ler a spec-mãe em passo nenhum. A
    // prescrição não foi lida e ignorada: não foi entregue. Medido em campo — o
    // trabalho que a spec-mãe reservava ao operador sumiu da decomposição, sob um
    // `## Decisão em aberto` que não é variante de `decisions`.
    let unreachable = unreachable_wave_prescriptions(content);
    if !unreachable.is_empty() {
        issues.push(json!({
            "severity": "WARN",
            "type": "wave-prescription-unreachable",
            "message": format!(
                "Prose assigning work to a wave BY ITS NUMBER sits under a heading OUTSIDE the \
                 canonical vocabulary, which no reader of this repository resolves: {}. Only \
                 `## Definitions`, `## Decisions` and `## Evidence` travel into a dispatched \
                 wave's prompt, and the prompt never tells the agent to read the parent spec — \
                 so the prescription is not ignored, it is undelivered. Move it under \
                 `## Decisions` (with its reason), or fold it into that wave's own `tasks` in \
                 the plan JSON.",
                unreachable.join(", ")
            ),
        }));
    }

    issues
}

/// Dispatch `mustard-rt run analyze-validation`.
pub fn run(spec: Option<&str>) {
    let Some(spec) = spec else {
        crash("No spec path provided. Use --spec <path>");
    };
    let abs_path = std::fs::canonicalize(spec)
        .unwrap_or_else(|_| PathBuf::from(spec));
    if !fs::exists(&abs_path) {
        crash(&format!("Spec file not found: {}", abs_path.display()));
    }
    let content = match fs::read_to_string(&abs_path) {
        Ok(c) => c,
        Err(e) => crash(&format!("{e}")),
    };
    let root = PathBuf::from(crate::shared::context::project_dir());
    let issues = validate(&root, &abs_path, &content);
    let out = json!({ "ok": issues.is_empty(), "issues": issues });
    println!("{out}");
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn clean_spec_has_no_issues() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("spec.md");
        // A coherent spec now also carries ACs that trace to the agent's work
        // (a behaviour AC + a trailing build-green safety AC).
        let body = "# Spec\n## Files\n- `a.rs` (create)\n### Backend Agent\n- [ ] t1\n- [ ] t2\n\n\
                    ## Acceptance Criteria\n\
                    - **AC-1** — when a.rs runs, then it returns ok.\n  Command: `curl -sf localhost`\n\
                    - **AC-2** — build green.\n  Command: `cargo build`\n";
        std::fs::write(&path, body).unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        let issues = validate(dir.path(), &path,&content);
        assert!(issues.is_empty(), "{issues:?}");
    }

    /// A REGRESSÃO que este teste tranca: uma flag BOOLEANA antes do seletor
    /// fazia o detector engolir o próprio seletor.
    ///
    /// `RUNNER_SCOPE_VALUE_FLAGS` era uma lista só para todas as famílias, e
    /// `-v`/`-w`/`-f`/`-c`/`-j` só tomam valor no `dotnet`. Então
    /// `go test ./... -v -run TestNewCase` pulava dois tokens no `-v`, engolia o
    /// `-run` e lia `TestNewCase` como posicional de escopo: sem seletor. O
    /// aviso `test-ac-no-control` não disparava para a população inteira que
    /// ele existe para pegar.
    ///
    /// Um comando por família, com a booleana ANTES do seletor.
    #[test]
    fn a_boolean_flag_before_the_selector_does_not_hide_it() {
        for cmd in [
            "go test ./... -v -run TestNewCase",
            "pytest -v tests/test_x.py::test_y",
            "vitest -w src/x.test.ts",
            "jest -u src/x.test.js",
            "cargo test -p mustard-rt --lib my_new_case",
            // …e o `dotnet`, a ÚNICA família em que `-v` toma valor: ele
            // continua consumindo `normal`, e o `--filter` depois é visto.
            "dotnet test -v normal --filter Name~Novo",
        ] {
            assert!(
                test_runner_has_selector(cmd),
                "o seletor tem de ser visto em `{cmd}`",
            );
        }
        // A outra metade: a suíte INTEIRA continua sem seletor, senão a
        // exigência de `Control:` viraria fricção em todo critério do acervo.
        for cmd in [
            "go test ./...",
            "pytest",
            "vitest run",
            "cargo test -p mustard-rt --lib",
            "dotnet test -v normal",
        ] {
            assert!(
                !test_runner_has_selector(cmd),
                "a suíte inteira não tem filtro que possa vir vazio: `{cmd}`",
            );
        }
        // Um POSICIONAL que carrega `=` é node id, não `--flag=valor`: lê-lo
        // como flag pulava exatamente o seletor.
        assert!(
            test_runner_has_selector("pytest tests/x.py::test_y[a=1]"),
            "um node id parametrizado é seleção por nome",
        );
    }

    /// A REGRESSÃO que este teste tranca: depois do `--`, todo token sem `-`
    /// contava como nome de teste, sem pular valor nenhum. `--test-threads 1`
    /// virava o nome `1`, e a suíte inteira escapava do lint de comando fraco;
    /// `RunConfiguration.MaxCpuCount=1` virava seleção, e o `test-ac-no-control`
    /// avisava num comando que não tem filtro para vir vazio.
    #[test]
    fn harness_values_after_the_dashes_are_not_test_names() {
        for cmd in [
            "cargo test -p mustard-rt --lib -- --test-threads 1",
            "cargo test -p mustard-rt --lib -- --test-threads=1 --nocapture",
            "cargo test -- --skip slow",
            "dotnet test -- RunConfiguration.MaxCpuCount=1",
            "dotnet test x.csproj -- RunConfiguration.MaxCpuCount=1 MSTest.Parallelize.Workers=4",
        ] {
            assert!(
                !test_runner_has_selector(cmd),
                "um valor depois do `--` não é nome de teste: `{cmd}`",
            );
        }
        assert!(
            is_weak_command_part("cargo test -p mustard-rt --lib -- --test-threads 1"),
            "a suíte inteira com `--test-threads 1` continua sendo a tautologia que V6 pega",
        );
        // …e a outra metade, que NÃO pode ser afrouxada junto: um NOME depois
        // do `--`, e um seletor ANTES dele seguido de runsettings.
        for cmd in [
            "cargo test -- --exact my::case",
            "cargo test -- --test-threads 1 my_case",
            "dotnet test --filter Name~Novo -- RunConfiguration.MaxCpuCount=1",
        ] {
            assert!(
                test_runner_has_selector(cmd),
                "o seletor tem de ser visto em `{cmd}`",
            );
        }
    }

    /// O cargo e o resto respondem pela MESMA varredura — a porta que julga o
    /// comando fraco e a que exige o `Control:` não podem discordar sobre o que
    /// é um seletor.
    ///
    /// Eram dois scanners para uma pergunta só (`cargo_test_has_filter` e
    /// `narrows_by_name`), com 13 das 15 flags repetidas literalmente entre eles.
    #[test]
    fn the_weak_reader_and_the_selector_reader_agree_on_cargo() {
        for cmd in [
            "cargo test",
            "cargo test -p mustard-rt --lib",
            "cargo test -p mustard-rt --lib my_case",
            "cargo test --features a,b --target-dir /tmp/x my_case",
            "cargo test -- --exact my::case",
            "cargo nextest run my_case",
            "cargo nextest run",
            "cargo nextest run --workspace",
        ] {
            assert_eq!(
                is_weak_command_part(cmd),
                !test_runner_has_selector(cmd),
                "as duas portas têm de dar a MESMA resposta para `{cmd}`",
            );
        }
    }

    /// A REGRESSÃO que este teste tranca: `cargo nextest run` era lido como
    /// comando FILTRADO, porque o `run` — subcomando do `nextest` — caía como
    /// posicional na varredura que começa no índice 2.
    ///
    /// O custo: um critério de SUÍTE INTEIRA ganhava `test-ac-no-control` no
    /// rascunho sem ter o modo de falha que o aviso endereça — o ruído que o
    /// gatilho por NOMEAÇÃO existe justamente para não causar. `vitest`/`jest`
    /// já tinham a isenção de subcomando; o cargo e o nextest não.
    #[test]
    fn a_runner_subcommand_is_not_read_as_a_filter() {
        // A SUÍTE INTEIRA, nas duas grafias do cargo: nada aqui pode vir vazio.
        for cmd in [
            "cargo nextest run",
            "cargo nextest run --workspace",
            "cargo nextest run -p mustard-rt",
            "cargo test -p mustard-rt",
        ] {
            assert!(
                !test_runner_has_selector(cmd),
                "`{cmd}` roda a suíte inteira — cobrar `Control:` dele recusaria o acervo",
            );
        }
        // …e a outra metade, que NÃO pode ser afrouxada junto: um comando que
        // estreita por nome continua devendo o controle.
        for cmd in [
            "cargo test -p mustard-rt my_case",
            "cargo nextest run -E 'test(my_case)'",
            "cargo nextest run my_case",
        ] {
            assert!(
                test_runner_has_selector(cmd),
                "`{cmd}` seleciona por nome e o filtro dele pode vir vazio",
            );
        }
    }

    /// V6: a criterion the NEGATIVE TEST already measured is not reported as a
    /// tautology on shape alone.
    ///
    /// This linter's own docstring says whether a search can fail is a fact
    /// about the repository which only `ac-negative-check` can establish — and
    /// the linter was not among that pass's readers, though its ledger lives in
    /// the very directory being validated. So a criterion MEASURED able to fail
    /// kept being reported as a rubber stamp.
    ///
    /// Two-sided: the same command, in the same shape, IS flagged when no
    /// measurement stands for it — so the fix cannot pass by silencing V6.
    #[test]
    fn weak_ac_defers_to_the_recorded_proof() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("spec.md");
        let body = "# Spec\n\n## Acceptance Criteria\n\
                    - **AC-1** — the marker is present.\n  Command: `rg -q marker src/lib.rs`\n\
                    - **AC-2** — build green.\n  Command: `cargo build`\n";
        std::fs::write(&path, body).unwrap();

        // No ledger: judged on shape, and a standalone search is weak.
        let issues = validate(dir.path(), &path, body);
        assert!(
            issues.iter().any(|i| i["type"] == "weak-ac"),
            "unmeasured, a search is a rubber stamp: {issues:?}",
        );

        // The negative test then MEASURES AC-1 red — it can fail, which is the
        // fact the shape could never settle.
        std::fs::write(
            dir.path().join(ac_negative_check::AC_PROOF_JSON),
            r#"{"spec":"m","criteria":[{"id":"AC-1","command":"rg -q marker src/lib.rs",
               "expect":null,"verdict":"proven","proof":"red","confirmation":"not-taken",
               "exit":1,"reason":null,"stderr_excerpt":""}],"amendments":[]}"#,
        )
        .unwrap();
        let issues = validate(dir.path(), &path, body);
        assert!(
            !issues.iter().any(|i| i["type"] == "weak-ac"),
            "a MEASURED criterion is not a rubber stamp: {issues:?}",
        );
    }

    /// Prosa que atribui trabalho a uma onda pelo número, sob um título
    /// FORA do vocabulário canônico, vira `wave-prescription-unreachable` — e o
    /// aviso diz para onde mover o texto.
    ///
    /// O título é o do defeito de campo, verbatim, sufixo e tudo: `## Decisão em
    /// aberto — como saber se uma previsão já foi efetivada` não resolve para
    /// chave nenhuma, e foi por aí que a prescrição se perdeu.
    ///
    /// Bilateral: a MESMA frase sob `## Decisions` viaja para o prompt da onda e
    /// não acusa nada, então a asserção não pode passar por o lint disparar em
    /// toda menção a onda.
    #[test]
    fn lint_prescricao_nominal_inalcancavel() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("spec.md");
        // `## Decisão em aberto …` NÃO é variante canônica de `decisions` — este
        // é exatamente o título não-canônico medido em campo.
        let heading = "## Decisão em aberto — como saber se uma previsão já foi efetivada";
        let body = format!(
            "# Spec\n\n{heading}\n\n\
             A onda 1 mede os três candidatos e escolhe.\n\n\
             ## Files\n- `a.rs` (create)\n\n### Backend Agent\n- [ ] t1\n- [ ] t2\n\n\
             ## Acceptance Criteria\n\
             - **AC-1** — when a.rs runs, then it returns ok.\n  Command: `curl -sf localhost`\n\
             - **AC-2** — build green.\n  Command: `cargo build`\n"
        );
        std::fs::write(&path, &body).unwrap();
        let issues = validate(dir.path(), &path, &body);
        let found = issues
            .iter()
            .find(|i| i["type"] == "wave-prescription-unreachable")
            .expect("a nominal prescription under a non-canonical heading must be flagged");
        let message = found["message"].as_str().unwrap_or_default();
        assert!(
            message.contains(heading),
            "the WARN must name the section it found: {message}"
        );
        assert!(
            message.contains("`## Decisions`"),
            "the WARN must say where to move the text: {message}"
        );

        // O outro lado: sob o título canônico, a mesma frase viaja — silêncio.
        let moved = body.replace(heading, "## Decisions");
        std::fs::write(&path, &moved).unwrap();
        let issues = validate(dir.path(), &path, &moved);
        assert!(
            !issues.iter().any(|i| i["type"] == "wave-prescription-unreachable"),
            "a prescription that DOES travel is not a finding: {issues:?}"
        );
    }

    /// O ruído que reprovou a primeira versão do lint: uma seção CANÔNICA que
    /// menciona uma onda pelo número não é achado nenhum.
    ///
    /// Medido no acervo: olhando só as três seções de material, o lint acusava 24
    /// seções em 103 specs — `## Arquivos`, `## Contexto`, `## Preocupações`,
    /// `## Critérios de Aceitação`, `## Limites`, `## Non-Goals` —, todas
    /// canônicas e todas menções de referência, não prescrição perdida. E o
    /// `## Concerns` é onde o passo 4 do `full-plan` despeja os próprios
    /// `validation.issues[]`, então o ruído se realimentaria em toda spec futura.
    #[test]
    fn uma_secao_canonica_nunca_e_prescricao_perdida() {
        for heading in [
            "## Arquivos",
            "## Files",
            "## Contexto",
            "## Critérios de Aceitação",
            "## Acceptance Criteria",
            "## Preocupações",
            "## Concerns",
            "## Limites",
            "## Non-Goals",
            "## Tarefas",
            "## Evidence",
        ] {
            let body = format!(
                "# Spec\n\n{heading}\n\n\
                 uma lição aprendida na onda 1 chega à onda 3, e a onda 2 já mediu isso.\n"
            );
            assert!(
                unreachable_wave_prescriptions(&body).is_empty(),
                "{heading} resolve para chave canônica e não pode virar achado: {:?}",
                unreachable_wave_prescriptions(&body),
            );
        }
    }

    /// A referência a uma onda que é CAMINHO, não prescrição, nunca é flagrada —
    /// o diretório `wave-1-rt`, o arquivo `wave-plan.md`, a coluna `| Wave |`.
    #[test]
    fn a_wave_path_is_not_a_prescription() {
        assert!(!names_a_wave_by_number("- `wave-1-rt/spec.md`"));
        assert!(!names_a_wave_by_number("- `.claude/spec/x/wave-plan.md`"));
        assert!(!names_a_wave_by_number("| Wave | Role | Depende de |"));
        assert!(!names_a_wave_by_number("quatro ondas dividiram três arquivos"));
        // E a prescrição, nas duas línguas.
        assert!(names_a_wave_by_number("a onda 1 mede os três candidatos e escolhe"));
        assert!(names_a_wave_by_number("as ondas 2 e 3 saem juntas"));
        assert!(names_a_wave_by_number("wave 2 owns the migration"));
    }

    /// `looks_like_file_path` names ONE concrete file, so a glob — which names a
    /// SET — is not one.
    ///
    /// Its consumer compares the answer byte-literally against declared paths,
    /// so accepting `src/*.rs` was a guaranteed warning on a correct plan. The
    /// strict recogniser that rejects it already lived in this file.
    #[test]
    fn looks_like_file_path_rejects_a_set_and_keeps_a_file() {
        // Sets, shapes and omissions — none of them is a file anybody wrote.
        assert!(!looks_like_file_path("src/*.rs"));
        assert!(!looks_like_file_path("plugin/**/*.md"));
        assert!(!looks_like_file_path("wave-N-{role}/spec.md"));
        assert!(!looks_like_file_path(".../spec.md"));
        // Still prose, as before.
        assert!(!looks_like_file_path("3.5"));
        assert!(!looks_like_file_path("e.g."));
        // And still every real path, in both separator spellings.
        assert!(looks_like_file_path("src/list.rs"));
        assert!(looks_like_file_path("Cargo.toml"));
        assert!(looks_like_file_path("src\\list.rs"), "a Windows spelling is a path");
        assert!(looks_like_file_path("`apps/rt/src/main.rs`"), "backticks are trimmed");
    }

    /// V6: a bare `cargo build` AC (not the trailing safety net) is flagged
    /// WEAK; the LAST AC (the build-green safety criterion) is exempt, and a
    /// behaviour AC with a real assertion command is never flagged.
    #[test]
    fn flags_weak_tautological_ac_command() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("spec.md");
        let body = "# Spec\n\n## Acceptance Criteria\n\
                    - **AC-1** — feature works.\n  Command: `cargo build`\n\
                    - **AC-2** — endpoint responds.\n  Command: `curl -sf localhost/health`\n\
                    - **AC-3** — build green.\n  Command: `rtk cargo build`\n";
        std::fs::write(&path, body).unwrap();
        let issues = validate(dir.path(), &path,body);
        let weak = issues
            .iter()
            .find(|i| i["type"] == json!("weak-ac"))
            .unwrap_or_else(|| panic!("expected weak-ac WARN: {issues:?}"));
        assert_eq!(weak["severity"], json!("WARN"));
        let msg = weak["message"].as_str().unwrap_or_default();
        assert!(msg.contains("AC-1"), "the planted cargo-build AC is named: {msg}");
        // AC-2 (real assertion) and AC-3 (trailing safety) are NOT flagged.
        assert!(!msg.contains("AC-2"), "a real behaviour AC is strong: {msg}");
        assert!(!msg.contains("AC-3"), "the trailing safety AC is exempt: {msg}");
    }

    /// V6: a `grep -q` "verification" is weak (asserts textual presence, not
    /// behaviour); a `cargo test` WITH a test-name filter is strong.
    #[test]
    fn weak_ac_flags_grep_but_not_filtered_cargo_test() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("spec.md");
        let body = "# Spec\n\n## Acceptance Criteria\n\
                    - **AC-1** — doc mentions it.\n  Command: `grep -q Modelo SKILL.md`\n\
                    - **AC-2** — the new unit passes.\n  Command: `cargo test -p mustard-rt my_new_case`\n\
                    - **AC-3** — build green.\n  Command: `cargo build`\n";
        std::fs::write(&path, body).unwrap();
        let issues = validate(dir.path(), &path,body);
        let msg = issues
            .iter()
            .find(|i| i["type"] == json!("weak-ac"))
            .and_then(|i| i["message"].as_str())
            .unwrap_or_default();
        assert!(msg.contains("AC-1"), "grep -q AC is weak: {issues:?}");
        assert!(!msg.contains("AC-2"), "filtered cargo test is strong: {issues:?}");
    }

    #[test]
    fn backtick_refs_reject_dotted_prose_keep_real_paths() {
        // Dotted-identifier prose in code spans is NOT a file path.
        assert!(backtick_file_refs("see `extensions.code` and `.message`").is_empty());
        assert!(backtick_file_refs("`error.extensions.code` / `err.message`").is_empty());
        // A path (separator) and a bare known-extension file ARE captured.
        let refs = backtick_file_refs("edit `src/foo.rs` and `Cargo.toml`");
        assert!(refs.contains(&"src/foo.rs".to_string()), "{refs:?}");
        assert!(refs.contains(&"Cargo.toml".to_string()), "{refs:?}");
    }

    /// The reference rule, both directions: routing punctuation keeps a real
    /// path IN, and the prose shapes that the widened character set would
    /// otherwise let in stay OUT.
    #[test]
    fn reference_rule_admits_routing_punctuation_and_rejects_prose_shapes() {
        // Route groups and dynamic segments are literal directories on disk.
        for token in ["app/(marketing)/page.tsx", "app/[slug]/route.ts", "app/[[...all]]/x.ts"] {
            assert!(is_file_reference(token), "routing punctuation is a path: {token}");
        }
        // Patterns, templates, elisions and bare extensions are not files.
        for token in [
            "plugin/**/*.md",           // glob — a set, not a file
            "wave-N-{role}/spec.md",    // template placeholder
            ".../spec.md",              // documentation elision
            ".tsx",                     // an extension being named
            "plugin//spec.md",          // malformed
            "apps/rt/src/commands/doctor/", // a directory
        ] {
            assert!(!is_file_reference(token), "prose/pattern is not a file ref: {token}");
        }
    }

    /// V6, all three directions: no exemption lets a search that cannot fail
    /// read as strong.
    ///
    /// The two escapes this locks shut were both live in the field. A
    /// search-for-ABSENCE was exempt as a "genuine post-condition", but
    /// `--files-without-match` exits 0 precisely when the pattern matches
    /// nothing. And ANY compound command was exempt on the reasoning that the
    /// author combined steps on purpose — so a presence search wearing a
    /// compound coat (`rg -q … && echo OK`) walked straight through. A
    /// genuinely combined command, with one part that really asserts something,
    /// must still read strong.
    #[test]
    fn a_search_that_cannot_fail_is_never_exempt() {
        // 1. A search is weak in EITHER direction — presence or absence.
        assert!(is_weak_ac_command("rg -q Foo src/lib.rs"), "presence search");
        assert!(is_weak_ac_command("rg --files-without-match Foo src/lib.rs"));
        assert!(is_weak_ac_command("grep -L Foo src/lib.rs"));
        assert!(is_weak_ac_command("rg -v Foo src/lib.rs"));

        // 2. A literal search chained to a step that asserts nothing is weak:
        //    every part is weak, so the compound coat changes nothing.
        assert!(is_weak_ac_command("rg -q 'fn build_report' src/lib.rs && echo OK"));
        assert!(is_weak_ac_command("grep -q Foo src/lib.rs; true"));
        assert!(is_weak_ac_command("cargo build && echo done"));

        // 3. A genuinely combined command keeps its strength — ONE part that
        //    really asserts something is enough.
        assert!(!is_weak_ac_command("cargo test -p mustard-rt my_case && ./verify.sh"));
        assert!(!is_weak_ac_command("rg -q Foo src/lib.rs && ./verify.sh"));
        assert!(!is_weak_ac_command("cargo build && curl -sf localhost/health"));
    }

    /// The operator split is quote-aware: an operator INSIDE a search pattern is
    /// not a compound boundary, so `rg 'a|b'` stays one (weak) part instead of
    /// being sliced into fragments that judge nothing.
    #[test]
    fn command_parts_split_only_on_top_level_operators() {
        assert_eq!(split_command_parts("cargo test foo"), vec!["cargo test foo"]);
        assert_eq!(
            split_command_parts("rg -q 'a|b' src && echo OK"),
            vec!["rg -q 'a|b' src", "echo OK"]
        );
        assert_eq!(
            split_command_parts("a && b || c; d | e"),
            vec!["a", "b", "c", "d", "e"]
        );
        // An unterminated quote: the split cannot be trusted, so the whole
        // string is judged as one part rather than as invented fragments.
        assert_eq!(split_command_parts("rg -q 'a && b"), vec!["rg -q 'a && b"]);
    }

    /// V6b: a FILTERED `cargo test` (strong, so NOT flagged weak) that declares
    /// no `Expect:` line raises `test-ac-no-expect` — a green suite proves the
    /// tests ran, not that this feature's behaviour holds. Adding an `Expect:`
    /// regex line suppresses the warn (the `expect_regex` evidence contract).
    #[test]
    fn expect_regex_test_ac_without_expect_warns_and_expect_line_clears_it() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("spec.md");
        // AC-1: filtered cargo test, no Expect ⇒ warns. AC-2: trailing safety,
        // exempt.
        let body = "# Spec\n\n## Acceptance Criteria\n\
                    - **AC-1** — the new parser case passes.\n  Command: `cargo test -p mustard-rt my_new_case`\n\
                    - **AC-2** — build green.\n  Command: `cargo build`\n";
        std::fs::write(&path, body).unwrap();
        let issues = validate(dir.path(), &path,body);
        let warn = issues
            .iter()
            .find(|i| i["type"] == json!("test-ac-no-expect"))
            .unwrap_or_else(|| panic!("expected test-ac-no-expect WARN: {issues:?}"));
        assert_eq!(warn["severity"], json!("WARN"));
        let msg = warn["message"].as_str().unwrap_or_default();
        assert!(msg.contains("AC-1"), "the un-asserted test AC is named: {msg}");
        assert!(!msg.contains("AC-2"), "the trailing safety AC is exempt: {msg}");

        // Same spec, AC-1 now declares an `Expect:` regex ⇒ no warn.
        let body2 = "# Spec\n\n## Acceptance Criteria\n\
                     - **AC-1** — the new parser case passes.\n  Command: `cargo test -p mustard-rt my_new_case`\n  Expect: `test result: ok`\n\
                     - **AC-2** — build green.\n  Command: `cargo build`\n";
        std::fs::write(&path, body2).unwrap();
        let issues2 = validate(dir.path(), &path,body2);
        assert!(
            !issues2.iter().any(|i| i["type"] == json!("test-ac-no-expect")),
            "a declared Expect line clears the warn: {issues2:?}"
        );
    }

    /// V6d: um critério cujo comando é EXECUTOR DE TESTE e não declara
    /// `Control:` vira `test-ac-no-control` — e declarar o controle limpa o
    /// aviso.
    ///
    /// O vocabulário é o da família inteira, não só o `cargo`: `pytest` sai com
    /// 0 quando o `-k` não casa nada exatamente como `cargo test` com um filtro
    /// errado, e é esse fato compartilhado que o aviso nomeia.
    ///
    /// Bilateral duas vezes: o critério final (a rede de segurança) nunca é
    /// nomeado, e o mesmo par comando+critério com um `Control:` declarado sai
    /// em silêncio — então a asserção não passa por o lint disparar em tudo.
    #[test]
    fn test_ac_without_control_warns_and_a_declared_control_clears_it() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("spec.md");
        let body = "# Spec\n\n## Acceptance Criteria\n\
                    - **AC-1** — the new parser case passes.\n  Command: `cargo test -p mustard-rt my_new_case`\n  Expect: `test result: ok`\n\
                    - **AC-2** — the python case passes.\n  Command: `pytest -k my_new_case`\n  Expect: `1 passed`\n\
                    - **AC-3** — build green.\n  Command: `cargo build`\n";
        std::fs::write(&path, body).unwrap();
        let issues = validate(dir.path(), &path, body);
        let warn = issues
            .iter()
            .find(|i| i["type"] == json!("test-ac-no-control"))
            .unwrap_or_else(|| panic!("expected test-ac-no-control WARN: {issues:?}"));
        assert_eq!(warn["severity"], json!("WARN"));
        let msg = warn["message"].as_str().unwrap_or_default();
        assert!(msg.contains("AC-1"), "o executor rust é nomeado: {msg}");
        assert!(msg.contains("AC-2"), "e o python também — a família toda: {msg}");
        assert!(!msg.contains("AC-3"), "a rede de segurança final é isenta: {msg}");

        // Com o `Control:` declarado, silêncio.
        let body2 = "# Spec\n\n## Acceptance Criteria\n\
                     - **AC-1** — the new parser case passes.\n  Command: `cargo test -p mustard-rt my_new_case`\n  Expect: `test result: ok`\n  Control: `cargo test -p mustard-rt`\n\
                     - **AC-2** — the python case passes.\n  Command: `pytest -k my_new_case`\n  Expect: `1 passed`\n  Control: `pytest --collect-only`\n\
                     - **AC-3** — build green.\n  Command: `cargo build`\n";
        std::fs::write(&path, body2).unwrap();
        let issues2 = validate(dir.path(), &path, body2);
        assert!(
            !issues2.iter().any(|i| i["type"] == json!("test-ac-no-control")),
            "um controle declarado limpa o aviso: {issues2:?}"
        );
    }

    /// V6d e o portão leem o MESMO critério: um `Control:` que ainda carrega o
    /// marcador do scaffold (`<command>`) não é um controle declarado — a prova
    /// negativa o recusa como `NotAttempted`, e o lint precisa avisar sobre
    /// exatamente esse critério, não calar por haver "alguma coisa" na linha.
    #[test]
    fn a_skeleton_control_is_no_control_for_the_lint_either() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("spec.md");
        let body = "# Spec\n\n## Acceptance Criteria\n\
                    - **AC-1** — the new parser case passes.\n  Command: `cargo test -p mustard-rt my_new_case`\n  Expect: `test result: ok`\n  Control: `<command>`\n\
                    - **AC-2** — build green.\n  Command: `cargo build`\n";
        std::fs::write(&path, body).unwrap();
        let issues = validate(dir.path(), &path, body);
        let warn = issues
            .iter()
            .find(|i| i["type"] == json!("test-ac-no-control"))
            .unwrap_or_else(|| panic!("expected test-ac-no-control WARN: {issues:?}"));
        let msg = warn["message"].as_str().unwrap_or_default();
        assert!(msg.contains("AC-1"), "o controle-esqueleto é nomeado como ausente: {msg}");
    }

    /// A mensagem de `missing-file` procura o nome-base sob outros prefixos: o
    /// caminho declarado relativo ao subprojeto (ou a um pedaço da árvore) é o
    /// caso de campo, e mandar marcar como novo ali é mandar criar uma segunda
    /// cópia do arquivo que já existe.
    ///
    /// Bilateral: um arquivo que não existe em lugar NENHUM continua recebendo a
    /// mensagem antiga, com os marcadores aceitos — a busca não pode virar uma
    /// desculpa para todo caminho errado.
    #[test]
    fn missing_file_names_the_prefix_where_it_exists() {
        let dir = tempdir().unwrap();
        // O arquivo existe, sob um prefixo que a spec não escreveu.
        let real = dir.path().join("apps").join("rt").join("src");
        std::fs::create_dir_all(&real).unwrap();
        std::fs::write(real.join("list.rs"), "// existing").unwrap();

        // …e um HOMÔNIMO: só o nome-base bate, o caminho não é sufixo nenhum.
        std::fs::write(real.join("mod.rs"), "// unrelated").unwrap();

        let path = dir.path().join("spec.md");
        let body = "# Spec\n## Files\n- `src/list.rs`\n- `ghost.rs`\n- `helpers/mod.rs`\n\
                    ### Backend Agent\n- [ ] t1\n- [ ] t2\n";
        std::fs::write(&path, body).unwrap();
        let issues = validate(dir.path(), &path, body);

        let found = issues
            .iter()
            .find(|i| i["type"] == json!("missing-file") && i["file"] == json!("src/list.rs"))
            .unwrap_or_else(|| panic!("expected the missing-file WARN: {issues:?}"));
        let msg = found["message"].as_str().unwrap_or_default();
        assert!(
            msg.contains("apps/rt/src/list.rs"),
            "a mensagem diz ONDE o arquivo está: {msg}"
        );
        assert!(
            msg.contains("relative to the repository root"),
            "e qual é a regra do caminho: {msg}"
        );
        assert!(
            !msg.contains("(novo)"),
            "e não manda marcar como novo um arquivo que já existe: {msg}"
        );

        // O outro lado: um arquivo que não existe em prefixo nenhum mantém a
        // mensagem antiga, com os marcadores aceitos.
        let ghost = issues
            .iter()
            .find(|i| i["type"] == json!("missing-file") && i["file"] == json!("ghost.rs"))
            .unwrap_or_else(|| panic!("a true miss must still be flagged: {issues:?}"));
        let ghost_msg = ghost["message"].as_str().unwrap_or_default();
        assert!(
            ghost_msg.contains("(create)") && ghost_msg.contains("(novo)"),
            "sem outro prefixo, a dica de marcador continua: {ghost_msg}"
        );

        // O terceiro lado: só o NOME-BASE bate. `mod.rs`, `index.ts` e `cli.rs`
        // fazem disso rotina, e a dica do marcador não pode ser SUBSTITUÍDA por
        // um endereço que aponta para outro arquivo.
        let homonym = issues
            .iter()
            .find(|i| i["type"] == json!("missing-file") && i["file"] == json!("helpers/mod.rs"))
            .unwrap_or_else(|| panic!("o homônimo continua sendo um WARN: {issues:?}"));
        let homonym_msg = homonym["message"].as_str().unwrap_or_default();
        assert!(
            homonym_msg.contains("(create)") && homonym_msg.contains("(novo)"),
            "a dica de marcador continua sendo a resposta principal: {homonym_msg}"
        );
        assert!(
            homonym_msg.contains("apps/rt/src/mod.rs") && homonym_msg.contains("DIFFERENT"),
            "e o homônimo entra como possibilidade, não como fato: {homonym_msg}"
        );
    }

    /// A varredura de prefixos só começa por uma referência que AINDA pode ser
    /// um arquivo.
    ///
    /// O gatilho não é escolhido: o `backtick_file_refs` recolhe o que está
    /// entre crases, e a seção `## Arquivos` de uma spec cita títulos de seção
    /// (`` `## ACCEPTANCE` ``) e prosa. Cada um deles chegava à varredura como
    /// "arquivo ausente" e abria até 4000 diretórios atrás de um arquivo com
    /// aquele nome.
    #[test]
    fn only_a_reference_that_could_be_a_file_starts_the_prefix_walk() {
        for path in ["src/list.rs", "Cargo.toml", "docs/notas", "apps\\rt\\main.rs", "a.rs"] {
            assert!(could_name_a_file(path), "isto pode ser um arquivo: {path}");
        }
        for prose in ["## ACCEPTANCE", "AC-1", "Command", "todo o resto", ""] {
            assert!(!could_name_a_file(prose), "isto não abre varredura: {prose}");
        }
        // E a porta fechada devolve o mesmo "não achei" de sempre, sem abrir
        // diretório nenhum — mesmo com o arquivo homônimo bem ali.
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("ACCEPTANCE"), "x").unwrap();
        assert!(
            refs_found_elsewhere(dir.path(), &["## ACCEPTANCE".to_string()]).is_empty(),
            "um título de seção não vira endereço de arquivo",
        );
    }

    /// V6c: an `Expect:` anchored at `^` against a per-file counting command can
    /// never match its own output, so the criterion is red whether or not the
    /// work is done — and by the time anyone notices, `ac-amend` refuses the
    /// repair (the corrected regex passes). Drafting time is the only door, so
    /// the lint must fire here.
    ///
    /// Reproduces the field case verbatim: `git grep -c … -- <path>` prints
    /// `path:3`, which `^[0-2]$` cannot match in either direction. Anchoring
    /// after the prefix (`:[0-2]$`) clears the warn — that is the repair the
    /// message names, so the test holds the message to it.
    #[test]
    fn expect_anchored_against_a_per_file_count_warns_and_the_prefix_form_clears_it() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("spec.md");
        // AC-1: anchored at `^` against `git grep -c` ⇒ warns. AC-2: the same
        // count anchored AFTER the prefix ⇒ silent. AC-3: trailing safety.
        let body = "# Spec\n\n## Acceptance Criteria\n\
                    - **AC-1** — CI stops compiling three times.\n  Command: `git grep -c \"run: cargo\" -- ci.yml`\n  Expect: `^[0-2]$`\n\
                    - **AC-2** — the record names windows.\n  Command: `git grep -ci windows -- notes.md`\n  Expect: `:[1-9][0-9]*$`\n\
                    - **AC-3** — build green.\n  Command: `cargo build`\n";
        std::fs::write(&path, body).unwrap();
        let issues = validate(dir.path(), &path, body);
        let warn = issues
            .iter()
            .find(|i| i["type"] == json!("expect-anchored-against-prefixed-output"))
            .unwrap_or_else(|| panic!("expected the anchored-Expect WARN: {issues:?}"));
        assert_eq!(warn["severity"], json!("WARN"));
        let msg = warn["message"].as_str().unwrap_or_default();
        assert!(msg.contains("AC-1"), "the impossible regex is named: {msg}");
        assert!(!msg.contains("AC-2"), "anchoring after the prefix is fine: {msg}");

        // A count command whose Expect already accounts for the prefix, and a
        // non-counting command anchored at `^`, both stay silent: the lint keys
        // off the PAIR, never off either half alone.
        let body2 = "# Spec\n\n## Acceptance Criteria\n\
                     - **AC-1** — CI stops compiling three times.\n  Command: `git grep -c \"run: cargo\" -- ci.yml`\n  Expect: `:[0-2]$`\n\
                     - **AC-2** — the version line is exact.\n  Command: `cargo pkgid -p mustard-rt`\n  Expect: `^mustard-rt`\n\
                     - **AC-3** — build green.\n  Command: `cargo build`\n";
        std::fs::write(&path, body2).unwrap();
        let issues2 = validate(dir.path(), &path, body2);
        assert!(
            !issues2.iter().any(|i| i["type"] == json!("expect-anchored-against-prefixed-output")),
            "neither half alone is a defect: {issues2:?}"
        );
    }

    /// The per-file-count probe keys off the count FLAG, not off any output —
    /// combined short flags count, `--count` counts, and a grep without a count
    /// flag does not (its output carries no `file:` prefix to trip over).
    #[test]
    fn counts_per_file_reads_the_count_flag_only() {
        assert!(counts_per_file("git grep -c foo -- a.md"));
        assert!(counts_per_file("git grep -ci foo -- a.md"), "combined short flags");
        assert!(counts_per_file("grep --count foo a.md"));
        assert!(!counts_per_file("git grep -q foo -- a.md"), "no count, no prefix");
        assert!(!counts_per_file("cargo test -p mustard-rt my_case"), "not a grep at all");
    }

    /// V7: a PRESENT-but-unparseable AC section with agent task blocks →
    /// `ac-task-gap` WARN. (An ABSENT AC section is left alone, like V5 — proven
    /// by `ac_format_validation_absent_section_unchanged`.)
    #[test]
    fn flags_ac_task_gap_when_agent_has_tasks_but_no_parseable_ac() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("spec.md");
        let body = "# Spec\n## Files\n- `a.rs` (create)\n### Backend Agent\n- [ ] t1\n- [ ] t2\n\n\
                    ## Acceptance Criteria\nfree prose, no parseable AC line here\n";
        std::fs::write(&path, body).unwrap();
        let issues = validate(dir.path(), &path,body);
        assert!(
            issues.iter().any(|i| i["type"] == json!("ac-task-gap")),
            "agent tasks with a broken AC section must warn: {issues:?}"
        );
    }

    /// `` `./src/x.rs` `` and `` `src/x.rs` `` are the same reference, and BOTH
    /// readers answer the same for both spellings: `ref_resolves` finds the file
    /// where it is, and the prefix walk recognises `apps/rt/src/x.rs` as the
    /// same file under another prefix (not a homonym). The `./` form used to
    /// fail the suffix match — `/./src/x.rs` ends nothing — so an existing file
    /// got the create-marker hint.
    #[test]
    fn a_dot_slash_reference_resolves_like_its_bare_spelling() {
        let dir = tempdir().unwrap();
        let spec_dir = dir.path().join("spec");
        std::fs::create_dir_all(&spec_dir).unwrap();
        let real = dir.path().join("apps").join("rt").join("src");
        std::fs::create_dir_all(&real).unwrap();
        std::fs::write(real.join("x.rs"), "// existing").unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("src").join("here.rs"), "// at root").unwrap();

        // Reader 1: resolution on disk agrees across spellings.
        for (dotted, bare) in [("./src/here.rs", "src/here.rs"), ("./src/x.rs", "src/x.rs")] {
            assert_eq!(
                ref_resolves(dotted, &spec_dir, dir.path(), &[]),
                ref_resolves(bare, &spec_dir, dir.path(), &[]),
                "`{dotted}` e `{bare}` são a mesma referência"
            );
        }
        assert!(ref_resolves("./src/here.rs", &spec_dir, dir.path(), &[]));
        assert!(!ref_resolves("./src/x.rs", &spec_dir, dir.path(), &[]));

        // Reader 2: the prefix walk sees the `./` form as the SAME file under
        // another prefix, exactly as it sees the bare form.
        let found = refs_found_elsewhere(
            dir.path(),
            &["./src/x.rs".to_string(), "src/x.rs".to_string()],
        );
        for spelling in ["./src/x.rs", "src/x.rs"] {
            let hit = found.get(spelling).unwrap_or_else(|| panic!("`{spelling}` not found"));
            assert_eq!(hit.path, "apps/rt/src/x.rs");
            assert!(hit.suffix, "`{spelling}` é o mesmo arquivo sob outro prefixo, não homônimo");
        }

        // And end to end: the WARN names where the file is, not the marker hint.
        let path = dir.path().join("spec.md");
        let body = "# Spec\n## Files\n- `./src/x.rs`\n\
                    ### Backend Agent\n- [ ] t1\n- [ ] t2\n";
        std::fs::write(&path, body).unwrap();
        let issues = validate(dir.path(), &path, body);
        let warn = issues
            .iter()
            .find(|i| i["type"] == json!("missing-file") && i["file"] == json!("./src/x.rs"))
            .unwrap_or_else(|| panic!("expected the missing-file WARN: {issues:?}"));
        let msg = warn["message"].as_str().unwrap_or_default();
        assert!(msg.contains("apps/rt/src/x.rs"), "a mensagem diz ONDE está: {msg}");
        assert!(!msg.contains("(novo)"), "e não manda criar uma segunda cópia: {msg}");

        // The normaliser itself: only a LEADING `./` goes, nothing else moves.
        assert_eq!(normalise_ref("./src/x.rs"), "src/x.rs");
        assert_eq!(normalise_ref("././src/x.rs"), "src/x.rs");
        assert_eq!(normalise_ref("src/./x.rs"), "src/./x.rs");
        assert_eq!(normalise_ref("src/x.rs"), "src/x.rs");
    }

    #[test]
    fn ref_resolves_against_subproject_root() {
        let dir = tempdir().unwrap();
        let spec_dir = dir.path().join("spec");
        std::fs::create_dir_all(&spec_dir).unwrap();
        // An existing file under a subproject root, referenced with a path
        // relative to the subproject (not the spec dir or cwd).
        let backend = dir.path().join("backend");
        std::fs::create_dir_all(backend.join("src")).unwrap();
        std::fs::write(backend.join("src").join("Payable.cs"), "// existing").unwrap();
        let roots = vec![backend.clone()];

        // Resolves via the subproject root — no false "missing".
        assert!(ref_resolves("src/Payable.cs", &spec_dir, dir.path(), &roots));
        // A genuinely-absent file still does NOT resolve — the fix must not mask
        // true misses/typos.
        assert!(!ref_resolves("src/Ghost.cs", &spec_dir, dir.path(), &roots));
        // With no subproject roots it falls back to the spec dir + the root.
        assert!(!ref_resolves("src/Payable.cs", &spec_dir, dir.path(), &[]));
    }

    #[test]
    fn flags_task_count_out_of_range() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("spec.md");
        std::fs::write(&path, "### Backend Agent\n- [ ] only one\n").unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        let issues = validate(dir.path(), &path,&content);
        assert!(issues.iter().any(|i| i["type"] == json!("task-count")));
    }

    /// A well-formed AC section (the drafter shape: `- **AC-N** — title` +
    /// indented `Command:` line) must NOT raise `unparseable-ac`.
    #[test]
    fn ac_format_validation_parseable_section_is_clean() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("spec.md");
        let body = "# Spec\n\n## Acceptance Criteria\n\
                    - **AC-1** — workspace builds green.\n  Command: `cargo build`\n\
                    - **AC-2** — tests pass.\n  Command: `cargo test`\n";
        std::fs::write(&path, body).unwrap();
        let issues = validate(dir.path(), &path,body);
        assert!(
            !issues.iter().any(|i| i["type"] == json!("unparseable-ac")),
            "{issues:?}"
        );
    }

    /// An AC section whose items the qa-run parser cannot read (no `Command:`
    /// anywhere) yields a WARN `unparseable-ac` with the format hint — the
    /// exact situation where qa-run later degrades to `overall: skip`.
    #[test]
    fn ac_format_validation_malformed_section_warns() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("spec.md");
        let body = "# Spec\n\n## Acceptance Criteria\n\
                    - AC um: roda os testes sem comando declarado\n\
                    - criterio solto sem id\n";
        std::fs::write(&path, body).unwrap();
        let issues = validate(dir.path(), &path,body);
        let issue = issues
            .iter()
            .find(|i| i["type"] == json!("unparseable-ac"))
            .unwrap_or_else(|| panic!("expected unparseable-ac WARN: {issues:?}"));
        assert_eq!(issue["severity"], json!("WARN"));
        let msg = issue["message"].as_str().unwrap_or_default();
        assert!(msg.contains("**AC-N**"), "hint must show the exact format: {msg}");
        assert!(msg.contains("Command:"), "hint must mention the Command: line: {msg}");
    }

    /// Roundtrip (TF marcador localizado): the pt-BR drafter marks net-new
    /// files `(novo)`/`(criar)` — both must suppress `missing-file` exactly
    /// like the EN canonical `(create)` (the run that motivated the fix
    /// produced 7 false `missing-file` WARNs from `(novo)` lines).
    #[test]
    fn roundtrip_localized_create_marker_suppresses_missing_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("spec.md");
        let body = "# Spec\n## Arquivos\n\
                    - `ghost_a.rs` (novo)\n\
                    - `ghost_b.rs` (criar)\n\
                    - `ghost_c.rs` (create)\n\
                    ### Backend Agent\n- [ ] t1\n- [ ] t2\n";
        std::fs::write(&path, body).unwrap();
        let issues = validate(dir.path(), &path,body);
        assert!(
            !issues.iter().any(|i| i["type"] == json!("missing-file")),
            "localized markers recognised: {issues:?}"
        );
        // The localized set must NOT mask true misses: an unmarked absent
        // file (and an `(editar)`-marked one, which claims to exist) still WARN.
        let body2 = "# Spec\n## Arquivos\n- `ghost.rs`\n- `gone.rs` (editar)\n\
                     ### Backend Agent\n- [ ] t1\n- [ ] t2\n";
        std::fs::write(&path, body2).unwrap();
        let issues2 = validate(dir.path(), &path,body2);
        let missing: Vec<&Value> = issues2
            .iter()
            .filter(|i| i["type"] == json!("missing-file"))
            .collect();
        assert_eq!(missing.len(), 2, "true misses still flagged: {issues2:?}");
        // The hint names the accepted markers from the shared i18n origin.
        let msg = missing[0]["message"].as_str().unwrap_or_default();
        assert!(msg.contains("(create)") && msg.contains("(novo)"), "hint lists synonyms: {msg}");
    }

    /// Roundtrip (leitor defensivo): a LEGACY spec already on disk with the
    /// duplicated AC heading (placeholder first, real list second) still
    /// validates clean — the defensive `section_block` returns the parseable
    /// section instead of the placeholder that used to trigger
    /// `unparseable-ac`.
    #[test]
    fn roundtrip_legacy_duplicated_ac_heading_still_validates() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("spec.md");
        let body = "# Spec\n\n## Critérios de Aceitação\n\nVer abaixo.\n\n\
                    ## Critérios de Aceitação\n\n\
                    - **AC-1** — workspace builds green.\n  Command: `cargo build`\n";
        std::fs::write(&path, body).unwrap();
        let issues = validate(dir.path(), &path,body);
        assert!(
            !issues.iter().any(|i| i["type"] == json!("unparseable-ac")),
            "legacy duplicated AC section parses: {issues:?}"
        );
        assert!(issues.is_empty(), "legacy spec validates ok:true: {issues:?}");
    }

    /// No AC section at all → behaviour unchanged (no `unparseable-ac`).
    #[test]
    fn ac_format_validation_absent_section_unchanged() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("spec.md");
        let body = "# Spec\n## Files\n- `a.rs` (create)\n### Backend Agent\n- [ ] t1\n- [ ] t2\n";
        std::fs::write(&path, body).unwrap();
        let issues = validate(dir.path(), &path,body);
        assert!(
            !issues.iter().any(|i| i["type"] == json!("unparseable-ac")),
            "{issues:?}"
        );
        // The clean-spec baseline stays clean overall.
        assert!(issues.is_empty(), "{issues:?}");
    }

    #[test]
    fn flags_layer_gap() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("spec.md");
        std::fs::write(
            &path,
            "## Files\n- `a.txt` (create)\n### Frontend Agent\n- [ ] t1\n- [ ] t2\n",
        )
        .unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        let issues = validate(dir.path(), &path,&content);
        assert!(issues.iter().any(|i| i["type"] == json!("layer-gap")));
    }
}
