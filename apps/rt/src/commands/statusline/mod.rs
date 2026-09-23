//! `mustard-rt run statusline` — render the Claude Code status bar.
//!
//! Reads the harness payload JSON from stdin and prints the bar on stdout.
//! On any failure (bad JSON, missing fields, panicking I/O) we print
//! `Claude` and exit cleanly — the harness must never see a non-zero exit.
//!
//! O que a barra mostra, cada dado uma vez e no idioma do projeto: na
//! primeira linha, o nome do projeto como link da página dele, a branch, a
//! spec (o nome só quando a branch não termina com ele; o link da página dela
//! fica no nome ou, sem o nome, na fase), a fase e o andamento das ondas como
//! contagem ("1 de 4 ondas", entregues de total), o uso da conversa num número
//! só (o já usado, o mesmo da barrinha), o tempo (`5h49m`) e o aviso vermelho
//! de quando o Mustard está desligado; na segunda, a versão do Mustard
//! (`Mustard 0.2.0`, só num projeto com o Mustard), a economia do rtk
//! (`rtk poupou 64%`) e o modelo. O custo não aparece: na assinatura ele é só
//! estimativa. A contagem de linhas mudadas (`+156-23`) também não: o exemplo
//! aprovado da barra não a traz. A versão do Claude Code o próprio Claude Code
//! já mostra.
//!
//! Submodules:
//! - [`segment`] — pure data ([`segment::Segment`]) and per-kind builders.
//! - [`theme`]   — palette / separator / `render_line`.
//! - [`preview`] — handler for the `--preview` flag.
//!
//! Theme selection: see [`theme::ENV_VAR`]. Default = `catppuccin` (powerline,
//! requires a Nerd Font). Users without Nerd Font set
//! `MUSTARD_STATUSLINE_THEME=default`.

pub mod cli;

pub mod preview;
pub mod segment;
// `theme` stays crate-internal: the module became `pub` when the `run` CLI
// split moved `StatuslineCmd` into `statusline::cli`, and its public items
// (`ThemeId::theme`, `render_line`, `DEFAULT`) hand out the crate-private
// `Theme` type - capping the module keeps that honest without leaking it.
pub(crate) mod theme;

use crate::shared::rtk_gain::{get_rtk_gain, RtkGain};
use segment::{
    compact_segment, context_segment, duration_segment, git_segment, inert_segment, model_segment, module_segment,
    mustard_segment, savings_segment, unit_segment, Segment,
};
use serde_json::Value;
use std::io::Read;
use std::path::PathBuf;
use theme::{render_line, Theme, ThemeId};

/// Build the ordered segment list from the parsed payload and the `rtk gain`
/// reading (`gain`, taken by the caller: it is the one subprocess the bar
/// does not own).
///
/// Builders that return `None` (zero duration, no git, etc.) are quietly
/// skipped, so the line stays compact when state is sparse.
///
/// `machine` é o valor de `CLAUDE_AUTOCOMPACT_PCT_OVERRIDE`, lido pelo
/// chamador (`render`, uma vez, do ambiente real do processo) e recebido
/// aqui como parâmetro — nunca lido direto nesta função — para o teste poder
/// variá-lo sem tocar o ambiente, que nesta máquina já traz a variável
/// definida.
fn build_segments(data: &Value, gain: Option<&RtkGain>, machine: Option<&str>) -> Vec<Segment> {
    let cwd: PathBuf = data
        .get("workspace")
        .and_then(|w| w.get("current_dir"))
        .or_else(|| data.get("cwd"))
        .and_then(Value::as_str)
        .map_or_else(
            || std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            PathBuf::from,
        );

    // A branch é lida uma vez: o segmento dela a mostra, e o da spec a
    // compara com o nome da spec.
    let branch = mustard_core::current_branch(&cwd);
    let mustard = mustard_core::ProjectConfig::exists(&cwd);
    let lang = mustard_core::ProjectConfig::load(&cwd).language().text_or_default();

    // A primeira linha: onde o trabalho está e o que a sessão gastou.
    let mut segs = vec![module_segment(&cwd)];
    segs.extend(git_segment(&cwd, branch.as_deref()));
    segs.extend(unit_segment(&cwd, branch.as_deref()));
    segs.extend(context_segment(data));
    segs.extend(duration_segment(data));
    // Vermelho: o plugin está desligado, e nenhuma trava roda. Sem ele, esse
    // estado parece com um saudável em todo o resto da barra.
    segs.extend(inert_segment(&cwd));
    // A segunda linha: a versão do Mustard, a economia do rtk e o modelo.
    if mustard {
        segs.push(mustard_segment());
    }
    segs.extend(savings_segment(gain, lang));
    segs.extend(compact_segment(data, machine, lang));
    segs.push(model_segment(data));
    segs
}

