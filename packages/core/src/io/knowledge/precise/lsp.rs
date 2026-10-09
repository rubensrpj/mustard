//! Optional live compiler queries using the same servers as init/doctor. No
//! shell, installation or model call. Cold sessions are deliberate: compiler
//! conclusions are never reused across edits, branches or dependency changes.
use super::{Location, PreciseSymbols, Reference, Relation, Resolution, byte_column, hash};
use crate::domain::knowledge::{Source, resources::Registry};
use crate::platform::code_tools::{CodeTool, code_tool_for_language};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::time::{Duration, Instant};

const DEADLINE: Duration = Duration::from_secs(15);
const MAX_FRAME: usize = 8 * 1024 * 1024;

struct Client {
    child: Child,
    input: SyncSender<(Value, SyncSender<Result<(), String>>)>,
    messages: Receiver<Result<Value, String>>,
    deadline: Instant,
}
impl Drop for Client {
    fn drop(&mut self) {
        // Also closes the pipe and terminates the reader on errors/timeouts.
        #[cfg(unix)]
        {
            let _ = Command::new("kill")
                .args(["-TERM", "--", &format!("-{}", self.child.id())])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
        #[cfg(windows)]
        {
            let _ = Command::new("taskkill")
                .args(["/PID", &self.child.id().to_string(), "/T", "/F"])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
fn frame(reader: &mut impl BufRead) -> Result<Value, String> {
    let mut size = None;
    let mut headers = 0;
    loop {
        let mut line = String::new();
        if reader
            .take(8193)
            .read_line(&mut line)
            .map_err(|e| e.to_string())?
            == 0
        {
            return Err("lsp-closed".into());
        }
        headers += line.len();
        if headers > 8192 {
            return Err("lsp-header-too-large".into());
        }
        if line == "\r\n" || line == "\n" {
            break;
        }
        if let Some((key, value)) = line.trim().split_once(':')
            && key.eq_ignore_ascii_case("Content-Length")
        {
            size = Some(
                value
                    .trim()
                    .parse::<usize>()
                    .map_err(|_| "lsp-invalid-length")?,
            );
        }
    }
    let size = size
        .filter(|n| *n <= MAX_FRAME)
        .ok_or("lsp-invalid-length")?;
    let mut bytes = vec![0; size];
    reader.read_exact(&mut bytes).map_err(|e| e.to_string())?;
    serde_json::from_slice(&bytes).map_err(|e| e.to_string())
}
impl Client {
    fn start(tree: &Path, tool: &CodeTool) -> Result<Self, String> {
        let mut command = crate::platform::process::command(tool.program);
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        let mut child = command
            .args(tool.lsp.args)
            .current_dir(tree)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("lsp-server-unavailable: {}: {e}", tool.program))?;
        let mut stdin = child.stdin.take().ok_or("lsp-stdin-unavailable")?;
        let (input, writes) = mpsc::sync_channel::<(Value, SyncSender<Result<(), String>>)>(1);
        std::thread::spawn(move || {
            while let Ok((message, done)) = writes.recv() {
                let bytes = message.to_string();
                let result = write!(stdin, "Content-Length: {}\r\n\r\n{bytes}", bytes.len())
                    .and_then(|()| stdin.flush())
                    .map_err(|e| e.to_string());
                if done.send(result).is_err() {
                    break;
                }
            }
        });
        let output = child.stdout.take().ok_or("lsp-stdout-unavailable")?;
        let (send, messages) = mpsc::sync_channel(16);
        std::thread::spawn(move || {
            let mut reader = BufReader::new(output);
            loop {
                let message = frame(&mut reader);
                let failed = message.is_err();
                if send.send(message).is_err() || failed {
                    break;
                }
            }
        });
        Ok(Self {
            child,
            input,
            messages,
            deadline: Instant::now() + DEADLINE,
        })
    }
    fn send(&mut self, message: &Value) -> Result<(), String> {
        let (done, waiting) = mpsc::sync_channel(1);
        self.input
            .try_send((message.clone(), done))
            .map_err(|_| "lsp-writer-unavailable")?;
        waiting
            .recv_timeout(self.deadline.saturating_duration_since(Instant::now()))
            .map_err(|_| "lsp-write-timeout-or-closed")?
    }
    fn request(
        &mut self,
        id: u32,
        method: &str,
        params: Value,
        tree: &Path,
    ) -> Result<Value, String> {
        self.send(&json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}))?;
        loop {
            let remaining = self.deadline.saturating_duration_since(Instant::now());
            let message = self
                .messages
                .recv_timeout(remaining)
                .map_err(|_| "lsp-timeout-or-closed")??;
            if let Some(method) = message["method"].as_str() {
                if message.get("id").is_some() {
                    // LSP servers may request configuration during initialization.
                    // Unknown requests get method-not-found rather than hanging.
                    let result = match method {
                        "workspace/configuration" => Some(json!(
                            message["params"]["items"]
                                .as_array()
                                .map(|items| items.iter().map(|_| json!({})).collect::<Vec<_>>())
                                .unwrap_or_default()
                        )),
                        "workspace/workspaceFolders" => {
                            Some(json!([{"uri":uri(tree),"name":"mustard"}]))
                        }
                        "client/registerCapability"
                        | "client/unregisterCapability"
                        | "window/workDoneProgress/create" => Some(Value::Null),
                        _ => None,
                    };
                    self.send(&match result {
                        Some(result) => json!({"jsonrpc":"2.0","id":message["id"],"result":result}),
                        None => json!({"jsonrpc":"2.0","id":message["id"],"error":{"code":-32601,"message":"Unsupported client request"}}),
                    })?;
                }
            } else if message["id"] == id {
                if let Some(error) = message.get("error") {
                    return Err(format!("lsp-query-error: {error}"));
                }
                return Ok(message["result"].clone());
            }
        }
    }
}
fn uri(path: &Path) -> String {
    let raw = path.to_string_lossy().replace('\\', "/");
    let raw = raw.strip_prefix("//?/").unwrap_or(&raw);
    let mut out: String = if raw.starts_with('/') {
        "file://".into()
    } else {
        "file:///".into()
    };
    for byte in raw.bytes() {
        if byte.is_ascii_alphanumeric() || b"/-._~:".contains(&byte) {
            out.push(char::from(byte));
        } else {
            use std::fmt::Write as _;
            let _ = write!(out, "%{byte:02X}");
        }
    }
    out
}
fn from_uri(value: &str) -> Result<PathBuf, String> {
    let raw = value
        .strip_prefix("file://")
        .ok_or("lsp-non-file-location")?;
    if !raw.starts_with('/') {
        return Err("lsp-remote-location".into());
    }
    let mut bytes = Vec::new();
    let mut at = 0;
    while at < raw.len() {
        if raw.as_bytes()[at] == b'%' {
            let escaped = raw.get(at + 1..at + 3).ok_or("lsp-invalid-uri")?;
            bytes.push(u8::from_str_radix(escaped, 16).map_err(|_| "lsp-invalid-uri")?);
            at += 3;
        } else {
            bytes.push(raw.as_bytes()[at]);
            at += 1;
        }
    }
    let path = String::from_utf8(bytes).map_err(|_| "lsp-invalid-uri")?;
    #[cfg(windows)]
    let path = path.strip_prefix('/').unwrap_or(&path).to_string();
    Ok(PathBuf::from(path))
}
fn coordinates(text: &str, position: &Value, encoding: i32) -> Result<(u64, u64), String> {
    let line = position["line"].as_u64().ok_or("lsp-invalid-line")?;
    let col = position["character"]
        .as_i64()
        .and_then(|v| i32::try_from(v).ok())
        .ok_or("lsp-invalid-column")?;
    let line_text = text
        .split('\n')
        .nth(usize::try_from(line).map_err(|_| "lsp-invalid-line")?)
        .ok_or("lsp-line-outside-source")?;
    Ok((
        line + 1,
        byte_column(line_text.trim_end_matches('\r'), col, encoding)?,
    ))
}
fn locations(
    tree: &Path,
    value: &Value,
    encoding: i32,
    relation: Relation,
    limit: usize,
) -> Result<(Vec<Reference>, bool), String> {
    let list = if value.is_null() {
        vec![]
    } else if let Some(list) = value.as_array() {
        list.clone()
    } else {
        vec![value.clone()]
    };
    let registry = Registry::load()?;
    let mut result = Vec::new();
    let mut incomplete = false;
    for item in list {
        let target = item["uri"]
            .as_str()
            .or_else(|| item["targetUri"].as_str())
            .ok_or("lsp-invalid-location")?;
        let path = from_uri(target)?
            .canonicalize()
            .map_err(|e| e.to_string())?;
        // Dependencies outside the checkout never cause hidden filesystem reads.
        let Ok(relative) = path.strip_prefix(tree) else {
            incomplete = true;
            continue;
        };
        let file = relative.to_string_lossy().replace('\\', "/");
        let Some(text) = super::super::investigation::safe_read(tree, &file, &registry) else {
            incomplete = true;
            continue;
        };
        let range = item
            .get("targetSelectionRange")
            .or_else(|| item.get("range"))
            .ok_or("lsp-invalid-range")?;
        let (line, column_bytes) = coordinates(&text, &range["start"], encoding)?;
        let (end_line, end_column_bytes) = coordinates(&text, &range["end"], encoding)?;
        if (end_line, end_column_bytes) < (line, column_bytes) {
            return Err("lsp-reversed-range".into());
        }
        if result.iter().any(|r: &Reference| {
            r.source.file == file && r.source.line == line && r.column_bytes == column_bytes
        }) {
            continue;
        }
        if result.len() >= limit.clamp(1, 256) {
            incomplete = true;
            continue;
        }
        let receipt_end = if end_column_bytes == 0 && end_line > line {
            end_line - 1
        } else {
            end_line
        };
        result.push(Reference {
            symbol: format!("{file}:{line}:{column_bytes}"),
            source: Source {
                file,
                line,
                end_line: receipt_end,
                sha256: hash(text.as_bytes()),
            },
            column_bytes,
            end_line,
            end_column_bytes,
            roles: i32::from(relation != Relation::References),
        });
    }
    for reference in &result {
        if super::super::investigation::safe_read(tree, &reference.source.file, &registry)
            .is_none_or(|text| hash(text.as_bytes()) != reference.source.sha256)
        {
            return Err("lsp-source-changed-during-query".into());
        }
    }
    Ok((result, incomplete))
}

pub struct LspSymbols<'a> {
    pub tree: &'a Path,
}
impl PreciseSymbols for LspSymbols<'_> {
    fn resolve(
        &self,
        location: &Location<'_>,
        relation: Relation,
        limit: usize,
    ) -> Result<Resolution, String> {
        let language = crate::domain::source_lang::language_of_path(location.file)
            .ok_or("lsp-language-unsupported")?;
        let tool = code_tool_for_language(language).ok_or("lsp-language-unsupported")?;
        resolve_with(self.tree, location, relation, limit, tool, language)
    }
}
fn resolve_with(
    tree: &Path,
    location: &Location<'_>,
    relation: Relation,
    limit: usize,
    tool: &CodeTool,
    language: &str,
) -> Result<Resolution, String> {
    let tree = tree.canonicalize().map_err(|e| e.to_string())?;
    let registry = Registry::load()?;
    let source = super::super::investigation::safe_read(&tree, location.file, &registry)
        .ok_or("lsp-source-unavailable")?;
    let line = location
        .line
        .checked_sub(1)
        .ok_or("lsp-line-starts-at-one")?;
    let line_text = source
        .split('\n')
        .nth(usize::try_from(line).map_err(|_| "lsp-invalid-line")?)
        .ok_or("lsp-line-outside-source")?;
    let byte = usize::try_from(location.column_bytes).map_err(|_| "lsp-invalid-column")?;
    let prefix = line_text.get(..byte).ok_or("lsp-invalid-byte-column")?;
    let mut client = Client::start(&tree, tool)?;
    let initialized = client.request(1, "initialize", json!({"processId":std::process::id(),"rootUri":uri(&tree),
        "workspaceFolders":[{"uri":uri(&tree),"name":"mustard"}],
        "capabilities":{"general":{"positionEncodings":["utf-8","utf-16"]},"textDocument":{"definition":{"linkSupport":true}}},
        "initializationOptions":tool.lsp.initialization_options.and_then(|s|serde_json::from_str::<Value>(s).ok())}), &tree)?;
    let capabilities = &initialized["capabilities"];
    let encoding = match capabilities["positionEncoding"]
        .as_str()
        .unwrap_or("utf-16")
    {
        "utf-8" => 1,
        "utf-16" => 2,
        "utf-32" => 3,
        _ => return Err("lsp-position-encoding-unsupported".into()),
    };
    let (method, capability) = match relation {
        Relation::Definitions => ("textDocument/definition", "definitionProvider"),
        Relation::References => ("textDocument/references", "referencesProvider"),
        Relation::Implementations => ("textDocument/implementation", "implementationProvider"),
    };
    if capabilities[capability].is_null() || capabilities[capability] == false {
        return Err("lsp-operation-unsupported".into());
    }
    client.send(&json!({"jsonrpc":"2.0","method":"initialized","params":{}}))?;
    let document = uri(&tree.join(location.file));
    let language =
        crate::domain::source_lang::lsp_language_of_path(location.file).unwrap_or(language);
    client.send(&json!({"jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{"uri":document,"languageId":language,"version":1,"text":source}}}))?;
    let character = match encoding {
        1 => prefix.len(),
        2 => prefix.encode_utf16().count(),
        _ => prefix.chars().count(),
    };
    let mut params =
        json!({"textDocument":{"uri":document},"position":{"line":line,"character":character}});
    if relation == Relation::References {
        params["context"] = json!({"includeDeclaration":false});
    }
    let reply = client.request(2, method, params, &tree)?;
    let (references, has_more) = locations(&tree, &reply, encoding, relation, limit)?;
    if super::super::investigation::safe_read(&tree, location.file, &registry).as_deref()
        != Some(&source)
    {
        return Err("lsp-source-changed-during-query".into());
    }
    Ok(Resolution {
        status: "live-language-server; source receipts; no persistent compiler cache".into(),
        producer: tool.program.into(),
        index_sha256: String::new(),
        symbols: references.iter().map(|r| r.symbol.clone()).collect(),
        references,
        has_more,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn framing_and_unicode_coordinates_follow_protocol() {
        let text = json!({"name":"ação"}).to_string();
        let message = format!(
            "Content-Type: application/vscode-jsonrpc; charset=utf-8\r\nContent-Length: {}\r\n\r\n{text}",
            text.len()
        );
        assert_eq!(
            frame(&mut std::io::Cursor::new(message)).unwrap()["name"],
            "ação"
        );
        assert!(
            frame(&mut std::io::Cursor::new(
                "Content-Length: 999999999\r\n\r\n"
            ))
            .is_err()
        );
        assert_eq!(
            coordinates("😀x", &json!({"line":0,"character":2}), 2).unwrap(),
            (1, 4)
        );
        assert!(coordinates("😀x", &json!({"line":0,"character":1}), 2).is_err());
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("ação espaço#x.ts");
        assert_eq!(from_uri(&uri(&path)).unwrap(), path);
        assert!(from_uri("file://remote/tmp/x").is_err());
    }
    #[test]
    fn location_links_keep_byte_ranges_and_disclose_omissions() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        std::fs::write(root.join("x.ts"), "😀foo();\n").unwrap();
        let range = json!({"start":{"line":0,"character":2},"end":{"line":0,"character":5}});
        let reply = json!([{"targetUri":uri(&root.join("x.ts")),"targetSelectionRange":range}]);
        let (found, more) = locations(&root, &reply, 2, Relation::Definitions, 10).unwrap();
        assert!(!more);
        assert_eq!(found[0].column_bytes, 4);
        assert_eq!(found[0].end_column_bytes, 7);
        assert_eq!(found[0].source.end_line, 1);
        assert!(
            locations(
                &root,
                &json!({"uri":"https://example.org/x","range":range}),
                2,
                Relation::Definitions,
                10
            )
            .is_err()
        );
    }
    #[cfg(unix)]
    #[test]
    fn live_client_handles_initialization_server_requests_and_definition() {
        use crate::platform::code_tools::LanguageServer;
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        std::fs::write(root.join("x.ts"), "const foo=1;\nfoo;\n").unwrap();
        let messages = [
            json!({"id":1,"result":{"capabilities":{"definitionProvider":true}}}),
            json!({"id":"configuration","method":"workspace/configuration","params":{"items":[{}]}}),
            json!({"id":2,"result":[{"uri":uri(&root.join("x.ts")),"range":{"start":{"line":0,"character":6},"end":{"line":0,"character":9}}}]}),
        ];
        let mut frames = String::new();
        for message in &messages {
            use std::fmt::Write as _;
            let body = message.to_string();
            write!(frames, "Content-Length: {}\r\n\r\n{body}", body.len()).unwrap();
        }
        std::fs::write(root.join("reply"), frames).unwrap();
        let tool = CodeTool {
            program: "sh",
            lsp: LanguageServer {
                args: &["-c", "cat reply; exec sleep 30"],
                initialization_options: None,
            },
            plugin: None,
            install_cmd: "",
            check: None,
            start_hint: None,
            pin: None,
        };
        let result = resolve_with(
            &root,
            &Location {
                file: "x.ts",
                line: 2,
                column_bytes: 0,
            },
            Relation::Definitions,
            10,
            &tool,
            "typescript",
        )
        .unwrap();
        assert_eq!(result.references.len(), 1);
        assert_eq!(result.references[0].source.line, 1);
    }
}
