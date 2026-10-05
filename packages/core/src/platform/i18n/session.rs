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
        // A página do projeto que ainda não nasceu: o início da sessão manda o
        // assistente publicar o template dela, uma vez só, e gravar o
        // endereço. `{template}` é o caminho do template instalado e
        // `{capabilities}` a declaração do banco de dados da página.
        ("session.project_page", Locale::PtBr) => {
            "[Mustard] A página do projeto ainda não foi publicada. Ela é a página do claude.ai que \
             lista as specs deste projeto, com a fase e o link de cada uma. Publique agora o \
             template dela, o arquivo `{template}`, lido uma vez só \
             e publicado como está, declarando o banco de dados da página: `{capabilities}`. Depois \
             grave o endereço com `mustard-rt run write publish --json \
             '{\"page\":\"project\",\"ok\":true,\"url\":\"…\"}'`. Não escreva o endereço na \
             resposta: ele fica na barra de status."
        }
        ("session.project_page", Locale::EnUs) => {
            "[Mustard] The project page has not been published yet. It is the claude.ai page that \
             lists this project's specs, with the phase and the link of each one. Publish its \
             template now, the file `{template}`, read only once and \
             published as it is, declaring the page's database: `{capabilities}`. Then record the \
             address with `mustard-rt run write publish --json \
             '{\"page\":\"project\",\"ok\":true,\"url\":\"…\"}'`. Never write the address in the \
             reply: it lives in the status line."
        }
        // O gasto: o início de toda sessão manda contar o que falta, contar
        // hoje de novo e copiar as linhas para a página do gasto, que o
        // comando `spend` prepara.
        ("session.spend", Locale::PtBr) => {
            "[Mustard] O gasto de hoje e dos dias fechados vai para a página do gasto a cada início de sessão. \
             Rode `mustard-rt run spend` e siga a resposta dele. Não escreva o endereço da página na resposta."
        }
        ("session.spend", Locale::EnUs) => {
            "[Mustard] The spend of today and of the closed days goes to the spend page at the start of each \
             session. Run `mustard-rt run spend` and follow its answer. Never write the page address in the reply."
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
    /// idiomas. Quem muda um texto de propósito grava aqui os dois números
    /// novos que a falha mostra.
    #[test]
    fn the_part_keeps_its_keys_and_texts() {
        crate::platform::i18n::tests::assert_part_unchanged(
            include_str!("session.rs"),
            super::PREFIXES,
            27,
            0x1a82_217f_3204_229f,
        );
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
