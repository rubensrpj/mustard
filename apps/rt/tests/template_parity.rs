//! Parity ratchet between the published `mustard-rt run` surface and every
//! product caller (templates, CLI sources, installer, packaging, command doc).
//!
//! Complements `run_command_surface.rs` (which locks the clap tree itself):
//!
//! - **FORWARD** — every `mustard-rt run <name>` a product file instructs must
//!   resolve to a registered subcommand, and every long flag typed on it must
//!   be one that command really declares. A template pointing at a name that no
//!   longer exists does not break the build — the command silently VANISHES at
//!   runtime; one typing a flag clap never registered dies with `error:
//!   unexpected argument` and exit 2. This walk turns both into a test failure.
//! - **REVERSE** — every registered subcommand must have at least one static
//!   product caller (prose instruction or spawned argv). Sem lista de
//!   exceções: com 22 comandos, um comando que nenhum texto chama é superfície
//!   escura — ele é entregue, apodrece, e nada percebe.
//!
//! - **GANCHOS** — o registro dos ganchos tem só os que ficam, nenhum que
//!   saiu, e casa com os eventos do `plugin/hooks/hooks.json`: uma entrada que
//!   chama evento sem gancho gasta uma chamada à toa.
//!
//! Deterministic: walks the repo tree only (sorted), no network, no env vars.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use clap::{Command, Subcommand};
use regex::Regex;
use mustard_core::domain::spec_state::State;
use mustard_rt::commands::flow::resume::{next_command, step_command, NEXT_BY_PHASE};
use mustard_rt::commands::flow::round::DONE_STEP;
use mustard_rt::commands::RunCmd;

/// Declared long flags that NO product prose spells, kept deliberately. Sorted
/// by `(command, flag)`; each justification says why a reader is never left
/// looking for this one.
///
/// The bar is not "it is minor". A flag reachable only by reading `--help` of a
/// command the docs never show is a feature that shipped to nobody, and the
/// honest fixes are to document it or to remove it. A row here says the flag is
/// reachable some OTHER way — it mirrors a documented sibling, it is the escape
/// hatch a refusal message prints, or it exists for a caller that is not prose.
const FLAG_WHITELIST: &[(&str, &str, &str)] = &[];

/// Caller spellings that precede a `run <name>` instruction in product files.
/// `$RtExe` is `install.ps1`'s handle for the freshly built `mustard-rt.exe`.
const CALLER_PREFIXES: &[&str] = &["mustard-rt run ", "mustard-rt.exe run ", "$RtExe run "];

/// The repo root, resolved from this crate (`apps/rt`).
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Build the `run` command tree exactly as `main.rs` hands it to clap.
fn run_command_tree() -> Command {
    let mut cmd = RunCmd::augment_subcommands(Command::new("run"));
    cmd.build();
    cmd
}

/// Every declared `run` subcommand name (clap's auto `help` excluded), sorted.
fn surface_names() -> Vec<String> {
    let cmd = run_command_tree();
    let mut names: Vec<String> = cmd
        .get_subcommands()
        .map(|c| c.get_name().to_string())
        .filter(|n| n != "help")
        .collect();
    names.sort_unstable();
    names
}

/// Recursively collect files under `dir` in a deterministic (sorted) order.
fn walk_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            let name = entry.file_name();
            if name == "node_modules" || name == "target" || name == ".git" {
                continue;
            }
            walk_files(&path, out);
        } else {
            out.push(path);
        }
    }
}

/// Read a file as lossy UTF-8; unreadable files degrade to an empty string.
fn read_lossy(path: &Path) -> String {
    fs::read(path).map_or_else(|_| String::new(), |b| String::from_utf8_lossy(&b).into_owned())
}

fn has_extension(path: &Path, exts: &[&str]) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| exts.contains(&e))
}

fn is_token_byte(b: u8) -> bool {
    b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'
}

/// One `mustard-rt run …` instruction as a product file spells it.
struct RunInvocation {
    /// The registered subcommand name, after the two-token collapse.
    name: String,
    /// Every long flag typed on THAT invocation, without its `--`.
    flags: Vec<String>,
}

/// Extract every `run <name> [--flag …]` instruction reachable through one of
/// the [`CALLER_PREFIXES`].
///
/// Um nome é um token só. As duas formas de dois tokens que existiam aqui
/// (`metrics wave-status` e `scan spec`, que o `main.rs` colava antes do clap)
/// saíram com os comandos que as usavam, e a colagem saiu junto: o `main.rs`
/// não reescreve mais argv nenhum.
fn extract_run_invocations(text: &str) -> Vec<RunInvocation> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    for prefix in CALLER_PREFIXES {
        let mut from = 0;
        while let Some(pos) = text[from..].find(prefix) {
            let start = from + pos + prefix.len();
            from = start;
            let mut end = start;
            while end < bytes.len() && is_token_byte(bytes[end]) {
                end += 1;
            }
            if end == start || !bytes[start].is_ascii_lowercase() {
                continue;
            }
            let name = text[start..end].to_string();
            let flags = long_flags_of(&text[end..]);
            out.push(RunInvocation { name, flags });
        }
    }
    out
}

/// The long flags of ONE invocation, read from the text that follows its name.
///
/// The sweep stops at the first byte that cannot still belong to the same
/// command — a newline, a closing backtick, a pipe, a chain operator, a
/// redirection — so a second command sharing the line never lends its flags to
/// the first. Every slice boundary lands on an ASCII byte, so prose full of
/// em-dashes is walked without ever cutting a character in half.
fn long_flags_of(rest: &str) -> Vec<String> {
    let bytes = rest.as_bytes();
    let stop = bytes
        .iter()
        .position(|b| matches!(b, b'\n' | b'`' | b'|' | b'&' | b';' | b'<' | b'>'))
        .unwrap_or(bytes.len());
    scan_long_flags(&rest[..stop])
}

