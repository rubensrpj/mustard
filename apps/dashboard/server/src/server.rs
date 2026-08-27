//! The HTTP face of the dashboard backend — the seam that replaced Tauri.
//!
//! Modelled on `apps/rt/src/commands/economy/otel/collector.rs`: a blocking
//! [`tiny_http`] server with no async runtime, bound to `127.0.0.1` unless the
//! operator names another host, dispatching by method + path.
//!
//! Routes:
//!   - `POST /api/{command}` — the body is the argument object the frontend
//!     used to hand `invoke(command, args)`; the response is the same JSON the
//!     command already returned. A `Result::Err` becomes `400` with the
//!     message, exactly as `invoke` rejected.
//!   - `GET  /api/commands`  — the dispatch table's names, for probes.
//!   - `GET  /api/events`    — server-sent events. The watcher's notifications
//!     leave through here instead of `AppHandle::emit`.
//!   - `GET  /*`             — the built React assets, with `index.html` as the
//!     fallback so the dashboard's own client-side routes resolve.
//!
//! Fail-open contract, inherited from the collector: a malformed request
//! answers `400` and a panicking command answers `500`, but neither ever takes
//! the server down. The accept loop additionally honours an in-process
//! shutdown flag — the seam the inline tests and the AC scripts drive.

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::fs::File;
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, TrySendError, sync_channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tiny_http::{Header, Method, Request, Response, Server};

use crate::watcher;

/// Default listen port. Mirrors the collector's `MUSTARD_OTEL_PORT` contract:
/// an env var names the port, and the code carries the fallback.
pub const DEFAULT_PORT: u16 = 7777;

/// Env var naming the listen port, the precedent being `MUSTARD_OTEL_PORT`.
pub const PORT_ENV: &str = "MUSTARD_DASHBOARD_PORT";

/// Env var naming the directory of built React assets. Checked before the
/// paths derived from the executable, so a packaged install and a `cargo run`
/// from a checkout both resolve without a flag.
pub const DIST_ENV: &str = "MUSTARD_DASHBOARD_DIST";

/// Loopback default. Exposing the dashboard requires `--host` explicitly: it
/// reads every project's `.claude/`, so reaching the network must be an act,
/// not a forgotten flag.
pub const DEFAULT_HOST: &str = "127.0.0.1";

/// How many successive ports [`bind`] tries before giving up. A taken port is
/// a minutes-for-nothing failure, so the server walks forward and prints the
/// port it actually got.
const PORT_WALK: u16 = 20;

/// How long the accept loop blocks before re-checking [`Ctx::shutdown`].
const ACCEPT_POLL: Duration = Duration::from_millis(200);

/// Idle gap after which an open events stream is sent a comment frame, so a
/// proxy between the operator (over Tailscale) and the server does not reap
/// the connection as dead.
const SSE_KEEPALIVE: Duration = Duration::from_secs(15);

/// Frames buffered per open events stream. A reader that falls this far behind
/// loses frames rather than growing the buffer without bound — the dashboard
/// refetches on the next frame it does receive, so a dropped notification
/// costs a delay, never correctness.
const SSE_BACKLOG: usize = 64;

// ---------------------------------------------------------------------------
// Server-sent events
// ---------------------------------------------------------------------------

/// One subscriber's slot on the [`EventBus`].
struct Sub {
    id: u64,
    tx: SyncSender<String>,
}

/// The fan-out point for everything the server pushes at open browsers.
///
/// The watcher used to hand its notifications to `AppHandle::emit`; it hands
/// them here instead, and each `GET /api/events` connection drains its own
/// channel. Cloning the bus shares the subscriber list.
#[derive(Clone, Default)]
pub struct EventBus {
    subs: Arc<Mutex<Vec<Sub>>>,
    next_id: Arc<AtomicU64>,
}

impl EventBus {
    /// Register a stream and return its handle. Dropping the [`Subscription`]
    /// unregisters it, so a browser tab closing needs no explicit teardown.
    fn subscribe(&self) -> Subscription {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = sync_channel(SSE_BACKLOG);
        if let Ok(mut subs) = self.subs.lock() {
            subs.push(Sub { id, tx });
        }
        Subscription {
            id,
            rx,
            bus: self.clone(),
        }
    }

    fn unsubscribe(&self, id: u64) {
        if let Ok(mut subs) = self.subs.lock() {
            subs.retain(|s| s.id != id);
        }
    }

