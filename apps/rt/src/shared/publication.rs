//! Native Cloudflare Pages Direct Upload. Only explicit commands call this
//! module. No model, generated script, CLI helper or private Claude API.
//! Assets use the official Wrangler hash format; unchanged assets are skipped.
//! A server-accepted deployment is persisted before checking readiness so a
//! repeated command resumes the same deployment instead of creating another.

use std::fmt::Write as _;
use std::path::Path;
use std::time::{Duration, Instant};

use base64::Engine as _;
use mustard_core::domain::config::PublicationConfig;
use mustard_core::io::fs::lock::LockedFile;
use mustard_core::io::sha256::Sha256;
use serde_json::{Value, json};

const API: &str = "https://api.cloudflare.com/client/v4";
const TOKEN_ENV: &str = "CLOUDFLARE_API_TOKEN";
const TIMEOUT: Duration = Duration::from_secs(60);

pub(crate) struct Asset {
    pub(crate) path: &'static str,
    pub(crate) content_type: &'static str,
    pub(crate) bytes: Vec<u8>,
}

pub(crate) fn publish(root: &Path, dir: &Path, scope: &str, assets: &[Asset]) -> Value {
    let Some(config) = mustard_core::ProjectConfig::load(root).publication else {
        return json!({"published":false,"transport_required":true,"reason":"publication-not-configured",
            "hint":"Configure publication com provider cloudflare-pages, accountId e projectName; forneça CLOUDFLARE_API_TOKEN no ambiente. Os arquivos continuam locais."});
    };
    let token = std::env::var(TOKEN_ENV)
        .ok()
        .filter(|token| !token.trim().is_empty());
    let Some(token) = token else {
        return json!({"published":false,"transport_required":true,"reason":"publication-token-missing"});
    };
    let http = Http {
        api: API.into(),
        agent: ureq::Agent::config_builder()
            .timeout_connect(Some(Duration::from_secs(5)))
            .max_redirects(0)
            .http_status_as_error(false)
            .build()
            .new_agent(),
        deadline: Instant::now() + TIMEOUT,
    };
    match deploy(&http, &config, &token, dir, scope, assets) {
        Ok(receipt) => receipt,
        Err(reason) => json!({"published":false,"transport_required":false,"reason":reason}),
    }
}

trait Transport {
    fn request(
        &self,
        method: &str,
        path: &str,
        token: &str,
        body: Option<(&str, Vec<u8>)>,
    ) -> Result<Value, String>;
}

struct Http {
    api: String,
    agent: ureq::Agent,
    deadline: Instant,
}

impl Transport for Http {
    fn request(
        &self,
        method: &str,
        path: &str,
        token: &str,
        body: Option<(&str, Vec<u8>)>,
    ) -> Result<Value, String> {
        let left = self
            .deadline
            .checked_duration_since(Instant::now())
            .ok_or("publication-timeout")?;
        let url = format!("{}{path}", self.api);
        let authorization = format!("Bearer {token}");
        let response = if method == "GET" {
            self.agent
                .get(&url)
                .header("Authorization", &authorization)
                .config()
                .timeout_global(Some(left))
                .build()
                .call()
        } else {
            let (content_type, bytes) = body.ok_or("publication-missing-body")?;
            self.agent
                .post(&url)
                .header("Authorization", &authorization)
                .header("Content-Type", content_type)
                .config()
                .timeout_global(Some(left))
                .build()
                .send(&bytes)
        };
        // Do not expose a response body, authentication header or URL in an
        // error. Remote diagnostic text may echo submitted credentials/data.
        let mut response = response.map_err(|_| "publication-network-error")?;
        if !response.status().is_success() {
            return Err(format!("publication-http-{}", response.status().as_u16()));
        }
        let value = response
            .body_mut()
            .read_json::<Value>()
            .map_err(|_| "publication-invalid-response")?;
        if value["success"] != true {
            return Err("publication-api-refused".into());
        }
        Ok(value.get("result").cloned().unwrap_or(Value::Null))
    }
}