/// Every long flag spelled anywhere in a stretch of text.
///
/// Shared by both directions of the flag ratchet, deliberately: FORWARD asks
/// whether a flag typed on an invocation is declared, REVERSE asks whether a
/// declared flag is ever typed, and the two must never disagree about what
/// counts as "typed". A flag opens on a `--` that starts a token and is followed
/// by a lowercase letter, so a markdown `---` fence, an em-dash run and the
/// `--force-with-lease` inside `--no-force-with-lease` are all read the same way
/// here as they are there.
fn scan_long_flags(seg: &str) -> Vec<String> {
    let sb = seg.as_bytes();
    let mut flags = Vec::new();
    let mut i = 0;
    while i + 2 < sb.len() {
        let opens = sb[i] == b'-'
            && sb[i + 1] == b'-'
            && sb[i + 2].is_ascii_lowercase()
            && (i == 0 || !(sb[i - 1].is_ascii_alphanumeric() || sb[i - 1] == b'-'));
        if !opens {
            i += 1;
            continue;
        }
        let flag_start = i + 2;
        let mut flag_end = flag_start;
        while flag_end < sb.len() && is_token_byte(sb[flag_end]) {
            flag_end += 1;
        }
        flags.push(seg[flag_start..flag_end].to_string());
        i = flag_end;
    }
    flags
}

/// Every long flag ONE subcommand declares, minus the two clap generates on its
/// own. `--help` and `--version` are the runtime's, not the product's: no file
/// has to document them and no whitelist should have to excuse them.
fn declared_long_flags(cmd: &Command) -> BTreeSet<&str> {
    cmd.get_arguments()
        .filter_map(clap::Arg::get_long)
        .filter(|f| *f != "help" && *f != "version")
        .collect()
}

/// Every long flag spelled anywhere in the product corpus.
///
/// The whole corpus is read as ONE text, because the question is whether a
/// reader can find the flag at all — not which file happens to carry it.
fn spelled_long_flags(root: &Path) -> BTreeSet<String> {
    forward_corpus(root)
        .iter()
        .flat_map(|p| scan_long_flags(&read_lossy(p)))
        .collect()
}

/// The names half of [`extract_run_invocations`], for the callers that ask only
/// which commands a file instructs.
fn extract_run_names(text: &str) -> Vec<String> {
    extract_run_invocations(text).into_iter().map(|inv| inv.name).collect()
}

/// The files whose `run <name>` instructions the FORWARD check validates.
fn forward_corpus(root: &Path) -> Vec<PathBuf> {
    let mut files = reverse_prose_corpus(root);
    walk_files(&root.join("packaging"), &mut files);
    files.push(root.join("MUSTARD-COMMANDS.md"));
    files
}

/// The prose half of the REVERSE caller corpus: templates (md/json, which
/// includes the settings.json seed), the CLI sources, and the installer.
fn reverse_prose_corpus(root: &Path) -> Vec<PathBuf> {
    let templates = root.join("apps/cli/templates");
    assert!(templates.is_dir(), "templates dir missing at {}", templates.display());
    let mut files = Vec::new();
    walk_files(&templates, &mut files);
    // The harness seeds (settings.json — whose permissions/statusLine name
    // `mustard-rt run` commands — and the injectable instruction files) moved
    // to `packages/core/templates/`, compiled into the binaries via
    // `include_str!`. They are product callers all the same.
    let core_templates = root.join("packages/core/templates");
    assert!(
        core_templates.is_dir(),
        "core seed dir missing at {}",
        core_templates.display()
    );
    walk_files(&core_templates, &mut files);
    files.retain(|p| has_extension(p, &["md", "json"]));

    // Mustard 2.0: the command/skill/ref callers moved from `apps/cli/templates`
    // into the `plugin/` tree (init ships them via the plugin, not a copy). Walk
    // it too so those `mustard-rt run <name>` instructions still count as product
    // callers — otherwise every plugin-hosted command reads as dark surface.
    let plugin = root.join("plugin");
    if plugin.is_dir() {
        let mut plugin_files = Vec::new();
        walk_files(&plugin, &mut plugin_files);
        plugin_files.retain(|p| has_extension(p, &["md", "json"]));
        files.extend(plugin_files);
    }

    let mut cli_sources = Vec::new();
    walk_files(&root.join("apps/cli/src"), &mut cli_sources);
    cli_sources.retain(|p| has_extension(p, &["rs"]));
    files.extend(cli_sources);
    let installer = root.join("install.ps1");
    assert!(installer.is_file(), "install.ps1 missing at {}", installer.display());
    files.push(installer);
    files
}

/// Collapse all whitespace runs to single spaces so multi-line argv arrays
/// (a quoted "run" and the quoted name split across lines) match their
/// single-line spelling.
fn squash_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `true` when some Rust source spawns `mustard-rt` with `["run", "<name>"]`.
///
/// rt sources exclude the registration/list surfaces (`cli.rs` family files,
/// `doctor.rs` known-list) and the command's own module — a command's own
/// docs are not a caller.
fn has_argv_caller(root: &Path, name: &str) -> bool {
    let needle = format!("\"run\", \"{name}\"");
    let own_module = format!("{}.rs", name.replace('-', "_"));

    let mut rt_sources = Vec::new();
    walk_files(&root.join("apps/rt/src"), &mut rt_sources);

    let excluded = |p: &Path| {
        p.file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n == "cli.rs" || n == "doctor.rs" || n == own_module)
    };
    rt_sources
        .iter()
        .filter(|p| has_extension(p, &["rs"]) && !excluded(p))
        .any(|p| squash_whitespace(&read_lossy(p)).contains(&needle))
}

