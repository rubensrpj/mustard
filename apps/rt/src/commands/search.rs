use crate::shared::search_gateway::quote;
use mustard_core::domain::code_search::Request;
use serde_json::json;
use std::fmt::Write as _;
use std::io::Write;
use std::path::Path;

/// O maior começo de uma linha que a resposta cortada mostra. Uma linha só de
/// um JSON ou de um código minificado passa de um megabyte; o começo dela
/// basta para saber onde a busca achou, e as linhas seguintes cabem depois.
const LINE_BYTES: usize = 1_000;

/// O espaço guardado no fim da resposta cortada para a nota que diz onde ela
/// cortou, além da linha que diz como pedir o resto.
const NOTE_BYTES: usize = 400;

/// A linha que diz como pedir o resto quando a que traz o comando inteiro não
/// cabe na resposta cortada.
const LONG_REST: &str = "# Whole native result: the command is too long to show here; run `mustard-rt run search --raw --` \
    with the same native arguments and save its output to a file.\n";

pub fn run(
    root: &Path,
    request: Option<&str>,
    argv: &[String],
    intent: Option<&str>,
    purpose: Option<&str>,
    choose: bool,
    raw: bool,
    shell_output: bool,
) {
    let started=std::time::Instant::now();
    let delivery_context=request.and_then(|text|mustard_core::domain::code_search::contract::DeliveryContext::from_json(text).ok().flatten());
    let result = (|| {
        let mut request: Request = match request {
            Some(text) if text.len() <= 64 * 1024 => Request::from_json(text)?,
            Some(_) => return Err("search-request-too-large".into()),
            None => Request::native(argv)?,
        };
        if let Some(intent) = intent {
            request.intent = intent.into();
        }
        if let Some(purpose) = purpose {
            request.purpose =
                mustard_core::domain::knowledge::investigation::Purpose::parse(purpose)
                    .ok_or("search-invalid-purpose")?;
        }
        request.choose |= choose;
        if raw && !matches!(request.tool.as_str(), "rg" | "grep" | "git") {
            return Err("search-raw-output-requires-native-tool".into());
        }
        if raw {
            request.purpose=mustard_core::domain::knowledge::investigation::Purpose::Locate;
            request.choose=false;
        }
        crate::shared::search_gateway::answer(root, &request).map(|answer| (answer, request))
    })();
    match result {
        Ok((mut answer, request)) => {
            if raw || shell_output {
                let git=mustard_core::platform::git::run(root,&["rev-parse","--show-toplevel"]);
                let tree=if git.ok {std::path::PathBuf::from(git.stdout.trim())}else{root.to_path_buf()};
                let receipt=if shell_output {delivery_context.as_ref().and_then(|context|
                    mustard_core::io::code_search::delivery::prepare(&mut answer,&request,&tree,context))}else{None};
                let output = if shell_output {
                    let view = mustard_core::io::code_search::presentation::agent(&answer, &request, root);
                    let enriched = answer.report["crossing_status"] == "enriched";
                    let cap = conversation_cap(enriched, view.representation);
                    conversation_answer(root, &request, view.stdout, receipt.as_deref(), cap)
                } else {
                    answer.stdout.clone()
                };
                answer.report["delivery_measurement"] =
                    json!({"basis":"same-request-stdout-bytes","native_bytes":answer.stdout.len(),"delivered_bytes":output.len()});
                let _=super::spec_events::conversation::record_output_call(root,"search",None,started,&answer.report,&String::from_utf8_lossy(&output),
                );
                let _ = std::io::stdout().write_all(&output);
                let _ = std::io::stderr().write_all(&answer.stderr);
            } else {
                // Explicit diagnostics can inspect exactly the host projection
                // without repeating source discovery or a paid selection.
        if std::env::var_os("MUSTARD_SEARCH_TRACE").is_some_and(|value|value=="1" || value=="projection") {
                    let view=mustard_core::io::code_search::presentation::agent(&answer,&request,root);
                    answer.report["_trace_agent_view"]=json!(String::from_utf8_lossy(&view.stdout));
                }
                let _=super::spec_events::conversation::record_output_call(root,"search",None,started,&answer.report,&format!("{}\n",answer.report));
                println!("{}", answer.report);
            }
            std::process::exit(answer.exit_code);
        }
        Err(reason) => {
            let fallback = if reason.starts_with("search-contract-")
                || reason.starts_with("search-intent-required")
                || reason == "search-unsupported-contract-version"
            {
                "correct-search-request; preserve original arguments and supply explicit intent and purpose"
            } else {
                "use-original-host-tool"
            };
            let report=json!({"ok":false,"reason":reason,"fallback":fallback,"executed":false,"remote_model_calls":0});
            let _=super::spec_events::conversation::record_output_call(root,"search",None,started,&report,&format!("{report}\n"));
            println!("{report}");
            std::process::exit(2);
        }
    }
}