    /// Push one named event to every open stream. Fail-silent by design: a
    /// poisoned lock, a serialisation failure or a saturated subscriber must
    /// never propagate back into the watcher callback that called this.
    pub fn emit<T: Serialize>(&self, event: &str, payload: &T) {
        let Ok(body) = serde_json::to_string(payload) else {
            return;
        };
        let frame = format!("event: {event}\ndata: {body}\n\n");
        let Ok(mut subs) = self.subs.lock() else {
            return;
        };
        // A disconnected receiver means the browser is gone; drop the slot so
        // the list does not accumulate dead tabs.
        subs.retain(|sub| !matches!(sub.tx.try_send(frame.clone()), Err(TrySendError::Disconnected(_))));
    }

    /// Number of open streams. Exposed for the AC probe and the inline tests.
    #[must_use]
    pub fn subscriber_count(&self) -> usize {
        self.subs.lock().map(|s| s.len()).unwrap_or(0)
    }
}

/// One open events stream. Unregisters itself on drop.
struct Subscription {
    id: u64,
    rx: Receiver<String>,
    bus: EventBus,
}

impl Drop for Subscription {
    fn drop(&mut self) {
        self.bus.unsubscribe(self.id);
    }
}

// ---------------------------------------------------------------------------
// Server context
// ---------------------------------------------------------------------------

/// Everything a request handler may need beyond its own arguments.
///
/// The Tauri equivalents were `tauri::State` (the watcher registry) and
/// `AppHandle` (the emit target); both collapse into this one value, shared by
/// every worker thread.
pub struct Ctx {
    /// Per-repo filesystem watchers, keyed by repo path.
    pub watchers: Arc<Mutex<watcher::WatcherState>>,
    /// Where watcher notifications go.
    pub bus: EventBus,
    /// The discovery root: the directory the server was started in, or
    /// `--root`. The native folder dialog died with Tauri, so this is the only
    /// answer to "which machine's projects?" — the server's own.
    pub root: PathBuf,
    /// Directory of built React assets served by `GET /*`.
    pub dist: PathBuf,
    /// Flipped to stop the accept loops.
    pub shutdown: Arc<AtomicBool>,
}

impl Ctx {
    /// Build a context rooted at `root`, resolving the asset directory through
    /// [`resolve_dist`].
    #[must_use]
    pub fn new(root: PathBuf) -> Self {
        Self {
            watchers: Arc::new(Mutex::new(watcher::WatcherState::default())),
            bus: EventBus::default(),
            root,
            dist: resolve_dist(),
            shutdown: Arc::new(AtomicBool::new(false)),
        }
    }
}

/// Locate the built React assets.
///
/// In order: `MUSTARD_DASHBOARD_DIST`, `<exe dir>/dist` (a packaged install
/// ships the assets beside the binary), then `<crate>/../dist` (a `cargo run`
/// out of a checkout, where `pnpm build` writes `apps/dashboard/dist`). The
/// last candidate is returned even when absent so the 404 body can name the
/// path the operator was expected to populate.
fn resolve_dist() -> PathBuf {
    if let Some(from_env) = std::env::var_os(DIST_ENV) {
        return PathBuf::from(from_env);
    }
    if let Some(beside_exe) = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join("dist")))
        .filter(|dir| dir.is_dir())
    {
        return beside_exe;
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("dist")
}

// ---------------------------------------------------------------------------
// Binding and the accept loop
// ---------------------------------------------------------------------------

/// Resolve the listen port from `--port`, then `MUSTARD_DASHBOARD_PORT`, then
/// [`DEFAULT_PORT`].
#[must_use]
pub fn resolve_port(explicit: Option<u16>) -> u16 {
    explicit.unwrap_or_else(|| {
        std::env::var(PORT_ENV)
            .ok()
            .and_then(|s| s.parse::<u16>().ok())
            .unwrap_or(DEFAULT_PORT)
    })
}

/// Bind `host` at `port`, walking forward to the next free port when it is
/// taken. Returns the server and the port it actually got — dying because
/// something else held the port costs minutes for nothing.
///
/// The port comes back from the bound socket rather than from the candidate,
/// because port `0` asks the OS for an ephemeral one and only the socket knows
/// which it handed out.
pub fn bind(host: &str, port: u16) -> Result<(Server, u16), String> {
    let mut last = String::new();
    // Port 0 is "any free port" — the OS already did the walking.
    let attempts = if port == 0 { 1 } else { PORT_WALK };
    for offset in 0..attempts {
        let candidate = port.saturating_add(offset);
        match Server::http(format!("{host}:{candidate}")) {
            Ok(server) => {
                let bound = server
                    .server_addr()
                    .to_ip()
                    .map_or(candidate, |addr| addr.port());
                return Ok((server, bound));
            }
            Err(e) => last = e.to_string(),
        }
    }
    Err(format!(
        "no free port in {port}..{} on {host}: {last}",
        port.saturating_add(attempts)
    ))
}

