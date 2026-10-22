//! The masked equivalent of a command whose supported outer configuration
//! reads go through `mustard-rt run config-read`. The hook refuses the
//! original and hands this command back; it never executes any part of the
//! caller's shell, nor offers a command for an unrecognised protected read.

use mustard_core::domain::model::contract::{Ctx, HookInput};

use super::{lex, reading};
use crate::commands::maint::config_read::supported;
use crate::shared::{config_key, search_gateway::{literal_shell, quote}};

/// `command` with each supported read of the configuration file replaced by
/// the masked read, the other steps kept in order. `None` when no read can be
/// replaced safely, or when the replaced command would still expose the key.
pub(super) fn masked_command(command: &str, input: &HookInput, ctx: &Ctx) -> Option<String> {
    let binary = std::env::current_exe().ok()?;
    let binary = quote(&crate::shared::paths::canonical(&binary.to_string_lossy()));
    // A travessia das etapas e a troca de trás para frente moram em
    // [`super::rewrite`], com a recusa da busca que varre o arquivo: as duas
    // reescritas fazem a mesma coisa, e a faziam cada uma na sua cópia
    // (21/10/2026).
    let rewritten = super::rewrite::rewritten(command, |trimmed| {
        let parts = lex::segments(trimmed);
        // Redirects, wrappers, nested shells/substitutions and scripts stay on
        // the refusal path. Literal patterns remain data, including $ in ''.
        let [part] = parts.as_slice() else { return None };
        let file = part.args.last().filter(|w| config_key::is_config_file(&w.text))?;
        if !part.redirects.is_empty() || !part.leading.is_empty()
            || !literal_shell(trimmed) || part.program.text != part.name()
            || !trimmed.strip_prefix(&part.program.raw).is_some_and(|rest| rest.starts_with(char::is_whitespace))
            || part.args.iter().any(|word| !literal_word(&word.raw))
        { return None; }
        let args: Vec<String> = part.args[..part.args.len()-1].iter().map(|w| w.text.clone()).collect();
        if !supported(part.name(), &args) { return None; }
        Some(format!("{binary} run config-read --file {} --reader {} --args-json {}",
            quote(&file.text), quote(part.name()), quote(&serde_json::to_string(&args).ok()?)))
    })?;
    // Recheck the whole command, not just the first offending segment. A
    // second read through a wildcard, recursion or substitution still denies.
    if reading::bash_reading(&lex::segments(&rewritten), &rewritten, input, ctx).is_some() { return None; }
    Some(rewritten)
}

// Do not freeze shell globs, tilde or brace expansion into literal argv.
// Existing quotes and escaped wildcard characters remain literal data.
fn literal_word(raw: &str) -> bool {
    let mut quote = None;
    let mut escaped = false;
    for c in raw.chars() {
        if escaped { escaped = false; continue; }
        if quote == Some('\'') {
            if c == '\'' { quote = None; }
            continue;
        }
        if c == '\\' { escaped = true; continue; }
        if quote == Some('"') {
            if c == '"' { quote = None; }
            continue;
        }
        if matches!(c, '\'' | '"') { quote = Some(c); continue; }
        if matches!(c, '*' | '?' | '[' | ']' | '~' | '{' | '}') { return false; }
    }
    true
}
