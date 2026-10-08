
use super::*;
use std::cell::RefCell;

type Request = (String, String, String, Option<(String, Vec<u8>)>);
struct Fake {
    calls: RefCell<Vec<Request>>,
    missing: bool,
    status: RefCell<&'static str>,
    ready_url: String,
    lookup_error: RefCell<bool>,
    project_source: Value,
}
impl Fake {
    fn new() -> Self {
        Self {
            calls: RefCell::new(Vec::new()),
            missing: true,
            status: RefCell::new("success"),
            ready_url: "https://abc.my-project.pages.dev".into(),
            lookup_error: RefCell::new(false),
            project_source: Value::Null,
        }
    }
}
impl Transport for Fake {
    fn request(
        &self,
        method: &str,
        path: &str,
        token: &str,
        body: Option<(&str, Vec<u8>)>,
    ) -> Result<Value, String> {
        self.calls.borrow_mut().push((
            method.into(),
            path.into(),
            token.into(),
            body.as_ref().map(|(t, b)| (t.to_string(), b.clone())),
        ));
        if path.ends_with("/upload-token") {
            return Ok(json!({"jwt":"asset-jwt"}));
        }
        if path.ends_with("/check-missing") {
            let value: Value = serde_json::from_slice(&body.unwrap().1).unwrap();
            return Ok(if self.missing {
                json!([value["hashes"][0]])
            } else {
                json!([])
            });
        }
        if path.ends_with("/upload") || path.ends_with("/upsert-hashes") {
            return Ok(Value::Null);
        }
        if path.ends_with("/deployments") {
            return Ok(json!({"id":"deploy-123"}));
        }
        if path.ends_with("/deployments/deploy-123") {
            if *self.lookup_error.borrow() {
                return Err("publication-network-error".into());
            }
            return Ok(
                json!({"id":"deploy-123","latest_stage":{"name":"deploy","status":*self.status.borrow()},"url":self.ready_url}),
            );
        }
        Ok(json!({"source":self.project_source}))
    }
}
fn config() -> PublicationConfig {
    PublicationConfig {
        provider: "cloudflare-pages".into(),
        account_id: "0123456789abcdef0123456789abcdef".into(),
        project_name: "my-project".into(),
        ..Default::default()
    }
}
fn assets() -> Vec<Asset> {
    vec![
        Asset {
            path: "/index.html",
            content_type: "text/html; charset=utf-8",
            bytes: b"<h1>Public snapshot</h1>".to_vec(),
        },
        Asset {
            path: "/snapshot.json",
            content_type: "application/json",
            bytes: b"{\"schema_version\":1}".to_vec(),
        },
    ]
}
fn run(http: &Fake, dir: &Path) -> Result<Value, String> {
    deploy(http, &config(), "account-token", dir, "spec-123", &assets())
}