/// Serve until [`Ctx::shutdown`] is raised.
///
/// `workers` threads share the accept loop; each request is handled on the
/// thread that accepted it, which is why every command body below is plain
/// synchronous code (the Tauri versions had to hop to `spawn_blocking` to keep
/// the UI thread free — there is no UI thread here).
pub fn serve(server: Server, ctx: Arc<Ctx>, workers: usize) {
    let server = Arc::new(server);
    let mut handles = Vec::with_capacity(workers);
    for _ in 0..workers.max(1) {
        let server = Arc::clone(&server);
        let ctx = Arc::clone(&ctx);
        handles.push(std::thread::spawn(move || accept_loop(&server, &ctx)));
    }
    for handle in handles {
        let _ = handle.join();
    }
}

/// One worker's accept loop. `recv_timeout` rather than `recv` so raising the
/// shutdown flag drains the loop on its own, without a dummy request.
fn accept_loop(server: &Server, ctx: &Arc<Ctx>) {
    while !ctx.shutdown.load(Ordering::SeqCst) {
        match server.recv_timeout(ACCEPT_POLL) {
            Ok(Some(request)) => handle_one(request, ctx),
            Ok(None) => {}
            Err(_) => break,
        }
    }
}

/// The number of accept threads to run. Bounded: the dashboard fans out a
/// handful of parallel fetches per page, and every extra thread is an extra
/// concurrent workspace walk.
#[must_use]
pub fn default_workers() -> usize {
    std::thread::available_parallelism()
        .map(std::num::NonZeroUsize::get)
        .unwrap_or(4)
        .clamp(2, 8)
}

// ---------------------------------------------------------------------------
// Routing
// ---------------------------------------------------------------------------

/// Route, execute, respond.
fn handle_one(mut request: Request, ctx: &Arc<Ctx>) {
    let method = request.method().clone();
    let url = request.url().to_string();
    let path = url.split('?').next().unwrap_or("").to_string();

    if method == Method::Get && path == "/api/events" {
        // The stream lives as long as the browser tab. Hand it to its own
        // thread so it never occupies a slot in the accept pool.
        let ctx = Arc::clone(ctx);
        std::thread::spawn(move || stream_events(request, &ctx));
        return;
    }

    if method == Method::Get && path == "/api/commands" {
        let names: Vec<&str> = COMMANDS.iter().map(|(name, _)| *name).collect();
        respond_json(request, 200, &serde_json::json!({ "commands": names }));
        return;
    }

    if let Some(name) = path.strip_prefix("/api/") {
        if method != Method::Post {
            respond_error(request, 405, "commands are POST");
            return;
        }
        let name = name.to_string();
        let mut body = String::new();
        if std::io::Read::read_to_string(request.as_reader(), &mut body).is_err() {
            respond_error(request, 400, "body read failed");
            return;
        }
        // An empty body is the no-argument call, not a malformed one.
        let args: Value = if body.trim().is_empty() {
            Value::Object(serde_json::Map::new())
        } else {
            match serde_json::from_str(&body) {
                Ok(v) => v,
                Err(e) => {
                    respond_error(request, 400, &format!("invalid JSON body: {e}"));
                    return;
                }
            }
        };
        match dispatch(&name, ctx, &args) {
            Ok(value) => respond_json(request, 200, &value),
            Err(DispatchError::Unknown) => {
                respond_error(request, 404, &format!("unknown command `{name}`"));
            }
            Err(DispatchError::Failed(msg)) => respond_error(request, 400, &msg),
            Err(DispatchError::Panicked) => {
                respond_error(request, 500, &format!("command `{name}` panicked"));
            }
        }
        return;
    }

    if method == Method::Get || method == Method::Head {
        serve_asset(request, &ctx.dist, &path);
        return;
    }
    respond_error(request, 404, "not found");
}

/// Why a `POST /api/{command}` did not produce a value.
enum DispatchError {
    /// No such name in [`COMMANDS`].
    Unknown,
    /// The command returned `Err` — the `invoke` rejection, verbatim.
    Failed(String),
    /// The command panicked. Tauri's `spawn_blocking(..).await` join error
    /// used to absorb this per-command; the server absorbs it here so one bad
    /// request cannot take a worker thread down with it.
    Panicked,
}

/// Look `name` up in the dispatch table and run it.
fn dispatch(name: &str, ctx: &Arc<Ctx>, args: &Value) -> Result<Value, DispatchError> {
    let (_, handler) = COMMANDS
        .iter()
        .find(|(candidate, _)| *candidate == name)
        .ok_or(DispatchError::Unknown)?;
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| handler(ctx, args)));
    match outcome {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(msg)) => Err(DispatchError::Failed(msg)),
        Err(_) => Err(DispatchError::Panicked),
    }
}

// ---------------------------------------------------------------------------
// Responses
// ---------------------------------------------------------------------------

