//! Explicit local generation adapter. It never installs, pulls or routes to
//! a remote model. The indexed search remains usable without this provider.
use std::net::IpAddr;
use std::path::Path;
use std::time::Duration;

use mustard_core::domain::config::{EnrichmentConfig, ProjectConfig};
use mustard_core::io::knowledge::enrichment::{
    self, Generation, GenerationRequest, ProviderLocation, SemanticEnrichmentProvider,
};
use serde_json::{Value, json};

struct LocalGenerator {
    config: EnrichmentConfig,
    digest: String,
    agent: ureq::Agent,
}

fn validate(config: &EnrichmentConfig) -> Result<(), String> {
    let uri: ureq::http::Uri = config
        .endpoint
        .parse()
        .map_err(|_| "knowledge-invalid-local-endpoint")?;
    let host = uri.host().unwrap_or_default().trim_matches(['[', ']']);
    let loopback = host.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback());
    if config.provider != "ollama"
        || uri.scheme_str() != Some("http")
        || !loopback
        || !matches!(uri.path(), "/" | "")
        || uri.query().is_some()
        || uri
            .authority()
            .is_some_and(|authority| authority.as_str().contains('@'))
        || config.model.trim().is_empty()
        || config.model.len() > 200
        || !(1..=300).contains(&config.timeout_seconds)
        || !(2048..=65536).contains(&config.context_tokens)
        || !(256..=8192).contains(&config.output_tokens)
        || config.output_tokens >= config.context_tokens / 2
    {
        return Err("knowledge-invalid-local-provider-config".into());
    }
    Ok(())
}

fn local_model(document: &Value) -> bool {
    ["remote_host", "remote_model"].iter().all(|field| {
        document
            .get(*field)
            .is_none_or(|value| value.is_null() || value.as_str() == Some(""))
    })
}

impl LocalGenerator {
    fn new(config: EnrichmentConfig) -> Result<Self, String> {
        validate(&config)?;
        let agent = ureq::Agent::config_builder()
            .proxy(None)
            .max_redirects(0)
            .timeout_connect(Some(Duration::from_secs(3)))
            .timeout_global(Some(Duration::from_secs(config.timeout_seconds)))
            .build()
            .new_agent();
        let base = config.endpoint.trim_end_matches('/');
        let mut response = agent
            .get(format!("{base}/api/tags"))
            .call()
            .map_err(|e| format!("knowledge-local-provider-unavailable: {e}"))?;
        let tags: Value = response
            .body_mut()
            .with_config()
            .limit(512_000)
            .read_json()
            .map_err(|e| e.to_string())?;
        let requested = if config.model.contains(':') {
            config.model.clone()
        } else {
            format!("{}:latest", config.model)
        };
        let model = tags["models"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|model| {
                model["name"].as_str() == Some(&requested)
                    || model["model"].as_str() == Some(&requested)
            })
            .ok_or("knowledge-local-model-not-installed")?;
        if !local_model(model) {
            return Err("knowledge-remote-model-refused".into());
        }
        let digest = model["digest"]
            .as_str()
            .filter(|digest| !digest.is_empty())
            .ok_or("knowledge-local-model-digest-missing")?
            .to_string();
        // /show also exposes remote metadata in versions where /tags omits it.
        let mut response = agent
            .post(format!("{base}/api/show"))
            .send_json(json!({"model":requested}))
            .map_err(|e| e.to_string())?;
        let show: Value = response
            .body_mut()
            .with_config()
            .limit(512_000)
            .read_json()
            .map_err(|e| e.to_string())?;
        if !local_model(&show) {
            return Err("knowledge-remote-model-refused".into());
        }
        Ok(Self {
            config,
            digest,
            agent,
        })
    }
}

impl SemanticEnrichmentProvider for LocalGenerator {
    fn location(&self) -> ProviderLocation {
        ProviderLocation::Local
    }
    fn identity(&self) -> String {
        json!({"provider":"ollama","model":self.config.model,"digest":self.digest,
            "endpoint":self.config.endpoint,"context":self.config.context_tokens,
            "output":self.config.output_tokens,"temperature":0,"think":false})
        .to_string()
    }

    fn generate(&self, request: &GenerationRequest) -> Result<Generation, String> {
        let mut response = self.agent.post(format!("{}/api/generate", self.config.endpoint.trim_end_matches('/')))
            .send_json(json!({"model":self.config.model,"prompt":request.prompt,"format":request.schema,
                "stream":false,"think":false,"keep_alive":"0s","options":{
                    "temperature":0,"num_ctx":self.config.context_tokens,"num_predict":self.config.output_tokens
                }})).map_err(|e| format!("knowledge-local-generation-failed: {e}"))?;
        let reply: Value = response
            .body_mut()
            .with_config()
            .limit(512_000)
            .read_json()
            .map_err(|e| e.to_string())?;
        if !local_model(&reply) {
            return Err("knowledge-remote-model-refused".into());
        }
        if reply["done"] != true || reply["done_reason"] == "length" || reply.get("error").is_some()
        {
            return Err("knowledge-local-generation-incomplete".into());
        }
        Ok(Generation {
            text: reply["response"]
                .as_str()
                .ok_or("knowledge-generation-no-response")?
                .into(),
            input_tokens: reply["prompt_eval_count"].as_u64(),
            output_tokens: reply["eval_count"].as_u64(),
        })
    }
}