#[test]
fn native_upload_sends_only_missing_assets_and_confirms_the_remote_deployment() {
    let dir = tempfile::tempdir().unwrap();
    let http = Fake::new();
    let answer = run(&http, dir.path()).unwrap();
    assert_eq!(answer["published"], true);
    assert_eq!(answer["remote_url"], http.ready_url);
    let calls = http.calls.borrow();
    let uploads = calls
        .iter()
        .filter(|c| c.1.ends_with("/upload"))
        .collect::<Vec<_>>();
    assert_eq!(uploads.len(), 1);
    let upload: Value = serde_json::from_slice(&uploads[0].3.as_ref().unwrap().1).unwrap();
    assert_eq!(upload.as_array().unwrap().len(), 1);
    assert_eq!(
        upload[0]["metadata"]["contentType"],
        "text/html; charset=utf-8"
    );
    assert_eq!(
        base64::engine::general_purpose::STANDARD
            .decode(upload[0]["value"].as_str().unwrap())
            .unwrap(),
        assets()[0].bytes
    );
    assert!(
        calls
            .iter()
            .filter(|c| c.1.starts_with("/pages/assets/"))
            .all(|c| c.2 == "asset-jwt")
    );
    let create = calls
        .iter()
        .find(|c| c.0 == "POST" && c.1.ends_with("/deployments"))
        .unwrap();
    assert_eq!(create.2, "account-token");
    let form = String::from_utf8(create.3.as_ref().unwrap().1.clone()).unwrap();
    assert!(
        form.contains("name=\"manifest\"")
            && form.contains("/index.html")
            && form.contains("/snapshot.json")
    );
    assert!(!form.contains("remote.json") && !form.contains("account-token"));
    let receipt = std::fs::read_to_string(dir.path().join("remote.json")).unwrap();
    assert!(!receipt.contains("account-token") && !receipt.contains("asset-jwt"));
}
#[test]
fn existing_assets_are_not_uploaded_and_a_pending_snapshot_resumes_without_another_deployment() {
    let dir = tempfile::tempdir().unwrap();
    let mut http = Fake::new();
    http.missing = false;
    *http.status.borrow_mut() = "active";
    let first = run(&http, dir.path()).unwrap();
    assert_eq!(first["pending"], true);
    assert!(first["remote_url"].is_null());
    assert!(!http.calls.borrow().iter().any(|c| c.1.ends_with("/upload")));
    *http.status.borrow_mut() = "success";
    http.calls.borrow_mut().clear();
    let next = run(&http, dir.path()).unwrap();
    assert_eq!(next["published"], true);
    assert_eq!(http.calls.borrow().len(), 1);
    assert_eq!(http.calls.borrow()[0].0, "GET");
}
#[test]
fn a_failed_readiness_check_keeps_the_accepted_id_for_a_later_explicit_retry() {
    let dir = tempfile::tempdir().unwrap();
    let http = Fake::new();
    *http.lookup_error.borrow_mut() = true;
    assert_eq!(
        run(&http, dir.path()).unwrap_err(),
        "publication-network-error"
    );
    let receipt: Value =
        serde_json::from_slice(&std::fs::read(dir.path().join("remote.json")).unwrap()).unwrap();
    assert_eq!(receipt["deployment_id"], "deploy-123");
    assert_eq!(receipt["published"], false);
    *http.lookup_error.borrow_mut() = false;
    http.calls.borrow_mut().clear();
    assert_eq!(run(&http, dir.path()).unwrap()["published"], true);
    assert_eq!(http.calls.borrow().len(), 1);
}
#[test]
fn another_explicit_call_can_retry_a_server_failure_but_never_claims_a_pending_link() {
    let dir = tempfile::tempdir().unwrap();
    let http = Fake::new();
    *http.status.borrow_mut() = "failure";
    let failed = run(&http, dir.path()).unwrap();
    assert_eq!(failed["failed"], true);
    assert_eq!(failed["pending"], false);
    assert!(failed["remote_url"].is_null());
    *http.status.borrow_mut() = "success";
    http.calls.borrow_mut().clear();
    assert_eq!(run(&http, dir.path()).unwrap()["published"], true);
    assert!(
        http.calls
            .borrow()
            .iter()
            .any(|c| c.0 == "POST" && c.1.ends_with("/deployments"))
    );
}
#[test]
fn invalid_destinations_and_git_connected_projects_are_refused_before_deploying() {
    let dir = tempfile::tempdir().unwrap();
    let http = Fake::new();
    let mut invalid = config();
    invalid.project_name = "../foreign".into();
    assert!(deploy(&http, &invalid, "token", dir.path(), "spec-123", &assets()).is_err());
    assert!(http.calls.borrow().is_empty());
    let mut http = Fake::new();
    http.project_source = json!({"type":"github"});
    assert_eq!(
        run(&http, dir.path()).unwrap_err(),
        "publication-needs-direct-upload-project"
    );
    assert_eq!(http.calls.borrow().len(), 1);
    assert!(!dir.path().join("remote.json").exists());
}
#[test]
fn concurrent_publication_is_rejected_and_an_untrusted_url_is_never_confirmed() {
    let dir = tempfile::tempdir().unwrap();
    let lock = LockedFile::exclusive(&dir.path().join("publish.lock")).unwrap();
    let mut http = Fake::new();
    assert_eq!(
        run(&http, dir.path()).unwrap_err(),
        "publication-in-progress"
    );
    assert!(http.calls.borrow().is_empty());
    drop(lock);
    http.ready_url = "https://abc.my-project.pages.dev.attacker.test".into();
    assert_eq!(
        run(&http, dir.path()).unwrap_err(),
        "publication-no-confirmed-url"
    );
    let receipt: Value =
        serde_json::from_slice(&std::fs::read(dir.path().join("remote.json")).unwrap()).unwrap();
    assert_eq!(receipt["published"], false);
}
#[test]
fn changed_content_creates_a_new_explicit_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let http = Fake::new();
    run(&http, dir.path()).unwrap();
    http.calls.borrow_mut().clear();
    let mut changed = assets();
    changed[0].bytes.extend_from_slice(b" updated");
    deploy(&http, &config(), "token", dir.path(), "spec-123", &changed).unwrap();
    assert!(
        http.calls
            .borrow()
            .iter()
            .any(|c| c.0 == "POST" && c.1.ends_with("/deployments"))
    );
}
#[test]
fn actual_http_errors_do_not_echo_credentials_or_remote_diagnostics() {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut bytes = [0u8; 4096];
        let n = stream.read(&mut bytes).unwrap();
        let request = String::from_utf8_lossy(&bytes[..n]);
        assert!(request.contains("Bearer secret-token"));
        let body = "secret-token private-diagnostic";
        stream.write_all(format!("HTTP/1.1 403 Forbidden\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).unwrap();
    });
    let http = Http {
        api: format!("http://{address}"),
        agent: ureq::Agent::config_builder()
            .max_redirects(0)
            .http_status_as_error(false)
            .build()
            .new_agent(),
        deadline: Instant::now() + Duration::from_secs(5),
    };
    assert_eq!(
        http.request("GET", "/project", "secret-token", None)
            .unwrap_err(),
        "publication-http-403"
    );
    server.join().unwrap();
}
