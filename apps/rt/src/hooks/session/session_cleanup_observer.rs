//! `session_cleanup_observer` — a faxina do fim da sessão.
//!
//! No `SessionEnd`, dois passos, na ordem de [`STEPS`]:
//!
//! 1. limpar o cache da barra de status, no diretório temporário;
//! 2. compactar o estado da sessão: as fotos de `.claude/.compact-state/` com
//!    mais de 24 horas saem, e a pasta sai quando fica vazia.
//!
//! Os dois mexem só em caminhos conhecidos e nenhum chama processo de fora: o
//! `SessionEnd` tem o prazo mais curto que o harness dá, e um passo que
//! pudesse travar deixaria a faxina pela metade.
//!
//! É só efeito colateral, sem veredito: `SessionCleanupObserver` é um
//! [`Observer`]. Nada aqui falha para quem chama.

use mustard_core::domain::model::contract::{Ctx, HookInput, Observer, Trigger};
use mustard_core::io::fs;
use mustard_core::ClaudePaths;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

/// As fotos de `.compact-state` com mais que isto saem: 24 horas.
const ONE_DAY_MS: u128 = 24 * 60 * 60 * 1000;

/// O nome do cache da barra de status no diretório temporário.
const STATUSLINE_CACHE: &str = "claude-statusline-git.json";

/// A faxina do fim da sessão.
pub struct SessionCleanupObserver;

/// Um passo da faxina: recebe a pasta `.claude` do projeto.
type Step = fn(&Path);

/// Os passos da faxina, na ordem em que rodam.
const STEPS: &[Step] = &[clean_statusline_cache, clean_compact_state];

/// Tira o cache da barra de status do diretório temporário.
fn clean_statusline_cache(_claude: &Path) {
    let _ = fs::remove_file(std::env::temp_dir().join(STATUSLINE_CACHE));
}

/// Tira as fotos de `.compact-state` com mais de 24 horas, e a pasta quando
/// ela fica vazia.
fn clean_compact_state(claude: &Path) {
    let dir = claude.join(".compact-state");
    let Ok(entries) = fs::read_dir(&dir) else {
        return;
    };
    let now = mustard_core::time::now_unix_millis() as u128;
    let mut remaining = 0;
    for entry in entries {
        let Ok(modified) = fs::modified(&entry.path) else {
            remaining += 1;
            continue;
        };
        let mtime_ms = modified.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis());
        if now.saturating_sub(mtime_ms) > ONE_DAY_MS {
            let _ = fs::remove_file(&entry.path);
        } else {
            remaining += 1;
        }
    }
    if remaining == 0 {
        // O `remove_dir` não tem equivalente na fachada; um uso só.
        let _ = std::fs::remove_dir(&dir);
    }
}

/// A pasta `.claude` do projeto em `cwd`, quando o caminho é de projeto.
fn claude_dir(cwd: &str) -> Option<PathBuf> {
    ClaudePaths::for_project(Path::new(cwd)).ok().map(|paths| paths.claude_dir())
}

impl Observer for SessionCleanupObserver {
    /// No `SessionEnd`, roda os [`STEPS`]; em qualquer outro evento, nada.
    fn observe(&self, input: &HookInput, ctx: &Ctx) {
        if ctx.trigger != Some(Trigger::SessionEnd) {
            return;
        }
        let Some(claude) = claude_dir(&ctx.project_dir_or_cwd(input)) else {
            return;
        };
        for step in STEPS {
            step(&claude);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, SystemTime};
    use tempfile::tempdir;

    fn ctx(dir: &Path, trigger: Trigger) -> Ctx {
        Ctx::for_test(dir.to_string_lossy().into_owned(), Some(trigger))
    }

    fn session_end_input() -> HookInput {
        HookInput { hook_event_name: Some("SessionEnd".to_string()), ..HookInput::default() }
    }

    /// Uma foto de `.compact-state` com a data de `age` atrás.
    fn snapshot(dir: &Path, name: &str, age: Duration) -> PathBuf {
        let compact = dir.join(".claude").join(".compact-state");
        std::fs::create_dir_all(&compact).unwrap();
        let path = compact.join(name);
        std::fs::write(&path, "foto").unwrap();
        let file = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
        file.set_modified(SystemTime::now() - age).unwrap();
        path
    }

    /// A faxina tem dois passos, e só eles: o cache da barra de status e o
    /// estado da sessão.
    #[test]
    fn the_cleanup_has_exactly_two_steps() {
        let expected: [Step; 2] = [clean_statusline_cache, clean_compact_state];
        assert_eq!(STEPS.len(), expected.len());
        for (step, want) in STEPS.iter().zip(expected) {
            assert!(std::ptr::fn_addr_eq(*step, want), "the steps are the two of the cleanup, in order");
        }
    }

    /// No fim da sessão, a foto velha sai e a nova fica; a pasta só sai
    /// vazia.
    #[test]
    fn old_compact_state_snapshots_leave_and_new_ones_stay() {
        let dir = tempdir().unwrap();
        let old = snapshot(dir.path(), "old.json", Duration::from_secs(2 * 24 * 60 * 60));
        let new = snapshot(dir.path(), "new.json", Duration::from_secs(60));
        SessionCleanupObserver.observe(&session_end_input(), &ctx(dir.path(), Trigger::SessionEnd));
        assert!(!old.exists(), "the old snapshot left");
        assert!(new.exists(), "the new one stays, and so does the folder");

        std::fs::remove_file(&new).unwrap();
        snapshot(dir.path(), "velha.json", Duration::from_secs(3 * 24 * 60 * 60));
        SessionCleanupObserver.observe(&session_end_input(), &ctx(dir.path(), Trigger::SessionEnd));
        assert!(!dir.path().join(".claude").join(".compact-state").exists(), "the empty folder leaves");
    }

    /// Fora do fim da sessão, nada muda; num projeto sem `.claude`, nada
    /// falha.
    #[test]
    fn outside_session_end_nothing_changes_and_nothing_fails() {
        let dir = tempdir().unwrap();
        let old = snapshot(dir.path(), "old.json", Duration::from_secs(2 * 24 * 60 * 60));
        SessionCleanupObserver.observe(&session_end_input(), &ctx(dir.path(), Trigger::PreToolUse));
        assert!(old.exists(), "another event cleans nothing");

        let empty = tempdir().unwrap();
        SessionCleanupObserver.observe(&session_end_input(), &ctx(empty.path(), Trigger::SessionEnd));
    }
}
