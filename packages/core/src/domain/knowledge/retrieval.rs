//! Deterministic vocabulary and resource preference. These rules help locate
//! evidence; they never certify behavior or manufacture a business explanation.
use std::collections::BTreeSet;

use crate::domain::normalize::{Languages, Normalizer};

const RULES: &str = include_str!("retrieval.txt");

pub(super) struct Terms {
    pub asked: Vec<Vec<String>>,
    executable: bool,
    data: bool,
    executable_kinds: BTreeSet<String>,
    data_kinds: BTreeSet<String>,
}

impl Terms {
    pub fn of(query: &str, languages: &Languages) -> Self {
        let mut normalizer = Normalizer::new(languages);
        let mut out = Self {
            asked: normalizer.query(query),
            executable: false,
            data: false,
            executable_kinds: BTreeSet::new(),
            data_kinds: BTreeSet::new(),
        };
        let original = out.asked.clone();
        for line in RULES
            .lines()
            .map(str::trim)
            .filter(|s| !s.is_empty() && !s.starts_with('#'))
        {
            let mut words = line.split_whitespace();
            let Some(first) = words.next() else { continue };
            if matches!(first, "@executable" | "@data") {
                let kinds = if first == "@executable" {
                    &mut out.executable_kinds
                } else {
                    &mut out.data_kinds
                };
                kinds.extend(words.map(str::to_string));
                continue;
            }
            let tagged = first.starts_with('@');
            let text = if tagged {
                words.collect::<Vec<_>>().join(" ")
            } else {
                line.to_string()
            };
            let aliases: BTreeSet<_> = normalizer.forms(&text).into_iter().flatten().collect();
            let mut matched = false;
            for (forms, own) in out.asked.iter_mut().zip(&original) {
                if own.iter().any(|form| aliases.contains(form)) {
                    matched = true;
                    forms.extend(aliases.iter().cloned());
                    forms.sort();
                    forms.dedup();
                }
            }
            if matched {
                out.executable |= first == "@action";
                out.data |= first == "@definition";
            }
        }
        out
    }

    pub fn weight(&self, kind: &str) -> f64 {
        if self.executable && !self.data && self.executable_kinds.contains(kind)
            || self.data && !self.executable && self.data_kinds.contains(kind)
        {
            1.25
        } else {
            1.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn technical_plural_aliases_keep_native_accents_and_do_not_require_project_specific_words() {
        let language=Languages::new(["pt-BR","en-US"]);
        let mut normalizer=Normalizer::new(&language);
        for (question,code) in [("requisições","request"),("cabeçalhos","header"),("durações","duration"),("exceções","exception")] {
            let terms=Terms::of(question,&language);
            let forms=normalizer.forms(code);
            assert!(terms.asked.iter().any(|slot|forms.iter().flatten().any(|form|slot.contains(form))),"{question}: {:?}",terms.asked);
        }
    }

    #[test]
    fn morphology_and_equivalent_words_keep_one_slot_and_do_not_chain_aliases() {
        let languages = Languages::new(["pt-BR", "en-US"]);
        let mut normalizer = Normalizer::new(&languages);
        let own = normalizer.query("Onde são calculados os indicadores?");
        let terms = Terms::of("Onde são calculados os indicadores?", &languages);
        assert_eq!(terms.asked.len(), own.len());
        let calculate = normalizer.forms("calculate");
        assert!(
            terms
                .asked
                .iter()
                .any(|slot| calculate[0].iter().any(|form| slot.contains(form)))
        );
        assert!(terms.weight("method") > terms.weight("field"));
        assert!((Terms::of("tabela para calcular", &languages).weight("method") - 1.0).abs() < f64::EPSILON);
        assert_eq!(
            Terms::of("UnknownIdentifier", &languages).asked,
            normalizer.query("UnknownIdentifier")
        );
    }
}
