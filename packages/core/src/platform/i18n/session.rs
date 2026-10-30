//! A sessão: o início, a entrega da obra juntada fora do fechamento e a
//! barra de status.
//!
//! Uma parte do catálogo de textos: quem lê chama `translate`, a porta do
//! catálogo, e nunca esta parte direto. Chave nova com um começo que esta
//! parte ainda não responde entra também em `PREFIXES`.

use super::Locale;

/// Os começos de chave (o trecho antes do primeiro ponto) que esta parte
/// responde. Nenhum deles é de outra parte.
pub(super) const PREFIXES: &[&str] = &["session", "specs", "statusline"];

/// O texto de `key` em `lang`, ou `None` quando a chave não está aqui.
pub(super) fn text(key: &str, lang: Locale) -> Option<&'static str> {
    Some(match (key, lang) {
        // Work-unit SURFACING — the three places the harness says out loud that
        // a work unit is somewhere other than the checkout, or that the exit
        // ritual is still owed. All three are user-facing (a listing legend, a
        // status-bar label, a session-start advisory), so they are
        // config-language and live here rather than inline at the surface.
        //
        // `specs.location.remote_only` explains the third value of the listing's
        // location column: a unit alive only on a remote, which the ref sweep
        // now reaches. `{count}` / `{branches}` in the advisory are
        // interpolated by the caller.
        ("specs.location.remote_only", Locale::PtBr) => {
            "Onde: {remoto}/{branch}=spec só no remoto, nenhuma branch local carrega o \
             diretório (busque a branch antes de agir)"
        }
        ("specs.location.remote_only", Locale::EnUs) => {
            "Where: {remote}/{branch}=spec only on the remote, no local branch carries the \
             directory (fetch the branch before acting)"
        }
        // O andamento é uma contagem, e não o número de uma onda: os números
        // das ondas não seguem a ordem, e o da onda que vem fica na retomada.
        ("statusline.wave", Locale::PtBr) => "{delivered} de {total} ondas",
        ("statusline.wave", Locale::EnUs) => "{delivered} of {total} waves",
        // A economia do rtk, na segunda linha da barra: `{pct}` vem de quem
        // chama, já arredondado.
        ("statusline.rtk", Locale::PtBr) => "rtk poupou {pct}%",
        ("statusline.rtk", Locale::EnUs) => "rtk saved {pct}%",
        // O indicador do consumo, na segunda linha da barra: `{change}` vem de
        // quem chama, já com sinal e com o `%` (`−25%`). Compara o último dia
        // de trabalho fechado com a média dos sete dias de trabalho anteriores
        // a ele.
        ("statusline.spend", Locale::PtBr) => "consumo {change} vs média de 7 dias",
        ("statusline.spend", Locale::EnUs) => "usage {change} vs 7-day average",
        ("statusline.harness.inert", Locale::PtBr) => "harness inerte",
        ("statusline.harness.inert", Locale::EnUs) => "harness inert",
        // Dormant is NOT inert: inert means someone switched the plugin off,
        // dormant means its binary never downloaded. Same consequence (no hook
        // runs), opposite remedy — so they must never share a label.
        ("statusline.harness.dormant", Locale::PtBr) => "harness dormente",
        ("statusline.harness.dormant", Locale::EnUs) => "harness dormant",
        // As branches mergeadas e a versão gravada no projeto, que o `doctor`
        // acusa.
        ("session.merged", Locale::PtBr) => {
            "[Mustard] O trabalho de {count} branch(es) já entrou na base por merge, e ela(s) segue(m) \
             viva(s): {branches}."
        }
        ("session.merged", Locale::EnUs) => {
            "[Mustard] The work of {count} branch(es) already went into the base through a merge, and \
             the branch(es) are still alive: {branches}."
        }
        // As recusas de `run deliver-merged`.
        ("session.deliver.no_spec", Locale::PtBr) => "A spec {spec} não existe neste projeto.",
        ("session.deliver.no_spec", Locale::EnUs) => "The spec {spec} does not exist in this project.",
        ("session.deliver.not_open", Locale::PtBr) => {
            "A spec {spec} está na fase {phase}: só a em execução, a fechada e a com o pull request \
             aberto são entregues por aqui."
        }
        ("session.deliver.not_open", Locale::EnUs) => {
            "The spec {spec} is in the phase {phase}: only a running, closed or pull-request-open \
             spec is delivered here."
        }
        ("session.deliver.not_in_base", Locale::PtBr) => {
            "Nada prova que a obra da spec {spec} entrou na base. O último commit gravado não chegou \
             a ela por um merge, ou a branch andou depois dele. Nada foi mudado."
        }
        ("session.deliver.not_in_base", Locale::EnUs) => {
            "Nothing proves the work of the spec {spec} went into the base. Its last recorded commit \
             did not reach it through a merge, or the branch moved past it. Nothing was changed."
        }
        ("session.deliver.in_flight", Locale::PtBr) => {
            "A spec {spec} ainda tem trabalho em andamento: onda sem volta assumida, revisão final \
             aberta ou conserto pendente. As cópias ficam, e nada foi mudado."
        }
        ("session.deliver.in_flight", Locale::EnUs) => {
            "The spec {spec} still has work in progress: a wave with no accepted return, an open final \
             review or a pending fix. The copies stay, and nothing was changed."
        }
        ("session.deliver.backlog", Locale::PtBr) => {
            "A spec {spec} ainda tem {count} tarefa(s) que não saíram: sem onda ou em onda ainda não \
             entregue. A obra não é entregue com elas de fora; as cópias ficam, e nada foi mudado."
        }
        ("session.deliver.backlog", Locale::EnUs) => {
            "The spec {spec} still has {count} task(s) that never went out: with no wave or in a wave \
             not delivered yet. The work is not delivered without them; the copies stay, and nothing \
             was changed."
        }
        ("session.deliver.not_started", Locale::PtBr) => {
            "mustard: as cópias da spec {spec} não saíram em segundo plano: {reason}"
        }
        ("session.deliver.not_started", Locale::EnUs) => {
            "mustard: the copies of the spec {spec} did not leave in the background: {reason}"
        }
        ("session.version.drift", Locale::PtBr) => {
            "[Mustard] Este projeto está com o Mustard {stamped}, e o que roda é o {running}. Sugira \
             `/mustard:upsert`."
        }
        ("session.version.drift", Locale::EnUs) => {
            "[Mustard] This project carries Mustard {stamped}, and the one running is {running}. \
             Suggest `/mustard:upsert`."
        }
        ("session.version.unstamped", Locale::PtBr) => "sem versão",
        ("session.version.unstamped", Locale::EnUs) => "unstamped",
        // A linha que abre a janela renovada, depois de `/clear` e da
        // compactação: o caminho da busca, a retomada e onde mora o mapa
        // inteiro.
        ("session.guide", Locale::PtBr) => {
            "Mustard: busque código com `mcp__mustard__search` ou `run search`, e `mustard-rt run \
             resume` retoma a spec. O mapa inteiro está em `.claude/mustard/session-map.md`."
        }
        ("session.guide", Locale::EnUs) => {
            "Mustard: search code with `mcp__mustard__search` or `run search`, and `mustard-rt run \
             resume` restores the spec. The full map is in `.claude/mustard/session-map.md`."
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::i18n::translate;

    /// Cada chave desta parte começa por um dos começos que ela declara e tem
    /// texto nos dois idiomas.
    #[test]
    fn the_part_answers_each_of_its_keys() {
        crate::platform::i18n::tests::assert_part_answers(include_str!("session.rs"), super::PREFIXES);
    }

    /// Work-unit surfacing copy is catalogue-driven in BOTH locales: the
    /// listing legend and the status-bar wave progress carry no language
    /// literal at their surface.
    #[test]
    fn i18n_translates_work_unit_surfacing_keys() {
        for key in ["specs.location.remote_only", "statusline.wave"] {
            for lang in [Locale::PtBr, Locale::EnUs] {
                assert_ne!(translate(key, lang), "<missing-key>", "{key} missing for {lang}");
            }
            assert_ne!(
                translate(key, Locale::PtBr),
                translate(key, Locale::EnUs),
                "{key} must differ per locale (proof it is catalogue-driven)"
            );
        }
        // O aviso de poda do início da sessão saiu com o comando que ele
        // mandava rodar, e a contagem de branches a apagar saiu da barra:
        // nenhum idioma guarda os dois textos.
        for lang in [Locale::PtBr, Locale::EnUs] {
            assert_eq!(translate("prune.pending.notice", lang), "<missing-key>", "the prune advisory left");
            assert_eq!(translate("statusline.prune.label", lang), "<missing-key>", "the prune count left the bar");
        }
    }
}
