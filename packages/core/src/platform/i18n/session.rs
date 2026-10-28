//! A sessão: os avisos do início, a linha da entrada de cada mensagem e a
//! barra de status.
//!
//! Uma parte do catálogo de textos: quem lê chama `translate`, a porta do
//! catálogo, e nunca esta parte direto. Chave nova com um começo que esta
//! parte ainda não responde entra também em `PREFIXES`.

use super::Locale;

/// Os começos de chave (o trecho antes do primeiro ponto) que esta parte
/// responde. Nenhum deles é de outra parte.
pub(super) const PREFIXES: &[&str] = &["session", "scratch", "specs", "statusline", "prompt_entry"];

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
        // Os avisos do início da sessão (`session_start_inject`).
        ("session.merged", Locale::PtBr) => {
            "[Mustard] O trabalho de {count} branch(es) já entrou na base por merge, e ela(s) segue(m) \
             viva(s): {branches}."
        }
        ("session.merged", Locale::EnUs) => {
            "[Mustard] The work of {count} branch(es) already went into the base through a merge, and \
             the branch(es) are still alive: {branches}."
        }
        // O merge feito por outra pessoa: o pull request da spec atual entrou,
        // e o início da sessão rodou o mesmo caminho do merge do Mustard.
        ("session.landed", Locale::PtBr) => {
            "[Mustard] O pull request #{pr} da spec {spec} entrou pelas mãos de outra pessoa: a \
             spec foi gravada como entregue."
        }
        ("session.landed", Locale::EnUs) => {
            "[Mustard] Pull request #{pr} of the spec {spec} was merged by someone else: the spec \
             was recorded as delivered."
        }
        // A obra de uma spec que o git prova na base, juntada fora da porta do
        // pull request: a frase de abertura, a entrega gravada ou as cópias
        // saindo antes dela, e o que houve com a branch.
        ("session.landed.outside", Locale::PtBr) => {
            "[Mustard] A branch {branch} da spec {spec} entrou na base fora do fechamento, sem a \
             revisão final."
        }
        ("session.landed.outside", Locale::EnUs) => {
            "[Mustard] The branch {branch} of the spec {spec} went into the base outside the close, \
             without the final review."
        }
        ("session.landed.outside_closed", Locale::PtBr) => {
            "[Mustard] A branch {branch} da spec {spec}, já fechada, entrou na base por um merge feito \
             fora do Mustard."
        }
        ("session.landed.outside_closed", Locale::EnUs) => {
            "[Mustard] The branch {branch} of the spec {spec}, already closed, went into the base \
             through a merge made outside Mustard."
        }
        ("session.landed.recorded", Locale::PtBr) => "A spec foi gravada como entregue.",
        ("session.landed.recorded", Locale::EnUs) => "The spec was recorded as delivered.",
        ("session.landed.leaving_one", Locale::PtBr) => {
            "A cópia da obra sai em segundo plano, e o código dela que nenhum commit tem fica \
             guardado em `refs/mustard/kept/{spec}/`. Quando ela sair, a spec é gravada como \
             entregue; o relatório fica em {report}."
        }
        ("session.landed.leaving_one", Locale::EnUs) => {
            "The work copy is leaving in the background, and its code no commit holds is kept under \
             `refs/mustard/kept/{spec}/`. When it is gone, the spec is recorded as delivered; the \
             report goes to {report}."
        }
        ("session.landed.leaving_many", Locale::PtBr) => {
            "As {count} cópias da obra saem em segundo plano, e o código delas que nenhum commit tem \
             fica guardado em `refs/mustard/kept/{spec}/`. Quando a última sair, a spec é gravada \
             como entregue; o relatório fica em {report}."
        }
        ("session.landed.leaving_many", Locale::EnUs) => {
            "Its {count} work copies are leaving in the background, and their code no commit holds is \
             kept under `refs/mustard/kept/{spec}/`. When the last one is gone, the spec is recorded \
             as delivered; the report goes to {report}."
        }
        ("session.landed.gone", Locale::PtBr) => {
            "A branch {branch} já tinha saído desta máquina e do servidor."
        }
        ("session.landed.gone", Locale::EnUs) => {
            "The branch {branch} had already left this machine and the server."
        }
        ("session.landed.kept", Locale::PtBr) => "A branch {branch} segue nesta máquina, como estava.",
        ("session.landed.kept", Locale::EnUs) => "The branch {branch} stays on this machine, as it was.",
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
        ("session.landed.tasks_left", Locale::PtBr) => {
            "[Mustard] A branch {branch} da spec {spec} entrou na base. Ainda faltam {count} \
             tarefa(s), sem onda ou em onda não entregue. A spec não foi gravada como entregue e \
             segue em obra. Rode as tarefas que faltam ou tire as que não vão sair."
        }
        ("session.landed.tasks_left", Locale::EnUs) => {
            "[Mustard] The branch {branch} of the spec {spec} went into the base. {count} task(s) \
             are still left, with no wave or in a wave not delivered yet. The spec was not recorded \
             as delivered and stays in progress. Run the tasks left or remove the ones that will \
             not go out."
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
        ("session.landed.settled", Locale::PtBr) => {
            "A base foi atualizada e a branch {branch} saiu desta máquina."
        }
        ("session.landed.settled", Locale::EnUs) => {
            "The base was updated and the branch {branch} left this machine."
        }
        ("session.landed.unsettled", Locale::PtBr) => {
            "A arrumação da branch {branch} não terminou ({reason}), e ela ficou nesta máquina."
        }
        ("session.landed.unsettled", Locale::EnUs) => {
            "Tidying up the branch {branch} did not finish ({reason}), and it stayed on this machine."
        }
        ("session.landed.pending", Locale::PtBr) => {
            "Pergunte ao usuário, pelo título, o que fazer com cada pendência nascida nela: {items}. \
             Cada uma pode virar spec, ficar na lista ou sair com motivo."
        }
        ("session.landed.pending", Locale::EnUs) => {
            "Ask the user, by title, what to do with each pending item born in it: {items}. Each \
             one can become a spec, stay on the list, or leave with a reason."
        }
        ("session.provider_silent", Locale::PtBr) => {
            "[Mustard] A spec {spec} está com o pull request aberto, e o provedor não respondeu se \
             ele entrou ({reason}): nada foi mudado."
        }
        ("session.provider_silent", Locale::EnUs) => {
            "[Mustard] The spec {spec} has its pull request open, and the provider did not answer \
             whether it was merged ({reason}): nothing was changed."
        }
        // Os pull requests dos submódulos da spec atual: o texto vem da porta
        // do pull request.
        ("session.submodules", Locale::PtBr) => "[Mustard] Na spec {spec}: {text}",
        ("session.submodules", Locale::EnUs) => "[Mustard] In the spec {spec}: {text}",
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
        ("session.version.stale", Locale::PtBr) => {
            "[Mustard] Esta sessão carregou o Mustard {running}, e o instalado é o {installed}: só \
             reabrir o Claude Code carrega o novo."
        }
        ("session.version.stale", Locale::EnUs) => {
            "[Mustard] This session loaded Mustard {running}, and {installed} is installed: only \
             reopening Claude Code loads the new one."
        }
        ("session.version.behind", Locale::PtBr) => {
            "[Mustard] O plugin do Claude Code está no Mustard {plugin}, e o binário que roda é o \
             {running}. Sugira `/mustard:upsert` e reabrir o Claude Code."
        }
        ("session.version.behind", Locale::EnUs) => {
            "[Mustard] The Claude Code plugin is on Mustard {plugin}, and the running binary is \
             {running}. Suggest `/mustard:upsert` and reopening Claude Code."
        }
        // A linha que abre a janela renovada, depois de `/clear` e da
        // compactação, antes dos princípios do condutor: o caminho da busca,
        // a retomada e onde mora o mapa inteiro.
        ("session.guide", Locale::PtBr) => {
            "Mustard: busque código com `mcp__mustard__search` ou `run search`, e `mustard-rt run \
             resume` retoma a spec. O mapa inteiro está em `.claude/mustard/session-map.md`."
        }
        ("session.guide", Locale::EnUs) => {
            "Mustard: search code with `mcp__mustard__search` or `run search`, and `mustard-rt run \
             resume` restores the spec. The full map is in `.claude/mustard/session-map.md`."
        }
        // O programa compilado da branch do Mustard, só no código-fonte dele,
        // quando falta ou está atrás do commit: `{running}` é o programa que
        // roda a sessão até a compilação acabar, `{compiled}` e `{head}` são
        // commits, `{progress}` o que aconteceu com a compilação em segundo
        // plano (um dos três textos abaixo).
        ("session.build.missing", Locale::PtBr) => {
            "[Mustard] O programa compilado da branch ainda não existe (commit {head}). {progress} \
             Até ela acabar, esta sessão roda o programa instalado ({running})."
        }
        ("session.build.missing", Locale::EnUs) => {
            "[Mustard] The branch's compiled program does not exist yet (commit {head}). {progress} \
             Until it finishes, this session runs the installed program ({running})."
        }
        ("session.build.behind", Locale::PtBr) => {
            "[Mustard] O programa compilado da branch é do commit {compiled}, atrás do commit {head}. \
             {progress} Até ela acabar, esta sessão roda o compilado anterior ({compiled})."
        }
        ("session.build.behind", Locale::EnUs) => {
            "[Mustard] The branch's compiled program is from commit {compiled}, behind commit {head}. \
             {progress} Until it finishes, this session runs the previous compiled program ({compiled})."
        }
        ("session.build.started", Locale::PtBr) => "A compilação foi solta em segundo plano.",
        ("session.build.started", Locale::EnUs) => "The build was started in the background.",
        ("session.build.in_progress", Locale::PtBr) => "Outra compilação já está em andamento.",
        ("session.build.in_progress", Locale::EnUs) => "Another build is already running.",
        ("session.build.not_started", Locale::PtBr) => {
            "A compilação não pôde ser iniciada em segundo plano: rode \
             `cargo build --release --locked -p mustard-rt -p scan -p mustard-cli` no repositório."
        }
        ("session.build.not_started", Locale::EnUs) => {
            "The build could not be started in the background: run \
             `cargo build --release --locked -p mustard-rt -p scan -p mustard-cli` in the repository."
        }

        // Aviso de disco do início da sessão: `{total}` e `{count}` são
        // preenchidos pelo chamador (`session_start_inject::disk_notice`).
        ("scratch.residue.notice", Locale::PtBr) => {
            "[Mustard] As cópias descartáveis antigas no diretório temporário somam {total} \
             em {count} pasta(s). Diga ao usuário que o disco está sendo gasto com sobras e \
             ofereça `mustard-rt run clean` para listar o que sai e \
             `mustard-rt run clean --apply` para apagar. Aviso, nunca bloqueio."
        }
        ("scratch.residue.notice", Locale::EnUs) => {
            "[Mustard] Old throwaway copies in the temp directory add up to {total} across \
             {count} folder(s). Tell the user the disk is being spent on leftovers and offer \
             `mustard-rt run clean` to list what would go and \
             `mustard-rt run clean --apply` to delete it. Advisory, never blocking."
        }
        ("prompt_entry.line", Locale::PtBr) => {
            "Responda em português do Brasil, em texto simples: frases curtas e nenhum código interno."
        }
        ("prompt_entry.line", Locale::EnUs) => {
            "Answer in American English, in plain text: short sentences and no internal codes."
        }
        ("prompt_entry.line.undeclared", Locale::PtBr) => {
            "Responda no idioma de quem escreve, em texto simples: frases curtas e nenhum código interno."
        }
        ("prompt_entry.line.undeclared", Locale::EnUs) => {
            "Answer in the language the user writes in, in plain text: short sentences and no internal codes."
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::i18n::translate;

    /// Esta parte guarda as mesmas chaves, com os mesmos textos nos dois
    /// idiomas, como o arquivo de impressões dela em `stamps/` guarda. Quem muda
    /// um texto de propósito regrava o arquivo pelo comando que a falha mostra.
    #[test]
    fn the_part_keeps_its_keys_and_texts() {
        crate::platform::i18n::tests::assert_part_unchanged("session", include_str!("session.rs"), super::PREFIXES);
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