/// Build a header, or `None` when the pair is not a valid header (impossible
/// for the static pairs used here — the fallback keeps this panic-free).
fn header(name: &str, value: &str) -> Option<Header> {
    Header::from_bytes(name.as_bytes(), value.as_bytes()).ok()
}

fn respond_json(request: Request, status: u16, value: &Value) {
    let body = serde_json::to_string(value).unwrap_or_else(|_| "null".to_string());
    let mut response = Response::from_string(body).with_status_code(status);
    if let Some(h) = header("Content-Type", "application/json; charset=utf-8") {
        response = response.with_header(h);
    }
    let _ = request.respond(response);
}

fn respond_error(request: Request, status: u16, message: &str) {
    respond_json(request, status, &serde_json::json!({ "error": message }));
}

// ---------------------------------------------------------------------------
// GET /api/events
// ---------------------------------------------------------------------------

/// Hold one server-sent-events connection open, writing frames as the bus
/// produces them.
///
/// The response is written by hand over the raw socket rather than through
/// `Response`, because the body has no end: `into_writer` is the documented
/// escape hatch for exactly this (CGI-style streaming).
fn stream_events(request: Request, ctx: &Arc<Ctx>) {
    let subscription = ctx.bus.subscribe();
    let mut writer = request.into_writer();
    let preamble = concat!(
        "HTTP/1.1 200 OK\r\n",
        "Content-Type: text/event-stream\r\n",
        "Cache-Control: no-cache\r\n",
        "Connection: close\r\n",
        "X-Accel-Buffering: no\r\n",
        "\r\n",
        // An opening comment flushes the headers, so `EventSource.onopen`
        // fires before the first real change lands.
        ": mustard dashboard events\n\n",
    );
    if writer.write_all(preamble.as_bytes()).is_err() || writer.flush().is_err() {
        return;
    }

    while !ctx.shutdown.load(Ordering::SeqCst) {
        let frame = match subscription.rx.recv_timeout(SSE_KEEPALIVE) {
            Ok(frame) => frame,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => ": keepalive\n\n".to_string(),
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        };
        // A write error is the browser having closed the tab — the normal end
        // of a stream, not a fault. Dropping `subscription` unregisters it.
        if writer.write_all(frame.as_bytes()).is_err() || writer.flush().is_err() {
            break;
        }
    }
}

// ---------------------------------------------------------------------------
// GET /* — the built React assets
// ---------------------------------------------------------------------------

/// Serve `url_path` out of `dist`, falling back to `index.html` so the
/// dashboard's own client-side routes resolve on a hard reload.
fn serve_asset(request: Request, dist: &Path, url_path: &str) {
    let index = dist.join("index.html");
    let target = match safe_join(dist, url_path) {
        Some(path) if path.is_file() => path,
        // Both a client-side route and a typo land here; the SPA renders its
        // own not-found, which is the behaviour the desktop shell had.
        _ => index.clone(),
    };

    let Ok(file) = File::open(&target) else {
        respond_error(
            request,
            404,
            &format!(
                "dashboard assets not found at {} — build them with `pnpm --filter mustard-dashboard build`, or point {DIST_ENV} at them",
                dist.display()
            ),
        );
        return;
    };
    let mut response = Response::from_file(file);
    if let Some(h) = header("Content-Type", content_type(&target)) {
        response = response.with_header(h);
    }
    // The hashed asset names Vite emits are immutable; index.html is not.
    let cache = if target == index {
        "no-cache"
    } else {
        "public, max-age=31536000, immutable"
    };
    if let Some(h) = header("Cache-Control", cache) {
        response = response.with_header(h);
    }
    let _ = request.respond(response);
}

/// Resolve `url_path` under `dist`, refusing anything that escapes it.
///
/// Only plain names survive: `..`, absolute segments and Windows prefixes are
/// rejected outright rather than normalised, so no request can read a file the
/// operator did not publish.
fn safe_join(dist: &Path, url_path: &str) -> Option<PathBuf> {
    let trimmed = url_path.trim_start_matches('/');
    if trimmed.is_empty() {
        return None;
    }
    let mut out = dist.to_path_buf();
    for segment in Path::new(trimmed).components() {
        match segment {
            Component::Normal(name) => out.push(name),
            _ => return None,
        }
    }
    Some(out)
}

/// Content type for the extensions Vite emits. Anything else is served as
/// opaque bytes — a wrong guess would be worse than none.
fn content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("js" | "mjs") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("json") => "application/json; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("ico") => "image/x-icon",
        Some("woff2") => "font/woff2",
        Some("woff") => "font/woff",
        Some("ttf") => "font/ttf",
        Some("map") => "application/json; charset=utf-8",
        _ => "application/octet-stream",
    }
}

