//! Os trechos de conserto: o que a rodada devolve ao agente de uma onda cuja
//! volta ela recusou, gravado na pasta de despacho da spec para a volta
//! pendente da onda. Gravam aqui a conferência depois da onda, o comando
//! declarado que caiu antes do commit e a reprovação de quem conduz; o gancho
//! da mensagem ao agente da onda e o do despacho do agente novo leem daqui.

use std::path::{Path, PathBuf};

use mustard_core::domain::spec_events::SpecLog;
use mustard_core::domain::wave_prompt::wave_title;
use mustard_core::io::spec_events as store;
use mustard_core::platform::i18n::{translate, Locale};

use super::answer::RoundRefusal;
use super::queue::waves_awaiting_new_agent;

/// O arquivo com o trecho que a conferência depois da onda, ou o comando
/// declarado que caiu antes do commit, devolveu para a onda `wave`, gravado
/// pela volta que a rodada recusou, ou com o motivo de quem conduz a obra,
/// pela volta que ele reprovou: a última entrega da onda
/// que nenhuma rodada assumiu. A entrega nova muda o arquivo, e o trecho de
/// uma volta velha nunca chega ao agente. Nada sem volta pendente da onda.
pub(crate) fn fix_file(root: &Path, spec: &str, log: &SpecLog, wave: u64) -> Option<PathBuf> {
    let back = log.unassumed_returns().into_iter().filter(|e| e.event_type == "delivered" && e.wave() == Some(wave)).map(|e| e.id).max()?;
    Some(fixes_dir(root, spec)?.join(format!("fix-{wave}-{back}.md")))
}

/// A recusa da conferência depois da onda, ou a do comando declarado que caiu
/// antes do commit, com, para cada onda recusada que espera um agente novo
/// ([`waves_awaiting_new_agent`]), a frase que manda o condutor despachar um
/// pelo título dela; a onda que não está ali segue com o conserto mandado ao
/// agente que a fez. Outra recusa sai como veio.
pub(super) fn new_agents_named(refused: RoundRefusal, root: &Path, spec: &str, log: &SpecLog, lang: Locale) -> RoundRefusal {
    match refused {
        RoundRefusal::AfterWave { mut text, question, fixes } => {
            let awaiting = waves_awaiting_new_agent(root, spec, log);
            for wave in fixes.iter().map(|(wave, _)| *wave).filter(|wave| awaiting.contains_key(wave)) {
                let line = translate("round.after_wave.new_agent", lang)
                    .replace("{wave}", &wave.to_string())
                    .replace("{title}", &wave_title(spec, wave, lang));
                text = format!("{text}\n\n{line}");
            }
            RoundRefusal::AfterWave { text, question, fixes }
        }
        RoundRefusal::CheckFailed(mut failure) => {
            let awaiting = waves_awaiting_new_agent(root, spec, log);
            let waves = failure.waves.iter().copied().filter(|wave| awaiting.contains_key(wave));
            failure.new_agents = waves.map(|wave| (wave, wave_title(spec, wave, lang))).collect();
            RoundRefusal::CheckFailed(failure)
        }
        other => other,
    }
}

/// A pasta de despacho da spec, onde moram os trechos de conserto.
fn fixes_dir(root: &Path, spec: &str) -> Option<PathBuf> {
    Some(store::spec_file(root, spec).ok()?.parent()?.join(".dispatch"))
}

/// A onda do trecho de conserto `path`, pelo nome que [`fix_file`] dá a ele;
/// nada para outro arquivo da pasta.
fn fix_wave(path: &Path) -> Option<u64> {
    let name = path.file_name()?.to_str()?.strip_prefix("fix-")?.strip_suffix(".md")?;
    let (wave, back) = name.split_once('-')?;
    back.parse::<u64>().ok().and(wave.parse().ok())
}

/// Apaga da pasta de despacho da spec cada trecho de conserto que não é mais
/// o da volta pendente da onda dele ([`fix_file`], pela leitura `log`): o da
/// volta velha, que a entrega nova trocou, e o da onda que a rodada já
/// comitou. Sem leitura, como no fechamento, todos saem. A pasta vazia sai
/// junto; o arquivo que não sai fica para a próxima varredura. Quem chama
/// segura a trava do passo do git e leu `log` sob ela: a rodada ao mesmo
/// tempo nunca grava um trecho mais novo que esta leitura.
pub(crate) fn sweep_fixes(root: &Path, spec: &str, log: Option<&SpecLog>) {
    let Some(dir) = fixes_dir(root, spec) else { return };
    let Ok(entries) = std::fs::read_dir(&dir) else { return };
    for path in entries.flatten().map(|entry| entry.path()) {
        let Some(wave) = fix_wave(&path) else { continue };
        if log.and_then(|log| fix_file(root, spec, log, wave)).as_ref() != Some(&path) {
            let _ = std::fs::remove_file(&path);
        }
    }
    let _ = std::fs::remove_dir(&dir);
}

/// Grava, para cada onda que a conferência depois da onda recusou, o trecho
/// dela no arquivo da volta recusada ([`fix_file`]), onde o gancho da
/// mensagem ao agente da onda o lê; e, para cada onda a quem volta o conserto
/// do comando declarado que caiu antes do commit, o comando e o fim da saída,
/// no idioma `lang`. Antes, toda recusa varre os trechos que ficaram velhos
/// ([`sweep_fixes`]); outra recusa não grava nada, e a falha de gravação
/// deixa a mensagem do condutor passar como veio.
pub(super) fn keep_fixes(root: &Path, spec: &str, log: &SpecLog, refused: &RoundRefusal, lang: Locale) {
    sweep_fixes(root, spec, Some(log));
    let fixes: Vec<(u64, String)> = match refused {
        RoundRefusal::AfterWave { fixes, .. } => fixes.clone(),
        RoundRefusal::CheckFailed(failure) => failure.waves.iter().map(|wave| (*wave, failure.fix(*wave, lang))).collect(),
        _ => return,
    };
    for (wave, section) in fixes {
        let Some(file) = fix_file(root, spec, log, wave) else { continue };
        let _ = file.parent().map(std::fs::create_dir_all);
        let _ = std::fs::write(&file, section);
    }
}
