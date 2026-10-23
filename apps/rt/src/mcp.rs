//! Minimal stdio MCP transport over the same versioned search contract as the
//! CLI and Mods. Content is always text; no conflicting output schema.
use std::io::{BufRead, Write};
use std::path::Path;
use serde_json::{Value, json};

fn error(id: Value, code: i32, message: &str) -> Value {
    json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}})
}

/// A sessão da conversa que fez a chamada `params`, dita no `_meta` dela. O
/// Codex 0.160.1 manda em toda `tools/call` o turno em `x-codex-turn-metadata`
/// (um objeto, com `thread_id`, que é a `CODEX_THREAD_ID` dos comandos dele, e
/// `session_id`) e os mesmos ids soltos em `threadId` e `sessionId`; o
/// servidor que ele sobe não recebe a `CODEX_THREAD_ID`. O `_meta` vale mais
/// que o ambiente do servidor, que é o de quem o subiu e fica velho depois do
/// `/clear`; sem ele, vale o ambiente, e sem os dois a chamada não tem sessão.
fn call_session(params: &Value) -> Option<String> {
    let meta = &params["_meta"];
    let turn = &meta["x-codex-turn-metadata"];
    [&turn["thread_id"], &turn["session_id"], &meta["threadId"], &meta["sessionId"]]
        .into_iter()
        .filter_map(Value::as_str)
        .map(str::trim)
        .find(|session| !session.is_empty())
        .map(str::to_string)
        .or_else(crate::shared::spec_state::session_from_env)
}

fn response(root: &Path, message: &Value, initialized: &mut bool) -> Option<Value> {
    if message["jsonrpc"] != "2.0" || !message["method"].is_string() {
        return Some(error(message.get("id").cloned().unwrap_or(Value::Null), -32600, "Invalid JSON-RPC request"));
    }
    let id = message.get("id")?.clone(); // Valid notifications never receive replies.
    if message["jsonrpc"] != "2.0" || !message["method"].is_string() || !(id.is_string() || id.is_number()) {
        return Some(error(id, -32600, "Invalid JSON-RPC request"));
    }
    let result = match message["method"].as_str().unwrap_or_default() {
        "initialize" => {
            *initialized = true;
            let requested = message["params"]["protocolVersion"].as_str().unwrap_or_default();
            let protocol = if ["2024-11-05", "2025-03-26", "2025-06-18", "2025-11-25"].contains(&requested) { requested } else { "2025-11-25" };
            json!({"protocolVersion":protocol,"capabilities":{"tools":{"listChanged":false}},
                "serverInfo":{"name":"mustard","version":env!("MUSTARD_VERSION_FULL")},
                "instructions":"Code search preserves native tool arguments. Include a short intent and purpose; choose enables optional configured Jev only for remaining ambiguity."})
        }
        "ping" => json!({}),
        _ if !*initialized => return Some(error(id, -32002, "Initialize the server first")),
        "tools/list" => json!({"tools":[{"name":"search","description":"Search or read current project code using native arguments, crossed with indexed functions, callers, contracts, tests and source-bound interpretations. Preserve native scope; use locate for literal searches.",
            "inputSchema":mustard_core::domain::code_search::contract::schema()}]}),
        "tools/call" => {
            if message["params"]["name"] != "search" { return Some(error(id, -32602, "Unknown tool")); }
            let started = std::time::Instant::now();
            let session = call_session(&message["params"]);
            let mut arguments = message["params"]["arguments"].clone();
            // The shared Mods contract omits this transport envelope version.
            if arguments.get("request").is_some() && arguments.get("schema_version").is_none() { arguments["schema_version"] = json!(1); }
            let result = mustard_core::domain::code_search::Request::from_json(&arguments.to_string())
                .and_then(|request| crate::shared::search_gateway::answer(root, &request).map(|answer| (answer, request)));
            match result {
                Ok((answer, request)) => {
                    let view = mustard_core::io::code_search::presentation::agent(&answer, &request, root);
                    // O texto vira UTF-8 válido antes do corte, que não parte um
                    // caractere: a troca de um byte inválido cresceria a resposta
                    // depois de ela caber no teto da conversa.
                    let mut text = String::from_utf8_lossy(&view.stdout).into_owned();
                    if !answer.stderr.is_empty() { text.push_str(&String::from_utf8_lossy(&answer.stderr)); }
                    let enriched = answer.report["crossing_status"] == "enriched";
                    let text = String::from_utf8_lossy(&crate::commands::search::conversation_answer(root, &request, text.into_bytes(), None, enriched)).into_owned();
                    let _ = crate::commands::spec_events::conversation::record_output_call_as(root, session.as_deref(), "search", None, started, &answer.report, &text);
                    json!({"content":[{"type":"text","text":text}],"isError":answer.report["ok"] != true})
                }
                Err(reason) => {
                    let report = json!({"ok":false,"reason":reason});
                    let _ = crate::commands::spec_events::conversation::record_output_call_as(root, session.as_deref(), "search", None, started, &report, &report.to_string());
                    json!({"content":[{"type":"text","text":report.to_string()}],"isError":true})
                }
            }
        }
        _ => return Some(error(id, -32601, "Method not found")),
    };
    Some(json!({"jsonrpc":"2.0","id":id,"result":result}))
}