fn deploy(
    http: &dyn Transport,
    config: &PublicationConfig,
    token: &str,
    dir: &Path,
    scope: &str,
    assets: &[Asset],
) -> Result<Value, String> {
    validate(config, scope, assets)?;
    for name in ["publish.lock", "remote.json"] {
        if std::fs::symlink_metadata(dir.join(name)).is_ok_and(|meta| meta.file_type().is_symlink())
        {
            return Err("publication-unsafe-receipt".into());
        }
    }
    let _lock = LockedFile::exclusive_if_free(&dir.join("publish.lock"))
        .map_err(|_| "publication-lock-error")?
        .ok_or("publication-in-progress")?;
    let mut digest = Sha256::new();
    for asset in assets {
        digest.update(asset.path.as_bytes());
        digest.update(&[0]);
        digest.update(&asset.bytes);
        digest.update(&[0]);
    }
    let version = digest.hex_digest();
    let branch = format!("mustard-{scope}");
    let identity = json!({"provider":config.provider,"account":config.account_id,"project":config.project_name,"branch":branch,"content":version});
    let receipt_path = dir.join("remote.json");
    let previous = std::fs::read(&receipt_path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok());
    let base = format!(
        "/accounts/{}/pages/projects/{}",
        config.account_id, config.project_name
    );
    let mut receipt = if let Some(previous) = previous.filter(|value| {
        value["identity"] == identity
            && value["failed"] != true
            && valid_id(value["deployment_id"].as_str())
    }) {
        previous
    } else {
        let project = http.request("GET", &base, token, None)?;
        if project
            .get("source")
            .is_some_and(|source| !source.is_null())
        {
            return Err("publication-needs-direct-upload-project".into());
        }
        let auth = http.request("GET", &format!("{base}/upload-token"), token, None)?;
        let jwt = auth["jwt"]
            .as_str()
            .filter(|jwt| !jwt.is_empty())
            .ok_or("publication-no-upload-token")?;
        let keys: Vec<String> = assets.iter().map(asset_key).collect();
        let hashes = json!({"hashes":keys}).to_string().into_bytes();
        let missing = http.request(
            "POST",
            "/pages/assets/check-missing",
            jwt,
            Some(("application/json", hashes.clone())),
        )?;
        let missing = missing
            .as_array()
            .ok_or("publication-invalid-missing-assets")?;
        if missing
            .iter()
            .any(|key| !keys.iter().any(|known| key == known))
        {
            return Err("publication-invalid-missing-assets".into());
        }
        let upload: Vec<Value> = assets.iter().zip(&keys).filter(|(_,key)|missing.iter().any(|value|value == *key))
            .map(|(asset,key)|json!({"key":key,"value":base64::engine::general_purpose::STANDARD.encode(&asset.bytes),
                "base64":true,"metadata":{"contentType":asset.content_type}})).collect();
        if !upload.is_empty() {
            http.request(
                "POST",
                "/pages/assets/upload",
                jwt,
                Some(("application/json", json!(upload).to_string().into_bytes())),
            )?;
        }
        http.request(
            "POST",
            "/pages/assets/upsert-hashes",
            jwt,
            Some(("application/json", hashes)),
        )?;
        let manifest: serde_json::Map<String, Value> = assets
            .iter()
            .zip(keys)
            .map(|(asset, key)| (asset.path.to_string(), json!(key)))
            .collect();
        // The boundary uses a hex digest, which cannot appear at a delimiter
        // position inside these JSON/plain branch values.
        let boundary = format!("mustard-{version}");
        let body = multipart(
            &boundary,
            &[
                ("branch", branch.clone()),
                ("manifest", Value::Object(manifest).to_string()),
                ("commit_message", format!("Mustard snapshot {version}")),
            ],
        );
        let deployment = http.request(
            "POST",
            &format!("{base}/deployments"),
            token,
            Some((&format!("multipart/form-data; boundary={boundary}"), body)),
        )?;
        if !valid_id(deployment["id"].as_str()) {
            return Err("publication-no-deployment-id".into());
        }
        let receipt =
            json!({"identity":identity,"deployment_id":deployment["id"],"published":false});
        mustard_core::io::fs::write_atomic(&receipt_path, receipt.to_string().as_bytes())
            .map_err(|_| "publication-receipt-write-error")?;
        receipt
    };
    let id = receipt["deployment_id"]
        .as_str()
        .ok_or("publication-no-deployment-id")?
        .to_string();
    let deployment = http.request("GET", &format!("{base}/deployments/{id}"), token, None)?;
    if deployment["id"] != id {
        return Err("publication-deployment-mismatch".into());
    }
    let ready = deployment["latest_stage"]["name"] == "deploy"
        && deployment["latest_stage"]["status"] == "success";
    let failed = matches!(
        deployment["latest_stage"]["status"].as_str(),
        Some("failure" | "canceled")
    );
    let url = deployment["url"]
        .as_str()
        .filter(|url| valid_url(url, &config.project_name));
    if ready && url.is_none() {
        return Err("publication-no-confirmed-url".into());
    }
    receipt["published"] = json!(ready);
    receipt["failed"] = json!(failed);
    receipt["remote_url"] = if ready { json!(url) } else { Value::Null };
    mustard_core::io::fs::write_atomic(&receipt_path, receipt.to_string().as_bytes())
        .map_err(|_| "publication-receipt-write-error")?;
    Ok(
        json!({"published":ready,"deployment_id":id,"remote_url":receipt["remote_url"],
        "provider":"cloudflare-pages","pending":!ready && !failed,"failed":failed,"transport_required":false,
        "hint":if ready {"Publicação confirmada pela API."} else if failed {"A publicação falhou no servidor. Consulte o projeto no Cloudflare e repita o comando para tentar novamente."} else {"Publicação aceita e ainda em processamento. Repita o comando para consultar a mesma publicação, sem criar outra."}}),
    )
}