/// A linha de cada segmento: a versão do Mustard, a economia do rtk e o
/// modelo vão para a segunda; todo o resto, para a primeira.
///
/// O Claude Code mostra uma linha por linha impressa (documentado), então são
/// dois `println!`, e não um truque de desenho. Uma linha sem segmento nenhum
/// não é impressa.
const fn is_place_row(kind: segment::SegmentKind) -> bool {
    use segment::SegmentKind as K;
    !matches!(kind, K::Mustard | K::Savings | K::Compact | K::Model)
}

/// Render the statusline from a parsed payload: one row per non-empty group,
/// no trailing newline. A row whose segments are all absent is dropped rather
/// than printed blank — with a sparse payload the bar stays a single line, the
/// shape it had before this split.
fn render(data: &Value) -> Vec<String> {
    let machine = std::env::var("CLAUDE_AUTOCOMPACT_PCT_OVERRIDE").ok();
    rows(ThemeId::from_env().theme(), build_segments(data, get_rtk_gain().as_ref(), machine.as_deref()))
}

/// The rows of `segs` in `theme`: the partition by [`is_place_row`], one
/// rendered line per non-empty group.
fn rows(theme: &Theme, segs: Vec<Segment>) -> Vec<String> {
    let (place, spend): (Vec<Segment>, Vec<Segment>) = segs.into_iter().partition(|s| is_place_row(s.kind));

    [place, spend]
        .into_iter()
        .filter(|group| !group.is_empty())
        .map(|group| render_line(theme, &group))
        .filter(|line| !line.is_empty())
        .collect()
}