#[test]
fn forward_every_instructed_run_name_is_registered() {
    let root = repo_root();
    let registered: BTreeSet<String> = surface_names().into_iter().collect();
    let mut offenders = Vec::new();
    for file in forward_corpus(&root) {
        let text = read_lossy(&file);
        for name in extract_run_names(&text) {
            if !registered.contains(&name) {
                let shown = file.strip_prefix(&root).unwrap_or(&file);
                offenders.push(format!("{} -> `run {name}`", shown.display()));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "product files instruct `mustard-rt run` names the CLI does not \
         register - the call dies silently at runtime. Fix the file or \
         register the command:\n{}",
        offenders.join("\n")
    );
}

/// Every long flag a product file types on a `mustard-rt run <name>`
/// instruction must be one that command really declares.
///
/// The NAME half of this ratchet has always been checked; the flag half was
/// blind, and that blindness shipped: a reference `/git` orders read told the
/// agent to run `doctor --only branch-protection`, a flag clap answers with
/// `error: unexpected argument '--only' found` and exit 2. An instruction that
/// dies on its own arguments is exactly as broken as one naming a command that
/// does not exist, and nothing in the repository could tell.
///
/// An unregistered NAME is skipped here — that is the other test's finding, and
/// reporting it twice buries the flag it was asked about.
#[test]
fn forward_every_instructed_flag_is_declared() {
    let root = repo_root();
    let tree = run_command_tree();
    let mut offenders = Vec::new();
    for file in forward_corpus(&root) {
        let text = read_lossy(&file);
        for inv in extract_run_invocations(&text) {
            let Some(cmd) = tree.get_subcommands().find(|c| c.get_name() == inv.name) else {
                continue;
            };
            let declared: BTreeSet<&str> = cmd
                .get_arguments()
                .filter_map(clap::Arg::get_long)
                .chain(["help"])
                .collect();
            for flag in inv.flags {
                if !declared.contains(flag.as_str()) {
                    let shown = file.strip_prefix(&root).unwrap_or(&file);
                    offenders
                        .push(format!("{} -> `run {} --{flag}`", shown.display(), inv.name));
                }
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "product files type `mustard-rt run` flags the CLI does not declare - \
         the call aborts with `error: unexpected argument`, exit 2, before doing \
         anything. Fix the file or declare the flag:\n{}",
        offenders.join("\n")
    );
}

/// O campo do próximo passo é uma instrução como a de qualquer arquivo do
/// produto, e passa pela mesma catraca: o nome que ele manda rodar tem de estar
/// registrado, a opção que ele já vem escrita, declarada, e a linha inteira,
/// com as opções que o comando exige, aceita pelo parser.
///
/// A ida anda pelos arquivos do repositório, e esta instrução não mora em
/// arquivo nenhum: o binário a monta na hora e a entrega no campo `command` da
/// resposta, de onde quem conduz a conversa a copia e roda. Um renome do
/// comando, a opção `--spec` deixando de ser declarada, ou uma opção
/// obrigatória que a linha não traz, entrega um passo que morre num erro do
/// parser e código 2 — e nada no repositório teria como acusar, porque nenhum
/// texto do produto escreve essa linha.
///
/// A instrução conferida é a que o próprio binário monta, nunca uma cópia do
/// formato dela escrita aqui: um teste que remontasse a linha à mão conferiria
/// a própria cópia e continuaria verde depois de a montagem mudar. Entram as
/// fases da tabela e o fechamento, que a rodada devolve com tudo aprovado.
#[test]
fn o_campo_do_proximo_passo_passa_pela_mesma_catraca() {
    let tree = run_command_tree();
    assert!(!NEXT_BY_PHASE.is_empty(), "a tabela do próximo passo está vazia");

    let estado = State {
        branch: Some("feature/alguma-spec".to_string()),
        base: Some("dev".to_string()),
        ..State::default()
    };
    let mut montadas: Vec<(String, Option<String>)> = NEXT_BY_PHASE
        .iter()
        .map(|(fase, _)| {
            (format!("a fase `{fase}`"), next_command(fase, "alguma-spec", &estado).as_str().map(str::to_string))
        })
        .collect();
    montadas.push(("a rodada com tudo aprovado".to_string(), step_command(DONE_STEP, "alguma-spec", &estado)));

    let mut offenders = Vec::new();
    for (quem, montado) in &montadas {
        let Some(instrucao) = montado.as_deref() else {
            offenders.push(format!("{quem} está na tabela e não monta comando nenhum"));
            continue;
        };
        let mut invocacoes = extract_run_invocations(instrucao);
        let Some(inv) = invocacoes.pop() else {
            offenders.push(format!("{quem} monta `{instrucao}`, que não é uma chamada de `mustard-rt run`"));
            continue;
        };
        let Some(cmd) = tree.get_subcommands().find(|c| c.get_name() == inv.name) else {
            offenders.push(format!("{quem} manda rodar `run {}`, que não é registrado", inv.name));
            continue;
        };
        let declaradas = declared_long_flags(cmd);
        for flag in inv.flags {
            if !declaradas.contains(flag.as_str()) {
                offenders.push(format!(
                    "{quem} manda rodar `run {} --{flag}`, que esse comando não declara",
                    inv.name
                ));
            }
        }
        // A linha inteira, como quem obedece a resposta a roda: o parser de
        // verdade cobra as opções obrigatórias que a conferência das opções
        // escritas não vê.
        let argv: Vec<&str> = instrucao.split_whitespace().skip(1).collect();
        if let Err(erro) = tree.clone().try_get_matches_from(argv) {
            offenders.push(format!("{quem} monta `{instrucao}`, que o parser recusa: {erro}"));
        }
    }
    assert!(
        offenders.is_empty(),
        "o campo do próximo passo entrega uma linha que o binário recusa - quem \
         obedecer a resposta gasta a chamada num erro do clap. Conserte a tabela \
         do próximo passo ou o comando que ela nomeia:\n{}",
        offenders.join("\n")
    );
}

/// Os ganchos que ficam: os únicos que o registro pode ter.
const KEPT_HOOKS: &[&str] = &[
    "approval_witness",
    "command_guard",
    "end_of_turn_check",
    "prompt_entry",
    "session_cleanup_observer",
    "session_start_inject",
    "statusline_heal_observer",
    "subagent_inject",
    "write_gate",
];

/// Os ganchos que saíram, pelo nome com que estavam registrados. Nenhum deles
/// volta ao registro.
const REMOVED_HOOKS: &[&str] = &[
    "active_spec_limit_gate",
    "amend_window_inject",
    "bash_command_gate",
    "boundary_gate",
    "change_request_log",
    "clarification_observer",
    "context_budget_gate",
    "delegation_advisory",
    "main_context_counter",
    "metrics_observer",
    "mold_gate",
    "picker_approval_observer",
    "plan_approval_observer",
    "post_edit",
    "prompt_submit_inject",
    "rewave_observer",
    "scan_gate",
    "session_knowledge_observer",
    "size_gate",
    "skill_usage_observer",
    "spec_hygiene_observer",
    "subagent_observer",
    "tool_result_observer",
    "tool_use_counter",
    "user_prompt_observer",
    "wave_complete_observer",
    "wave_start_observer",
    "wikilink_footer_observer",
    "worktree_create",
];

/// Os comandos `run` que saíram, pelo nome com que estavam registrados: os
/// que viraram parte de outro comando e os que saíram sem substituto. Nenhum
/// deles volta ao que o binário imprime nem à prosa.
const REMOVED_COMMANDS: &[&str] = &[
    "ac-add",
    "ac-amend",
    "ac-negative-check",
    "active-specs",
    "adapt-cursor",
    "agent-prompt-render",
    "amend-finalize",
    "analyze-validation",
    "approve-spec",
    "artifact-update",
    "base-candidates",
    "capability",
    "change-request",
    "claude-dir-prune",
    "close-orchestrate",
    "close-pipeline",
    "complete-spec",
    "context-slice",
    "dependency-precheck",
    "diagnose-otel",
    "diff-context",
    "digest-adherence-finalize",
    "doc-page",
    "docs-stale-check",
    "emit-event",
    "emit-phase",
    "emit-pipeline",
    "equivalence-learn",
    "event-projections",
    "exec-rewave-check",
    "feature",
    "finding-collect",
    "gate-regression-check",
    "git-delete",
    "git-settle",
    "glossary-coverage",
    "grill-capture",
    "language-audit",
    "maint-deps",
    "maint-validate",
    "mark-checklist-item",
    "mark-finding",
    "material-add",
    "metrics",
    "metrics-wave-status",
    "notebook",
    "orient",
    "otel-collector",
    "otel-stop",
    "pipeline-summary",
    "plan-materialize",
    "plan-prepare",
    "pr-edit",
    "pr-list",
    "pr-ready",
    "qa-run",
    "rebuild-specs",
    "rehook",
    "resume-bootstrap",
    "review-dispatch",
    "review-prefetch",
    "review-result",
    "scan-guards-apply",
    "scan-guards-list",
    "scan-lapidation",
    "scan-patterns-apply",
    "scan-patterns-decline",
    "scan-patterns-list",
    "scan-patterns-relay",
    "scan-patterns-sweep",
    "scan-spec",
    "scope-classify",
    "scope-decompose",
    "scratch-gc",
    "security-scan",
    "spec-children",
    "spec-children-tree",
    "spec-doc",
    "spec-draft",
    "status",
    "tactical-fix-create",
    "tactical-fix-detect",
    "unhook",
    "verify-pipeline",
    "wave-advance",
    "wave-collapse",
    "wave-dependency",
    "wave-done",
    "wave-files",
    "wave-overlap-check",
    "wave-scaffold",
    "wave-size-check",
    "wave-tree",
    "work-unit-open",
    "worktree-gc",
];

/// `true` quando o byte pode continuar um nome de comando ou de gancho.
fn is_name_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'-' || b == b'_'
}

/// Os começos de cada ocorrência de `name` inteiro em `text`: nem colado a
/// outra letra antes, nem continuado depois.
fn whole_name_at<'a>(text: &'a str, name: &'a str) -> impl Iterator<Item = usize> + 'a {
    let bytes = text.as_bytes();
    text.match_indices(name).map(|(at, _)| at).filter(move |&at| {
        let end = at + name.len();
        (at == 0 || !is_name_byte(bytes[at - 1])) && bytes.get(end).is_none_or(|b| !is_name_byte(*b))
    })
}

/// Os nomes que saíram e que `text` ainda dá ao leitor. Um gancho conta em
/// qualquer lugar, porque o nome dele não é palavra comum. Um comando conta
/// quando o texto manda rodá-lo (`run <nome>`) ou, se o nome tem hífen, quando
/// o cita como código (`` `<nome>` ``): é assim que o leitor o toma por um
/// comando que existe. Um nome de uma palavra só, como `status`, citado como
/// código é outra coisa, e não conta.
fn removed_names_in(text: &str) -> Vec<&'static str> {
    let bytes = text.as_bytes();
    let hooks = REMOVED_HOOKS.iter().filter(|name| whole_name_at(text, name).next().is_some());
    let commands = REMOVED_COMMANDS.iter().filter(|name| {
        whole_name_at(text, name).any(|at| {
            text[..at].ends_with("run ") || (name.contains('-') && at > 0 && bytes[at - 1] == b'`')
        })
    });
    hooks.chain(commands).copied().collect()
}

/// O literal de string que começa em `rest[0]` (`"…"`, `b"…"`, `r#"…"#` ou
/// `br"…"`), com o tamanho que ele ocupa no código. O texto sai como o
/// programa o vê: os escapes comuns viram o caractere, os outros viram um
/// espaço, e a continuação de linha some com os espaços que a seguem.
fn string_literal(rest: &[u8]) -> Option<(String, usize)> {
    let prefix = usize::from(matches!(rest.first(), Some(b'b' | b'c')));
    let raw = rest.get(prefix) == Some(&b'r');
    let hashes = if raw { rest[prefix + 1..].iter().take_while(|c| **c == b'#').count() } else { 0 };
    let open = prefix + usize::from(raw) + hashes;
    if rest.get(open) != Some(&b'"') {
        return None;
    }
    let body = &rest[open + 1..];
    if raw {
        let close = [&b"\""[..], &vec![b'#'; hashes]].concat();
        let end = body.windows(close.len()).position(|w| w == close.as_slice())?;
        return Some((String::from_utf8_lossy(&body[..end]).into_owned(), open + 1 + end + close.len()));
    }
    let mut text = Vec::new();
    let mut j = 0;
    while j < body.len() && body[j] != b'"' {
        if body[j] != b'\\' {
            text.push(body[j]);
            j += 1;
            continue;
        }
        let escaped = body.get(j + 1).copied().unwrap_or(b' ');
        j += 2;
        match escaped {
            b'\n' | b'\r' => j += body[j..].iter().take_while(|c| c.is_ascii_whitespace()).count(),
            b'n' => text.push(b'\n'),
            b't' => text.push(b'\t'),
            b'"' | b'\\' | b'\'' => text.push(escaped),
            _ => text.push(b' '),
        }
    }
    Some((String::from_utf8_lossy(&text).into_owned(), open + 1 + j + 1))
}

/// O tamanho do literal de caractere que começa em `rest[0]`, ou 1 quando o
/// apóstrofo abre um tempo de vida, como `'static`.
fn char_literal_len(rest: &[u8]) -> usize {
    if rest.get(1) == Some(&b'\\') {
        return rest[2..].iter().position(|c| *c == b'\'').map_or(1, |at| at + 3);
    }
    let width = match rest.get(1) {
        Some(0xC0..=0xDF) => 2,
        Some(0xE0..=0xEF) => 3,
        Some(0xF0..=0xFF) => 4,
        _ => 1,
    };
    if rest.get(1 + width) == Some(&b'\'') { width + 2 } else { 1 }
}

/// O texto de cada literal de string do código de produção de um arquivo
/// `.rs`, em ordem. Os comentários ficam de fora, e cada item marcado com
/// `#[cfg(test)]` também, até o `;` ou a vírgula dele ou até o fim do bloco de
/// chaves dele; as chaves dentro de um literal não contam.
fn production_literals(source: &str) -> Vec<String> {
    const TEST_ONLY: &[u8] = b"#[cfg(test)]";
    let b = source.as_bytes();
    let mut out = Vec::new();
    // O item de teste que está sendo pulado: a fundura das chaves e dos
    // parênteses dele, e se o bloco de chaves já abriu.
    let mut skipping: Option<(usize, usize, bool)> = None;
    let mut i = 0;
    while i < b.len() {
        let rest = &b[i..];
        // Um prefixo de literal (`b`, `r`, `c`) só abre um literal no começo
        // de um nome, nunca no fim de outro.
        let literal = match rest[0] {
            b'"' => string_literal(rest),
            b'b' | b'r' | b'c' if i == 0 || !(b[i - 1].is_ascii_alphanumeric() || b[i - 1] == b'_') => {
                string_literal(rest)
            }
            _ => None,
        };
        if rest.starts_with(b"//") {
            i += rest.iter().position(|c| *c == b'\n').unwrap_or(rest.len());
        } else if rest.starts_with(b"/*") {
            i += rest.windows(2).position(|w| w == b"*/").map_or(rest.len(), |end| end + 2);
        } else if let Some((text, len)) = literal {
            if skipping.is_none() {
                out.push(text);
            }
            i += len;
        } else if rest[0] == b'\'' {
            i += char_literal_len(rest);
        } else if skipping.is_none() && rest.starts_with(TEST_ONLY) {
            skipping = Some((0, 0, false));
            i += TEST_ONLY.len();
        } else {
            if let Some((braces, nest, opened)) = skipping.as_mut() {
                let ends = match rest[0] {
                    b'{' => {
                        (*braces, *opened) = (*braces + 1, true);
                        false
                    }
                    b'}' => {
                        *braces = braces.saturating_sub(1);
                        *braces == 0
                    }
                    b'(' | b'[' => {
                        *nest += 1;
                        false
                    }
                    b')' | b']' if *nest > 0 => {
                        *nest -= 1;
                        false
                    }
                    b')' | b']' | b';' | b',' => !*opened && *nest == 0,
                    _ => false,
                };
                if ends {
                    skipping = None;
                }
            }
            i += 1;
        }
    }
    out
}

/// O que o código de produção de um arquivo `.rs` pode imprimir: cada
/// literal, e cada comando mandado rodar por argumentos separados
/// (`"run", "<nome>"`), escrito como `run <nome>`.
fn rust_texts(source: &str) -> Vec<String> {
    let literals = production_literals(source);
    let argv: Vec<String> =
        literals.windows(2).filter(|pair| pair[0] == "run").map(|pair| format!("run {}", pair[1])).collect();
    literals.into_iter().chain(argv).collect()
}

/// Todo texto que o binário imprime ou grava para o leitor, com a origem de
/// cada um: a prosa do plugin e os moldes que o instalador grava no projeto;
/// cada literal de string do código de produção, que é de onde saem o
/// catálogo de textos, as dicas e as recusas; a ajuda de cada comando `run`; e
/// o próximo passo de cada fase.
fn printed_texts(root: &Path) -> Vec<(String, String)> {
    let shown = |path: &Path| path.strip_prefix(root).unwrap_or(path).display().to_string();
    let mut texts = Vec::new();
    for dir in ["plugin", "packages/core/templates"] {
        let mut files = Vec::new();
        walk_files(&root.join(dir), &mut files);
        assert!(!files.is_empty(), "{dir} holds no text to sweep");
        texts.extend(files.iter().map(|file| (shown(file), read_lossy(file))));
    }
    for dir in ["apps/rt/src", "apps/cli/src", "packages/core/src"] {
        let mut files = Vec::new();
        walk_files(&root.join(dir), &mut files);
        for file in files.iter().filter(|f| has_extension(f, &["rs"])) {
            texts.extend(rust_texts(&read_lossy(file)).into_iter().map(|text| (shown(file), text)));
        }
    }
    let tree = run_command_tree();
    texts.push(("run --help".to_string(), tree.clone().render_long_help().to_string()));
    for sub in tree.get_subcommands() {
        texts.push((format!("run {} --help", sub.get_name()), sub.clone().render_long_help().to_string()));
    }
    let state = State { branch: Some("feature/alguma-spec".to_string()), base: Some("dev".to_string()), ..State::default() };
    for (phase, _) in NEXT_BY_PHASE {
        let next = next_command(phase, "alguma-spec", &state);
        texts.push((format!("the next step of `{phase}`"), next.as_str().unwrap_or_default().to_string()));
    }
    let done = step_command(DONE_STEP, "alguma-spec", &state).unwrap_or_default();
    texts.push(("the next step once every wave is approved".to_string(), done));
    texts
}

/// Nenhum texto que o binário imprime ou grava — o catálogo, as dicas, a
/// ajuda, o próximo passo — nem a prosa do plugin dá ao leitor um comando ou
/// um gancho que saiu. Cortar um comando e esquecer uma frase que o cita manda
/// quem lê gastar uma chamada num comando que não existe.
#[test]
fn no_printed_text_names_a_command_or_hook_that_left() {
    let root = repo_root();
    let surface: BTreeSet<String> = surface_names().into_iter().collect();
    let back: Vec<&&str> = REMOVED_COMMANDS.iter().filter(|name| surface.contains(**name)).collect();
    assert!(back.is_empty(), "commands that left are registered again: {back:?}");

    let texts = printed_texts(&root);
    let helps = texts.iter().filter(|(origin, _)| origin.ends_with("--help")).count();
    assert_eq!(helps, surface.len() + 2, "every `run` command's help is swept, plus `run --help` and `help`");
    assert!(texts.len() > 2_000, "the sweep read only {} texts", texts.len());
    // A varredura enxerga o que procura: o catálogo de textos está nela.
    assert!(
        texts.iter().any(|(origin, text)| {
            // No Windows o caminho vem com a barra invertida.
            origin.replace('\\', "/").ends_with("i18n/flow.rs") && text.contains("mustard-rt run open")
        }),
        "the sweep never reads the catalog",
    );

    let found: Vec<String> = texts
        .iter()
        .flat_map(|(origin, text)| removed_names_in(text).into_iter().map(move |name| format!("{origin}: {name}")))
        .collect();
    assert!(
        found.is_empty(),
        "texts the binary prints or writes still name a command or hook that left - \
         whoever reads them is sent to something that does not exist:\n{}",
        found.join("\n")
    );
}

/// A varredura acha o nome que saiu em cada forma que o leitor toma por
/// comando, e deixa passar o que não é chamada: o comando que fica, a palavra
/// comum citada como código, o comentário e o código de teste.
#[test]
fn the_sweep_finds_each_way_a_removed_name_reaches_the_reader() {
    assert_eq!(removed_names_in("rode `mustard-rt run qa-run --spec x`"), ["qa-run"]);
    assert_eq!(removed_names_in("O `spec-draft` saiu do fluxo."), ["spec-draft"]);
    assert_eq!(removed_names_in("mustard-rt run status"), ["status"]);
    assert_eq!(removed_names_in("o gancho amend_window_inject grava"), ["amend_window_inject"]);
    for clean in ["mustard-rt run statusline", "o campo `status`", "[qa-run] aviso", "run open-spec-draft", "subagent_inject"] {
        assert!(removed_names_in(clean).is_empty(), "{clean}");
    }

    let source = concat!(
        "// \"`qa-run`\" num comentário\n",
        "fn a() -> &'static str { let _ = '\"'; let _ = b'{'; \"rode \\\n    `mustard-rt run git-settle`\" }\n",
        "const B: &str = r#\"um \"cru\" `emit-event`\"#;\n",
        "#[cfg(test)]\nconst C: &[&str] = &[\"`spec-doc`\"];\n",
        "const D: &str = \"fica\";\n",
        "#[cfg(test)]\nmod tests {\n    fn t() { let _ = \"{ `wave-done`\"; }\n}\n",
        "fn e() { spawn(&[\"run\", \"orient\"]); }\n",
    );
    let texts = rust_texts(source);
    assert_eq!(texts, ["rode `mustard-rt run git-settle`", "um \"cru\" `emit-event`", "fica", "run", "orient", "run orient"]);
    let found: Vec<&str> = texts.iter().flat_map(|text| removed_names_in(text)).collect();
    assert_eq!(found, ["git-settle", "emit-event", "orient"]);
}

/// Os comentários de um arquivo `.rs`, cada um com a sua linha, e o código que
/// sobra sem eles. Um literal de string ou de caractere nunca abre comentário,
/// e no código ele vira um par de aspas vazio.
fn comments_and_code(source: &str) -> (Vec<(usize, String)>, String) {
    let b = source.as_bytes();
    let breaks: Vec<usize> = source.match_indices('\n').map(|(at, _)| at).collect();
    let (mut comments, mut code) = (Vec::new(), Vec::new());
    let mut i = 0;
    while i < b.len() {
        let rest = &b[i..];
        let line = 1 + breaks.partition_point(|at| *at < i);
        let literal = match rest[0] {
            b'"' => string_literal(rest),
            b'b' | b'r' | b'c' if i == 0 || !(b[i - 1].is_ascii_alphanumeric() || b[i - 1] == b'_') => {
                string_literal(rest)
            }
            _ => None,
        };
        if rest.starts_with(b"//") || rest.starts_with(b"/*") {
            let len = if rest[1] == b'/' {
                rest.iter().position(|c| *c == b'\n').unwrap_or(rest.len())
            } else {
                rest.windows(2).position(|w| w == b"*/").map_or(rest.len(), |end| end + 2)
            };
            let text = String::from_utf8_lossy(&rest[..len]).into_owned();
            comments.extend(text.lines().enumerate().map(|(k, part)| (line + k, part.to_string())));
            i += len;
        } else if let Some((_, len)) = literal {
            code.extend_from_slice(b"\"\"");
            i += len;
        } else if rest[0] == b'\'' {
            code.push(b'\'');
            i += char_literal_len(rest);
        } else {
            code.push(rest[0]);
            i += 1;
        }
    }
    (comments, String::from_utf8_lossy(&code).into_owned())
}

/// Os códigos de spec que um comentário cita: o código do Mustard
/// (`MSTD-RULE-0005`), o rótulo com hífen de critério, ponto ou limite
/// (`AC-3`, `P-17`, `L-3.3`), a letra com número de regra, onda ou tarefa
/// (`R5`, `W4`, `T1.7`, `W8A-2`) e a seção com o sinal de parágrafo. O que
/// está entre crases ou aspas é dado, não citação: o formato de um código ou a
/// entrada de um teste.
fn spec_codes_in(comment: &str) -> Vec<String> {
    static QUOTED: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r#"`[^`]*`|"[^"]*"|“[^”]*”"#).expect("the quote pattern compiles"));
    // Um código começa o texto ou vem depois de um caractere que não o
    // continua: `release/2026-Q3` e `U+E0B0` não são códigos.
    static CODE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(concat!(
            r"(?:^|[^\w/.+-])(",
            r"MSTD-[A-Z]+-\d+",
            r"|(?:AC|CT|[A-Z])-(?:[A-Z]{1,2}\d*-?)?\d+(?:\.\d+)*\b",
            r"|[A-Z]+\d+[A-Z]+-\d+",
            r"|[A-Z]\d{1,2}(?:\.\d+)*\b",
            r"|§\s*\d",
            r")",
        ))
        .expect("the code pattern compiles")
    });
    let bare = QUOTED.replace_all(comment, " ");
    CODE.captures_iter(&bare).map(|c| c[1].to_string()).collect()
}