fn validate(config: &PublicationConfig, scope: &str, assets: &[Asset]) -> Result<(), String> {
    if config.provider != "cloudflare-pages"
        || config.account_id.len() != 32
        || !config.account_id.bytes().all(|b| b.is_ascii_hexdigit())
        || !config
            .project_name
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphanumeric)
        || !config
            .project_name
            .as_bytes()
            .last()
            .is_some_and(u8::is_ascii_alphanumeric)
        || config.project_name.is_empty()
        || config.project_name.len() > 58
        || !config
            .project_name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        || scope.is_empty()
        || scope.len() > 58
        || !scope
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
    {
        return Err("publication-invalid-configuration".into());
    }
    if assets.len() != 2
        || assets[0].path == assets[1].path
        || assets.iter().any(|asset| {
            !matches!(asset.path, "/index.html" | "/snapshot.json")
                || asset.bytes.len() > 25 * 1024 * 1024
        })
    {
        return Err("publication-invalid-assets".into());
    }
    Ok(())
}

fn valid_id(id: Option<&str>) -> bool {
    id.is_some_and(|id| {
        !id.is_empty()
            && id.len() <= 64
            && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
    })
}

fn valid_url(url: &str, project: &str) -> bool {
    let Some(host) = url
        .strip_prefix("https://")
        .map(|rest| rest.trim_end_matches('/'))
    else {
        return false;
    };
    let suffix = format!(".{project}.pages.dev");
    host.strip_suffix(&suffix).is_some_and(|prefix| {
        !prefix.is_empty()
            && prefix
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
    })
}

fn asset_key(asset: &Asset) -> String {
    let encoded = base64::engine::general_purpose::STANDARD.encode(&asset.bytes);
    let extension = asset
        .path
        .rsplit_once('.')
        .map_or("", |(_, extension)| extension);
    blake3::hash(format!("{encoded}{extension}").as_bytes()).to_hex()[..32].to_string()
}

fn multipart(boundary: &str, fields: &[(&str, String)]) -> Vec<u8> {
    let mut body = String::new();
    for (name, value) in fields {
        let _ = write!(
            body,
            "--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n"
        );
    }
    let _ = write!(body, "--{boundary}--\r\n");
    body.into_bytes()
}

mod export;
pub(crate) use export::{prepare_snapshot, upload_prepared};

#[cfg(test)]
mod tests;