/// A resposta da busca de `request` que vai para a conversa, pelo terminal ou
/// pelo MCP: `view` no teto `cap` que [`conversation_cap`] der, com a linha
/// que diz como pedir o resultado inteiro quando sai cortada.
pub(crate) fn conversation_answer(root: &Path, request: &Request, view: Vec<u8>, token: Option<&str>, cap: usize) -> Vec<u8> {
    conversation_output(view, token, cap, || rest_line(root, request))
}

/// O teto da investigação: a busca com finalidade que junta o código do mapa
/// ao resultado. Ela só roda quando o agente pede, e o resultado inteiro
/// segue a um pedido de `purpose=locate`.
pub(crate) const INVESTIGATION_BYTES: usize = 4_096;

/// O teto da resposta de representação `representation`: o da investigação
/// para a evidência da tarefa; fora dela, cheio para a busca enriquecida e
/// metade para a que caiu no modo reserva.
///
/// O modo reserva entrega o que o programa nativo achou, sem a triagem que
/// escolhe o que importa. Em campo, uma busca assim devolveu 34.568 bytes de
/// uma vez, enquanto as maiores enriquecidas ficaram perto de 9.800
/// (21/10/2026). Quem não sabe escolher não pode ocupar mais que quem sabe: o
/// corte diz quantas linhas ficaram de fora e como pedir o resto.
pub(crate) fn conversation_cap(enriched: bool, representation: &str) -> usize {
    let cap = super::knowledge::CONVERSATION_BYTES;
    if representation == "current-task-evidence" {
        INVESTIGATION_BYTES
    } else if enriched {
        cap
    } else {
        cap / 2
    }
}

/// A resposta da busca que vai para a conversa. A que cabe em `cap` bytes sai
/// inteira, com o recibo da entrega de `token`. A maior sai cortada por
/// [`fit_conversation`] e sem o recibo: ele confirmaria a entrega de trechos
/// que ficaram de fora, e a busca seguinte os daria como já vistos.
pub(crate) fn conversation_output(mut output: Vec<u8>, token: Option<&str>, cap: usize, rest: impl FnOnce() -> String) -> Vec<u8> {
    if output.len() > cap {
        return fit_conversation(&output, cap, &rest());
    }
    if let Some(token) = token {
        let receipt = mustard_core::io::code_search::delivery::receipt(token, &output);
        output.extend_from_slice(receipt.as_bytes());
    }
    output
}

/// `output` cortada até caber em `cap` bytes. Cada linha de mais de
/// [`LINE_BYTES`] mostra só o começo e diz quantos bytes dela ficaram de fora;
/// as linhas que ainda não cabem saem do fim. A nota do fim diz o tamanho da
/// resposta inteira, quantas linhas mostram só o começo e a partir de que
/// linha o resto ficou de fora, e `rest` diz como pedir o resto. O `rest` de
/// mais de um quarto do teto, como o de um pedido de 64 KB, dá lugar a
/// [`LONG_REST`], para a nota e ele caberem junto das linhas achadas.
fn fit_conversation(output: &[u8], cap: usize, rest: &str) -> Vec<u8> {
    let rest = if rest.len() > cap / 4 { LONG_REST } else { rest };
    let budget = cap.saturating_sub(rest.len() + NOTE_BYTES);
    let lines: Vec<&[u8]> = output.split_inclusive(|byte| *byte == b'\n').collect();
    let mut fitted = Vec::with_capacity(cap);
    let (mut kept, mut shortened) = (0, 0);
    for line in &lines {
        let body = line.strip_suffix(b"\n").unwrap_or(line);
        let mut shown = body.len().min(LINE_BYTES);
        // O corte não parte ao meio um caractere de vários bytes.
        while shown > 0 && shown < body.len() && body[shown] & 0xC0 == 0x80 {
            shown -= 1;
        }
        let marker = if shown < body.len() { format!(" [line cut: {} more bytes]", body.len() - shown) } else { String::new() };
        if fitted.len() + shown + marker.len() + 1 > budget {
            break;
        }
        fitted.extend_from_slice(&body[..shown]);
        fitted.extend_from_slice(marker.as_bytes());
        fitted.push(b'\n');
        kept += 1;
        shortened += usize::from(!marker.is_empty());
    }
    let mut note = format!(
        "# Answer cut to fit the conversation cap of {cap} bytes; the whole answer has {} bytes in {} {}.",
        output.len(),
        lines.len(),
        if lines.len() == 1 { "line" } else { "lines" }
    );
    if shortened > 0 {
        let _ = write!(note, " Lines over {LINE_BYTES} bytes showing only their start: {shortened}.");
    }
    if kept + 1 == lines.len() {
        let _ = write!(note, " Line {} is left out.", lines.len());
    } else if kept < lines.len() {
        let _ = write!(note, " Lines {} to {} are left out.", kept + 1, lines.len());
    }
    note.push('\n');
    fitted.extend_from_slice(note.as_bytes());
    fitted.extend_from_slice(rest.as_bytes());
    fitted
}

