//! O que a rodada já fez quando uma recusa chega no meio do despacho: o
//! commit, o que ela gravou e os envios já gravados saem junto da recusa, e
//! nunca somem da resposta.

use std::path::Path;

use mustard_core::platform::i18n::Locale;
use serde_json::{Value, json};

use super::answer::{RoundRefusal, end_answer};

/// O que a rodada já fez antes do despacho, e os envios que ele grava: é o
/// que a recusa do meio do despacho leva junto ([`Done::after_work`]).
pub(super) struct Done {
    pub(super) recorded: Vec<Value>,
    pub(super) formatted: Vec<String>,
    pub(super) warnings: Vec<Value>,
    pub(super) commit: Option<Value>,
    pub(super) dispatched: Vec<Value>,
}

impl Done {
    /// A recusa `refusal` do despacho, com o que a rodada já tinha feito
    /// (`self`): o commit, o que ela gravou e os envios já gravados não somem da
    /// resposta. Sem nada feito, a recusa sai como veio; a do ciclo já leva o
    /// que foi feito.
    pub(super) fn after_work(&self, refusal: RoundRefusal, root: &Path, spec: &str, lang: Locale) -> RoundRefusal {
        let nothing = self.commit.is_none() && self.recorded.is_empty() && self.dispatched.is_empty();
        if nothing || matches!(refusal, RoundRefusal::WaveLoop { .. } | RoundRefusal::AfterWork { .. }) {
            return refusal;
        }
        let mut answered = json!({ "spec": spec, "recorded": self.recorded, "formatted": self.formatted, "dispatch": self.dispatched });
        if let Some(commit) = &self.commit {
            answered["commit"] = commit.clone();
        }
        if !self.warnings.is_empty() {
            answered["warnings"] = json!(self.warnings);
        }
        end_answer(root, spec, &mut answered, &refusal.message(lang), lang);
        RoundRefusal::AfterWork { refusal: Box::new(refusal), answered }
    }
}