// ---------------------------------------------------------------------------
// The dispatch table
// ---------------------------------------------------------------------------

/// One entry's body: the arguments object in, the command's own JSON out.
type Handler = fn(&Arc<Ctx>, &Value) -> Result<Value, String>;

/// Read one `invoke`-style argument out of the request body.
///
/// The frontend passes these keys in camelCase (`repoPath`, `specName`) and
/// Tauri's serde used to map them onto the snake_case parameters. That mapping
/// is explicit now: each table entry names the camelCase key it reads, and the
/// snake_case spelling is accepted as well so a `curl` probe can use either.
///
/// A missing key deserializes from `null`, which is exactly what `Option<T>`
/// wants and exactly what a required `T` rejects — so optional arguments stay
/// optional without a second extractor.
fn arg<T: DeserializeOwned>(args: &Value, key: &str) -> Result<T, String> {
    let snake = snake_case(key);
    let raw = args
        .get(key)
        .or_else(|| args.get(snake.as_str()))
        .cloned()
        .unwrap_or(Value::Null);
    serde_json::from_value(raw).map_err(|e| format!("argument `{key}`: {e}"))
}

/// `repoPath` → `repo_path`. ASCII-only, matching the key vocabulary the
/// frontend actually sends.
fn snake_case(camel: &str) -> String {
    let mut out = String::with_capacity(camel.len() + 4);
    for ch in camel.chars() {
        if ch.is_ascii_uppercase() {
            out.push('_');
            out.push(ch.to_ascii_lowercase());
        } else {
            out.push(ch);
        }
    }
    out
}

/// Serialize a command's return value. A type that cannot serialize is a
/// programming error, not a request error, so it surfaces as a message rather
/// than silently becoming `null`.
fn encode<T: Serialize>(value: &T) -> Result<Value, String> {
    serde_json::to_value(value).map_err(|e| e.to_string())
}

/// Wire a `Result`-returning command into the table. The invocation mirrors
/// the function's own signature: one `"key"` per parameter, in order.
macro_rules! cmd {
    ($f:path $(, $key:literal)* $(,)?) => {
        |_ctx: &Arc<Ctx>, _args: &Value| -> Result<Value, String> {
            encode(&$f($(arg(_args, $key)?),*)?)
        }
    };
}

/// Wire an infallible command (one that answers with a zeroed/empty body
/// instead of an error) into the table.
macro_rules! cmd_ok {
    ($f:path $(, $key:literal)* $(,)?) => {
        |_ctx: &Arc<Ctx>, _args: &Value| -> Result<Value, String> {
            encode(&$f($(arg(_args, $key)?),*))
        }
    };
}

