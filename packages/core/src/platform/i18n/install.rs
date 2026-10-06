//! O diagnóstico da instalação: o que o `doctor` acusa sobre a proteção da
//! branch, as chaves do `mustard.json`, as sobras do Mustard e o que o scan
//! escreve; e as duas linhas da limpeza sobre a regra de bloqueio trocada no
//! lugar: a do plano, que diz o que o `/mustard:upsert` vai trocar, e a do
//! relatório, que diz o que já foi trocado.
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
        ("doctor.switches.rtk_missing", Locale::PtBr) => {
            "A opção `rtk` do mustard.json está ligada, mas o gancho `rtk hook claude` não está no \
             .claude/settings.local.json. Rode `mustard-rt run upsert` para alinhar os dois."
        }
        ("doctor.switches.rtk_missing", Locale::EnUs) => {
            "The `rtk` option in mustard.json is on, but the `rtk hook claude` hook is not in \
             .claude/settings.local.json. Run `mustard-rt run upsert` to line the two up."
        }
        ("doctor.switches.rtk_left", Locale::PtBr) => {
            "A opção `rtk` do mustard.json está desligada, mas o gancho `rtk hook claude` continua no \
             .claude/settings.local.json. Rode `mustard-rt run upsert` para alinhar os dois."
        }
        ("doctor.switches.rtk_left", Locale::EnUs) => {
            "The `rtk` option in mustard.json is off, but the `rtk hook claude` hook is still in \
             .claude/settings.local.json. Run `mustard-rt run upsert` to line the two up."
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
             ele as tira e troca no lugar as regras de bloqueio escritas errado."
        }
        ("doctor.claude_md.leftovers", Locale::EnUs) => {
            "Mustard leftovers in files that are not its own: {paths}. Run `mustard-rt run upsert`: \
             it takes them out and swaps in place the deny rules written wrong."
        }
        ("cleanup.deny_rule_to_swap", Locale::PtBr) => {
            "{file}: o /mustard:upsert troca a regra de bloqueio `{from}` por `{to}`, no mesmo lugar. \
             Ela segue bloqueando o mesmo comando, e o Claude Code para de avisar; a linha mudada é \
             sua para comitar."
        }
        ("cleanup.deny_rule_to_swap", Locale::EnUs) => {
            "{file}: /mustard:upsert swaps the deny rule `{from}` for `{to}`, in the same place. It \
             still blocks the same command, and Claude Code stops warning; the changed line is yours \
             to commit."
        }
        ("cleanup.deny_rule_swapped", Locale::PtBr) => {
            "{file}: a regra de bloqueio `{from}` virou `{to}`, no mesmo lugar. Ela bloqueia o mesmo \
             comando, e o Claude Code para de avisar; a linha mudada é sua para comitar."
        }
        ("cleanup.deny_rule_swapped", Locale::EnUs) => {
            "{file}: the deny rule `{from}` became `{to}`, in the same place. It blocks the same \
             command, and Claude Code stops warning; the changed line is yours to commit."
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
            include_str!("install.rs"),
            super::PREFIXES,
            14,
            0xea98_e94c_67e0_5d3a,
        );
    }

    /// As mensagens do diagnóstico das chaves do projeto e das sobras saem
    /// nos dois idiomas, com as vagas que o chamador preenche.
    #[test]
    fn i18n_translates_switch_and_cleanup_keys() {
        for (key, slots) in [
            ("doctor.switches.off", &[][..]),
            ("doctor.switches.rtk_missing", &[][..]),
            ("doctor.switches.rtk_left", &[][..]),
            ("doctor.switches.signature_on", &[][..]),
            ("doctor.switches.language_unknown", &["{field}", "{value}"][..]),
            ("doctor.claude_md.leftovers", &["{paths}"][..]),
            ("cleanup.deny_rule_to_swap", &["{file}", "{from}", "{to}"][..]),
            ("cleanup.deny_rule_swapped", &["{file}", "{from}", "{to}"][..]),
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
