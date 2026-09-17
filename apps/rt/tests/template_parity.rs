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

use clap::{Command, Subcommand};
use mustard_rt::commands::flow::resume::{next_command, NEXT_BY_PHASE};
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
const FLAG_WHITELIST: &[(&str, &str, &str)] = &[

    (
        "clean",
        "apply",
        "o interruptor que apaga de verdade: sem ele o comando só lista, e é \
         essa a leitura que a prosa do agente ensina. A ajuda do próprio \
         comando diz que o padrão é listar",
    ),
    (
        "close",
        "report",
        "the report of the last round, handed back by the same flow prose that \
         will call the close; a close without it only closes what is already \
         recorded, which is what the bare command does",
    ),
    (
        "discard",
        "remote",
        "keeps the server branch, which belongs to everyone: without it only the \
         local branch goes, and a team that does not allow deleting a branch \
         never needs to know the flag exists",
    ),
    (
        "grill",
        "condensed",
        "the one-sentence request of the flow's survey step \
         (commands/flow/grill.rs); the \
         flow prose calling grill is rewritten together with the rest of the flow",
    ),
    (
        "grill",
        "kinds",
        "the work type of the flow's survey step (commands/flow/grill.rs), \
         asked back by its own refusal; a work type that the project declares \
         is not a value the prose can spell",
    ),
    (
        "pr-open",
        "fill",
        "o caminho do submódulo, que não tem spec própria: título e corpo saem \
         dos commits. A prosa da porta ensina o caminho com spec, que é o de \
         todo dia; este é o do repositório sem spec",
    ),
    (
        "round",
        "report",
        "the report of the previous round, handed back by the same flow prose \
         that will call the round; a round without it only dispatches, which is \
         what the bare command does",
    ),
    (
        "statusline",
        "preview",
        "quem chama a barra de status é o Claude Code, não o assistente; o \
         `--preview` é a forma de um operador ver a linha uma vez, e a ajuda \
         do comando a descreve",
    ),
];

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
/// registrado, e a opção que ele já vem escrita, declarada.
///
/// A ida anda pelos arquivos do repositório, e esta instrução não mora em
/// arquivo nenhum: o binário a monta na hora e a entrega no campo `command` da
/// resposta, de onde quem conduz a conversa a copia e roda. Um renome do
/// comando, ou a opção `--spec` deixando de ser declarada, entrega um passo que
/// morre em `error: unexpected argument` e código 2 — e nada no repositório
/// teria como acusar, porque nenhum texto do produto escreve essa linha.
///
/// A instrução conferida é a que o próprio binário monta, nunca uma cópia do
/// formato dela escrita aqui: um teste que remontasse a linha à mão conferiria
/// a própria cópia e continuaria verde depois de a montagem mudar.
#[test]
fn o_campo_do_proximo_passo_passa_pela_mesma_catraca() {
    let tree = run_command_tree();
    assert!(!NEXT_BY_PHASE.is_empty(), "a tabela do próximo passo está vazia");

    let mut offenders = Vec::new();
    for (fase, _) in NEXT_BY_PHASE {
        let montado = next_command(fase, "alguma-spec");
        let Some(instrucao) = montado.as_str() else {
            offenders.push(format!("a fase `{fase}` está na tabela e não monta comando nenhum"));
            continue;
        };
        let mut invocacoes = extract_run_invocations(instrucao);
        let Some(inv) = invocacoes.pop() else {
            offenders.push(format!("a fase `{fase}` monta `{instrucao}`, que não é uma chamada de `mustard-rt run`"));
            continue;
        };
        let Some(cmd) = tree.get_subcommands().find(|c| c.get_name() == inv.name) else {
            offenders.push(format!("a fase `{fase}` manda rodar `run {}`, que não é registrado", inv.name));
            continue;
        };
        let declaradas = declared_long_flags(cmd);
        for flag in inv.flags {
            if !declaradas.contains(flag.as_str()) {
                offenders.push(format!(
                    "a fase `{fase}` manda rodar `run {} --{flag}`, que esse comando não declara",
                    inv.name
                ));
            }
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

/// O revisor e o agente de onda aprendem, pela própria instrução, a
/// compilar a cópia descartável na compilação compartilhada e a apagá-la pela
/// porta `clean --path`, nunca pela exclusão recursiva que a trava nega.
///
/// O caminho escrito na prosa é conferido contra o do código
/// ([`shared_target_dir`](mustard_rt::commands::maint::scratch_gc::shared_target_dir)):
/// se um mudar sem o outro, as cópias passam a compilar num lugar que a porta
/// não mede nem esvazia. O roteiro do agente de onda é conferido nos DOIS
/// blocos — o de despacho e o de nova tentativa —, porque o agente que refaz
/// uma onda também compila.
#[test]
fn review_agent_teaches_shared_target_and_scratch_gc() {
    const SHARED_TARGET: &str = "CARGO_TARGET_DIR=\"$HOME/.cache/mustard/scratch-target\"";
    const CLEANUP: &str = "mustard-rt run clean --path \"$D\"";

    let code = mustard_rt::commands::maint::scratch_gc::shared_target_dir()
        .expect("the home directory resolves in the test environment");
    assert!(
        code.ends_with(".cache/mustard/scratch-target"),
        "the prose names $HOME/.cache/mustard/scratch-target; the code builds {}",
        code.display()
    );

    let root = repo_root();
    let review = read_lossy(&root.join("plugin/agents/mustard-review.md"));
    assert!(review.contains(SHARED_TARGET), "the reviewer must build scratch copies in the shared target");
    assert!(review.contains(CLEANUP), "the reviewer must remove its scratch copy through clean --path");

    let _ = root;
}

/// A regra injetada do material manda toda página mostrada ao usuário
/// passar pelo `page`, escrita em markdown, e ser publicada no claude.ai, e o
/// endereço da página da spec ser gravado como evento, pela porta `write`.
///
/// Lida do template que o binário embute e conferida pelo mesmo extrator da
/// catraca: a chamada tem de ser uma invocação de verdade, não o nome solto na
/// prosa. Confere o fato, nunca a frase — prosa se reescreve.
#[test]
fn material_rule_sends_every_page_through_the_page_command() {
    let material = read_lossy(&repo_root().join("packages/core/templates/mustard/material.md"));
    let invocations = extract_run_invocations(&material);
    assert!(
        invocations
            .iter()
            .any(|inv| inv.name == "page" && inv.flags.iter().any(|f| f == "body")),
        "the material rule never tells the reader to run `mustard-rt run page --body <page.md>`"
    );
    assert!(
        !invocations.iter().any(|inv| inv.name == "doc-page"),
        "the material rule still names the old `doc-page`"
    );
    assert!(
        material.contains("claude.ai"),
        "the material rule never says the page is published on claude.ai"
    );
    assert!(
        invocations
            .iter()
            .any(|inv| inv.name == "write" && inv.flags.iter().any(|f| f == "json")),
        "a regra do material não manda gravar o endereço publicado como evento, \
         pela porta `write publish --json`"
    );
}