/// Every command the dashboard can call, by the name the frontend uses.
///
/// This is what `tauri::generate_handler![]` used to be. Forgetting an entry
/// still compiles — but `GET /api/commands` lists the table, so a probe can
/// tell the two apart, which the macro never allowed.
static COMMANDS: &[(&str, Handler)] = &[
    // --- workspace --------------------------------------------------------
    ("dashboard_subprojects", cmd!(crate::dashboard_subprojects, "repoPath")),
    ("dashboard_skills", cmd!(crate::dashboard_skills, "repoPath")),
    ("dashboard_recent_events", cmd!(crate::dashboard_recent_events, "repoPath", "limit")),
    ("dashboard_workspace_summary", cmd!(crate::dashboard_workspace_summary, "repoPath")),
    ("workspace_health", cmd_ok!(crate::workspace_health, "repoPath")),
    ("dashboard_read_env", cmd!(crate::dashboard_read_env, "repoPath")),
    ("dashboard_write_env", cmd!(crate::dashboard_write_env, "repoPath", "env")),
    ("dashboard_consumption", cmd!(crate::dashboard_consumption, "repoPath")),
    ("dashboard_friction", cmd!(crate::dashboard_friction, "repoPath")),
    ("dashboard_active_pipelines", cmd!(crate::dashboard_active_pipelines, "repoPath")),
    // --- specs ------------------------------------------------------------
    ("dashboard_specs", cmd!(crate::dashboard_specs, "repoPath")),
    ("dashboard_spec_markdown", cmd!(crate::dashboard_spec_markdown, "repoPath", "specName")),
    ("dashboard_spec_complete", cmd!(crate::dashboard_spec_complete, "repoPath", "specName")),
    ("dashboard_spec_cancel", cmd!(crate::dashboard_spec_cancel, "repoPath", "specName")),
    ("dashboard_spec_reactivate", cmd!(crate::dashboard_spec_reactivate, "repoPath", "specName")),
    ("dashboard_spec_card", cmd!(crate::dashboard_spec_card, "repoPath", "spec")),
    ("dashboard_spec_cards", cmd!(crate::dashboard_spec_cards, "repoPath")),
    ("dashboard_spec_waves", cmd!(crate::dashboard_spec_waves, "repoPath", "spec")),
    ("dashboard_spec_waves_planned", cmd!(crate::dashboard_spec_waves_planned, "repoPath", "spec")),
    ("dashboard_spec_wave_files", cmd!(crate::dashboard_spec_wave_files, "repoPath", "spec", "wave")),
    ("dashboard_spec_checklist_progress", cmd!(crate::dashboard_spec_checklist_progress, "repoPath", "spec")),
    ("dashboard_spec_quality", cmd!(crate::dashboard_spec_quality, "repoPath", "spec")),
    ("dashboard_spec_action", cmd!(crate::dashboard_spec_action, "repoPath", "spec", "action")),
    ("dashboard_spec_children", cmd!(crate::dashboard_spec_children, "repoPath", "parent")),
    ("spec_children_tree", cmd!(crate::spec_children_tree, "spec", "projectPath")),
    ("dashboard_spec_plan_staleness", cmd!(crate::spec_staleness::dashboard_spec_plan_staleness, "repoPath", "spec", "startedAt")),
    // --- knowledge --------------------------------------------------------
    ("dashboard_search_knowledge", cmd!(crate::dashboard_search_knowledge, "repoPath", "query", "limit")),
    ("dashboard_knowledge_browse", cmd!(crate::dashboard_knowledge_browse, "repoPath", "limit")),
    // --- telemetry / economy ---------------------------------------------
    ("dashboard_prompt_economy", cmd_ok!(crate::telemetry::dashboard_prompt_economy, "scope")),
    ("dashboard_economy_summary", cmd_ok!(crate::telemetry::dashboard_economy_summary, "scope")),
    ("dashboard_economy_savings_breakdown", cmd_ok!(crate::telemetry::dashboard_economy_savings_breakdown, "scope")),
    ("dashboard_economy_context_routing", cmd_ok!(crate::telemetry::dashboard_economy_context_routing, "scope")),
    ("dashboard_economy_per_spec_costs", cmd_ok!(crate::telemetry::dashboard_economy_per_spec_costs, "scope")),
    ("dashboard_economy_per_wave_costs", cmd_ok!(crate::telemetry::dashboard_economy_per_wave_costs, "scope")),
    ("dashboard_spec_trace", cmd_ok!(crate::telemetry::dashboard_spec_trace, "projectPath", "specName")),
    ("dashboard_session_trace", cmd_ok!(crate::telemetry::dashboard_session_trace, "projectPath", "sessionId")),
    ("dashboard_sessions", cmd_ok!(crate::telemetry::dashboard_sessions, "repoPath", "limit")),
    ("collector_health", cmd_ok!(crate::telemetry::collector_health, "repoPath")),
    // --- settings ---------------------------------------------------------
    ("set_language", cmd!(crate::commands::settings::set_language, "repoPath", "lang")),
    ("set_tone", cmd!(crate::commands::settings::set_tone, "repoPath", "tone")),
    ("read_settings", cmd!(crate::commands::settings::read_settings, "repoPath")),
    // --- project registry & discovery ------------------------------------
    ("discover_projects", discover_projects_handler),
    ("dashboard_discovery_root", discovery_root_handler),
    ("dashboard_projects_list", cmd!(crate::projects::list_registered)),
    ("dashboard_projects_add", cmd!(crate::projects::register, "path")),
    ("dashboard_projects_remove", cmd!(crate::projects::unregister, "path")),
    ("detect_project_mustard", cmd!(crate::projects::detect_project_mustard, "path")),
    ("uninstall_mustard", cmd!(crate::projects::uninstall_mustard, "path")),
    ("dashboard_project_overview", cmd!(crate::project_overview::dashboard_project_overview, "repoPath")),
    ("dashboard_deps_outdated", cmd!(crate::project_overview::dashboard_deps_outdated, "repoPath", "projectDir", "kind")),
    // --- maintenance ------------------------------------------------------
    ("artifact_update_check", cmd!(crate::artifact_update::artifact_update_check, "projectPath")),
    ("artifact_update_apply", cmd!(crate::artifact_update::artifact_update_apply, "projectPath")),
    ("is_mustard_repo", cmd_ok!(crate::artifact_update::is_mustard_repo, "projectPath")),
    ("doctor_status", cmd!(crate::doctor::doctor_status, "projectPath")),
    // --- git / files ------------------------------------------------------
    ("dashboard_git_info", cmd!(crate::git_info::dashboard_git_info, "repoPath")),
    ("dashboard_git_log", cmd!(crate::git_info::dashboard_git_log, "repoPath", "gitRef", "limit")),
    ("dashboard_read_file", cmd!(crate::file_read::dashboard_read_file, "repoPath", "relPath")),
    // --- live refresh -----------------------------------------------------
    ("dashboard_watch_repos", watch_repos_handler),
];

