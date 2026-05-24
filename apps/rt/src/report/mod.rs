//! A shared, dependency-free HTML report generator for the `run` face.
//!
//! Several `run` subcommands (`qa-run`, `metrics`, `event-projections`,
//! `verify-pipeline`) accept `--format json|html`. JSON is the default — it is
//! what the pipeline consumes. HTML is an *additional* artifact: a single,
//! self-contained `.html` file with embedded CSS and no external dependencies,
//! meant for a human to open in a browser.
//!
//! Fail-open contract: rendering an HTML page must never crash a `run`
//! subcommand. The caller decides what to print; if it ever cannot build a
//! page it can still emit valid JSON instead. The functions here are pure —
//! they build a `String` and never touch the filesystem or exit the process.

use std::fmt::Write as _;

/// Embedded stylesheet — kept terse; the report is a utilitarian artifact, not
/// a marketing page. Dark-first to match the Mustard dashboard aesthetic.
const STYLE: &str = "\
:root{color-scheme:dark}\
*{box-sizing:border-box}\
body{margin:0;padding:2rem;font:14px/1.6 -apple-system,BlinkMacSystemFont,'Segoe UI',Inter,sans-serif;background:#0e0e11;color:#e4e4e7}\
h1{font-size:1.4rem;margin:0 0 .25rem}\
.meta{color:#8b8b94;font-size:.8rem;margin-bottom:1.5rem}\
.card{background:#18181b;border:1px solid #27272a;border-radius:8px;padding:1rem 1.25rem;margin-bottom:1rem}\
.card h2{font-size:.95rem;margin:0 0 .75rem;color:#a1a1aa}\
table{width:100%;border-collapse:collapse;font-size:.85rem}\
th,td{text-align:left;padding:.4rem .6rem;border-bottom:1px solid #27272a}\
th{color:#8b8b94;font-weight:600}\
tr:last-child td{border-bottom:none}\
.pass{color:#4ade80}.fail{color:#f87171}.skip{color:#fbbf24}\
.pill{display:inline-block;padding:.1rem .5rem;border-radius:999px;font-size:.75rem;font-weight:600}\
.pill.pass{background:#14321f;color:#4ade80}\
.pill.fail{background:#3a1a1a;color:#f87171}\
.pill.skip{background:#322a14;color:#fbbf24}\
pre{background:#0e0e11;border:1px solid #27272a;border-radius:6px;padding:.75rem;overflow:auto;font-size:.8rem;margin:0}\
.kv{display:flex;gap:.5rem;margin:.2rem 0}\
.kv .k{color:#8b8b94;min-width:9rem}\
";

/// HTML-escape a string for safe interpolation into element text / attributes.
#[must_use]
pub fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

/// A self-contained HTML page: a document builder that callers feed sections
/// into. The finished string carries its own `<style>` — no external assets.
pub struct Report {
    title: String,
    subtitle: String,
    body: String,
}

impl Report {
    /// Start a report page with a title and a subtitle (shown as `.meta`).
    #[must_use]
    pub fn new(title: impl Into<String>, subtitle: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            subtitle: subtitle.into(),
            body: String::new(),
        }
    }

    /// Append a `.card` section with a heading and pre-rendered inner HTML.
    pub fn section(&mut self, heading: &str, inner_html: &str) -> &mut Self {
        self.body.push_str("<div class=\"card\"><h2>");
        self.body.push_str(&escape(heading));
        self.body.push_str("</h2>");
        self.body.push_str(inner_html);
        self.body.push_str("</div>");
        self
    }

    /// Append a `.card` whose body is a `<pre>` block of escaped text — used
    /// to embed the raw JSON projection alongside the rendered view.
    pub fn pre_section(&mut self, heading: &str, text: &str) -> &mut Self {
        let inner = format!("<pre>{}</pre>", escape(text));
        self.section(heading, &inner)
    }

    /// Render the finished standalone HTML document.
    #[must_use]
    pub fn render(&self) -> String {
        format!(
            "<!doctype html>\n<html lang=\"en\"><head><meta charset=\"utf-8\">\
<meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\
<title>{title}</title><style>{style}</style></head><body>\
<h1>{title}</h1><div class=\"meta\">{subtitle}</div>{body}</body></html>\n",
            title = escape(&self.title),
            subtitle = escape(&self.subtitle),
            style = STYLE,
            body = self.body,
        )
    }
}

/// Build a `<table>` from a header row and string cells. Each row is rendered
/// verbatim as escaped text — callers that need status colouring should use
/// [`table_with_classes`] instead.
#[must_use]
pub fn table(headers: &[&str], rows: &[Vec<String>]) -> String {
    let mut html = String::from("<table><thead><tr>");
    for h in headers {
        let _ = write!(html, "<th>{}</th>", escape(h));
    }
    html.push_str("</tr></thead><tbody>");
    for row in rows {
        html.push_str("<tr>");
        for cell in row {
            let _ = write!(html, "<td>{}</td>", escape(cell));
        }
        html.push_str("</tr>");
    }
    html.push_str("</tbody></table>");
    html
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_neutralizes_markup() {
        assert_eq!(escape("<b>&\"'"), "&lt;b&gt;&amp;&quot;&#39;");
    }

    #[test]
    fn report_renders_standalone_document() {
        let mut r = Report::new("QA", "spec: demo");
        r.pre_section("Raw", "{\"ok\":true}");
        let html = r.render();
        assert!(html.starts_with("<!doctype html>"));
        assert!(html.contains("<style>"));
        // No external resource references — fully self-contained.
        assert!(!html.contains("http://") && !html.contains("https://"));
        assert!(!html.contains("src=") && !html.contains("href="));
        assert!(html.contains("spec: demo"));
        assert!(html.ends_with("</html>\n"));
    }

    #[test]
    fn table_builds_rows() {
        let html = table(&["A", "B"], &[vec!["1".into(), "2".into()]]);
        assert!(html.contains("<th>A</th>"));
        assert!(html.contains("<td>1</td>"));
    }
}