/// Os nomes de função que carregam um código de spec, como `ac8_…` ou
/// `…_t1_3_…`: uma letra (ou `ac`) com um ou dois dígitos, entre sublinhados.
fn spec_coded_fn_names(code: &str) -> Vec<String> {
    static NAME: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"\bfn\s+([A-Za-z0-9_]+)").expect("the fn pattern compiles"));
    static CODED: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?:^|_)(?:ac|[a-z])\d{1,2}(?:_\d+)*(?:_|$)").expect("the name pattern compiles")
    });
    NAME.captures_iter(code).map(|c| c[1].to_string()).filter(|n| CODED.is_match(n)).collect()
}

/// Nenhum comentário nem nome de teste do código cita código de spec: a spec
/// fica fora do git, e o código que aponta para ela não leva a lugar nenhum.
/// Cada comentário diz o comportamento em palavras.
#[test]
fn no_comment_or_test_name_cites_a_spec_code() {
    let root = repo_root();
    let mut files = Vec::new();
    for dir in ["apps", "packages"] {
        walk_files(&root.join(dir), &mut files);
    }
    let files: Vec<&PathBuf> = files.iter().filter(|f| has_extension(f, &["rs"])).collect();
    assert!(files.len() > 300, "the sweep read only {} Rust files", files.len());
    let mut found = Vec::new();
    let mut read = 0;
    for file in files {
        let shown = file.strip_prefix(&root).unwrap_or(file).display().to_string();
        let (comments, code) = comments_and_code(&read_lossy(file));
        read += comments.len();
        for (line, comment) in comments {
            for cited in spec_codes_in(&comment) {
                found.push(format!("{shown}:{line}: {cited} in {}", comment.trim()));
            }
        }
        found.extend(spec_coded_fn_names(&code).into_iter().map(|name| format!("{shown}: fn {name}")));
    }
    assert!(read > 20_000, "the sweep read only {read} comment lines");
    assert!(
        found.is_empty(),
        "comments or test names still cite a spec code - say the behaviour in words:\n{}",
        found.join("\n")
    );
}