/// `discover_projects` without a `root` scans from where the server was
/// started. The native folder dialog died with Tauri, and the operator's
/// decision was that the machine running the backend is the machine whose
/// projects are shown — so the default is not a guess, it is the contract.
fn discover_projects_handler(ctx: &Arc<Ctx>, args: &Value) -> Result<Value, String> {
    let root: Option<String> = arg(args, "root")?;
    let root = root.map_or_else(|| ctx.root.clone(), PathBuf::from);
    encode(&crate::discovery::discover(&root)?)
}

/// Where [`discover_projects_handler`] scans by default, so the frontend can
/// show the operator which machine's tree it is looking at.
fn discovery_root_handler(ctx: &Arc<Ctx>, _args: &Value) -> Result<Value, String> {
    encode(&ctx.root.to_string_lossy())
}

/// Attach a filesystem watcher to each repo. What used to reach the frontend
/// through `AppHandle::emit` now leaves through [`EventBus`], so this is the
/// one entry that needs the context.
fn watch_repos_handler(ctx: &Arc<Ctx>, args: &Value) -> Result<Value, String> {
    let repo_paths: Vec<String> = arg(args, "repoPaths")?;
    for path in repo_paths {
        if let Err(e) = watcher::ensure_watching(
            Arc::clone(&ctx.watchers),
            path.clone(),
            ctx.bus.clone(),
        ) {
            eprintln!("dashboard_watch_repos: failed for {path}: {e}");
        }
    }
    Ok(Value::Null)
}

#[cfg(test)]
mod tests {
    use super::*;
    // `Write` is already in scope from the parent module.
    use std::io::Read as _;
    use std::net::TcpListener;
    use std::net::TcpStream;

    /// Bring up a server on an ephemeral port in the background. Returns the
    /// port, the context (for the shutdown flag and the bus) and the join
    /// handle.
    fn spawn(root: PathBuf, dist: PathBuf) -> (u16, Arc<Ctx>, std::thread::JoinHandle<()>) {
        let (server, port) = bind("127.0.0.1", 0).unwrap();
        let mut ctx = Ctx::new(root);
        ctx.dist = dist;
        let ctx = Arc::new(ctx);
        let serving = Arc::clone(&ctx);
        let handle = std::thread::spawn(move || serve(server, serving, 2));
        (port, ctx, handle)
    }

    fn stop(ctx: &Arc<Ctx>, handle: std::thread::JoinHandle<()>) {
        ctx.shutdown.store(true, Ordering::SeqCst);
        handle.join().unwrap();
    }

    /// Minimal blocking HTTP request — avoids pulling in a client crate, the
    /// same shape the collector's tests use.
    fn http(port: u16, method: &str, path: &str, body: &str) -> (u16, String) {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        let req = format!(
            "{method} {path} HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(req.as_bytes()).unwrap();
        let mut raw = Vec::new();
        let _ = stream.read_to_end(&mut raw);
        let resp = String::from_utf8_lossy(&raw).into_owned();
        let status = resp
            .lines()
            .next()
            .and_then(|l| l.split_whitespace().nth(1))
            .and_then(|s| s.parse::<u16>().ok())
            .unwrap_or(0);
        let body = resp.split("\r\n\r\n").nth(1).unwrap_or("").to_string();
        (status, body)
    }

    #[test]
    fn command_answers_with_the_same_json_invoke_returned() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(".claude").join("spec")).unwrap();
        let (port, ctx, handle) = spawn(tmp.path().to_path_buf(), tmp.path().join("dist"));

        let body = format!(
            r#"{{"repoPath":{}}}"#,
            serde_json::to_string(&tmp.path().to_string_lossy()).unwrap()
        );
        let (status, out) = http(port, "POST", "/api/dashboard_specs", &body);
        assert_eq!(status, 200);
        assert!(out.trim_start().starts_with('['), "specs is a JSON array, got {out}");

