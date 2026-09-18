//! `session_start_inject` — o início da sessão, como uma lista de avisos.
//!
//! O `SessionStart` roda na abertura, depois de `/clear` e depois da
//! compactação. Cada aviso é um item de [`NOTICES`], com a sua condição e o seu
//! texto: acrescentar um aviso é somar um item. Na ordem em que a janela os
//! lê:
//!
//! 1. **O terreno** — o resumo do mapa do projeto, lido do censo do `/scan`.
//! 2. **Os textos declarados** — as entradas `on: sessionStart` de
//!    `mustard.json#inject`, que voltam depois de `/clear` e da compactação.
//! 3. **A retomada** — a spec atual, a fase, o último passo e o próximo item,
//!    a mesma linha que o `resume` devolve.
//! 4. **As pendências** — uma linha só, com a contagem.
//! 5. **O pull request da spec atual** — com a spec atual em "pull request
//!    aberto", o provedor é perguntado só pelo pull request dela; se ele
//!    entrou pelas mãos de outra pessoa, o mesmo caminho do merge do Mustard
//!    roda antes de qualquer aviso (a spec gravada como entregue, a base
//!    atualizada, a branch local apagada), e o aviso diz o que foi feito e
//!    pede a pergunta das pendências nascidas na spec. Se o provedor não
//!    responde, o aviso diz isso e nada muda. Com o pull request ainda aberto
//!    e a spec mexendo em submódulo, os pull requests dos submódulos são
//!    conferidos: o que entrou leva o ponteiro ao principal, e o aviso diz
//!    qual falta ou que o principal ficou pronto.
//! 6. **As branches mergeadas** — as outras branches cujo trabalho já entrou
//!    na base e que seguem vivas, só pelo git local, sem pergunta nenhuma ao
//!    provedor.
//! 7. **O disco** — as cópias descartáveis antigas acima de 5 GB.
//! 8. **A versão velha do Mustard** — a gravada no projeto, a do plugin
//!    carregado ou a do plugin instalado, quando uma delas ficou para trás.
//!
//! ## Até 3 kB
//!
//! Tudo junto cabe em [`MAX_BYTES`]. Quando o todo passa do teto, os avisos
//! cedem o lugar um a um, na vez de cada um, com uma linha no stderr dizendo
//! qual saiu: o terreno primeiro, depois a versão, o disco, as branches
//! mergeadas e a contagem das pendências, e só então os textos declarados.
//! Entre os textos declarados está o mapa do início da sessão, que substitui
//! as regras antigas: ele é o último a sair. A retomada e o relato do pull
//! request da spec atual nunca cedem: um diz onde a spec está, o outro conta
//! uma branch apagada e uma spec entregue.
//!
//! ## As leituras da máquina são argumento
//!
//! O registro de plugins do Claude Code e o diretório temporário moram fora
//! do projeto. O [`Check`] os lê uma vez e os entrega a [`session_start_core`],
//! que decide: um teste que monta um projeto temporário entrega "nada", e a
//! máquina de quem roda a suíte nunca entra num veredito.
//!
//! Nunca barra: devolve `Inject` com os avisos, ou `Allow` sem nenhum.

use mustard_core::domain::model::contract::{Check, Ctx, HookInput, Trigger, Verdict};
use mustard_core::domain::spec_state::SpecState;
use mustard_core::platform::error::Error;
use mustard_core::platform::i18n::{translate, Locale};
use std::path::Path;

use crate::commands::maint::scratch_gc::{human_bytes, survey, ScratchRoots};
use crate::commands::review::pr_door::{merged_elsewhere, MergedElsewhere};
use crate::hooks::session::injectables;
use crate::shared::branch_state::merged_by_another;

/// O teto do texto que o início da sessão coloca: 3 kB.
const MAX_BYTES: usize = 3_000;

/// Quantas branches o aviso do merge nomeia antes de só contar o resto.
const MERGED_NAMES: usize = 4;

/// O início da sessão.
pub struct SessionStartInject;

/// O que os avisos leem.
struct Probe<'a> {
    root: &'a Path,
    session: Option<&'a str>,
    lang: Locale,
    /// A janela foi renovada: `/clear` ou compactação.
    refreshed: bool,
    /// A versão que o registro de plugins dá como instalada, quando ele
    /// respondeu.
    installed: Option<&'a str>,
    /// Onde procurar as cópias descartáveis e a partir de quanto avisar.
    scratch: Option<&'a ScratchProbe>,
    /// O que o provedor respondeu sobre o pull request da spec atual, e o que
    /// foi feito com a resposta: a spec e o resultado.
    landing: Option<&'a (String, MergedElsewhere)>,
}