/// A varredura acha cada forma de citar um código de spec e deixa passar o
/// dado entre crases ou aspas, as siglas comuns e o comentário dentro de um
/// texto.
#[test]
fn the_comment_sweep_finds_each_spec_code_and_lets_data_pass() {
    for (comment, cited) in [
        ("// fecha a regra (MSTD-RULE-0038)", "MSTD-RULE-0038"),
        ("/// AC-7 — installs privately", "AC-7"),
        ("// Wave-plans use AC-W4-1", "AC-W4-1"),
        ("// Spec contract AC-A-14", "AC-A-14"),
        ("// the limit L-3.3 holds", "L-3.3"),
        ("// D4: materialise the verdict", "D4"),
        ("// Layer promotion guard (T1.7)", "T1.7"),
        ("/// W8A-2 supersedes the old reader", "W8A-2"),
        ("// see the audit § 4", "§ 4"),
    ] {
        assert_eq!(spec_codes_in(comment), [cited], "{comment}");
    }
    for clean in [
        "/// like `MSTD-CRIT-0016`",
        "/// o número escrito `P-12` vira `12`",
        r#"/// um código como "MSTD-RULE-0008" e uma frase"#,
        "/// UTF-8, SHA-256, BCP-47 and ISO-8601 are not codes",
        "/// the U+E0B0 glyph and the release/2026-Q3 line",
    ] {
        assert!(spec_codes_in(clean).is_empty(), "{clean}");
    }

    let source = concat!(
        "/// T3 — um código\n",
        "fn ac8_host_is_clean() { let _ = \"// R5 num texto\"; let _ = '\"'; }\n",
        "/* bloco\n   com W4 */\n",
        "fn similarity_x1024() {}\n",
    );
    let (comments, code) = comments_and_code(source);
    assert_eq!(comments, [(1, "/// T3 — um código".to_string()), (3, "/* bloco".to_string()), (4, "   com W4 */".to_string())]);
    assert_eq!(spec_coded_fn_names(&code), ["ac8_host_is_clean"]);
}