pub(crate) fn run(root: &Path) {
    let input = std::io::stdin(); let mut output = std::io::stdout().lock(); let mut initialized = false;
    for line in input.lock().lines() {
        let Ok(line) = line else { break; };
        let reply = if line.len() > 1_048_576 { Some(error(Value::Null, -32600, "Message exceeds 1 MiB")) }
            else { match serde_json::from_str::<Value>(&line) {
                Ok(value) if value.is_object() => response(root, &value, &mut initialized),
                Ok(_) => Some(error(Value::Null, -32600, "Invalid JSON-RPC request")),
                Err(_) => Some(error(Value::Null, -32700, "Invalid JSON")),
            }};
        if let Some(reply) = reply && writeln!(output, "{reply}").and_then(|()| output.flush()).is_err() { break; }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A chamada que o `codex` 0.160.1 desta máquina mandou a um servidor MCP
    /// de teste (`codex exec`, ferramenta `ping`), como chegou ao servidor.
    const CODEX_CALL: &str = include_str!("../tests/fixtures/codex-tools-call.json");
    const CODEX_THREAD: &str = "01a16614-52a7-73e3-a596-56c652948934";

    /// A sessão da chamada sai do `_meta` que o Codex manda: o turno vale pelo
    /// `thread_id` e, sem ele, pelo `session_id`; sem o turno, valem os ids
    /// soltos, `threadId` e depois `sessionId`. Vazio, ausente ou de outro tipo
    /// não é sessão (e, nos testes, o ambiente não responde por ninguém).
    #[test]
    fn the_session_of_a_call_comes_from_the_meta_codex_sends() {
        let call: Value = serde_json::from_str(CODEX_CALL).unwrap();
        let meta = call["params"]["_meta"].clone();
        assert_eq!(call_session(&call["params"]).as_deref(), Some(CODEX_THREAD), "a chamada como o Codex a mandou");

        let mut loose = meta.clone();
        loose.as_object_mut().unwrap().remove("x-codex-turn-metadata");
        assert_eq!(call_session(&json!({"_meta": loose})).as_deref(), Some(CODEX_THREAD), "sem o turno, os ids soltos");

        // Nas chamadas vistas a conversa e a sessão têm o mesmo id; a ordem
        // abaixo é a da regra, não a de uma chamada vista.
        let call = |meta: Value| call_session(&json!({"name": "search", "_meta": meta}));
        let turn = json!({"session_id": "raiz", "thread_id": "conversa"});
        assert_eq!(call(json!({"x-codex-turn-metadata": turn, "threadId": "solta"})).as_deref(), Some("conversa"));
        assert_eq!(call(json!({"x-codex-turn-metadata": {"session_id": " raiz ", "thread_id": " "}})).as_deref(), Some("raiz"));
        assert_eq!(call(json!({"threadId": "solta", "sessionId": "outra"})).as_deref(), Some("solta"));
        assert_eq!(call(json!({"sessionId": "outra"})).as_deref(), Some("outra"));
        for none in [json!({}), json!({"threadId": ""}), json!({"sessionId": 7}), json!({"x-codex-turn-metadata": "não é objeto"})] {
            assert_eq!(call(none.clone()), None, "{none}");
        }
        assert_eq!(call_session(&json!({"name": "search"})), None);
    }

    #[test]
    fn search_transport_returns_text_for_read_grep_and_errors() {
        let dir = tempfile::tempdir().unwrap(); let root = dir.path();
        std::fs::write(root.join("a.rs"), "fn restore() {}\n").unwrap();
        let mut initialized = false;
        let msg = |method: &str, params: Value| json!({"jsonrpc":"2.0","id":1,"method":method,"params":params});
        assert_eq!(response(root, &msg("tools/list", json!({})), &mut initialized).unwrap()["error"]["code"], -32002);
        let init = response(root, &msg("initialize", json!({"protocolVersion":"2025-11-25"})), &mut initialized).unwrap();
        assert_eq!(init["result"]["protocolVersion"], "2025-11-25");
        let tools = response(root, &msg("tools/list", json!({})), &mut initialized).unwrap();
        assert!(tools["result"]["tools"][0].get("outputSchema").is_none());
        for (tool, input) in [("Read", json!({"file_path":root.join("a.rs"),"offset":1,"limit":1})),
            ("Grep", json!({"pattern":"restore","path":root,"output_mode":"content"}))] {
            let result = response(root, &msg("tools/call", json!({"name":"search","arguments":{"request":{"tool":tool,"input":input,"intent":"Read restoration function","purpose":"locate"}}})), &mut initialized).unwrap();
            assert_eq!(result["result"]["isError"], false, "{result}");
            assert!(result["result"]["content"][0]["text"].as_str().unwrap().contains("restore"));
        }
        let bad = response(root, &msg("tools/call", json!({"name":"search","arguments":{}})), &mut initialized).unwrap();
        assert_eq!(bad["result"]["isError"], true);
        let empty=response(root,&msg("tools/call",json!({"name":"search","arguments":{"request":{"tool":"Grep","input":{"pattern":"not_present_anywhere","path":root},"intent":"Literal lookup","purpose":"locate"}}})),&mut initialized).unwrap();
        assert_eq!(empty["result"]["isError"],false,"no matches is a successful native search");
        assert!(response(root, &json!({"jsonrpc":"2.0","method":"notifications/initialized"}), &mut initialized).is_none());
        assert_eq!(response(root, &json!({"unexpected":true}), &mut initialized).unwrap()["error"]["code"], -32600);
    }

    /// Uma busca pelo MCP que acha um JSON de uma linha só, de mais de um
    /// megabyte, responde abaixo do teto da conversa, de 25.000 bytes: a linha
    /// mostra só o começo, e a nota do fim traz o comando que pede o resto.
    #[test]
    fn a_search_hitting_a_line_over_a_megabyte_answers_under_the_cap_through_the_mcp() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let line = format!("{{\"note\":\"needle\",\"vocab\":\"{}\"}}", "ç".repeat(600_000));
        std::fs::write(root.join("tokenizer.json"), format!("{line}\n")).unwrap();
        let mut initialized = false;
        let msg = |method: &str, params: Value| json!({"jsonrpc":"2.0","id":1,"method":method,"params":params});
        response(&root, &msg("initialize", json!({"protocolVersion":"2025-11-25"})), &mut initialized).unwrap();
        let request = json!({"tool":"Grep","input":{"pattern":"needle","path":root,"output_mode":"content"},"intent":"Find the tokenizer note","purpose":"locate"});
        let result = response(&root, &msg("tools/call", json!({"name":"search","arguments":{"request":request}})), &mut initialized).unwrap();
        assert_eq!(result["result"]["isError"], false, "{result}");
        let text = result["result"]["content"][0]["text"].as_str().unwrap();
        let start: String = text.chars().take(300).collect();
        // Esta busca não enriquece, então vale o teto do modo reserva, lido
        // do dono dele: um número escrito aqui envelheceria junto.
        let cap = crate::commands::search::conversation_cap(false);
        assert!(text.len() <= cap, "{} bytes: {start}", text.len());
        // O começo da linha e a marca do corte, sem o número de bytes que
        // sobraram: ele muda quando o cabeçalho da resposta ganha uma linha, e
        // o que o teste precisa provar é o corte, que o teto acima já mede.
        assert!(text.contains(&format!("\n1:{} [line cut: ", &line[..998])), "{text}");
        assert!(text.contains(" more bytes]\n"), "{text}");
        assert!(text.contains(&format!("# Answer cut to fit the conversation cap of {cap} bytes;")), "{text}");
        let target = root.display();
        let rest = format!(
            "# Whole native result: mustard-rt run search --root '{target}' --raw -- 'rg' '--color=never' '--no-heading' \
             '--with-filename' '-n' '-e' 'needle' '--' '{target}' > <file>\n"
        );
        assert!(text.ends_with(&rest), "{text}");
    }
}
