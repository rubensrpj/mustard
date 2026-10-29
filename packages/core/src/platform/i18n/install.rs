//! Os textos da instalação. São dois grupos: o diagnóstico, que é o que o
//! `doctor` acusa sobre a proteção da branch, as chaves do `mustard.json`, as
//! sobras do Mustard e o que o scan escreve; e as duas linhas da limpeza sobre
//! a regra de bloqueio trocada no lugar, a do plano, que diz o que o
//! `/mustard:upsert` vai trocar, e a do relatório, que diz o que já foi
//! trocado.
//!
//! Uma parte do catálogo de textos: quem lê chama `translate`, a porta do
//! catálogo, e nunca esta parte direto. Chave nova com um começo que esta
//! parte ainda não responde entra também em `PREFIXES`.

use super::Locale;

/// Os começos de chave (o trecho antes do primeiro ponto) que esta parte
/// responde. Nenhum deles é de outra parte.
pub(super) const PREFIXES: &[&str] = &["doctor", "cleanup"];

/// O texto de `key` em `lang`, ou `None` quando a chave não está aqui.
pub(super) fn text(key: &str, lang: Locale) -> Option<&'static str> {
    Some(match (key, lang) {
        ("doctor.protection.flow_missing", Locale::PtBr) => {
            "Este projeto não declara base nenhuma em `mustard.json#git.flow`, então nenhuma \
             branch fica protegida, nem aqui nem no servidor. Não há o que perguntar ao provedor."
        }
        ("doctor.protection.flow_missing", Locale::EnUs) => {
            "This project declares no base in `mustard.json#git.flow`, so no branch is protected, \
             here or on the server. There is nothing to ask the provider about."
        }
        ("doctor.protection.protected", Locale::PtBr) => {
            "`{base}`: protegida no {provider}."
        }
        ("doctor.protection.protected", Locale::EnUs) => {
            "`{base}`: protected at {provider}."
        }
        ("doctor.protection.open", Locale::PtBr) => {
            "`{base}`: o {provider} não tem regra nenhuma para ela — qualquer pessoa com direito \
             de envio escreve direto nessa branch."
        }
        ("doctor.protection.open", Locale::EnUs) => {
            "`{base}`: {provider} has no rule for it — anyone with push rights writes straight to \
             that branch."
        }
        ("doctor.protection.unasked", Locale::PtBr) => {
            "`{base}`: não deu para perguntar ao {provider} ({reason}). Isto não é \
             \"desprotegida\": é pergunta que não chegou a ser feita."
        }
        ("doctor.protection.unasked", Locale::EnUs) => {
            "`{base}`: {provider} could not be asked ({reason}). That is not \"unprotected\" — it \
             is a question that was never asked."
        }
        ("doctor.protection.fix", Locale::PtBr) => {
            "como ligar: no GitHub, Settings → Rules → Rulesets (ou Branch protection rules); no \
             Azure DevOps, Project settings → Repositories → Policies, na branch. Para declarar \
             as bases, rode `mustard init` e responda a pergunta das bases."
        }
        ("doctor.protection.fix", Locale::EnUs) => {
            "how to turn it on: on GitHub, Settings → Rules → Rulesets (or Branch protection \
             rules); on Azure DevOps, Project settings → Repositories → Policies, on the branch. \
             To declare the bases, run `mustard init` and answer the bases question."
        }
        // The commit of the three Mustard programs, as the `doctor` reads them.
        ("doctor.binaries.unstamped", Locale::PtBr) => {
            "Sem commit na linha de versão: {programs}. Esse programa responde só o número da \
             versão, e nada prova de que código ele veio; troque-o por um que diga o commit."
        }
        ("doctor.binaries.unstamped", Locale::EnUs) => {
            "No commit in the version line: {programs}. That program answers only the version \
             number, and nothing proves which code it came from; replace it with one that says the commit."
        }
        ("doctor.binaries.diverge", Locale::PtBr) => {
            "Os programas do Mustard vêm de commits diferentes: {stamps}. O que roda pode não ser \
             o código que se pensa; atualize o que ficou para trás até os três dizerem o mesmo commit."
        }
        ("doctor.binaries.diverge", Locale::EnUs) => {
            "The Mustard programs come from different commits: {stamps}. What runs may not be the \
             code you think; update the one left behind until all three say the same commit."
        }
        // The `doctor` checks that what the scan writes stays outside git.
        ("doctor.scan_output.visible", Locale::PtBr) => {
            "O mapa do scan fica visível para o git: {paths}. O scan só escreve fora do git: tire \
             esses arquivos do git (a instalação privada já os exclui)."
        }
        ("doctor.scan_output.visible", Locale::EnUs) => {
            "The scan map is visible to git: {paths}. The scan only writes outside git: take these \
             files out of git (the private install already excludes them)."
        }
        // The switches `mustard.json` holds, as the `doctor` reads them.
        ("doctor.switches.off", Locale::PtBr) => {
            "O Mustard está desligado neste projeto (`enabled: false` no mustard.json): nenhum \
             gancho dele age aqui. Para religar, tire a chave ou ponha `true`."
        }
        ("doctor.switches.off", Locale::EnUs) => {
            "Mustard is turned off in this project (`enabled: false` in mustard.json): none of its \
             hooks act here. To turn it back on, remove the key or set it to `true`."
        }
        ("doctor.switches.rtk_hook_left", Locale::PtBr) => {
            "O gancho do próprio rtk (`rtk hook claude`) continua no .claude/settings.local.json. \
             O gancho do Mustard já faz a troca do rtk, e os dois trocando o mesmo comando disputam \
             a vez. Rode `mustard-rt run upsert` para tirá-lo."
        }
        ("doctor.switches.rtk_hook_left", Locale::EnUs) => {
            "rtk's own hook (`rtk hook claude`) is still in .claude/settings.local.json. Mustard's \
             hook already makes rtk's swap, and two hooks rewriting the same command race each \
             other. Run `mustard-rt run upsert` to take it out."
        }
        ("doctor.switches.rtk_hook_home", Locale::PtBr) => {
            "O gancho do próprio rtk (`rtk hook claude`) está nas configurações da pasta pessoal \
             ({path}), posto por `rtk init -g`. O gancho do Mustard já faz a troca do rtk, e os \
             dois trocando o mesmo comando disputam a vez: tire o gancho desse arquivo."
        }
        ("doctor.switches.rtk_hook_home", Locale::EnUs) => {
            "rtk's own hook (`rtk hook claude`) is in the personal settings ({path}), put there by \
             `rtk init -g`. Mustard's hook already makes rtk's swap, and two hooks rewriting the \
             same command race each other: take the hook out of that file."
        }
        ("doctor.switches.signature_on", Locale::PtBr) => {
            "A assinatura do Claude Code nos commits e pull requests está ligada no \
             .claude/settings.local.json (`attribution`). Rode `mustard-rt run upsert` para desligá-la."
        }
        ("doctor.switches.signature_on", Locale::EnUs) => {
            "Claude Code's signature on commits and pull requests is on in \
             .claude/settings.local.json (`attribution`). Run `mustard-rt run upsert` to turn it off."
        }
        ("doctor.switches.language_unknown", Locale::PtBr) => {
            "O campo `{field}` do mustard.json vale `{value}`, que não é um idioma aceito: use \
             `pt-BR` ou `en-US`. Até lá, o campo é lido como ausente."
        }
        ("doctor.switches.language_unknown", Locale::EnUs) => {
            "The `{field}` field in mustard.json is `{value}`, which is not an accepted language: \
             use `pt-BR` or `en-US`. Until then, the field is read as absent."
        }
        ("doctor.claude_md.leftovers", Locale::PtBr) => {
            "Sobras do Mustard em arquivos que não são dele: {paths}. Rode `mustard-rt run upsert`: \
             ele tira as dele e lista o que você pode tirar do arquivo da equipe."
        }
        ("doctor.claude_md.leftovers", Locale::EnUs) => {
            "Mustard leftovers in files that are not its own: {paths}. Run `mustard-rt run upsert`: \
             it takes out its own and lists what you can take out of the team file."
        }
        ("cleanup.deny_rule_to_swap", Locale::PtBr) => {
            "{file}: a regra de bloqueio `{from}` pode virar `{to}`, no mesmo lugar. Ela segue \
             bloqueando o mesmo comando, e o Claude Code para de avisar. O arquivo é da equipe, e o \
             Mustard não o grava."
        }
        ("cleanup.deny_rule_to_swap", Locale::EnUs) => {
            "{file}: the deny rule `{from}` can become `{to}`, in the same place. It still blocks the \
             same command, and Claude Code stops warning. The file is the team's, and Mustard does not \
             write it."
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
        crate::platform::i18n::tests::assert_part_answers(include_str!("install.rs"), super::PREFIXES);
    }

    /// As mensagens do diagnóstico das chaves do projeto e das sobras saem
    /// nos dois idiomas, com as vagas que o chamador preenche.
    #[test]
    fn i18n_translates_switch_and_cleanup_keys() {
        for (key, slots) in [
            ("doctor.switches.off", &[][..]),
            ("doctor.switches.rtk_hook_left", &[][..]),
            ("doctor.switches.rtk_hook_home", &["{path}"][..]),
            ("doctor.switches.signature_on", &[][..]),
            ("doctor.switches.language_unknown", &["{field}", "{value}"][..]),
            ("doctor.claude_md.leftovers", &["{paths}"][..]),
            ("doctor.binaries.unstamped", &["{programs}"][..]),
            ("doctor.binaries.diverge", &["{stamps}"][..]),
            ("cleanup.deny_rule_to_swap", &["{file}", "{from}", "{to}"][..]),
        ] {
            let (pt, en) = (translate(key, Locale::PtBr), translate(key, Locale::EnUs));
            assert_ne!(pt, "<missing-key>", "{key} missing in pt-BR");
            assert_ne!(en, "<missing-key>", "{key} missing in en-US");
            assert_ne!(pt, en, "{key} must differ per locale");
            for slot in slots {
                assert!(pt.contains(slot) && en.contains(slot), "{key} lost {slot}");
            }
        }
    }
}
