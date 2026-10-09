//! Deterministic task context and matching, shared by discovery and live search.
//! Coverage counts written clues; it is not a probability of semantic correctness.
use crate::domain::normalize::{Languages, Normalizer};
use crate::domain::vocabulary::aho::KeyedAutomaton;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Purpose {
    #[default]
    Locate,
    Understand,
    Spec,
    Implement,
    Validate,
}

impl Purpose {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "locate" => Some(Self::Locate),
            "understand" => Some(Self::Understand),
            "spec" => Some(Self::Spec),
            "implement" => Some(Self::Implement),
            "validate" => Some(Self::Validate),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Task<'a> {
    pub intent: &'a str,
    pub purpose: Purpose,
}

pub struct Matcher {
    normalizer: Normalizer,
    pub slots: Vec<Vec<String>>,
    needles: Option<KeyedAutomaton<usize>>,
}

impl Matcher {
    pub fn new(text: &str, languages: &Languages) -> Result<Self, String> {
        let slots = super::resources::query_terms(text, languages);
        let needles: BTreeSet<_> = slots
            .iter()
            .flatten()
            // Short prefixes are a coarse prefilter: inflected spellings may
            // differ at the stem's end (e.g. study/studies). Full normalized
            // words are checked below; a prefix is never evidence by itself.
            .map(|word| word.chars().take(3).collect::<String>())
            .collect();
        let needles = if needles.is_empty() {
            None
        } else {
            Some(
                KeyedAutomaton::from_groups([(0, needles.into_iter().collect())])
                    .map_err(|e| e.to_string())?,
            )
        };
        Ok(Self {
            normalizer: Normalizer::new(languages),
            slots,
            needles,
        })
    }

    pub fn matched(&mut self, text: &str) -> BTreeSet<usize> {
        let forms: BTreeSet<_> = self.normalizer.forms(text).into_iter().flatten().collect();
        self.slots
            .iter()
            .enumerate()
            .filter(|(_, slot)| slot.iter().any(|word| forms.contains(word)))
            .map(|(at, _)| at)
            .collect()
    }

    /// Only a prefilter. Stems/aliases must still match complete normalized
    /// words, so a substring such as `auth` inside an unrelated name is not proof.
    pub fn might_match(&self, text: &str) -> bool {
        !text.is_ascii()
            || self
                .needles
                .as_ref()
                .is_some_and(|needles| needles.is_match(&text.to_ascii_lowercase()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_matches_keep_query_slots_and_reject_substring_only_evidence() {
        let mut matcher = Matcher::new("request header", &Languages::new(["en-US"])).unwrap();
        assert_eq!(matcher.matched("requestHeader"), BTreeSet::from([0, 1]));
        let mut exact = Matcher::new("quartz", &Languages::new(["en-US"])).unwrap();
        assert!(exact.matched("quartzite").is_empty());
        assert_eq!(matcher.matched("request request request").len(), 1);
        let mut inflected = Matcher::new("studies", &Languages::new(["en-US"])).unwrap();
        assert!(inflected.might_match("study notes"));
        assert!(!inflected.matched("study notes").is_empty());
        assert_eq!(Purpose::parse("implement"), Some(Purpose::Implement));
        assert_eq!(Purpose::parse("guess"), None);
    }
}