/// Os eventos que o manifesto do Claude Code registra.
fn manifest_events(root: &Path) -> BTreeSet<String> {
    let text = read_lossy(&root.join("plugin").join("hooks").join("hooks.json"));
    let manifest: serde_json::Value = serde_json::from_str(&text).expect("hooks.json is JSON");
    manifest["hooks"].as_object().expect("hooks.json has hooks").keys().cloned().collect()
}

/// Os cortes terminaram: nenhum gancho que saiu segue registrado, o registro
/// tem só os que ficam, o manifesto só chama evento que tem gancho e todo
/// evento com gancho está no manifesto; e nenhum comando `run` fica sem
/// chamador.
#[test]
fn reverse_every_registered_name_has_a_caller_or_a_justification() {
    let root = repo_root();

    let registry = mustard_rt::registry::Registry::new();
    let mut registered = registry.ids();
    let back: Vec<&str> = registered.iter().copied().filter(|id| REMOVED_HOOKS.contains(id)).collect();
    assert!(back.is_empty(), "hooks that left are registered again: {back:?}");
    registered.sort_unstable();
    assert_eq!(registered, KEPT_HOOKS, "the registry holds exactly the hooks that stay");
    let with_hook: BTreeSet<String> =
        registry.triggers().into_iter().map(|trigger| trigger.as_event_name().to_string()).collect();
    assert_eq!(
        manifest_events(&root),
        with_hook,
        "an entry of hooks.json calls an event with no hook, or a hook has no entry that calls it",
    );

    let instructed: BTreeSet<String> = reverse_prose_corpus(&root)
        .iter()
        .flat_map(|p| extract_run_names(&read_lossy(p)))
        .collect();
    let mut dark = Vec::new();
    for name in surface_names() {
        if instructed.contains(&name) || has_argv_caller(&root, &name) {
            continue;
        }
        dark.push(name);
    }
    assert!(
        dark.is_empty(),
        "registered `run` subcommands with no product caller (templates, CLI \
         sources, installer, settings template, rt argv spawns) - dark \
         surface. Wire a caller or remove the registration:\n{}",
        dark.join("\n")
    );
}

