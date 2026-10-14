//! API context tiers are reference metadata, not subscription billing.
//! Unknown aliases or missing full-prompt usage must stay unknown.
use serde_json::{Value, json};

pub(super) const TARGET: u64 = 100_000;

pub(super) fn report(host: &str, model: &str, input_tokens: Option<u64>) -> Value {
    // Claude's `haiku` alias does not prove a 5.5 execution.
    let policy = match (host, model) {
        ("claude", "claude-haiku-5-5") => Some((100_000, 5.0, 5.0,
            "https://platform.claude.com/docs/en/models/haiku-5-5/overview")),
        ("codex", "gpt-6-luna") => Some((272_000, 2.0, 1.5,
            "https://developers.openai.com/api/docs/models/gpt-6-luna")),
        _ => None,
    };
    let tier = match (policy, input_tokens) {
        (Some((threshold, ..)), Some(tokens)) => if tokens > threshold {"long"} else {"standard"},
        _ => "unknown",
    };
    json!({"max_input_tokens":TARGET,"target_observed":input_tokens.map(|tokens|tokens<=TARGET),
        "full_input_tokens":input_tokens,"usage_source":"caller receipt; not independently verified host telemetry",
        "api_reference":{"tier":tier,"long_context_above_tokens":policy.map(|p|p.0),
            "long_input_and_cache_multiplier":policy.map(|p|p.1),"long_output_multiplier":policy.map(|p|p.2),
            "source":policy.map(|p|p.3),"checked_at":policy.map(|_|"2026-10-12")},
        "subscription_cost":"unknown",
        "next":if input_tokens.is_some_and(|tokens|tokens>TARGET) {
            "Do not accept an over-budget session receipt; use native evidence or prepare a smaller isolated task without the parent transcript."
        } else {"Reuse current knowledge before requesting another interpretation."}})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_prompt_boundaries_follow_the_actual_model_without_guessing_aliases() {
        for (host, model, boundary, input, output) in [
            ("claude", "claude-haiku-5-5", 100_000, 5.0, 5.0),
            ("codex", "gpt-6-luna", 272_000, 2.0, 1.5),
        ] {
            assert_eq!(report(host, model, Some(boundary))["api_reference"]["tier"], "standard");
            let over = report(host, model, Some(boundary + 1));
            assert_eq!(over["api_reference"]["tier"], "long");
            assert_eq!(over["api_reference"]["long_input_and_cache_multiplier"], input);
            assert_eq!(over["api_reference"]["long_output_multiplier"], output);
            assert_eq!(over["target_observed"], false);
            assert_eq!(over["subscription_cost"], "unknown");
        }
        let luna = report("codex", "gpt-6-luna", Some(100_001));
        assert_eq!(luna["api_reference"]["tier"], "standard");
        assert_eq!(luna["target_observed"], false);
        for (host, model, tokens) in [("claude", "haiku", Some(300_000)),
            ("codex", "inherit", Some(90_000)), ("claude", "claude-haiku-5-5", None)] {
            let unknown = report(host, model, tokens);
            assert_eq!(unknown["api_reference"]["tier"], "unknown");
            assert_eq!(unknown["target_observed"], json!(tokens.map(|n|n<=TARGET)));
        }
    }
}