/// Dispatch `mustard-rt run statusline`.
///
/// `preview = true` short-circuits to the [`preview`] handler (no stdin read,
/// no JSON parse). `preview = false` does the live render.
pub fn run(preview: bool) {
    if preview {
        preview::run();
        return;
    }
    let mut buf = String::new();
    if std::io::stdin().read_to_string(&mut buf).is_err() {
        println!("Claude");
        return;
    }
    match serde_json::from_str::<Value>(&buf) {
        Ok(data) => {
            for line in render(&data) {
                println!("{line}");
            }
        }
        Err(_) => println!("Claude"),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::spec_state::seed_event;
    use serde_json::json;
    use std::path::Path;

    /// A full payload renders TWO rows, split by role.
    ///
    /// This replaces a test that pinned the bar to one row. That pin recorded a
    /// 2026-05-21 decision to drop a pipeline BANNER — a second line that
    /// repeated state the bar already showed. What is here now is not that: the
    /// same segments, redistributed, because one row had grown to 171 visible
    /// characters and terminals cut the tail. Claude Code documents one row per
    /// printed line, so two rows is a supported layout, not a workaround.
    #[test]
    fn a_full_payload_renders_two_rows_split_by_role() {
        let data = json!({
            "workspace": { "current_dir": ".", "project_dir": "." },
            "model": { "display_name": "Opus 4.7" },
            "version": "2.1.146",
            "cost": {
                "total_duration_ms": 1000,
                "total_lines_added": 10,
                "total_lines_removed": 2,
                "total_cost_usd": 0.42
            },
            "context_window": {
                "remaining_percentage": 70,
                "total_input_tokens": 50000,
                "total_output_tokens": 10000
            }
        });
        let lines = render(&data);
        assert_eq!(lines.len(), 2, "place row + spend row — got {lines:?}");
        assert!(lines.iter().all(|l| !l.is_empty()), "no blank row is printed: {lines:?}");

        let (first, second) = (&lines[0], &lines[1]);
        assert!(first.contains("30%"), "the used share of the conversation goes on the first row: {first}");
        assert!(second.contains("Opus 4.7"), "the model goes on the second: {second}");
        for gone in ["$0.42", "60k", "2.1.146", "+10-2"] {
            assert!(
                !first.contains(gone) && !second.contains(gone),
                "no cost, token total, Claude Code version or changed lines ({gone}): {lines:?}"
            );
        }
    }

    /// O texto que o terminal mostra: a sequência do link sai, o rótulo fica.
    fn visible(text: &str) -> String {
        let mut out = String::new();
        let mut rest = text;
        while let Some(start) = rest.find('\u{1b}') {
            out.push_str(&rest[..start]);
            let tail = &rest[start..];
            let end = if tail.starts_with("\u{1b}]8;") {
                tail.find("\u{1b}\\").map(|i| i + 2)
            } else {
                tail.find('m').map(|i| i + 1)
            };
            let Some(end) = end else { return out };
            rest = &tail[end..];
        }
        out.push_str(rest);
        out
    }

    fn link(url: &str, label: &str) -> String {
        format!("\u{1b}]8;;{url}\u{1b}\\{label}\u{1b}]8;;\u{1b}\\")
    }

    /// Um repositório em `root` na branch `branch`, com um commit, o Mustard
    /// (no idioma `lang`) e o que ele grava fora do git, como num projeto
    /// real, e um arquivo novo fora do git (`?1`).
    fn project_on_branch(root: &Path, branch: &str, lang: &str) {
        std::fs::create_dir_all(root).unwrap();
        let git = |args: &[&str]| assert!(mustard_core::platform::git::run(root, args).ok, "git {args:?}");
        git(&["init", "-q", "."]);
        git(&["symbolic-ref", "HEAD", &format!("refs/heads/{branch}")]);
        git(&["-c", "user.email=t@t", "-c", "user.name=t", "-c", "commit.gpgsign=false",
            "commit", "-q", "--allow-empty", "-m", "semente"]);
        std::fs::write(root.join(".git").join("info").join("exclude"), "mustard.json\n.claude/\n").unwrap();
        std::fs::write(root.join("notas.txt"), "rascunho\n").unwrap();
        std::fs::write(root.join("mustard.json"), format!(r#"{{"version":"0.0.1-velha","language":{{"text":"{lang}"}}}}"#))
            .unwrap();
    }

    /// Os dados que o Claude Code manda à barra numa sessão de verdade, como
    /// no exemplo aprovado: 5h49m de sessão, 24% da conversa usados, passado
    /// dos 200 mil tokens, com custo e com 156 linhas acrescentadas e 23
    /// tiradas no repositório.
    fn example_payload(root: &Path) -> Value {
        let dir = root.to_string_lossy();
        json!({
            "hook_event_name": "Status",
            "session_id": "0f6c2d9e-5b1a-4c3e-9d7f-2a8b4e6c1d3f",
            "transcript_path": format!("{dir}/.claude/transcript.jsonl"),
            "cwd": dir,
            "model": { "id": "claude-opus-5[1m]", "display_name": "Opus 5 (1M context)" },
            "workspace": { "current_dir": dir, "project_dir": dir },
            "version": "2.1.267",
            "output_style": { "name": "default" },
            "exceeds_200k_tokens": true,
            "cost": {
                "total_cost_usd": 12.5,
                "total_duration_ms": (5 * 3600 + 49 * 60 + 12) * 1000,
                "total_api_duration_ms": 2_310_000,
                "total_lines_added": 156,
                "total_lines_removed": 23
            },
            "context_window": {
                "total_input_tokens": 230_000,
                "total_output_tokens": 10_000,
                "context_window_size": 1_000_000,
                "used_percentage": 24,
                "remaining_percentage": 76,
                "current_usage": {
                    "input_tokens": 8_500,
                    "output_tokens": 1_200,
                    "cache_creation_input_tokens": 5_000,
                    "cache_read_input_tokens": 215_300
                }
            }
        })
    }

    const GAIN: RtkGain = RtkGain { saved: 356_500_000, pct: 64.2 };

    /// The two rows as the terminal shows them in the `minimal` theme, without
    /// the red warning of a switched-off plugin: that one reads this machine's
    /// plugin settings, not the project. `machine` (`None` here) keeps these
    /// exact-match tests deterministic no matter what this shell's own
    /// `CLAUDE_AUTOCOMPACT_PCT_OVERRIDE` is set to — the compact segment gets
    /// its own tests, below.
    fn shown_rows(data: &Value) -> Vec<String> {
        let mut segs = build_segments(data, Some(&GAIN), None);
        segs.retain(|s| s.kind != segment::SegmentKind::Inert);
        rows(ThemeId::Minimal.theme(), segs).iter().map(|line| visible(line)).collect()
    }

    /// `shown_rows`, mas com a fatia de compactação da máquina explícita —
    /// para as duas linhas próprias do ponto de corte, abaixo.
    fn shown_rows_with_machine(data: &Value, machine: &str) -> Vec<String> {
        let mut segs = build_segments(data, Some(&GAIN), Some(machine));
        segs.retain(|s| s.kind != segment::SegmentKind::Inert);
        rows(ThemeId::Minimal.theme(), segs).iter().map(|line| visible(line)).collect()
    }

    /// A barra do exemplo aprovado, na sessão numa branch cujo nome termina
    /// com o nome da spec, com todos os dados que a sessão real traz: o nome
    /// da spec não se repete, a fase sai no idioma do projeto e leva o link da
    /// página da spec, o uso da conversa é um número só (o já usado), o tempo
    /// sai em horas e minutos, e não aparecem o total de tokens, o aviso de
    /// 200 mil, o custo nem a contagem de linhas mudadas (+156-23), que a
    /// sessão traz. A primeira linha tem só projeto, branch, spec, uso da
    /// conversa e tempo.
    #[test]
    fn statusline_draws_the_approved_example_with_the_project_and_spec_links() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("portal-florestal-backend");
        project_on_branch(&root, "feature/pi-kpis-plantio", "pt-BR");
        let project_url = "https://claude.ai/code/artifacts/projeto-portal";
        let spec_url = "https://claude.ai/code/artifacts/spec-pi-kpis-plantio";
        seed_event(&root, "pi-kpis-plantio", "state", json!({"phase": "survey", "branch": "feature/pi-kpis-plantio"}));
        seed_event(&root, "pi-kpis-plantio", "publish",
            json!({"page": "spec", "milestone": "round", "ok": true, "url": spec_url}));
        std::fs::write(
            root.join(".claude").join("spec").join("index.ndjson"),
            format!("{}\n", mustard_core::domain::spec_index::project_line(Some(project_url))),
        )
        .unwrap();
        let data = example_payload(&root);
        assert_eq!((data["cost"]["total_lines_added"].as_i64(), data["cost"]["total_lines_removed"].as_i64()), (Some(156), Some(23)));

        let segs = build_segments(&data, Some(&GAIN), None);
        let text = |kind: segment::SegmentKind| {
            segs.iter().find(|s| s.kind == kind).map(|s| s.text.clone()).unwrap_or_else(|| panic!("no {kind:?}: {segs:?}"))
        };
        assert_eq!(text(segment::SegmentKind::Module), link(project_url, "portal-florestal-backend"), "the project page link");
        assert_eq!(text(segment::SegmentKind::Unit), format!("\u{25b8} {}", link(spec_url, "levantamento")), "the phase carries the spec link");

        let rows = shown_rows(&data);
        assert_eq!(
            rows,
            vec![
                "portal-florestal-backend  \u{2387} feature/pi-kpis-plantio ?1  \u{25b8} levantamento  \
                 \u{2588}\u{2588}\u{2591}\u{2591}\u{2591}\u{2591}\u{2591}\u{2591}\u{2591}\u{2591} 24%  5h49m"
                    .to_string(),
                format!("Mustard {}  \u{26A1} rtk poupou 64%  Opus 5 (1M context)", mustard_core::harness_version()),
            ],
        );
        for count in ["+156", "-23"] {
            assert!(rows.iter().all(|row| !row.contains(count)), "no count of changed lines ({count}): {rows:?}");
        }
    }

    /// Num projeto com o Mustard, a segunda linha começa com "Mustard" e a
    /// versão que roda, e a economia do rtk sai no idioma do projeto; fora de
    /// um projeto com o Mustard, a versão não aparece.
    #[test]
    fn statusline_second_row_starts_with_the_mustard_version_and_the_rtk_savings_in_the_project_language() {
        let version = mustard_core::harness_version();
        let dir = tempfile::tempdir().unwrap();
        for (lang, savings) in [("pt-BR", "rtk poupou 64%"), ("en-US", "rtk saved 64%")] {
            let root = dir.path().join(format!("loja-{lang}"));
            project_on_branch(&root, "dev", lang);
            let rows = shown_rows(&example_payload(&root));
            assert_eq!(rows.len(), 2, "{rows:?}");
            assert_eq!(rows[1], format!("Mustard {version}  \u{26A1} {savings}  Opus 5 (1M context)"), "{lang}");
            assert!(!rows[0].contains("Mustard"), "the version is not on the first row: {rows:?}");
        }

        let bare = dir.path().join("sem-mustard");
        project_on_branch(&bare, "dev", "pt-BR");
        std::fs::remove_file(bare.join("mustard.json")).unwrap();
        let rows = shown_rows(&example_payload(&bare));
        assert_eq!(rows[1], "\u{26A1} rtk poupou 64%  Opus 5 (1M context)", "no Mustard, no version: {rows:?}");
    }

    /// O ponto de corte e a distância entram na segunda linha, depois da
    /// economia do rtk e antes do modelo, no formato `compacta em Nk -
    /// faltam Nk`: a fatia da máquina (aqui, 25) vezes a janela do modelo
    /// (aqui, 1 milhão, porque o nome do exemplo cita "1M") dá o ponto de
    /// corte; ele menos os tokens já usados (230 mil + 10 mil, do exemplo
    /// aprovado) dá quanto falta.
    #[test]
    fn a_segunda_linha_mostra_o_ponto_de_corte_e_quanto_falta() {
        let version = mustard_core::harness_version();
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("loja");
        project_on_branch(&root, "dev", "pt-BR");
        let rows = shown_rows_with_machine(&example_payload(&root), "25");
        assert_eq!(
            rows[1],
            format!("Mustard {version}  \u{26A1} rtk poupou 64%  compacta em 250k - faltam 10k  Opus 5 (1M context)"),
            "{rows:?}"
        );
    }

    /// Sem a variável de compactação definida na máquina, a segunda linha
    /// fica exatamente como era antes desta onda — nenhum ponto de corte
    /// inventado, e a barra não perde nem ganha nada além disso.
    #[test]
    fn sem_a_fatia_na_maquina_a_segunda_linha_nao_muda() {
        let version = mustard_core::harness_version();
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("loja");
        project_on_branch(&root, "dev", "pt-BR");
        let rows = shown_rows(&example_payload(&root));
        assert_eq!(
            rows[1],
            format!("Mustard {version}  \u{26A1} rtk poupou 64%  Opus 5 (1M context)"),
            "{rows:?}"
        );
    }

    /// Sem o endereço da página do projeto — a lista das specs falta, está
    /// ilegível ou não o traz —, o nome do projeto sai sem link, e o resto da
    /// barra sai igual ao de quando o endereço existe, sem erro.
    #[test]
    fn statusline_without_the_project_address_draws_the_name_plain_and_the_rest_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let index_line = |url: Option<&str>| format!("{}\n", mustard_core::domain::spec_index::project_line(url));
        let states: [(&str, Option<Vec<u8>>); 5] = [
            ("com-endereco", Some(index_line(Some("https://claude.ai/code/artifacts/projeto")).into_bytes())),
            ("sem-lista", None),
            ("lista-ilegivel", Some(b"{ isto nao e json\n".to_vec())),
            ("lista-binaria", Some(vec![0xff, 0xfe, 0x00, 0x9f])),
            ("lista-sem-endereco", Some(index_line(None).into_bytes())),
        ];
        let mut seen: Vec<Vec<String>> = Vec::new();
        for (name, index) in states {
            let root = dir.path().join(name).join("loja");
            project_on_branch(&root, "feature/checkout", "pt-BR");
            seed_event(&root, "checkout", "state", json!({"phase": "plan", "branch": "feature/checkout"}));
            if let Some(bytes) = index {
                std::fs::write(root.join(".claude").join("spec").join("index.ndjson"), bytes).unwrap();
            }
            let data = example_payload(&root);
            let module = build_segments(&data, Some(&GAIN), None)
                .into_iter()
                .find(|s| s.kind == segment::SegmentKind::Module)
                .expect("the project name is always on the bar");
            if name == "com-endereco" {
                assert_eq!(module.text, link("https://claude.ai/code/artifacts/projeto", "loja"), "{name}");
            } else {
                assert_eq!(module.text, "loja", "no address, no link ({name})");
            }
            seen.push(shown_rows(&data));
        }
        assert!(seen.iter().all(|rows| rows == &seen[0]), "the rest of the bar is the same: {seen:#?}");
        assert!(seen[0][0].starts_with("loja  \u{2387} feature/checkout ?1  \u{25b8} plano  "), "{:?}", seen[0]);
    }

    /// Retrato da barra com uma spec em execução na onda 2 de 4, na branch
    /// dela: aparecem o link da página do projeto, a branch, a fase com o link
    /// da página da spec (o nome já está na branch) e o andamento como
    /// contagem — "1 de 4 ondas", uma entregue das quatro, nos dois idiomas, e
    /// nunca o número da onda que vem. Da versão, só a do Mustard que roda;
    /// nem a gravada no projeto, nem a do Claude Code.
    #[test]
    fn a_session_with_a_spec_running_wave_two_of_four_shows_the_links_the_phase_and_the_progress() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("loja");
        std::fs::create_dir_all(&root).unwrap();
        let git = |args: &[&str]| assert!(mustard_core::platform::git::run(&root, args).ok, "git {args:?}");
        git(&["init", "-q", "."]);
        git(&["symbolic-ref", "HEAD", "refs/heads/feature/checkout"]);
        git(&["-c", "user.email=t@t", "-c", "user.name=t", "-c", "commit.gpgsign=false",
            "commit", "-q", "--allow-empty", "-m", "semente"]);
        std::fs::write(root.join("mustard.json"), r#"{"version":"0.0.1-velha","language":{"text":"pt-BR"}}"#).unwrap();
        let project_url = "https://claude.ai/code/artifacts/projeto-loja";
        let spec_url = "https://claude.ai/code/artifacts/spec-checkout";
        let said = seed_event(&root, "checkout", "message", json!({"author": "user", "text": "o plano"}));
        seed_event(&root, "checkout", "state", json!({"phase": "plan", "branch": "feature/checkout", "base": "dev"}));
        for n in 1..=4 {
            seed_event(&root, "checkout", "wave", json!({"n": n, "text": format!("Onda {n}."), "criteria": [said],
                "done_when": "x", "origin": said}));
        }
        crate::shared::spec_state::approve_in(&root.join(".claude").join("spec").join("checkout"));
        seed_event(&root, "checkout", "state", json!({"phase": "running", "author": "binary"}));
        seed_event(&root, "checkout", "delivered", json!({"wave": 1, "text": "Pronta.", "files": ["a.rs"], "author": "wave"}));
        seed_event(&root, "checkout", "publish",
            json!({"page": "spec", "milestone": "round", "ok": true, "url": spec_url}));
        std::fs::write(
            root.join(".claude").join("spec").join("index.ndjson"),
            format!("{}\n", mustard_core::domain::spec_index::project_line(Some(project_url))),
        )
        .unwrap();

        let data = serde_json::json!({
            "workspace": { "current_dir": root.to_string_lossy() },
            "model": { "display_name": "Claude Opus 5" },
            "version": "2.1.267",
        });
        let segs = build_segments(&data, None, None);
        let text = |kind: segment::SegmentKind| {
            segs.iter().find(|s| s.kind == kind).map(|s| s.text.clone()).unwrap_or_else(|| panic!("no {kind:?}: {segs:?}"))
        };
        assert_eq!(text(segment::SegmentKind::Module), link(project_url, "loja"), "the project page link");
        assert!(text(segment::SegmentKind::Git).starts_with("\u{2387} feature/checkout"), "the branch");
        assert_eq!(
            text(segment::SegmentKind::Unit),
            format!("\u{25b8} {} 1 de 4 ondas", link(spec_url, "em execução")),
            "the phase as the spec link and the count of delivered waves"
        );

        let lines: Vec<String> = render(&data).iter().map(|line| visible(line)).collect();
        let shown = lines.join("\n");
        for expected in ["loja", "feature/checkout", "\u{25b8} em execução 1 de 4 ondas", "Opus 5"] {
            assert!(shown.contains(expected), "{expected} is on the bar: {shown}");
        }
        assert!(shown.contains(&format!("Mustard {}", mustard_core::harness_version())), "the running Mustard: {shown}");
        assert!(!shown.contains("onda 2"), "the number of the next wave stays in the resume line: {shown}");
        for version in ["2.1.267", "v2.1", "0.0.1-velha"] {
            assert!(!shown.contains(version), "no other version on the bar ({version}): {shown}");
        }
        assert!(!shown.contains('\u{2702}'), "no branch count to prune: {shown}");

        // Em inglês, a mesma contagem.
        std::fs::write(root.join("mustard.json"), r#"{"version":"0.0.1-velha","language":{"text":"en-US"}}"#).unwrap();
        let english = segment::unit_segment(&root, Some("feature/checkout")).expect("the spec is on the bar");
        assert_eq!(english.text, format!("\u{25b8} {} 1 of 4 waves", link(spec_url, "running")));
    }

    /// Every segment kind lands on exactly one row — a kind added later without
    /// a home would silently vanish from the bar, which is the failure mode a
    /// partition invites.
    #[test]
    fn every_rendered_segment_reaches_a_row() {
        let data = json!({
            "workspace": { "current_dir": ".", "project_dir": "." },
            "model": { "display_name": "Opus 4.7" },
            "version": "2.1.146",
            "cost": { "total_cost_usd": 0.42, "total_duration_ms": 1000 }
        });
        let built = build_segments(&data, None, None);
        let placed = built.iter().filter(|s| is_place_row(s.kind)).count();
        let spent = built.iter().filter(|s| !is_place_row(s.kind)).count();
        assert_eq!(placed + spent, built.len(), "the partition is total by construction");
        assert!(placed > 0 && spent > 0, "a full payload feeds both rows");
    }

    /// A sparse payload keeps the bar at ONE row: an empty group is dropped, not
    /// printed blank. This is what the split must not cost — a fresh session
    /// with no numbers yet should look exactly as it did before.
    #[test]
    fn render_falls_back_to_one_row_with_minimal_payload() {
        let lines = render(&json!({ "model": { "id": "claude-opus" } }));
        assert!(!lines.is_empty(), "the bar never disappears");
        assert!(lines.iter().all(|l| !l.is_empty()), "no blank row: {lines:?}");
    }
}