/// Every long flag the binary declares is spelled by some product prose, or
/// carries a justified [`FLAG_WHITELIST`] row.
///
/// The REVERSE of `forward_every_instructed_flag_is_declared`, and the half that
/// was blind. That one asks whether a flag a file TYPES exists; this one asks
/// whether a flag that EXISTS is ever typed. An undocumented flag does not
/// break — it simply never gets used, which is how `--explains-symptom`,
/// `--allow-no-qa` and `--no-fetch` could sit in the surface with no reader able
/// to learn they were there.
///
/// Citation is loose ON PURPOSE: the flag has to appear somewhere in the corpus,
/// not necessarily on its own command's invocation. Whether a flag is typed on
/// the RIGHT command is the forward test's question, and asking it twice would
/// report one defect as two. What this measures is narrower and it is the thing
/// nothing measured: can a reader find out the flag exists at all.
#[test]
fn reverse_every_declared_flag_is_documented() {
    let root = repo_root();
    let spelled = spelled_long_flags(&root);
    let tree = run_command_tree();
    let mut dark = Vec::new();
    for cmd in tree.get_subcommands() {
        let name = cmd.get_name();
        if name == "help" {
            continue;
        }
        for flag in declared_long_flags(cmd) {
            if spelled.contains(flag)
                || FLAG_WHITELIST.iter().any(|(c, f, _)| *c == name && *f == flag)
            {
                continue;
            }
            dark.push(format!("run {name} --{flag}"));
        }
    }
    assert!(
        dark.is_empty(),
        "declared `run` flags no product file ever spells - they ship, and the \
         only way to learn one exists is to read `--help` of a command the docs \
         do not show. Document the flag, drop it, or add a JUSTIFIED \
         FLAG_WHITELIST row:\n{}",
        dark.join("\n")
    );
}

