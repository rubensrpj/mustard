//! `mustard-dashboard` — serve the Mustard dashboard over HTTP.
//!
//! Replaces the desktop shell this dashboard used to ship as. On Linux that
//! shell drew through WebKitGTK, the weakest of its three webview engines and
//! the source of the sluggishness that motivated this change; it also panicked from inside
//! `gtk`/`tao` when there was no graphical session, printing library paths
//! instead of anything actionable. This binary prints where the dashboard is
//! listening and keeps serving.
//!
//! ```text
//! mustard-dashboard [--port N] [--host ADDR] [--root DIR] [--no-open]
//! ```

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use mustard_dashboard::server::{self, Ctx, DEFAULT_HOST};

/// What the argv parse produced.
struct Args {
    host: String,
    port: Option<u16>,
    root: Option<PathBuf>,
    open: bool,
    help: bool,
}

const USAGE: &str = "\
mustard-dashboard — serve the Mustard dashboard over HTTP

USAGE:
    mustard-dashboard [OPTIONS]

OPTIONS:
    --port <N>     Listen port. Falls back to $MUSTARD_DASHBOARD_PORT, then 7777.
                   A taken port is not fatal: the next free one is used and printed.
    --host <ADDR>  Listen address. Defaults to 127.0.0.1. Naming another address
                   (e.g. 0.0.0.0) is what exposes the dashboard to the network —
                   it never happens on its own, because the dashboard reads the
                   .claude/ of every project on this machine.
    --root <DIR>   Root the project scan walks. Defaults to the current directory:
                   the projects shown are the ones on the machine running this.
    --no-open      Never launch a browser, even with a graphical session.
    -h, --help     Print this help.
";

fn main() -> ExitCode {
    let args = match parse(std::env::args().skip(1)) {
        Ok(args) => args,
        Err(message) => {
            eprintln!("mustard-dashboard: {message}\n\n{USAGE}");
            return ExitCode::FAILURE;
        }
    };
    if args.help {
        println!("{USAGE}");
        return ExitCode::SUCCESS;
    }

    let root = match args.root {
        Some(root) => root,
        None => match std::env::current_dir() {
            Ok(cwd) => cwd,
            Err(e) => {
                eprintln!("mustard-dashboard: cannot read the current directory: {e}");
                return ExitCode::FAILURE;
            }
        },
    };

    let requested = server::resolve_port(args.port);
    let (http, port) = match server::bind(&args.host, requested) {
        Ok(bound) => bound,
        Err(e) => {
            eprintln!("mustard-dashboard: {e}");
            return ExitCode::FAILURE;
        }
    };
    if port != requested {
        println!("mustard-dashboard: port {requested} was taken, using {port}");
    }

    // A wildcard bind is not an address anyone can paste. `--host 0.0.0.0` is
    // the path the tutorials tell operators to use for reaching the panel from
    // another machine, so printing `http://0.0.0.0:7777/` hands them a URL that
    // cannot work and says nothing about what would (found in review).
    let wildcard = args.host == "0.0.0.0" || args.host == "::";
    let url = if wildcard {
        format!("http://127.0.0.1:{port}/")
    } else {
        format!("http://{}:{port}/", args.host)
    };
    println!("mustard-dashboard: serving {} at {url}", root.display());
    if wildcard {
        println!(
            "mustard-dashboard: also reachable on every interface at port {port} — \
             from another machine use http://<this machine's address>:{port}/"
        );
    }

    // `bound_to` carries the address the guard compares incoming `Host` headers
    // against — the port that `bind` actually got, never the one requested.
    let ctx = Arc::new(Ctx::new(root).bound_to(&args.host, port));
    if args.open && has_graphical_session() {
        open_browser(&url);
    } else if args.open {
        // The old desktop shell panicked here. Saying where the dashboard is, is the
        // whole of what a headless host needs.
        println!("mustard-dashboard: no graphical session — open {url} yourself");
    }

    server::serve(http, ctx, server::default_workers());
    ExitCode::SUCCESS
}

