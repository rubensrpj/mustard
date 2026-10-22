//! O texto do reenvio de uma onda: o pedido gravado no envio anterior, com
//! as ondas em andamento de agora, os passos já gravados, o resto que o agente
//! anterior deixou guardado e o aviso de começar vendo o que mudou na cópia.

use std::collections::BTreeSet;
use std::path::Path;

use mustard_core::domain::spec_events::SpecLog;
use mustard_core::platform::i18n::{Locale, translate};
use serde_json::Value;

use super::keep::{KeptRest, kept_rest};

/// O texto do reenvio: o pedido gravado no envio anterior, com a parte das
/// ondas em andamento recalculada para as `running` de agora — a gravada
/// fica velha assim que outra onda entrega, entra ou sai de cena —, mais os
/// passos já gravados desta onda, o resto guardado que o agente novo confere
/// e continua ([`kept_block`]) e o aviso de começar vendo o que mudou na
/// cópia. Sem passo nenhum e sem resto guardado, só o aviso.
pub(super) fn resend_text(previous: &str, running: &[(u64, Vec<String>)], steps: &[String], kept: &[KeptRest], lang: Locale) -> String {
    let mut parts = vec![refresh_running(previous, running, lang)];
    if !steps.is_empty() {
        parts.push(translate("round.resume.steps", lang).to_string());
        parts.push(steps.join("\n"));
    }
    parts.extend(kept_block(kept, lang));
    parts.push(translate("round.resume.notice", lang).to_string());
    parts.join("\n\n")
}

/// O resto guardado que o reenvio do envio `previous` da onda `wave` cita
/// ([`kept_rest`]): o dos agentes que este reenvio continua — o do próprio
/// `previous` e o de cada envio que ele reenviou antes, pelo campo `resends`,
/// contando cada versão de cada um. O resto de um agente que fechou sem que
/// o seguinte o trouxesse de volta segue citado no reenvio seguinte.
pub(super) fn kept_rest_of(root: &Path, spec: &str, log: &SpecLog, wave: u64, previous: u64) -> Vec<KeptRest> {
    let mut dispatches = BTreeSet::new();
    let mut at = Some(previous);
    while let Some(sent) = at {
        if !dispatches.insert(log.dispatch_position(sent)) {
            break;
        }
        at = log.send_field(sent, "resends").and_then(Value::as_u64);
    }
    let of_chain = |sent: u64| {
        log.get(sent).is_some_and(|event| event.event_type == "send" && event.wave() == Some(wave))
            && dispatches.contains(&log.dispatch_position(sent))
    };
    kept_rest(root, spec, wave, of_chain)
}

/// O bloco do resto guardado no reenvio: a ordem de conferir cada ref e
/// continuar dela, só descartando com o motivo gravado, e uma linha por ref,
/// com os arquivos que ela traz e, na de submódulo, a pasta dele na cópia.
/// Sem resto guardado, nada.
fn kept_block(kept: &[KeptRest], lang: Locale) -> Option<String> {
    if kept.is_empty() {
        return None;
    }
    let lines = kept.iter().map(|rest| {
        let files = rest.files.iter().map(|file| format!("`{file}`")).collect::<Vec<_>>().join(", ");
        match &rest.folder {
            None => format!("- `{}`: {files}", rest.refname),
            Some(folder) => translate("round.resume.kept_in", lang)
                .replace("{ref}", &rest.refname)
                .replace("{folder}", folder)
                .replace("{files}", &files),
        }
    });
    let mut block = vec![translate("round.resume.kept", lang).to_string()];
    block.extend(lines);
    Some(block.join("\n"))
}

/// A parte das ondas em andamento do pedido anterior (`## Como trabalhar`), trocada
/// pela de agora: a marca é a linha do rótulo
/// (`prompt.execution.running`) e as linhas indentadas logo depois dela, uma
/// por onda, no mesmo formato que o primeiro envio escreve. Sem onda em
/// andamento nenhuma agora, a linha do rótulo e as dela somem; sem elas no
/// texto anterior e com onda em andamento agora, elas nascem no fim da
/// seção. Sem a seção `## Como trabalhar` no texto anterior, nada muda.
fn refresh_running(previous: &str, running: &[(u64, Vec<String>)], lang: Locale) -> String {
    let header = format!("## {}", translate("prompt.part.work", lang));
    let label = format!("- {}", translate("prompt.execution.running", lang));
    let lines: Vec<&str> = previous.lines().collect();
    let Some(head) = lines.iter().position(|line| *line == header) else {
        return previous.to_string();
    };
    // A linha em branco logo depois do título abre a seção; o fim dela é a
    // próxima linha em branco, ou o fim do texto, quando é a última seção.
    let content_at = head + 2;
    let stop = lines[content_at..].iter().position(|line| line.is_empty()).map_or(lines.len(), |n| content_at + n);
    let old_at = lines[content_at..stop].iter().position(|line| *line == label).map(|n| content_at + n);
    let old_end = old_at.map_or(stop, |at| lines[at + 1..stop].iter().position(|line| !line.starts_with("  - ")).map_or(stop, |n| at + 1 + n));
    let fresh: Vec<String> = running
        .iter()
        .map(|(wave, files)| {
            let name = translate("prompt.execution.wave", lang).replace("{n}", &wave.to_string());
            let files: Vec<String> = files.iter().map(|file| format!("`{file}`")).collect();
            if files.is_empty() { format!("  - {name}") } else { format!("  - {name}: {}", files.join(", ")) }
        })
        .collect();
    let cut_at = old_at.unwrap_or(stop);
    let mut out: Vec<String> = lines[..cut_at].iter().map(|l| (*l).to_string()).collect();
    if !fresh.is_empty() {
        out.push(label);
        out.extend(fresh);
    }
    out.extend(lines[old_end..].iter().map(|l| (*l).to_string()));
    out.join("\n")
}