/// The flag whitelist stays sorted, stays real, and stays necessary.
#[test]
fn flag_whitelist_stays_sorted_live_and_not_redundant() {
    let root = repo_root();
    let spelled = spelled_long_flags(&root);
    let tree = run_command_tree();

    for pair in FLAG_WHITELIST.windows(2) {
        assert!(
            (pair[0].0, pair[0].1) < (pair[1].0, pair[1].1),
            "FLAG_WHITELIST must stay sorted: ({}, {}) before ({}, {})",
            pair[0].0,
            pair[0].1,
            pair[1].0,
            pair[1].1
        );
    }
    for (name, flag, justification) in FLAG_WHITELIST {
        let cmd = tree
            .get_subcommands()
            .find(|c| c.get_name() == *name)
            .unwrap_or_else(|| panic!("FLAG_WHITELIST names `run {name}`, which is not registered"));
        assert!(
            declared_long_flags(cmd).contains(*flag),
            "FLAG_WHITELIST names `run {name} --{flag}`, which that command no \
             longer declares - drop the row"
        );
        assert!(
            !justification.trim().is_empty(),
            "FLAG_WHITELIST entry `run {name} --{flag}` carries no justification"
        );
        assert!(
            !spelled.contains(*flag),
            "FLAG_WHITELIST entry `run {name} --{flag}` IS spelled in product \
             prose now - the row is redundant, drop it"
        );
    }
}

/// O mapa do início da sessão manda toda página mostrada ao usuário passar
/// pelo `page`, escrita em markdown, e ser publicada no claude.ai, nos dois
/// idiomas.
///
/// Lido do texto que o binário embute e grava no projeto, e conferido pelo
/// mesmo extrator da catraca: a chamada tem de ser uma invocação de verdade,
/// não o nome solto na prosa. Confere o fato, nunca a frase.
#[test]
fn the_session_map_sends_every_page_through_the_page_command() {
    for text in [mustard_core::platform::i18n::Locale::PtBr, mustard_core::platform::i18n::Locale::EnUs] {
        let map = mustard_core::session_map(text);
        let invocations = extract_run_invocations(map);
        assert!(
            invocations.iter().any(|inv| inv.name == "page"),
            "the {text} session map never tells the reader to run `mustard-rt run page`"
        );
        assert!(map.contains("claude.ai"), "the {text} session map never says the page is published on claude.ai");
        assert!(map.contains("markdown"), "the {text} session map never says a page is written in markdown");
    }
}
