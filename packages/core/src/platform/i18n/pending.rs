//! As pendências: a contagem, a trava do fim da resposta e as recusas do
//! comando que as grava e as remove.
//!
//! Uma parte do catálogo de textos: quem lê chama `translate`, a porta do
//! catálogo, e nunca esta parte direto. Chave nova com um começo que esta
//! parte ainda não responde entra também em `PREFIXES`.

use super::Locale;

/// Os começos de chave (o trecho antes do primeiro ponto) que esta parte
/// responde. Nenhum deles é de outra parte.
pub(super) const PREFIXES: &[&str] = &["pending"];

/// O texto de `key` em `lang`, ou `None` quando a chave não está aqui.
pub(super) fn text(key: &str, lang: Locale) -> Option<&'static str> {
    Some(match (key, lang) {
        // The count line of the open pending items, at the session start
        // (`apps/rt/src/hooks/session/session_start_inject.rs`) and in the
        // `run pending` listing. `{count}` comes from the caller.
        ("pending.count.one", Locale::PtBr) => {
            "[Mustard] 1 pendência aberta. A lista inteira sai com `mustard-rt run pending`."
        }
        ("pending.count.one", Locale::EnUs) => {
            "[Mustard] 1 open pending item. The whole list comes from `mustard-rt run pending`."
        }
        ("pending.count.many", Locale::PtBr) => {
            "[Mustard] {count} pendências abertas. A lista inteira sai com `mustard-rt run pending`."
        }
        ("pending.count.many", Locale::EnUs) => {
            "[Mustard] {count} open pending items. The whole list comes from `mustard-rt run pending`."
        }
        // The complement of the count line, when there are idle items. It
        // starts with a space: it goes glued to its end. `{stale}` comes from
        // the caller.
        ("pending.count.stale", Locale::PtBr) => {
            " {stale} delas estão paradas há mais de 30 dias: rode `mustard-rt run pending --stale` \
             e pergunte ao usuário, numa pergunta só, quais ficam."
        }
        ("pending.count.stale", Locale::EnUs) => {
            " {stale} of them have been idle for over 30 days: run `mustard-rt run pending --stale` \
             and ask the user, in one question, which ones stay."
        }
        // The sweep, the two-call removal and the undo of `run pending`
        // (`apps/rt/src/commands/event/pending.rs`).
        ("pending.stale.question", Locale::PtBr) => {
            "Estas pendências estão paradas há mais de 30 dias. Marque as que ficam; as outras \
             saem como vencidas."
        }
        ("pending.stale.question", Locale::EnUs) => {
            "These pending items have been idle for over 30 days. Mark the ones that stay; the \
             others leave as expired."
        }
        ("pending.expired_reason", Locale::PtBr) => "vencida",
        ("pending.expired_reason", Locale::EnUs) => "expired",
        ("pending.remove.preview", Locale::PtBr) => {
            "Sairiam {count} pendências: {items}. Motivo: {reason}. Confirme com o usuário e rode \
             de novo com `--confirm {token}`."
        }
        ("pending.remove.preview", Locale::EnUs) => {
            "{count} pending items would leave: {items}. Reason: {reason}. Confirm with the user \
             and run again with `--confirm {token}`."
        }
        ("pending.nothing_matches", Locale::PtBr) => {
            "Nenhuma pendência aberta casa com {selector}. Nada foi removido."
        }
        ("pending.nothing_matches", Locale::EnUs) => {
            "No open pending item matches {selector}. Nothing was removed."
        }
        ("pending.confirm_mismatch", Locale::PtBr) => {
            "A lista mudou desde a prévia: o conjunto de agora não é o que foi confirmado. Rode a \
             prévia de novo. Nada foi removido."
        }
        ("pending.confirm_mismatch", Locale::EnUs) => {
            "The list changed since the preview: the current set is not the confirmed one. Run \
             the preview again. Nothing was removed."
        }
        ("pending.reason_required", Locale::PtBr) => {
            "Toda remoção leva um motivo: passe `--reason`. Nada foi removido."
        }
        ("pending.reason_required", Locale::EnUs) => {
            "Every removal needs a reason: pass `--reason`. Nothing was removed."
        }
        ("pending.selector_required", Locale::PtBr) => {
            "Diga o que remover: `--id`, `--term` ou `--before`. Nada foi removido."
        }
        ("pending.selector_required", Locale::EnUs) => {
            "Say what to remove: `--id`, `--term` or `--before`. Nothing was removed."
        }
        ("pending.bad_date", Locale::PtBr) => {
            "A data {date} não se lê: use o formato AAAA-MM-DD, como 2026-08-01. Nada foi removido."
        }
        ("pending.bad_date", Locale::EnUs) => {
            "The date {date} does not parse: use the YYYY-MM-DD form, like 2026-08-01. Nothing \
             was removed."
        }
        ("pending.not_dropped", Locale::PtBr) => {
            "A pendência {id} não está descartada: só uma descartada volta a aberta. Nada mudou."
        }
        ("pending.not_dropped", Locale::EnUs) => {
            "The pending item {id} is not dropped: only a dropped item goes back to open. Nothing \
             changed."
        }
        // The end-of-answer pending rule
        // (`apps/rt/src/hooks/task/pending_gate.rs`). `{count}` and `{items}`
        // come from the caller; the list uses the spelling of
        // `format_pending_items`. The block asks for the title, never the
        // number: the writing rule bars the internal code ("P-3") in the
        // conversation.
        ("pending.gate.block", Locale::PtBr) => {
            "[Mustard] A spec {spec} fechou neste turno, e a mensagem final não cita {count} das \
             pendências abertas que nasceram nela: {items}. O trabalho combinado sobrevive à spec \
             que fechou: reescreva a mensagem de fechamento citando cada uma pelo título, sem o \
             número. Uma pendência que não vale mais só sai da lista com um motivo: \
             `mustard-rt run pending --close <id> --reason \"…\"` quando foi entregue, ou \
             `mustard-rt run pending --drop <id> --reason \"…\"` quando o usuário desistiu."
        }
        ("pending.gate.block", Locale::EnUs) => {
            "[Mustard] The spec {spec} closed in this turn, and the final message does not name \
             {count} of the open pending items born in it: {items}. Agreed work outlives the spec \
             that closed: rewrite the closing message naming each one by title, without the \
             number. An item that no longer stands leaves the list only with a reason: \
             `mustard-rt run pending --close <id> --reason \"…\"` when it was delivered, or \
             `mustard-rt run pending --drop <id> --reason \"…\"` when the user gave it up."
        }
        // Recusa do `run pending --add` (`apps/rt/src/commands/event/pending.rs`):
        // o título repetido, sem ligar para maiúscula nem acento.
        ("pending.duplicate", Locale::PtBr) => {
            "Já existe uma pendência aberta com esse título: {id} \"{title}\". Nada foi gravado. \
             Para mudar o combinado, feche a antiga com `mustard-rt run pending --close {id} \
             --reason \"…\"` ou use outro título."
        }
        ("pending.duplicate", Locale::EnUs) => {
            "An open pending item already has this title: {id} \"{title}\". Nothing was written. \
             To change what was agreed, close the old one with `mustard-rt run pending --close \
             {id} --reason \"…\"` or pick another title."
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
            include_str!("pending.rs"),
            super::PREFIXES,
            14,
            0xcb66_b5e0_929d_1da5,
        );
    }

    /// The pending advisories come from the catalog in both languages, and
    /// each carries the slots the caller fills. The texts of the end-of-answer
    /// hooks that left (the summary delivery, the QA on `Stop`, the reminder to
    /// record the conversation) and the next-message advisory left with them,
    /// the invented-name defect left with its measure, and the two texts of the
    /// old-flow criteria copy left with it.
    #[test]
    fn i18n_translates_doc_and_pending_keys() {
        for (key, slots) in [
            ("doc.section.flow", &[][..]),
            ("pending.count.one", &[][..]),
            ("pending.count.many", &["{count}"][..]),
            ("pending.count.stale", &["{stale}"][..]),
            ("pending.stale.question", &[][..]),
            ("pending.expired_reason", &[][..]),
            ("pending.remove.preview", &["{count}", "{items}", "{reason}", "{token}"][..]),
            ("pending.nothing_matches", &["{selector}"][..]),
            ("pending.confirm_mismatch", &[][..]),
            ("pending.reason_required", &[][..]),
            ("pending.selector_required", &[][..]),
            ("pending.bad_date", &["{date}"][..]),
            ("pending.not_dropped", &["{id}"][..]),
            ("pending.gate.block", &["{spec}", "{count}", "{items}"][..]),
            ("pending.duplicate", &["{id}", "{title}"][..]),
            ("scratch.residue.notice", &["{total}", "{count}"][..]),
            ("session.landed.settled", &["{branch}"][..]),
            ("session.landed.unsettled", &["{branch}", "{reason}"][..]),
            ("session.landed.pending", &["{items}"][..]),
            ("statusline.wave", &["{delivered}", "{total}"][..]),
        ] {
            let (pt, en) = (translate(key, Locale::PtBr), translate(key, Locale::EnUs));
            assert_ne!(pt, "<missing-key>", "{key} missing in pt-BR");
            assert_ne!(en, "<missing-key>", "{key} missing in en-US");
            assert_ne!(pt, en, "{key} must differ per locale");
            for slot in slots {
                assert!(pt.contains(slot) && en.contains(slot), "{key} lost {slot}");
            }
        }
        // O aviso de sobras manda, nos dois idiomas, para o comando de limpeza
        // que existe: listar sem nada e apagar com `--apply`.
        for lang in [Locale::PtBr, Locale::EnUs] {
            let notice = translate("scratch.residue.notice", lang);
            assert!(notice.contains("`mustard-rt run clean`"), "the notice names the list call: {notice}");
            assert!(notice.contains("`mustard-rt run clean --apply`"), "and the delete call: {notice}");
        }
        for key in [
            "deliver.order",
            "deliver.publish",
            "stopgate.block.reason",
            "crystallise.nudge",
            "clarity.next.head",
            "clarity.unexplained_term",
            "pending.notice",
            "spec_events.criteria_from_spec_md",
            "spec_events.drafted_spec",
        ] {
            for lang in [Locale::PtBr, Locale::EnUs] {
                assert_eq!(translate(key, lang), "<missing-key>", "{key} left with its hook");
            }
        }
    }
}