pub fn generate(
    root: &Path,
    tree: &Path,
    query: &mustard_core::io::knowledge::Query<'_>,
) -> Result<String, String> {
    if query.text.trim().is_empty() || query.text.len() > 2000 {
        return Err("knowledge-enrichment-needs-short-query".into());
    }
    let config = ProjectConfig::load(root);
    if config.unreadable {
        return Err("knowledge-project-config-unreadable".into());
    }
    let config = config
        .knowledge
        .ok_or("knowledge-local-provider-not-configured")?;
    let provider = LocalGenerator::new(config)?;
    enrichment::enrich(root, tree, query.text, query.file, query.limit, &provider)
        .map(|value| value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    fn serve(replies: Vec<Value>) -> (String, std::thread::JoinHandle<Vec<String>>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            let mut requests = Vec::new();
            for reply in replies {
                let (mut connection, _) = listener.accept().unwrap();
                connection
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut data = Vec::new();
                let mut buffer = [0; 4096];
                loop {
                    let count = connection.read(&mut buffer).unwrap();
                    assert!(count > 0);
                    data.extend_from_slice(&buffer[..count]);
                    let text = String::from_utf8_lossy(&data);
                    if let Some(header_end) = text.find("\r\n\r\n") {
                        let length = text[..header_end]
                            .lines()
                            .find_map(|line| {
                                line.to_lowercase()
                                    .strip_prefix("content-length:")
                                    .and_then(|length| length.trim().parse::<usize>().ok())
                            })
                            .unwrap_or(0);
                        if data.len() >= header_end + 4 + length {
                            break;
                        }
                    }
                }
                requests.push(String::from_utf8(data).unwrap());
                let body = reply.to_string();
                write!(connection,"HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{}",body.len(),body).unwrap();
            }
            requests
        });
        (endpoint, server)
    }

    #[test]
    fn native_adapter_uses_local_schema_and_observed_token_counts() {
        let (endpoint, server) = serve(vec![
            json!({"models":[{"name":"fixture:small","digest":"abc"}]}),
            json!({"capabilities":["completion"]}),
            json!({"done":true,"done_reason":"stop","response":"{\"title\":\"Backup\"}","prompt_eval_count":42,"eval_count":10}),
        ]);
        let provider = LocalGenerator::new(EnrichmentConfig {
            provider: "ollama".into(),
            model: "fixture:small".into(),
            endpoint,
            ..Default::default()
        })
        .unwrap();
        let result = provider
            .generate(&GenerationRequest {
                prompt: "Explain only this excerpt".into(),
                schema: json!({"type":"object"}),
            })
            .unwrap();
        assert_eq!(result.input_tokens, Some(42));
        assert_eq!(result.output_tokens, Some(10));
        assert!(provider.identity().contains("abc"));
        let requests = server.join().unwrap();
        assert!(requests[0].starts_with("GET /api/tags"));
        assert!(requests[1].starts_with("POST /api/show"));
        let body: Value =
            serde_json::from_str(requests[2].split("\r\n\r\n").nth(1).unwrap()).unwrap();
        assert_eq!(body["stream"], false);
        assert_eq!(body["think"], false);
        assert_eq!(body["format"]["type"], "object");
        assert_eq!(body["options"]["num_ctx"], 8192);
        assert!(!requests.iter().any(|request| request.contains("/api/pull")));
    }

    #[test]
    fn cloud_metadata_is_refused_before_generation() {
        let (endpoint, server) = serve(vec![
            json!({"models":[{"name":"fixture:small","digest":"abc"}]}),
            json!({"remote_host":"https://ollama.com","remote_model":"fixture:small"}),
        ]);
        let result = LocalGenerator::new(EnrichmentConfig {
            provider: "ollama".into(),
            model: "fixture:small".into(),
            endpoint,
            ..Default::default()
        });
        assert!(result.is_err());
        assert_eq!(server.join().unwrap().len(), 2);
    }
    #[test]
    fn loopback_only_adapter_rejects_remote_hosts_and_invalid_options() {
        let config = EnrichmentConfig {
            provider: "ollama".into(),
            model: "fixture:small".into(),
            ..Default::default()
        };
        assert!(validate(&config).is_ok());
        for endpoint in [
            "https://example.com",
            "http://127.0.0.1.attacker.com",
            "http://user@127.0.0.1",
            "http://127.0.0.1?key=x",
            "http://127.0.0.1/api",
            "http://localhost",
        ] {
            assert!(
                validate(&EnrichmentConfig {
                    endpoint: endpoint.into(),
                    ..config.clone()
                })
                .is_err(),
                "{endpoint}"
            );
        }
        assert!(
            validate(&EnrichmentConfig {
                endpoint: "http://[::1]:11434".into(),
                ..config.clone()
            })
            .is_ok()
        );
        assert!(!local_model(
            &json!({"remote_host":"https://ollama.com","remote_model":"cloud"})
        ));
        assert!(
            validate(&EnrichmentConfig {
                output_tokens: 8192,
                ..config
            })
            .is_err()
        );
    }
}
