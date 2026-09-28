//! A conferência de cada volta contra a cópia da onda dela, antes da junção.
//! A lista que a entrega cita vira só conferência: o que entra no commit é o
//! que a cópia da onda mudou de fato, pelo `git status` dela — inclusive o
//! arquivo que a entrega não citou. A divergência entre as duas vira aviso,
//! com quantos arquivos mudaram, quantos a onda citou e quais ficaram de fora
//! da citação. A volta que a conferência recusa segura só a própria onda
//! ([`HeldReturn`]), e as outras seguem.

use std::collections::BTreeSet;
use std::path::Path;

use mustard_core::domain::spec_events::SpecLog;
use mustard_core::platform::i18n::{translate, Locale};
use serde_json::{json, Value};

use super::answer::RoundRefusal;
use super::commit::{real_changed_files, unknown_file};
use super::report::WaveReport;
use super::stops::{hold_refused, HeldReturn};

/// Confere cada volta de `waves` contra a cópia dela, lida em `log`: troca a
/// lista citada pelos arquivos que a cópia mudou, e tira de `waves` a volta
/// sem título de commit com a cópia mudada e a que cita arquivo que não está
/// no disco nem no git. Devolve os avisos de divergência, no idioma `lang`, e
/// cada volta tirada, com a recusa dela.
pub(super) fn check_against_copies(
    root: &Path,
    log: &SpecLog,
    waves: &mut Vec<WaveReport>,
    lang: Locale,
) -> (Vec<Value>, Vec<HeldReturn>) {
    let mut warnings: Vec<Value> = Vec::new();
    let mut needs_commit: BTreeSet<u64> = BTreeSet::new();
    for wave in waves.iter_mut() {
        let Some(actual) = real_changed_files(root, log, wave.wave) else { continue };
        // Cópia sem diff nenhum (comum nos testes, que escrevem direto na
        // raiz do checkout em vez da cópia da onda) não conta como
        // divergência nem apaga a lista declarada: sem nada de real para
        // comparar, a conferência não tem o que dizer. É também a cópia da
        // onda que só foi conferir: sem arquivo mudado, não há o que comitar.
        if actual.is_empty() {
            continue;
        }
        // A cópia mudou arquivo de verdade: a entrega precisa do resumo do
        // commit, mesmo tendo voltado sem citar arquivo nenhum. É a única
        // conferência da volta que depende da cópia, e por isso fica aqui, e
        // não na gravação: o que a onda mexeu nunca entra no repositório
        // principal sem título de commit. A volta fica de fora, com a cópia
        // intacta, e o aviso pede que o agente grave a entrega de novo.
        if wave.commit.is_none() {
            needs_commit.insert(wave.wave);
            continue;
        }
        let declared: BTreeSet<&str> = wave.files.iter().map(String::as_str).collect();
        let actual_set: BTreeSet<&str> = actual.iter().map(String::as_str).collect();
        if declared != actual_set {
            let left_out: Vec<String> = actual_set.difference(&declared).map(|s| (*s).to_string()).collect();
            warnings.push(json!({
                "reason": "files-diverged",
                "wave": wave.wave,
                "hint": translate("round.files_diverged", lang)
                    .replace("{wave}", &wave.wave.to_string())
                    .replace("{changed}", &actual.len().to_string())
                    .replace("{declared}", &declared.len().to_string())
                    .replace("{missing}", &left_out.join(", ")),
            }));
        }
        wave.files = actual;
    }
    let refused = hold_refused(waves, |wave| {
        if needs_commit.contains(&wave.wave) {
            return Err(RoundRefusal::ReturnNeedsCommit { wave: wave.wave });
        }
        unknown_file(root, log, std::slice::from_ref(wave))
    });
    (warnings, refused)
}
