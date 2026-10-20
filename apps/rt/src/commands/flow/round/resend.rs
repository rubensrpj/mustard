//! O texto do reenvio de uma onda: o pedido gravado no envio anterior, com
//! as ondas em andamento de agora, os passos já gravados e o aviso de começar
//! vendo o que mudou na cópia.

use mustard_core::platform::i18n::{Locale, translate};

/// O texto do reenvio: o pedido gravado no envio anterior, com a parte das
/// ondas em andamento recalculada para as `running` de agora — a gravada
/// fica velha assim que outra onda entrega, entra ou sai de cena —, mais os
/// passos já gravados desta onda e o aviso de começar vendo o que mudou na
/// cópia. Sem passo nenhum, só o aviso.
pub(super) fn resend_text(previous: &str, running: &[(u64, Vec<String>)], steps: &[String], lang: Locale) -> String {
    let mut parts = vec![refresh_running(previous, running, lang)];
    if !steps.is_empty() {
        parts.push(translate("round.resume.steps", lang).to_string());
        parts.push(steps.join("\n"));
    }
    parts.push(translate("round.resume.notice", lang).to_string());
    parts.join("\n\n")
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
