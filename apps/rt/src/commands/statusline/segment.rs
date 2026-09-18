//! Statusline segments — pure data ([`Segment`]) plus the per-kind builders
//! that turn the harness JSON payload into segments. Themes (in `theme.rs`)
//! own all color/separator decisions; this module only produces *text*.
//!
//! The one exception is [`Segment::override_fg`], used by [`context_segment`]
//! when the per-segment threshold (yellow / red) needs to override the theme
//! default. Theme renderers honor it.

use super::theme::Color;
use crate::shared::rtk_gain::RtkGain;
use crate::shared::spec_state::DiskSpecState;
use mustard_core::domain::spec_events::{Block, BlockQuery};
use mustard_core::domain::spec_state::SpecState;
use mustard_core::{ClaudePaths, SupportedLocale};
use serde_json::Value;
use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::Path;
use mustard_core::platform::git as git_exec;

/// All segment kinds the statusline knows how to render. New kinds must be
/// appended (themes index a `[Style; SEGMENT_KIND_COUNT]` by `kind as usize`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum SegmentKind {
    Module = 0,
    Git = 1,
    Context = 2,
    Duration = 3,
    Savings = 4,
    Diff = 5,
    /// A versão do Mustard que roda, no começo da segunda linha. Ocupa a casa
    /// do custo, que saiu da barra.
    Mustard = 6,
    Model = 7,
    /// A spec desta sessão, a fase dela e o andamento das ondas.
    Unit = 8,
    /// The plugin is installed but switched off, so no hook runs.
    Inert = 9,
}

/// Count of kinds — keep in sync with the last variant.
pub const SEGMENT_KIND_COUNT: usize = 10;

/// A single line element with no theme coupling. Builders return
/// `Option<Segment>` so a missing payload field omits the segment cleanly.
#[derive(Debug, Clone)]
pub struct Segment {
    pub kind: SegmentKind,
    pub text: String,
    /// Per-render fg override — used by `context_segment` and `inert_segment`
    /// for threshold coloring. **Honored only by flat separators**
    /// (`Pipe` / `Whitespace`). Powerline themes ignore it so the palette
    /// stays harmonic; the override clashing with a fixed bg looks worse than
    /// the missing signal.
    pub override_fg: Option<Color>,
}