/// Um aviso do início da sessão: o nome, o texto — que só existe quando a
/// condição vale — e a vez dele de ceder o lugar quando o todo passa do teto:
/// o menor número sai primeiro, e `None` nunca sai.
struct Notice {
    name: &'static str,
    text: fn(&Probe<'_>) -> Option<String>,
    cedes: Option<u8>,
}

/// Os avisos, na ordem em que a janela os lê. Acrescentar um aviso é somar um
/// item. Os textos declarados, onde mora o mapa do início da sessão, são os
/// últimos a ceder.
const NOTICES: &[Notice] = &[
    Notice { name: "terrain", text: terrain_notice, cedes: Some(0) },
    Notice { name: "declared", text: declared_notice, cedes: Some(5) },
    Notice { name: "resume", text: resume_notice, cedes: None },
    Notice { name: "pending", text: pending_notice_of, cedes: Some(4) },
    Notice { name: "landed", text: landed_notice, cedes: None },
    Notice { name: "merged", text: merged_notice, cedes: Some(3) },
    Notice { name: "disk", text: disk_notice, cedes: Some(2) },
    Notice { name: "version", text: version_notice, cedes: Some(1) },
];

impl Check for SessionStartInject {
    /// Lê a máquina — o registro de plugins e o diretório temporário — e
    /// entrega a leitura a [`session_start_core`], que decide.
    fn evaluate(&self, input: &HookInput, ctx: &Ctx) -> Result<Verdict, Error> {
        let scratch = ScratchProbe::from_env(input);
        session_start_core(input, ctx, mustard_core::installed_harness_version().as_deref(), Some(&scratch))
    }
}

/// A metade que decide, com as leituras da máquina recebidas: `installed` é
/// a versão que o registro de plugins dá como instalada, e `scratch` a
/// varredura das cópias descartáveis. `None` nas duas — o que todo teste que
/// não fala delas entrega — cala os avisos que dependem delas.
fn session_start_core(
    input: &HookInput,
    ctx: &Ctx,
    installed: Option<&str>,
    scratch: Option<&ScratchProbe>,
) -> Result<Verdict, Error> {
    if ctx.trigger != Some(Trigger::SessionStart) {
        return Ok(Verdict::Allow);
    }
    let cwd = ctx.project_dir_or_cwd(input);
    let root = Path::new(&cwd);
    let session = session_of(input);
    let refreshed = input
        .raw
        .get("source")
        .and_then(|v| v.as_str())
        .is_some_and(|s| s.eq_ignore_ascii_case("compact") || s.eq_ignore_ascii_case("clear"));
    // O merge feito por outra pessoa age antes de qualquer aviso: com a spec
    // entregue e a branch arrumada, a retomada já lê o estado novo.
    let landing = spec_merged_elsewhere(root, session.as_deref());
    let probe = Probe {
        root,
        session: session.as_deref(),
        lang: crate::shared::context::config::project_config_cached(root).language().text_or_default(),
        refreshed,
        installed,
        scratch,
        landing: landing.as_ref(),
    };
    let texts = within_cap(NOTICES.iter().filter_map(|notice| (notice.text)(&probe).map(|text| (notice, text))).collect());
    Ok(if texts.is_empty() { Verdict::Allow } else { Verdict::Inject { context: texts.join("\n\n") } })
}

/// Os textos que cabem no teto, na ordem da lista: enquanto o todo passa de
/// [`MAX_BYTES`], sai o aviso cuja vez de ceder vem primeiro, com uma linha no
/// stderr.
fn within_cap(mut shown: Vec<(&Notice, String)>) -> Vec<String> {
    let size = |shown: &[(&Notice, String)]| {
        shown.iter().map(|(_, text)| text.len()).sum::<usize>() + 2 * shown.len().saturating_sub(1)
    };
    while size(&shown) > MAX_BYTES {
        let next = shown
            .iter()
            .enumerate()
            .filter_map(|(at, (notice, _))| notice.cedes.map(|turn| (turn, at)))
            .min();
        let Some((_, at)) = next else {
            break;
        };
        let (notice, text) = shown.remove(at);
        eprintln!(
            "mustard: o aviso `{}` do início da sessão ({} bytes) saiu para o todo caber em {MAX_BYTES} bytes",
            notice.name,
            text.len()
        );
    }
    shown.into_iter().map(|(_, text)| text).collect()
}

/// O id da sessão, quando o harness o mandou.
fn session_of(input: &HookInput) -> Option<String> {
    input
        .session_id
        .clone()
        .or_else(|| input.raw.get("sessionId").and_then(|v| v.as_str()).map(str::to_string))
        .filter(|s| !s.trim().is_empty())
}

// ---------------------------------------------------------------------------
// Os avisos
// ---------------------------------------------------------------------------

/// O terreno: o resumo do mapa do projeto, do censo do `/scan`.
fn terrain_notice(probe: &Probe<'_>) -> Option<String> {
    crate::commands::orient::render_terrain(&crate::commands::orient::compute_orientation(probe.root), probe.lang)
}

/// Os textos declarados para o início da sessão; com a janela renovada, eles
/// voltam mesmo com a marca de entregue.
fn declared_notice(probe: &Probe<'_>) -> Option<String> {
    injectables::collect(&probe.root.to_string_lossy(), probe.session, probe.refreshed)
}

/// A linha de retomada da spec atual.
fn resume_notice(probe: &Probe<'_>) -> Option<String> {
    crate::commands::flow::resume::current_line(probe.root, probe.session)
}

/// A contagem das pendências abertas.
fn pending_notice_of(probe: &Probe<'_>) -> Option<String> {
    pending_notice(probe.root, probe.lang)
}

/// Uma linha com a contagem de pendências abertas e o comando que mostra a
/// lista ([`count_line`](crate::commands::event::pending::count_line)).
///
/// `None` num projeto sem `mustard.json`, quando nada está aberto e quando a
/// lista não se lê: quem explica o conserto é o `run pending`.
pub(crate) fn pending_notice(root: &Path, lang: Locale) -> Option<String> {
    if !mustard_core::ProjectConfig::exists(root) {
        return None;
    }
    crate::commands::event::pending::count_line(root, lang)
}

/// A spec atual em "pull request aberto", com o que o provedor respondeu sobre
/// o pull request dela e o que foi feito com a resposta
/// ([`merged_elsewhere`]): o provedor é perguntado só por esse pull request.
///
/// `None` num projeto sem `mustard.json`, sem spec atual, com a spec em outra
/// fase e quando o pull request segue aberto.
fn spec_merged_elsewhere(root: &Path, session: Option<&str>) -> Option<(String, MergedElsewhere)> {
    if !mustard_core::ProjectConfig::exists(root) {
        return None;
    }
    let spec = crate::shared::spec_state::DiskSpecState::new(root).active(session)?;
    merged_elsewhere(root, &spec, session).map(|found| (spec, found))
}

/// O pull request da spec atual: o que foi feito com ele, pelo caminho do
/// merge, ou o silêncio do provedor sobre ele. `None` quando não há nada a
/// dizer.
fn landed_notice(probe: &Probe<'_>) -> Option<String> {
    probe.landing.map(|(spec, found)| landing_text(spec, found, probe.lang))
}

/// As branches cujo trabalho já entrou na base e que seguem vivas, pela
/// conferência única do git local ([`merged_by_another`]), que nunca pergunta
/// ao provedor.
///
/// `None` num projeto sem `mustard.json` e quando não há nada a dizer.
fn merged_notice(probe: &Probe<'_>) -> Option<String> {
    if !mustard_core::ProjectConfig::exists(probe.root) {
        return None;
    }
    let lang = probe.lang;
    let config = crate::shared::context::config::project_config_cached(probe.root);
    let flow = crate::shared::work_kind::BaseFlow::of_at(&config.git, probe.root);
    // A branch que o caminho do merge acabou de arrumar já foi dita acima; a
    // cópia dela no servidor, que fica sem `git.deleteRemoteBranch`, não volta
    // aqui como se ninguém tivesse cuidado dela.
    let landed = match probe.landing {
        Some((_, MergedElsewhere::Landed { branch, .. })) => Some(branch.as_str()),
        _ => None,
    };
    let merged: Vec<_> =
        merged_by_another(probe.root, &flow).into_iter().filter(|state| Some(state.branch.as_str()) != landed).collect();
    if merged.is_empty() {
        return None;
    }
    let named: Vec<&str> = merged.iter().take(MERGED_NAMES).map(|state| state.branch.as_str()).collect();
    let rest = merged.len() - named.len();
    let branches = if rest > 0 { format!("{} (+{rest})", named.join(", ")) } else { named.join(", ") };
    Some(
        translate("session.merged", lang)
            .replace("{count}", &merged.len().to_string())
            .replace("{branches}", &branches),
    )
}

/// O texto do pull request da spec `spec`: o que o caminho do merge fez — a
/// spec entregue, a arrumação da branch e a pergunta das pendências nascidas
/// nela — ou o silêncio do provedor.
fn landing_text(spec: &str, found: &MergedElsewhere, lang: Locale) -> String {
    match found {
        MergedElsewhere::Unanswered { reason } => translate("session.provider_silent", lang)
            .replace("{spec}", spec)
            .replace("{reason}", reason),
        MergedElsewhere::Submodules(found) => translate("session.submodules", lang)
            .replace("{spec}", spec)
            .replace("{text}", &found.text(lang).unwrap_or_default()),
        MergedElsewhere::Landed { pr, branch, settle, pending_open } => {
            let mut text = translate("session.landed", lang).replace("{pr}", &pr.to_string()).replace("{spec}", spec);
            text.push(' ');
            text.push_str(&tidy_text(branch, settle.as_ref(), lang));
            if !pending_open.is_empty() {
                let items = crate::commands::event::pending::format_pending_items(pending_open, pending_open.len());
                text.push(' ');
                text.push_str(&translate("session.landed.pending", lang).replace("{items}", &items));
            }
            text
        }
    }
}

/// A arrumação da branch `branch`, pelo relatório dela: a base atualizada e a
/// branch fora da máquina, ou o motivo por que ela ficou. Sem relatório, foi
/// uma promoção de base para base, que não tem branch a arrumar.
fn tidy_text(branch: &str, settle: Option<&serde_json::Value>, lang: Locale) -> String {
    let Some(report) = settle else {
        return translate("session.landed.unsettled", lang)
            .replace("{branch}", branch)
            .replace("{reason}", "base-to-base-promotion");
    };
    let unit = |field: &str| report.get("unit").and_then(|unit| unit.get(field));
    if report["ok"] == serde_json::json!(true) && unit("branchDeleted") == Some(&serde_json::json!(true)) {
        return translate("session.landed.settled", lang).replace("{branch}", branch);
    }
    let reason = report["reason"]
        .as_str()
        .or_else(|| unit("action").and_then(serde_json::Value::as_str).filter(|action| *action != "settled"))
        .unwrap_or("branch-kept");
    translate("session.landed.unsettled", lang).replace("{branch}", branch).replace("{reason}", reason)
}

/// O disco: as cópias descartáveis antigas acima do limite, com o total e
/// quantas pastas. A conta sai da mesma varredura do `clean`, então o total é
/// o que ele apagaria. `None` sem a leitura da máquina, num projeto sem
/// `mustard.json` e abaixo do limite.
fn disk_notice(probe: &Probe<'_>) -> Option<String> {
    let scratch = probe.scratch?;
    if !mustard_core::ProjectConfig::exists(probe.root) {
        return None;
    }
    let found = survey(&scratch.roots);
    let total = found.candidates_bytes();
    if total <= scratch.warn_bytes {
        return None;
    }
    Some(
        translate("scratch.residue.notice", probe.lang)
            .replace("{total}", &human_bytes(total))
            .replace("{count}", &found.candidates.len().to_string()),
    )
}

/// A versão velha do Mustard: a gravada no projeto difere da que roda, o
/// plugin carregado ficou atrás do instalado, ou o plugin instalado ficou
/// atrás do binário. O primeiro que se prova fala.
fn version_notice(probe: &Probe<'_>) -> Option<String> {
    let running = mustard_core::harness_version();
    stamp_drift(probe.root, &running, probe.lang)
        .or_else(|| stale_plugin(&running, probe.installed, probe.lang))
        .or_else(|| plugin_behind(&running, probe.installed, probe.lang))
}

/// A versão gravada no `mustard.json` não é a que roda. Sem `mustard.json`,
/// nada: o projeto não tem o Mustard.
fn stamp_drift(root: &Path, running: &str, lang: Locale) -> Option<String> {
    if !mustard_core::ProjectConfig::exists(root) {
        return None;
    }
    let stamped = mustard_core::ProjectConfig::load(root).version;
    if stamped.as_deref() == Some(running) {
        return None;
    }
    let stamped = stamped.unwrap_or_else(|| translate("session.version.unstamped", lang).to_string());
    Some(translate("session.version.drift", lang).replace("{stamped}", &stamped).replace("{running}", running))
}

/// A sessão carregou um plugin mais velho que o instalado: só reabrir o
/// Claude Code carrega o novo. Sem resposta do registro, nada.
fn stale_plugin(running: &str, installed: Option<&str>, lang: Locale) -> Option<String> {
    let installed = installed.filter(|latest| mustard_core::is_behind(running, latest))?;
    Some(translate("session.version.stale", lang).replace("{running}", running).replace("{installed}", installed))
}

/// O plugin instalado ficou atrás do binário que roda — o caso da instalação
/// por pacote, que troca o binário e não toca no plugin. Sem resposta do
/// registro, nada.
fn plugin_behind(running: &str, plugin: Option<&str>, lang: Locale) -> Option<String> {
    let plugin = plugin.filter(|p| mustard_core::is_behind(p, running))?;
    Some(translate("session.version.behind", lang).replace("{running}", running).replace("{plugin}", plugin))
}

/// Variável que ajusta, em bytes, a partir de quanto o aviso de disco aparece.
const SCRATCH_WARN_ENV: &str = "MUSTARD_SCRATCH_WARN_BYTES";

/// O limite padrão do aviso de disco: 5 GiB.
const DEFAULT_SCRATCH_WARN_BYTES: u64 = 5 * 1024 * 1024 * 1024;

/// O que o aviso de disco precisa da máquina: onde varrer e a partir de quanto
/// avisar.
struct ScratchProbe {
    roots: ScratchRoots,
    warn_bytes: u64,
}

impl ScratchProbe {
    /// As raízes desta máquina e o limite de `MUSTARD_SCRATCH_WARN_BYTES`. A
    /// pasta da sessão que está abrindo nunca conta como sobra, e a
    /// compilação compartilhada fica fora: medi-la seria mais uma passada pela
    /// árvore inteira em todo início de sessão.
    fn from_env(input: &HookInput) -> Self {
        let mut roots = ScratchRoots::from_env();
        if let Some(session) = session_of(input) {
            roots.current_session = session;
        }
        roots.shared_target = None;
        let warn_bytes = std::env::var(SCRATCH_WARN_ENV)
            .ok()
            .and_then(|v| v.trim().parse::<u64>().ok())
            .unwrap_or(DEFAULT_SCRATCH_WARN_BYTES);
        Self { roots, warn_bytes }
    }
}

/// O texto do início da sessão depois de `/clear` na sessão `session`, sem as
/// leituras da máquina. É por ele que o teste da retomada confere que o
/// início da sessão traz a mesma linha que o `resume`.
#[cfg(test)]
pub(crate) fn started_after_clear(root: &Path, session: &str) -> String {
    let input = HookInput {
        hook_event_name: Some("SessionStart".to_string()),
        session_id: Some(session.to_string()),
        raw: serde_json::json!({ "source": "clear" }),
        ..HookInput::default()
    };
    let ctx = Ctx::for_test(root.to_string_lossy().into_owned(), Some(Trigger::SessionStart));
    match session_start_core(&input, &ctx, None, None) {
        Ok(Verdict::Inject { context }) => context,
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::tempdir;

    /// O registro de plugins que um teste entrega: nenhum. Os avisos de plugin
    /// calam por construção, em qualquer máquina.
    const NO_REGISTRY: Option<&str> = None;

    /// A varredura do temporário que um teste entrega: nenhuma.
    const NO_SCRATCH: Option<&ScratchProbe> = None;

    fn ctx(dir: &Path) -> Ctx {
        Ctx::for_test(dir.to_string_lossy().into_owned(), Some(Trigger::SessionStart))
    }

    fn session_input(session_id: &str, source: &str) -> HookInput {
        HookInput {
            hook_event_name: Some("SessionStart".to_string()),
            session_id: Some(session_id.to_string()),
            raw: json!({ "source": source }),
            ..HookInput::default()
        }
    }

    fn context_of(root: &Path, input: &HookInput, installed: Option<&str>, scratch: Option<&ScratchProbe>) -> String {
        match session_start_core(input, &ctx(root), installed, scratch).unwrap() {
            Verdict::Inject { context } => context,
            _ => String::new(),
        }
    }

    /// Um projeto com o Mustard na versão que roda: o aviso de versão cala.
    fn installed_project(root: &Path) {
        std::fs::write(root.join("mustard.json"), format!(r#"{{"version":"{}"}}"#, mustard_core::harness_version()))
            .unwrap();
    }

    fn git(dir: &Path, args: &[&str]) {
        let out = std::process::Command::new("git").args(args).current_dir(dir).output().expect("git on PATH");
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    }

    /// Os avisos são uma lista, na ordem em que a janela os lê, cada um com a
    /// vez de ceder o lugar: o terreno primeiro, os textos declarados por
    /// último, e a retomada e o relato do pull request nunca.
    #[test]
    fn the_notices_are_a_typed_list_in_reading_order() {
        let names: Vec<&str> = NOTICES.iter().map(|n| n.name).collect();
        assert_eq!(names, ["terrain", "declared", "resume", "pending", "landed", "merged", "disk", "version"]);
        let mut ceding: Vec<(u8, &str)> = NOTICES.iter().filter_map(|n| n.cedes.map(|turn| (turn, n.name))).collect();
        ceding.sort_unstable();
        let order: Vec<&str> = ceding.into_iter().map(|(_, name)| name).collect();
        assert_eq!(order, ["terrain", "version", "disk", "merged", "pending", "declared"]);
        let kept: Vec<&str> = NOTICES.iter().filter(|n| n.cedes.is_none()).map(|n| n.name).collect();
        assert_eq!(kept, ["resume", "landed"]);
    }

    /// Fora do início da sessão, nada; sem aviso nenhum, `Allow`.
    #[test]
    fn nothing_to_say_is_allow() {
        let dir = tempdir().unwrap();
        let other = Ctx::for_test(dir.path().to_string_lossy().into_owned(), Some(Trigger::PreToolUse));
        assert_eq!(SessionStartInject.evaluate(&session_input("s", "startup"), &other).unwrap(), Verdict::Allow);
        let verdict = session_start_core(&session_input("s", "startup"), &ctx(dir.path()), NO_REGISTRY, NO_SCRATCH);
        assert_eq!(verdict.unwrap(), Verdict::Allow);
    }

    /// O relato do pull request feito por outra pessoa, com três pendências,
    /// cabe ao lado do mapa, da retomada e da contagem; um texto declarado
    /// grande demais para caber com a retomada sai por último, depois de todos
    /// os avisos que cedem, e a retomada e o relato ficam.
    #[test]
    fn the_landing_report_fits_and_an_oversized_declared_text_goes_last() {
        let resume = (&NOTICES[2], "Retomada: spec uma-spec-de-nome-longo, fase running; último passo: round; próximo: onda 12.".to_string());
        let pending = (&NOTICES[3], translate("pending.count.many", Locale::PtBr).replace("{count}", "12"));
        let merged = (&NOTICES[5], translate("session.merged", Locale::PtBr).replace("{count}", "6")
            .replace("{branches}", "feature/uma, feature/duas, feature/tres, feature/quatro (+2)"));
        let disk = (&NOTICES[6], translate("scratch.residue.notice", Locale::PtBr).replace("{total}", "12.3 GiB").replace("{count}", "14"));
        let version = (&NOTICES[7], translate("session.version.behind", Locale::PtBr).replace("{running}", "0.10.100").replace("{plugin}", "0.10.99"));

        let items: Vec<crate::commands::event::pending::OpenPending> = ["Humanize", "HTML padrão da spec", "Revisor de fora"]
            .iter()
            .enumerate()
            .map(|(n, title)| crate::commands::event::pending::OpenPending { id: format!("P-{}", n + 10), title: title.to_string() })
            .collect();
        let settled = serde_json::json!({"ok": true, "unit": {"branchDeleted": true}});
        let landed = MergedElsewhere::Landed {
            pr: 1234,
            branch: "feature/uma-spec-de-nome-longo".into(),
            settle: Some(settled),
            pending_open: items,
        };
        let landed = landing_text("uma-spec-de-nome-longo", &landed, Locale::PtBr);
        assert!(landed.contains("Revisor de fora") && landed.contains("saiu desta máquina"), "{landed}");
        let map = mustard_core::session_map(Locale::PtBr).trim().to_string();
        let with_landing = vec![
            (&NOTICES[1], map.clone()),
            resume.clone(),
            pending.clone(),
            (&NOTICES[4], landed.clone()),
        ];
        let kept = within_cap(with_landing);
        assert!(kept.contains(&map) && kept.contains(&landed), "the landing report fits beside the real map: {kept:?}");

        let too_big = vec![
            (&NOTICES[0], "t".repeat(400)),
            (&NOTICES[1], "d".repeat(2_950)),
            resume.clone(),
            pending,
            merged,
            disk,
            version,
        ];
        let kept = within_cap(too_big);
        assert_eq!(kept, vec![resume.1], "every ceding notice went, the declared text last, and the resume stays");
    }

    /// O mapa do início da sessão, nos dois idiomas, chega inteiro numa sessão
    /// com spec aberta, pendências, versão velha, branches mergeadas e sobras
    /// no disco: o todo passaria do teto, e são os avisos que saem antes dele.
    /// A retomada fica, e o todo cabe nos 3 kB.
    #[test]
    fn the_real_session_map_is_never_the_first_to_go() {
        use mustard_core::platform::i18n::Locale;
        for lang in [Locale::PtBr, Locale::EnUs] {
            let dir = tempdir().unwrap();
            let project = dir.path().join("projeto");
            let root = project.as_path();
            std::fs::create_dir_all(root).unwrap();
            git(root, &["init", "-q", "."]);
            git(root, &["config", "user.email", "t@t"]);
            git(root, &["config", "user.name", "t"]);
            git(root, &["config", "commit.gpgsign", "false"]);
            git(root, &["checkout", "-q", "-b", "dev"]);
            std::fs::write(root.join(".git/info/exclude"), ".claude/\nmustard.json\n").unwrap();
            let config = json!({
                "version": "0.0.1-velha",
                "language": {"text": lang.as_str()},
                "git": {"flow": {"*": "dev"}},
                "inject": mustard_core::platform::project_seed::default_inject_entries(),
            });
            std::fs::write(root.join("mustard.json"), config.to_string()).unwrap();
            mustard_core::platform::project_seed::seed_harness_texts(&root.join(".claude"), lang).unwrap();
            std::fs::write(root.join("README.md"), "loja\n").unwrap();
            git(root, &["add", "README.md"]);
            git(root, &["commit", "-q", "-m", "seed"]);
            // Nomes longos, como os de uma obra de verdade: o aviso do merge
            // nomeia quatro deles, e é ele que leva o todo acima do teto
            // mesmo com o mapa curto.
            let landed_branches = [
                "feature/trava-de-pendencias-abertas-no-fim-da-resposta-do-assistente-principal",
                "feature/pagina-do-projeto-com-menu-lateral-busca-e-filtro-por-fase-da-spec",
                "fix/merge-feito-por-outra-pessoa-no-inicio-da-sessao-seguinte-do-usuario",
                "feature/uma-entrega-ja-mergeada-pelo-colega-antes-da-revisao-final-do-dono",
                "fix/prova-que-roda-zero-testes-no-fechamento-da-spec-e-da-revisao-final",
            ];
            for landed in landed_branches {
                git(root, &["checkout", "-q", "-b", landed]);
                git(root, &["commit", "-q", "--allow-empty", "-m", "work"]);
                git(root, &["checkout", "-q", "dev"]);
                git(root, &["merge", "-q", "--no-ff", "-m", "merge", landed]);
            }
            for title in ["Humanize", "HTML padrão da spec", "Revisor de fora"] {
                let out = crate::commands::event::pending::pending_at(&crate::commands::event::pending::PendingOpts {
                    root: root.to_path_buf(),
                    add: true,
                    title: Some(title.to_string()),
                    detail: Some("combinado na conversa".to_string()),
                    ..crate::commands::event::pending::PendingOpts::default()
                });
                assert_eq!(out["ok"], json!(true), "seed: {out}");
            }
            let spec = "relatorios-dos-agentes-aceitos-como-vem";
            let branch = format!("feature/{spec}");
            git(root, &["checkout", "-q", "-b", &branch]);
            assert_eq!(crate::commands::spec_events::write::record_open(root, spec, &branch, "dev"), Ok(true));

            // As sobras no disco, acima do limite.
            let temp_root = dir.path().join("tmp");
            let old = temp_root.join("tmp.old");
            std::fs::create_dir_all(old.join("apps").join("rt")).unwrap();
            std::fs::write(old.join("Cargo.toml"), "[workspace]\n").unwrap();
            std::fs::write(old.join("apps").join("rt").join("big.bin"), vec![0u8; 4096]).unwrap();
            crate::commands::maint::scratch_gc::backdate_tree(&old, 24);
            let scratch = ScratchProbe {
                roots: ScratchRoots {
                    temp_root,
                    shared_target: None,
                    cap_bytes: u64::MAX,
                    current_session: "s-mapa".to_string(),
                    current_dir: None,
                    home: None,
                    clock: crate::commands::maint::scratch_gc::AgeClock::Modified,
                    owner_uid: crate::commands::maint::scratch_gc::current_uid(),
                    now: std::time::SystemTime::now(),
                },
                warn_bytes: 1024,
            };

            let map = mustard_core::session_map(lang).trim().to_string();
            let input = session_input("s-mapa", "clear");
            let context = context_of(root, &input, NO_REGISTRY, Some(&scratch));
            assert!(context.contains(&map), "{lang:?}: the whole map arrives: {context}");
            assert!(context.len() <= MAX_BYTES, "{lang:?}: {} bytes", context.len());
            let resume = crate::commands::flow::resume::current_line(root, Some("s-mapa")).expect("an open spec");
            assert!(context.contains(&resume), "{lang:?}: the resume stays: {context}");

            // Sem o teto, o todo passaria dos 3 kB: o teste mede a situação em
            // que alguém tem de sair.
            let probe = Probe {
                root,
                session: Some("s-mapa"),
                lang,
                refreshed: true,
                installed: NO_REGISTRY,
                scratch: Some(&scratch),
                landing: None,
            };
            let all: Vec<String> = NOTICES.iter().filter_map(|notice| (notice.text)(&probe)).collect();
            assert!(all.iter().any(|text| text.contains("0.0.1-velha")), "{lang:?}: the old version speaks: {all:?}");
            assert!(all.iter().any(|text| text.contains("uma-entrega-ja-mergeada")), "{lang:?}: {all:?}");
            assert!(all.iter().any(|text| text.contains("mustard-rt run clean")), "{lang:?}: {all:?}");
            assert!(all.join("\n\n").len() > MAX_BYTES, "{lang:?}: the whole would not fit: {}", all.join("\n\n").len());
            let gone = all.iter().filter(|text| !context.contains(text.as_str())).count();
            assert!(gone >= 1, "{lang:?}: some notice gave way: {context}");
        }
    }

    /// O texto declarado sai uma vez por sessão e volta depois de `/clear` e
    /// da compactação.
    #[test]
    fn the_declared_text_comes_once_and_again_after_clear_or_compact() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        std::fs::write(
            root.join("mustard.json"),
            format!(
                r#"{{"version":"{}","inject":[{{"on":"sessionStart","file":".claude/mustard/mapa.md","once":true}}]}}"#,
                mustard_core::harness_version()
            ),
        )
        .unwrap();
        std::fs::create_dir_all(root.join(".claude/mustard")).unwrap();
        std::fs::write(root.join(".claude/mustard/mapa.md"), "MAPA-DO-INICIO\n").unwrap();

        assert!(context_of(root, &session_input("s1", "startup"), NO_REGISTRY, NO_SCRATCH).contains("MAPA-DO-INICIO"));
        assert!(!context_of(root, &session_input("s1", "resume"), NO_REGISTRY, NO_SCRATCH).contains("MAPA-DO-INICIO"));
        for source in ["clear", "compact"] {
            assert!(
                context_of(root, &session_input("s1", source), NO_REGISTRY, NO_SCRATCH).contains("MAPA-DO-INICIO"),
                "{source} brings it back"
            );
        }
    }

    /// As pendências abertas viram uma linha só, com a contagem e sem título.
    #[test]
    fn the_pending_notice_is_one_line_with_the_count() {
        let lang = Locale::PtBr;
        let bare = tempdir().unwrap();
        assert_eq!(pending_notice(bare.path(), lang), None, "not installed");

        let dir = tempdir().unwrap();
        let root = dir.path();
        installed_project(root);
        assert_eq!(pending_notice(root, lang), None, "nothing open");
        for title in ["HTML padrao da spec", "Humanize"] {
            let out = crate::commands::event::pending::pending_at(&crate::commands::event::pending::PendingOpts {
                root: root.to_path_buf(),
                add: true,
                title: Some(title.to_string()),
                detail: Some("combinado na conversa".to_string()),
                ..crate::commands::event::pending::PendingOpts::default()
            });
            assert_eq!(out["ok"], json!(true), "seed: {out}");
        }
        let context = context_of(root, &session_input("s-pend", "startup"), NO_REGISTRY, NO_SCRATCH);
        let line = translate("pending.count.many", lang).replace("{count}", "2");
        assert!(context.contains(&line), "the count line: {context}");
        assert!(!context.contains("Humanize") && !context.contains("P-1"), "no item is listed: {context}");
    }

    /// O merge feito por outra pessoa vira um aviso com a branch que entrou
    /// na base e segue viva, e nunca com a que ainda está em andamento; o
    /// aviso não manda rodar comando nenhum.
    #[test]
    fn a_branch_merged_by_someone_else_is_named_and_no_command_is_suggested() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        git(root, &["init", "."]);
        git(root, &["config", "user.email", "t@t"]);
        git(root, &["config", "user.name", "t"]);
        git(root, &["config", "commit.gpgsign", "false"]);
        git(root, &["checkout", "-b", "dev"]);
        std::fs::write(
            root.join("mustard.json"),
            format!(r#"{{"version":"{}","git":{{"flow":{{"*":"dev","dev":"main"}}}}}}"#, mustard_core::harness_version()),
        )
        .unwrap();
        git(root, &["add", "-A", "-f", "."]);
        git(root, &["commit", "-m", "seed"]);
        git(root, &["checkout", "-b", "dev_landed"]);
        git(root, &["commit", "--allow-empty", "-m", "work"]);
        git(root, &["checkout", "dev"]);
        git(root, &["merge", "--no-ff", "-m", "merge", "dev_landed"]);
        git(root, &["branch", "dev_live"]);
        git(root, &["checkout", "dev_live"]);
        git(root, &["commit", "--allow-empty", "-m", "in flight"]);
        git(root, &["checkout", "dev"]);

        let context = context_of(root, &session_input("s-merged", "startup"), NO_REGISTRY, NO_SCRATCH);
        let expected = translate("session.merged", Locale::PtBr).replace("{count}", "1").replace("{branches}", "dev_landed");
        assert!(context.contains(&expected), "the merged branch is named: {context}");
        assert!(!context.contains("dev_live"), "the branch in flight is not: {context}");
        assert!(!context.contains("mustard-rt run") && !context.contains("git-settle"), "no command: {context}");
    }

    /// Com as sobras acima do limite, o aviso de disco traz o total e o
    /// comando que limpa; no limite exato, nada aparece.
    #[test]
    fn the_disk_notice_shows_above_the_limit_only() {
        let dir = tempdir().unwrap();
        let root = dir.path().join("project");
        std::fs::create_dir_all(&root).unwrap();
        installed_project(&root);

        let temp_root = dir.path().join("tmp");
        let old = temp_root.join("tmp.old");
        std::fs::create_dir_all(old.join("apps").join("rt")).unwrap();
        std::fs::write(old.join("Cargo.toml"), "[workspace]\n").unwrap();
        std::fs::write(old.join("apps").join("rt").join("big.bin"), vec![0u8; 4096]).unwrap();
        crate::commands::maint::scratch_gc::backdate_tree(&old, 24);
        let total = std::fs::metadata(old.join("Cargo.toml")).unwrap().len() + 4096;

        let probe = |warn_bytes: u64| ScratchProbe {
            roots: ScratchRoots {
                temp_root: temp_root.clone(),
                shared_target: None,
                cap_bytes: u64::MAX,
                current_session: "s-scratch".to_string(),
                current_dir: None,
                home: None,
                clock: crate::commands::maint::scratch_gc::AgeClock::Modified,
                owner_uid: crate::commands::maint::scratch_gc::current_uid(),
                now: std::time::SystemTime::now(),
            },
            warn_bytes,
        };
        let above = context_of(&root, &session_input("s-scratch", "startup"), NO_REGISTRY, Some(&probe(1024)));
        assert!(above.contains(&human_bytes(total)), "carries the total: {above}");
        assert!(above.contains("mustard-rt run clean"), "names the cleanup command: {above}");
        let below = context_of(&root, &session_input("s-scratch", "startup"), NO_REGISTRY, Some(&probe(total)));
        assert!(!below.contains("mustard-rt run clean"), "below the limit nothing shows: {below}");
        assert!(old.exists(), "the notice only reads");
    }

    /// A versão velha do Mustard fala por um dos três lados, e cala quando
    /// nada se prova: a gravada no projeto, o plugin carregado atrás do
    /// instalado e o plugin instalado atrás do binário.
    #[test]
    fn an_old_mustard_version_is_said_once_and_only_when_proven() {
        let lang = Locale::PtBr;
        let bare = tempdir().unwrap();
        assert_eq!(stamp_drift(bare.path(), "1.0.0", lang), None, "not installed");

        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("mustard.json"), r#"{"version":"1.0.0"}"#).unwrap();
        assert_eq!(stamp_drift(dir.path(), "1.0.0", lang), None, "aligned");
        let drift = stamp_drift(dir.path(), "1.0.1", lang).expect("the stamp is behind");
        assert!(drift.contains("1.0.0") && drift.contains("1.0.1") && drift.contains("/mustard:upsert"), "{drift}");
        std::fs::write(dir.path().join("mustard.json"), "{}").unwrap();
        let unstamped = stamp_drift(dir.path(), "1.0.1", lang).expect("no stamp is drift");
        assert!(unstamped.contains(translate("session.version.unstamped", lang)), "{unstamped}");

        let stale = stale_plugin("0.1.42", Some("0.1.43"), lang).expect("the loaded plugin is behind");
        assert!(stale.contains("0.1.42") && stale.contains("0.1.43") && !stale.contains('\n'), "{stale}");
        assert_eq!(stale_plugin("0.1.43", Some("0.1.43"), lang), None);
        assert_eq!(stale_plugin("0.2.0", Some("0.1.43"), lang), None);
        assert_eq!(stale_plugin("0.1.42", None, lang), None);

        let behind = plugin_behind("0.1.50", Some("0.1.49"), lang).expect("the plugin is behind the binary");
        assert!(behind.contains("0.1.50") && behind.contains("0.1.49") && behind.contains("/mustard:upsert"), "{behind}");
        assert_eq!(plugin_behind("0.1.50", Some("0.1.50"), lang), None);
        assert_eq!(plugin_behind("0.1.49", Some("0.1.50"), lang), None);
        assert_eq!(plugin_behind("0.1.50", None, lang), None);
        assert!(plugin_behind("0.1.10", Some("0.1.9"), lang).is_some(), "numeric, not lexical");

        // Os três juntos dão um aviso só.
        let root = tempdir().unwrap();
        std::fs::write(root.path().join("mustard.json"), r#"{"version":"0.0.0-velha"}"#).unwrap();
        let running = mustard_core::harness_version();
        let context = context_of(root.path(), &session_input("s-v", "startup"), Some("999.0.0"), NO_SCRATCH);
        let drift = stamp_drift(root.path(), &running, lang).unwrap();
        assert!(context.contains(&drift), "{context}");
        assert!(!context.contains("999.0.0"), "one version notice only: {context}");
    }
}