        stop(&ctx, handle);
    }

    #[test]
    fn snake_case_key_is_accepted_too() {
        let tmp = tempfile::tempdir().unwrap();
        let (port, ctx, handle) = spawn(tmp.path().to_path_buf(), tmp.path().join("dist"));
        let body = format!(
            r#"{{"repo_path":{}}}"#,
            serde_json::to_string(&tmp.path().to_string_lossy()).unwrap()
        );
        let (status, _) = http(port, "POST", "/api/dashboard_specs", &body);
        assert_eq!(status, 200);
        stop(&ctx, handle);
    }

    #[test]
    fn unknown_command_is_404_and_a_bad_argument_is_400() {
        let tmp = tempfile::tempdir().unwrap();
        let (port, ctx, handle) = spawn(tmp.path().to_path_buf(), tmp.path().join("dist"));

        let (status, _) = http(port, "POST", "/api/nope", "{}");
        assert_eq!(status, 404);

        // `repoPath` is required; omitting it is the `invoke` rejection.
        let (status, out) = http(port, "POST", "/api/dashboard_specs", "{}");
        assert_eq!(status, 400);
        assert!(out.contains("repoPath"), "the error names the argument: {out}");

        // Malformed JSON is a 400 too — and the server is still alive.
        let (status, _) = http(port, "POST", "/api/dashboard_specs", "{not json");
        assert_eq!(status, 400);
        let (status, _) = http(port, "GET", "/api/commands", "");
        assert_eq!(status, 200);

        stop(&ctx, handle);
    }

    #[test]
    fn every_table_name_is_unique() {
        let mut seen: Vec<&str> = COMMANDS.iter().map(|(name, _)| *name).collect();
        let total = seen.len();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), total, "duplicate command name in the table");
    }

    #[test]
    fn assets_fall_back_to_index_html() {
        let tmp = tempfile::tempdir().unwrap();
        let dist = tmp.path().join("dist");
        std::fs::create_dir_all(dist.join("assets")).unwrap();
        std::fs::write(dist.join("index.html"), "<!doctype html><title>x</title>").unwrap();
        std::fs::write(dist.join("assets").join("app.js"), "export {};").unwrap();
        let (port, ctx, handle) = spawn(tmp.path().to_path_buf(), dist);

        let (status, body) = http(port, "GET", "/assets/app.js", "");
        assert_eq!(status, 200);
        assert!(body.contains("export"), "the real asset is served");

        // A client-side route has no file behind it — index.html answers.
        let (status, body) = http(port, "GET", "/specs/some-spec", "");
        assert_eq!(status, 200);
        assert!(body.contains("<!doctype html>"));

        stop(&ctx, handle);
    }

    #[test]
    fn traversal_out_of_dist_is_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let dist = tmp.path().join("dist");
        std::fs::create_dir_all(&dist).unwrap();
        std::fs::write(dist.join("index.html"), "<!doctype html>").unwrap();
        std::fs::write(tmp.path().join("secret.txt"), "SECRET").unwrap();

        // `..` never resolves — the request degrades to the SPA shell.
        assert!(safe_join(&dist, "/../secret.txt").is_none());

        let (port, ctx, handle) = spawn(tmp.path().to_path_buf(), dist);
        let (_, body) = http(port, "GET", "/../secret.txt", "");
        assert!(!body.contains("SECRET"), "must not escape the asset root");
        stop(&ctx, handle);
    }

    #[test]
    fn events_stream_opens_and_carries_an_emitted_frame() {
        let tmp = tempfile::tempdir().unwrap();
        let (port, ctx, handle) = spawn(tmp.path().to_path_buf(), tmp.path().join("dist"));

        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        stream
            .write_all(b"GET /api/events HTTP/1.1\r\nHost: localhost\r\nAccept: text/event-stream\r\n\r\n")
            .unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();

        // Wait for the subscription to register before emitting, so the frame
        // cannot race ahead of the reader.
        for _ in 0..200 {
            if ctx.bus.subscriber_count() > 0 {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(ctx.bus.subscriber_count(), 1);
        ctx.bus
            .emit("dashboard:fs-change", &serde_json::json!({ "kind": "events" }));

        let mut buf = [0u8; 1024];
        let mut seen = String::new();
        while !seen.contains("dashboard:fs-change") {
            let n = stream.read(&mut buf).unwrap();
            assert!(n > 0, "stream closed before the frame arrived");
            seen.push_str(&String::from_utf8_lossy(&buf[..n]));
        }
        assert!(seen.contains("text/event-stream"), "headers name the media type");
        assert!(seen.contains(r#"data: {"kind":"events"}"#), "payload survives: {seen}");

        drop(stream);
        stop(&ctx, handle);
    }

    #[test]
    fn bind_walks_past_a_taken_port() {
        let held = TcpListener::bind("127.0.0.1:0").unwrap();
        let taken = held.local_addr().unwrap().port();
        let (server, got) = bind("127.0.0.1", taken).unwrap();
        assert_ne!(got, taken, "a held port must not be reused");
        assert!(got > taken && got < taken + PORT_WALK);
        drop(server);
        drop(held);
    }

    #[test]
    fn snake_case_maps_the_wire_keys() {
        assert_eq!(snake_case("repoPath"), "repo_path");
        assert_eq!(snake_case("spec"), "spec");
        assert_eq!(snake_case("startedAt"), "started_at");
    }
}