/// A linha que diz como pedir o resultado inteiro de `request`: a busca
/// nativa com `--raw`, que entrega os bytes exatos dela, gravada num arquivo.
/// O pedido que não tem busca nativa, como a leitura de um arquivo, se repete
/// menor.
pub(crate) fn rest_line(root: &Path, request: &Request) -> String {
    let Ok((tool, args)) = mustard_core::io::code_search::arguments(request) else {
        return "# Rest: repeat the request with a smaller range or limit.\n".into();
    };
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let native: Vec<String> = std::iter::once(tool).chain(args).map(|arg| quote(&arg)).collect();
    format!(
        "# Whole native result: mustard-rt run search --root {} --raw -- {} > <file>\n",
        quote(&root.to_string_lossy()),
        native.join(" ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A resposta que cabe no teto sai inteira e leva o recibo da entrega; a
    /// cortada sai sem ele, porque o recibo confirmaria trechos que ficaram de
    /// fora.
    #[test]
    fn only_an_answer_within_the_cap_carries_the_delivery_receipt() {
        let small = b"src/a.ts:1:export const alpha = 1;\n".to_vec();
        let whole = conversation_output(small.clone(), Some("token"), 2_000, || unreachable!("an answer within the cap is not cut"));
        let receipt = mustard_core::io::code_search::delivery::receipt("token", &small);
        assert_eq!(whole, [small, receipt.into_bytes()].concat());
        let long = format!("src/a.ts:1:export const alpha = \"{}\";\n", "a".repeat(5_000)).into_bytes();
        let cut = String::from_utf8(conversation_output(long, Some("token"), 2_000, || "# Rest: x\n".into())).unwrap();
        assert!(cut.len() <= 2_000, "{cut}");
        assert!(!cut.contains("# mustard-delivery:"), "{cut}");
        assert!(cut.ends_with("# Rest: x\n"), "{cut}");
    }

    /// O corte de uma linha longa que cai no meio de um `ç` volta para o
    /// começo dele: a resposta segue UTF-8 válido e conta como faltando os dois
    /// bytes do `ç`. A linha curta sai inteira e sem marca de corte, e a única
    /// linha que não cabe é dita sozinha na nota.
    #[test]
    fn a_cut_inside_a_multibyte_character_backs_off_to_its_start() {
        let long = format!("src/a.ts:1:{}ç{}", "a".repeat(988), "b".repeat(500));
        assert_eq!(&long.as_bytes()[999..1001], "ç".as_bytes(), "the 1000th byte falls inside the ç");
        let output = format!("{long}\nsrc/b.ts:2:const short = 1;\nsrc/c.ts:3:{}\n", "c".repeat(3_000));
        assert_eq!(output.len(), 4_542);
        let fitted = String::from_utf8(fit_conversation(output.as_bytes(), 2_000, "# Rest: x\n")).expect("the cut answer is valid UTF-8");
        let expected = format!(
            "src/a.ts:1:{} [line cut: 502 more bytes]\nsrc/b.ts:2:const short = 1;\n\
             # Answer cut to fit the conversation cap of 2000 bytes; the whole answer has 4542 bytes in 3 lines. \
             Lines over 1000 bytes showing only their start: 1. Line 3 is left out.\n# Rest: x\n",
            "a".repeat(988)
        );
        assert_eq!(fitted, expected);
    }

    /// A linha do resto que passa de um quarto do teto, como a de um pedido de
    /// 64 KB, dá lugar à curta, e a resposta de uma linha só cabe no teto com
    /// o começo dela.
    #[test]
    fn a_rest_line_too_long_for_the_cap_gives_way_to_a_short_one() {
        let output = format!("{}\n", "a".repeat(5_000));
        let rest = format!("# Whole native result: mustard-rt run search --raw -- rg -e '{}' > <file>\n", "x".repeat(3_000));
        let fitted = String::from_utf8(fit_conversation(output.as_bytes(), 2_000, &rest)).unwrap();
        let expected = format!(
            "{} [line cut: 4000 more bytes]\n# Answer cut to fit the conversation cap of 2000 bytes; the whole answer has \
             5001 bytes in 1 line. Lines over 1000 bytes showing only their start: 1.\n{LONG_REST}",
            "a".repeat(1_000)
        );
        assert_eq!(fitted, expected);
        assert!(fitted.len() <= 2_000, "{} bytes", fitted.len());
    }

    /// A investigação sai em até 4 KB, mesmo enriquecida; as outras
    /// representações seguem no teto da conversa.
    #[test]
    fn an_investigation_answer_fits_four_kilobytes() {
        let cap = conversation_cap(true, "current-task-evidence");
        assert_eq!(cap, 4_096);
        assert_eq!(conversation_cap(true, "grouped-matches"), super::super::knowledge::CONVERSATION_BYTES);
        let evidence = "7 | let value = compute(7);\n".repeat(400);
        let fitted = conversation_output(evidence.into_bytes(), Some("token"), cap, || "# Rest: x\n".into());
        assert!(fitted.len() <= 4_096, "{} bytes", fitted.len());
        assert!(!String::from_utf8(fitted).unwrap().contains("# mustard-delivery:"));
    }
}