/// Parse the four flags. Hand-rolled rather than pulling `clap` in: the whole
/// surface is four options, and this binary should stay a thin front door to
/// [`server::serve`].
fn parse(argv: impl Iterator<Item = String>) -> Result<Args, String> {
    let mut args = Args {
        host: DEFAULT_HOST.to_string(),
        port: None,
        root: None,
        open: true,
        help: false,
    };
    let mut argv = argv.peekable();
    while let Some(flag) = argv.next() {
        match flag.as_str() {
            "-h" | "--help" => args.help = true,
            "--no-open" => args.open = false,
            "--port" => {
                let raw = argv.next().ok_or("--port needs a value")?;
                args.port = Some(
                    raw.parse::<u16>()
                        .map_err(|_| format!("--port expects a number in 0..65535, got {raw:?}"))?,
                );
            }
            "--host" => args.host = argv.next().ok_or("--host needs a value")?,
            "--root" => args.root = Some(PathBuf::from(argv.next().ok_or("--root needs a value")?)),
            other => return Err(format!("unknown option {other:?}")),
        }
    }
    Ok(args)
}

/// Whether a browser can be launched at all.
///
/// On Linux the answer is `DISPLAY` or `WAYLAND_DISPLAY` being set and
/// non-empty — over SSH or in a container both are empty, and that is exactly
/// the case the old desktop shell used to die in. Other platforms have no equivalent
/// convention: a desktop session is assumed, and a failing launcher is
/// harmless because the URL was already printed.
fn has_graphical_session() -> bool {
    let named = ["DISPLAY", "WAYLAND_DISPLAY"]
        .iter()
        .any(|key| std::env::var(key).is_ok_and(|v| !v.trim().is_empty()));
    if named {
        return true;
    }
    !cfg!(target_os = "linux")
}

/// Best-effort browser launch. A missing launcher is not an error — the URL is
/// on stdout either way.
fn open_browser(url: &str) {
    #[cfg(target_os = "macos")]
    let mut command = mustard_dashboard::process_util::no_window_command("open");
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut c = mustard_dashboard::process_util::no_window_command("cmd");
        c.args(["/C", "start", ""]);
        c
    };
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let mut command = mustard_dashboard::process_util::no_window_command("xdg-open");

    if command.arg(url).spawn().is_err() {
        println!("mustard-dashboard: could not launch a browser — open {url} yourself");
    }
}

#[cfg(test)]
mod tests {
    use super::{DEFAULT_HOST, parse};
    use mustard_dashboard::server::{DEFAULT_PORT, PORT_ENV};

    fn args(raw: &[&str]) -> Vec<String> {
        raw.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn defaults_are_loopback_and_open() {
        let parsed = parse(args(&[]).into_iter()).unwrap();
        assert_eq!(parsed.host, DEFAULT_HOST);
        assert!(parsed.port.is_none());
        assert!(parsed.root.is_none());
        assert!(parsed.open, "a browser opens unless --no-open says otherwise");
    }

    #[test]
    fn every_flag_parses() {
        let parsed = parse(
            args(&["--port", "9001", "--host", "0.0.0.0", "--root", "/tmp/x", "--no-open"])
                .into_iter(),
        )
        .unwrap();
        assert_eq!(parsed.port, Some(9001));
        assert_eq!(parsed.host, "0.0.0.0");
        assert_eq!(parsed.root.as_deref(), Some(std::path::Path::new("/tmp/x")));
        assert!(!parsed.open);
    }

    #[test]
    fn a_bad_flag_or_a_missing_value_is_rejected() {
        assert!(parse(args(&["--nope"]).into_iter()).is_err());
        assert!(parse(args(&["--port"]).into_iter()).is_err());
        assert!(parse(args(&["--port", "http"]).into_iter()).is_err());
    }

    /// The env var is the fallback, not an override: an explicit `--port`
    /// wins, matching the collector's `MUSTARD_OTEL_PORT` contract.
    #[test]
    fn explicit_port_beats_the_env_var() {
        assert_eq!(super::server::resolve_port(Some(9001)), 9001);
        // Nothing set anywhere → the documented default.
        if std::env::var(PORT_ENV).is_err() {
            assert_eq!(super::server::resolve_port(None), DEFAULT_PORT);
        }
    }
}