impl Segment {
    #[must_use]
    pub fn new(kind: SegmentKind, text: impl Into<String>) -> Self {
        Self {
            kind,
            text: text.into(),
            override_fg: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Builders — one per kind. Each is `pub` so `preview.rs` can build a
// synthetic line. The orchestration that picks which builders to call lives
// in `mod.rs`.
// ---------------------------------------------------------------------------

/// O nome do projeto (a pasta de `cwd`, ou `"?"`), como link para a página
/// do projeto quando o índice das specs traz o endereço dela.
#[must_use]
pub fn module_segment(cwd: &Path) -> Segment {
    let module = cwd
        .file_name()
        .map_or_else(|| "?".to_string(), |n| n.to_string_lossy().to_string());
    let text = match project_page_url(cwd) {
        Some(url) => hyperlink(&url, &module),
        None => module,
    };
    Segment::new(SegmentKind::Module, text)
}

/// O endereço da página do projeto, na linha do projeto do índice das specs
/// do checkout principal. Um caractere de controle no endereço quebraria a
/// sequência do link, e aí não há endereço.
fn project_page_url(cwd: &Path) -> Option<String> {
    let root = mustard_core::io::spec_events::spec_root(cwd);
    let index = ClaudePaths::for_project(&root).ok()?.spec_index_path();
    let content = std::fs::read_to_string(index).ok()?;
    mustard_core::domain::spec_index::project_url(&content).filter(|url| !url.chars().any(char::is_control))
}

/// `⎇ branch +N~N?N` or `⎇ branch ✓`. `branch` é a do checkout, lida uma vez
/// por desenho e dividida com [`unit_segment`]; `None` (fora de um repositório,
/// sem o `git` ou com a HEAD solta) não desenha nada.
#[must_use]
pub fn git_segment(cwd: &Path, branch: Option<&str>) -> Option<Segment> {
    let branch = branch?;
    let porcelain = git(cwd, &["status", "--porcelain"]).unwrap_or_default();
    let (mut staged, mut modified, mut untracked) = (0u32, 0u32, 0u32);
    for line in porcelain.lines() {
        if line.starts_with("??") {
            untracked += 1;
        } else {
            let mut chars = line.chars();
            let x = chars.next().unwrap_or(' ');
            let y = chars.next().unwrap_or(' ');
            if matches!(x, 'M' | 'A' | 'D' | 'R' | 'C') {
                staged += 1;
            }
            if matches!(y, 'M' | 'D') {
                modified += 1;
            }
        }
    }
    let mut status = String::new();
    if staged > 0 {
        let _ = write!(status, "+{staged}");
    }
    if modified > 0 {
        let _ = write!(status, "~{modified}");
    }
    if untracked > 0 {
        let _ = write!(status, "?{untracked}");
    }
    let suffix = if status.is_empty() {
        " \u{2713}".to_string()
    } else {
        format!(" {status}")
    };
    Some(Segment::new(
        SegmentKind::Git,
        format!("\u{2387} {branch}{suffix}"),
    ))
}

/// `██░░░░░░░░ 24%` — o uso da conversa num número só: o já usado, 100 menos
/// `context_window.remaining_percentage`, o mesmo que a barrinha de 10 casas
/// desenha. O total em tokens e o aviso de 200 mil ficam de fora: repetiam o
/// mesmo dado de outro jeito. `None` sem o campo.
#[must_use]
pub fn context_segment(data: &Value) -> Option<Segment> {
    let rem = data.get("context_window")?.get("remaining_percentage")?.as_f64()?;
    let remaining = (rem.round() as i64).clamp(0, 100);
    let used = 100 - remaining;
    let bar_len = 10i64;
    let cells = ((used as f64 / 100.0) * bar_len as f64).round() as i64;
    let cells = cells.clamp(0, bar_len);
    let bar = format!(
        "{}{}",
        "\u{2588}".repeat(cells as usize),
        "\u{2591}".repeat((bar_len - cells) as usize),
    );
    let mut s = Segment::new(SegmentKind::Context, format!("{bar} {used}%"));
    // A cor pelo que sobra: vermelho forte abaixo de 20%, vermelho abaixo de
    // 40%, amarelo abaixo de 60%.
    if remaining < 20 {
        s.override_fg = Some(Color::Ansi(9)); // bright red
    } else if remaining < 40 {
        s.override_fg = Some(Color::Ansi(1)); // red
    } else if remaining < 60 {
        s.override_fg = Some(Color::Ansi(3)); // yellow
    }
    Some(s)
}

/// O tempo da sessão: `5h49m` de uma hora em diante, `2m5s` ou `45s` abaixo
/// dela. A parte zerada da direita some (`5h`, `2m`). `None` com a duração
/// zerada ou ausente.
#[must_use]
pub fn duration_segment(data: &Value) -> Option<Segment> {
    let dur_ms = data.get("cost")?.get("total_duration_ms")?.as_i64()?;
    if dur_ms <= 0 {
        return None;
    }
    let h = dur_ms / 3_600_000;
    let m = (dur_ms % 3_600_000) / 60_000;
    let s = (dur_ms % 60_000) / 1000;
    let text = if h > 0 {
        if m > 0 {
            format!("{h}h{m}m")
        } else {
            format!("{h}h")
        }
    } else if m > 0 {
        if s > 0 {
            format!("{m}m{s}s")
        } else {
            format!("{m}m")
        }
    } else {
        format!("{s}s")
    };
    Some(Segment::new(SegmentKind::Duration, text))
}

/// `⚡ rtk poupou 64%` — a economia do rtk, no idioma do projeto. `gain` é a
/// leitura do `rtk gain`, feita por quem desenha; `None` quando o rtk não tem
/// nada a dizer.
#[must_use]
pub fn savings_segment(gain: Option<&RtkGain>, lang: SupportedLocale) -> Option<Segment> {
    let gain = gain?;
    if gain.saved <= 0 && gain.pct <= 0.0 {
        return None;
    }
    let pct = gain.pct.round() as i64;
    let label = mustard_core::translate("statusline.rtk", lang).replace("{pct}", &pct.to_string());
    Some(Segment::new(SegmentKind::Savings, format!("\u{26A1} {label}")))
}

/// `Mustard 0.2.0` — a versão do Mustard que roda, no começo da segunda linha.
/// Quem desenha só a pede num projeto com o Mustard.
#[must_use]
pub fn mustard_segment() -> Segment {
    Segment::new(SegmentKind::Mustard, format!("Mustard {}", mustard_core::harness_version()))
}

/// `+N-N`. Returns `None` when both numbers are zero.
#[must_use]
pub fn diff_segment(data: &Value) -> Option<Segment> {
    let la = data
        .get("cost")
        .and_then(|c| c.get("total_lines_added"))
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let lr = data
        .get("cost")
        .and_then(|c| c.get("total_lines_removed"))
        .and_then(Value::as_i64)
        .unwrap_or(0);
    if la == 0 && lr == 0 {
        return None;
    }
    let mut parts = String::new();
    if la > 0 {
        let _ = write!(parts, "+{la}");
    }
    if lr > 0 {
        let _ = write!(parts, "-{lr}");
    }
    Some(Segment::new(SegmentKind::Diff, parts))
}

/// `Opus 4.7` etc. Strips the `Claude ` / `claude-` prefix to keep the line
/// tight.
#[must_use]
pub fn model_segment(data: &Value) -> Segment {
    let raw = data
        .get("model")
        .and_then(|m| m.get("display_name").or_else(|| m.get("id")))
        .and_then(Value::as_str)
        .unwrap_or("Claude");
    let short = raw
        .strip_prefix("Claude ")
        .or_else(|| raw.strip_prefix("claude-"))
        .unwrap_or(raw);
    Segment::new(SegmentKind::Model, short.to_string())
}

/// `▸ {spec} {fase} 1 de 4 ondas` — a spec desta sessão, a fase dela e o
/// andamento das ondas.
///
/// Quem reabre o terminal vê onde parou sem digitar nada. A spec vem da escada
/// única ([`current_spec`]). O nome dela só aparece quando `branch` não termina
/// com ele: na branch da própria spec, o nome já está na barra, e aí sai só a
/// fase (`▸ levantamento`). A fase sai no idioma do projeto, com as mesmas
/// chaves das páginas (`page.phase.*`). Com a página da spec publicada, o
/// link dela (OSC 8, aceito pela barra do Claude Code) fica no nome e, sem o
/// nome, passa para a fase. Sem fase conhecida, o nome aparece sempre, para o
/// link não sumir. O andamento aparece com a spec aprovada ou em execução e
/// com ondas no plano, como contagem: quantas ondas foram entregues e quantas
/// o plano tem. O número de uma onda não aparece, porque os números não seguem
/// a ordem; o da onda que vem fica na linha de retomada. `None` fora de um
/// projeto com o Mustard e sem spec atual.
///
/// [`current_spec`]: crate::shared::context::checkout::current_spec
#[must_use]
pub fn unit_segment(cwd: &Path, branch: Option<&str>) -> Option<Segment> {
    if !mustard_core::ProjectConfig::exists(cwd) {
        return None;
    }
    let root = cwd.to_string_lossy();
    let slug = crate::shared::context::checkout::current_spec(&root).filter(|s| !s.is_empty())?;
    // Um endereço com caractere de controle (arquivo editado à mão) quebraria
    // a sequência e sujaria a barra; nesse caso o texto sai sem link.
    let url = crate::commands::spec::spec_doc::published_url(cwd, &slug)
        .filter(|url| !url.chars().any(char::is_control));
    let linked = |label: &str| url.as_deref().map_or_else(|| label.to_string(), |url| hyperlink(url, label));
    // A fase é conveniência, não o ponto: um arquivo de eventos ilegível ainda
    // deixa a spec nomeada.
    let phase = crate::shared::spec_state::lock_state(cwd, &slug).and_then(|state| state.phase);
    let named = phase.is_none() || !branch.is_some_and(|branch| branch.ends_with(slug.as_str()));
    let lang = mustard_core::ProjectConfig::load(cwd).language().text_or_default();
    let mut text = String::from("\u{25b8}");
    if named {
        let _ = write!(text, " {}", linked(&slug));
    }
    if let Some(phase) = phase {
        let label = mustard_core::translate(&format!("page.phase.{phase}"), lang);
        let label = if named { label.to_string() } else { linked(label) };
        let _ = write!(text, " {label}");
        if matches!(phase, "approved" | "running")
            && let Some((delivered, total)) = wave_progress(cwd, &slug)
        {
            let progress = mustard_core::translate("statusline.wave", lang)
                .replace("{delivered}", &delivered.to_string())
                .replace("{total}", &total.to_string());
            let _ = write!(text, " {progress}");
        }
    }
    Some(Segment::new(SegmentKind::Unit, text))
}

/// O andamento das ondas da spec `slug`: quantas ondas do plano foram
/// entregues e quantas o plano tem. `None` sem arquivo de eventos ou sem onda
/// no plano.
fn wave_progress(cwd: &Path, slug: &str) -> Option<(usize, usize)> {
    let log = DiskSpecState::new(cwd).log(slug)?;
    let planned: BTreeSet<u64> = log
        .block(BlockQuery::Block(Block::Waves))
        .into_iter()
        .filter(|e| e.event_type == "wave")
        .filter_map(|e| e.wave())
        .collect();
    if planned.is_empty() {
        return None;
    }
    let delivered = log.delivered_waves().intersection(&planned).count();
    Some((delivered, planned.len()))
}

/// `label` como hiperlink OSC 8 para `url`: `ESC ]8;;URL ESC \ label ESC ]8;; ESC \`.
/// O terminal mostra só `label`; os bytes da sequência não ocupam coluna.
fn hyperlink(url: &str, label: &str) -> String {
    format!("\u{1b}]8;;{url}\u{1b}\\{label}\u{1b}]8;;\u{1b}\\")
}

/// Is the Mustard plugin listed in `settings` and switched OFF?
///
/// `Some(true)` disabled, `Some(false)` enabled, `None` when the question
/// cannot be answered — no file, unparseable, or the plugin unlisted (a source
/// checkout with no plugin install is not a defect). The marketplace suffix
/// varies by install (`mustard@mustard-local`, `mustard@mustard`, …), so the
/// name before the `@` is what decides.
///
/// Shared with the doctor's `inject-delivery` check, which reports the same
/// state as a FAIL: one reader, so the bar and the diagnosis cannot disagree.
#[must_use]
pub fn plugin_switched_off(settings: &Path) -> Option<bool> {
    let text = std::fs::read_to_string(settings).ok()?;
    let json: Value = serde_json::from_str(&text).ok()?;
    json.get("enabledPlugins")?
        .as_object()?
        .iter()
        .find(|(key, _)| key.split('@').next() == Some("mustard"))
        .map(|(_, value)| value.as_bool() == Some(false))
}

/// `⨯ harness inerte` — the plugin is installed and switched OFF.
///
/// With the plugin disabled no hook runs at all: no router, no gates. Measured
/// in the field 2026-08-25, that state is indistinguishable from a working
/// harness — the bar rendered normally while nothing was enforced, and three
/// attempts were spent discovering it by error. Red, because it is not
/// something owed; it is the harness not running.
///
/// `None` when the switch cannot be read (no settings file, unreadable, the
/// plugin unlisted) or when it is enabled. Never claims health it did not
/// measure: an unanswerable question renders nothing.
#[must_use]
pub fn inert_segment(cwd: &Path) -> Option<Segment> {
    if !mustard_core::ProjectConfig::exists(cwd) {
        return None;
    }
    // Through `claude_config_dir()`, which honours `CLAUDE_CONFIG_DIR`. A
    // hardcoded `$HOME/.claude` left this flag silent for an operator who moved
    // their config: the bar looked healthy while no hook ran, which is the very
    // state it exists to show (found in review).
    let settings = mustard_core::platform::harness::claude_config_dir()?.join("settings.json");
    let switched_off = plugin_switched_off(&settings) == Some(true);

    // Second road to the same dead end, and it is the one that hid for a whole
    // session: the plugin is ON, but its binary never downloaded, so
    // `mustard-boot` exits 0 and no hook runs either. The bar can still render
    // — a leftover binary from an earlier version draws it — which is precisely
    // why the state was invisible (field, 2026-08-28). Same red, different
    // word, because "switch it back on" and "the download never happened" are
    // opposite remedies.
    let key = if switched_off {
        "statusline.harness.inert"
    } else if crate::commands::doctor::bootstrap_check::harness_dormant() {
        "statusline.harness.dormant"
    } else {
        return None;
    };

    let lang = mustard_core::ProjectConfig::load(cwd).language().text_or_default();
    let label = mustard_core::translate(key, lang);
    let mut seg = Segment::new(SegmentKind::Inert, format!("\u{2a2f} {label}"));
    seg.override_fg = Some(Color::Ansi(1));
    Some(seg)
}

// ---------------------------------------------------------------------------
// git helper — local to this module
// ---------------------------------------------------------------------------

fn git(cwd: &Path, args: &[&str]) -> Option<String> {
    git_exec::run(cwd, args).out().filter(|s| !s.is_empty())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// The bar names the active unit and its stage, and says so when the
    /// harness is inert.
    ///
    /// Both halves answer the same question: what does the operator see without
    /// typing anything? A unit parked in PLAN was invisible, and a switched-off
    /// plugin rendered exactly like a working one.
    #[test]
    fn statusline_names_the_active_unit_and_flags_an_inert_harness() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        // `current_spec` memoises per PROCESS — right for a hook, which is one
        // invocation, but it means each state below needs its own root: asking
        // about a root before seeding it would cache the empty answer.
        let bare = tempfile::tempdir().unwrap();
        assert!(unit_segment(bare.path(), None).is_none(), "not a Mustard project: quiet");
        assert!(inert_segment(bare.path()).is_none());

        let idle = tempfile::tempdir().unwrap();
        std::fs::write(idle.path().join("mustard.json"), r#"{"version":"1.0.0"}"#).unwrap();
        assert!(unit_segment(idle.path(), None).is_none(), "no active unit must render nothing");

        std::fs::write(root.join("mustard.json"), r#"{"version":"1.0.0","language":{"text":"pt-BR"}}"#).unwrap();
        // A unit in PLAN, with the checkout on its branch: the current-spec
        // ladder every consumer reads names it, and the bar shows its stage
        // (the branch segment already carries the name).
        crate::shared::spec_state::seed_event(
            root,
            "roteador-didatico",
            "state",
            serde_json::json!({ "phase": "plan" }),
        );
        crate::shared::spec_state::stand_on_spec_branch(root, "roteador-didatico");

        let seg = unit_segment(root, Some("feature/roteador-didatico")).expect("an active unit must reach the bar");
        assert!(seg.text.contains("plano"), "the stage is missing: {}", seg.text);
    }

    /// O texto que o terminal mostra: a sequência OSC 8 sai, o rótulo fica.
    fn visible(text: &str) -> String {
        let mut out = String::new();
        let mut rest = text;
        while let Some(start) = rest.find("\u{1b}]8;") {
            out.push_str(&rest[..start]);
            let Some(end) = rest[start..].find("\u{1b}\\") else {
                return out;
            };
            rest = &rest[start + end + 2..];
        }
        out.push_str(rest);
        out
    }

    /// Na branch da própria spec (a branch termina com o nome dela), o nome
    /// não se repete: sai só a fase, no idioma do projeto, e o link da página
    /// da spec, quando ela tem endereço, vai para a fase.
    #[test]
    fn statusline_on_the_spec_branch_shows_only_the_phase_and_the_link_moves_to_it() {
        let url = "https://claude.ai/code/artifacts/pagina-ligada";
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("mustard.json"), r#"{"version":"1.0.0","language":{"text":"pt-BR"}}"#).unwrap();
        crate::shared::spec_state::seed_event(root, "pagina-ligada", "state", serde_json::json!({ "phase": "plan" }));
        let git = |args: &[&str]| {
            assert!(git_exec::run(root, args).ok, "git {args:?}");
        };
        git(&["init", "."]);
        git(&["symbolic-ref", "HEAD", "refs/heads/feature/pagina-ligada"]);
        let branch = Some("feature/pagina-ligada");

        let plain = unit_segment(root, branch).expect("the unit on its own branch reaches the bar");
        assert_eq!(plain.text, "\u{25b8} plano", "no address yet: the phase alone, in the project language");

        crate::shared::spec_state::seed_event(
            root,
            "pagina-ligada",
            "publish",
            serde_json::json!({ "page": "spec", "milestone": "round", "ok": true, "url": url }),
        );
        let linked = unit_segment(root, branch).expect("a published unit reaches the bar");
        assert_eq!(linked.text, format!("\u{25b8} {}", hyperlink(url, "plano")), "the phase carries the spec link");
        assert_eq!(visible(&linked.text), "\u{25b8} plano", "the link hides nothing and adds nothing");

        // Fora da branch de uma spec, nada a aponta como atual: só a variável
        // de ambiente apontaria, e um teste não a muda. Um arquivo que sobrou
        // na pasta velha de estado não conta.
        let away = tempfile::tempdir().unwrap();
        std::fs::write(away.path().join("mustard.json"), r#"{"version":"1.0.0"}"#).unwrap();
        crate::shared::spec_state::seed_event(away.path(), "outra-unidade", "state", serde_json::json!({ "phase": "plan" }));
        let states = away.path().join(".claude/.pipeline-states");
        std::fs::create_dir_all(&states).unwrap();
        std::fs::write(states.join("outra-unidade.json"), "{}").unwrap();
        if std::env::var_os("MUSTARD_ACTIVE_SPEC").is_none() {
            assert!(unit_segment(away.path(), Some("dev")).is_none(), "a leftover state file names no unit");
        }
    }

    /// Com a spec aberta numa branch cujo nome não termina com o dela, a barra
    /// mostra o nome da spec, com o link da página dela, e a fase depois do
    /// nome, fora do link.
    ///
    /// No teste, a escada acha a spec pela branch dela (só a variável de
    /// ambiente apontaria outra, e um teste não a muda); a branch que a barra
    /// compara é a que o desenho passa, a mesma do segmento da branch.
    #[test]
    fn statusline_on_a_branch_of_another_name_shows_the_linked_spec_name_and_then_the_phase() {
        let url = "https://claude.ai/code/artifacts/outro-nome";
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("mustard.json"), r#"{"version":"1.0.0","language":{"text":"pt-BR"}}"#).unwrap();
        crate::shared::spec_state::seed_event(root, "outro-nome", "state", serde_json::json!({ "phase": "survey" }));
        crate::shared::spec_state::stand_on_spec_branch(root, "outro-nome");

        let plain = unit_segment(root, Some("feature/outro-nome-v2")).expect("the unit reaches the bar");
        assert_eq!(plain.text, "\u{25b8} outro-nome levantamento", "the name, then the phase");

        crate::shared::spec_state::seed_event(
            root,
            "outro-nome",
            "publish",
            serde_json::json!({ "page": "spec", "milestone": "round", "ok": true, "url": url }),
        );
        for branch in [Some("feature/outro-nome-v2"), Some("dev"), None] {
            let linked = unit_segment(root, branch).expect("a published unit reaches the bar");
            assert_eq!(
                linked.text,
                format!("\u{25b8} {} levantamento", hyperlink(url, "outro-nome")),
                "only the name is the link ({branch:?})"
            );
        }
    }

    /// O nome do projeto vira link para a página do projeto quando o índice
    /// das specs traz o endereço dela; sem ele, fica só o nome.
    #[test]
    fn the_project_name_links_to_the_project_page_when_the_index_has_it() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("loja");
        std::fs::create_dir_all(root.join(".claude").join("spec")).unwrap();
        assert_eq!(module_segment(&root).text, "loja");
        let url = "https://claude.ai/code/artifacts/projeto";
        std::fs::write(
            root.join(".claude").join("spec").join("index.ndjson"),
            format!("{}\n", mustard_core::domain::spec_index::project_line(Some(url))),
        )
        .unwrap();
        let linked = module_segment(&root);
        assert_eq!(linked.text, hyperlink(url, "loja"));
        assert_eq!(visible(&linked.text), "loja");
    }

    /// The inert flag reads the plugin switch, and never claims health it could
    /// not measure: an absent or unlisted switch answers "cannot tell".
    ///
    /// Exercises the decision directly rather than through `$HOME`: a test that
    /// mutates process-wide environment races every other test in the binary,
    /// and the thing worth pinning here is the verdict, not the path lookup.
    #[test]
    fn the_inert_flag_reads_the_plugin_switch_and_stays_silent_when_unanswerable() {
        let dir = tempfile::tempdir().unwrap();
        let settings = dir.path().join("settings.json");
        let verdict = |body: Option<&str>| {
            match body {
                Some(b) => std::fs::write(&settings, b).unwrap(),
                None => {
                    let _ = std::fs::remove_file(&settings);
                }
            }
            plugin_switched_off(&settings)
        };

        assert_eq!(verdict(None), None, "no settings file: unanswerable, not green");
        assert_eq!(verdict(Some("{ not json")), None, "unparseable: unanswerable");
        assert_eq!(verdict(Some(r#"{"enabledPlugins":{}}"#)), None, "unlisted: unanswerable");
        assert_eq!(
            verdict(Some(r#"{"enabledPlugins":{"mustard@mustard-local":true}}"#)),
            Some(false),
            "an enabled plugin is measured as running",
        );
        // The marketplace suffix varies by install, so the name before `@` decides.
        assert_eq!(
            verdict(Some(r#"{"enabledPlugins":{"other@x":true,"mustard@whatever":false}}"#)),
            Some(true),
            "a disabled plugin is measured whatever marketplace it came from",
        );
    }

    #[test]
    fn module_segment_uses_cwd_basename() {
        let m = module_segment(Path::new("/tmp/foo-project"));
        assert_eq!(m.text, "foo-project");
        assert_eq!(m.kind, SegmentKind::Module);
    }

    /// O uso da conversa é um número só, o já usado, o mesmo que a barrinha
    /// desenha: sobram 70%, então 30% e três casas cheias; sobram 76%, então
    /// 24% e duas casas. O total em tokens e o aviso de 200 mil não aparecem.
    #[test]
    fn context_segment_shows_only_the_used_share_like_the_bar() {
        let seg = context_segment(&json!({
            "exceeds_200k_tokens": true,
            "context_window": {
                "remaining_percentage": 70,
                "total_input_tokens": 250_000,
                "total_output_tokens": 10_000,
            }
        }))
        .unwrap();
        assert_eq!(seg.text, "\u{2588}\u{2588}\u{2588}\u{2591}\u{2591}\u{2591}\u{2591}\u{2591}\u{2591}\u{2591} 30%");
        // 70% left is above all thresholds → no override, even past 200k.
        assert!(seg.override_fg.is_none());

        let seg = context_segment(&json!({ "context_window": { "remaining_percentage": 76 } })).unwrap();
        assert_eq!(seg.text, "\u{2588}\u{2588}\u{2591}\u{2591}\u{2591}\u{2591}\u{2591}\u{2591}\u{2591}\u{2591} 24%");
    }

    #[test]
    fn context_segment_low_pct_overrides_fg_red() {
        let seg = context_segment(&json!({
            "context_window": { "remaining_percentage": 10 }
        }))
        .unwrap();
        assert!(seg.override_fg.is_some());
    }

    #[test]
    fn duration_segment_formats_minutes() {
        let seg = duration_segment(&json!({ "cost": { "total_duration_ms": 125_000 } })).unwrap();
        assert_eq!(seg.text, "2m5s");
    }

    /// De uma hora em diante, o tempo sai em horas e minutos, sem os
    /// segundos; abaixo de uma hora, como antes.
    #[test]
    fn duration_segment_formats_hours_and_minutes() {
        let at = |ms: i64| duration_segment(&json!({ "cost": { "total_duration_ms": ms } })).unwrap().text;
        assert_eq!(at((5 * 3600 + 49 * 60 + 12) * 1000), "5h49m");
        assert_eq!(at(3_600_000), "1h");
        assert_eq!(at(3_599_000), "59m59s");
        assert_eq!(at(45_000), "45s");
    }

    #[test]
    fn duration_segment_none_when_zero() {
        assert!(duration_segment(&json!({ "cost": { "total_duration_ms": 0 } })).is_none());
    }

    #[test]
    fn diff_segment_omits_when_both_zero() {
        assert!(diff_segment(&json!({ "cost": {} })).is_none());
        let seg = diff_segment(&json!({
            "cost": { "total_lines_added": 100, "total_lines_removed": 5 }
        }))
        .unwrap();
        assert_eq!(seg.text, "+100-5");
    }

    #[test]
    fn model_segment_strips_prefixes() {
        let s = model_segment(&json!({ "model": { "display_name": "Claude Opus 4.7" } }));
        assert_eq!(s.text, "Opus 4.7");
        let s = model_segment(&json!({ "model": { "id": "claude-sonnet-4-6" } }));
        assert_eq!(s.text, "sonnet-4-6");
        // Fallback when both are absent
        let s = model_segment(&json!({}));
        assert_eq!(s.text, "Claude");
    }

}
