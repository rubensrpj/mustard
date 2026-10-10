//! Shared semantic colors for native Mods and generated browser pages.
//! The data module is JSON with the ES-module wrapper required by Mods.
use serde_json::Value;
use std::fmt::Write as _;
use std::sync::OnceLock;

const THEME: &str = include_str!("../../../../plugin/ui/theme.js");
const LAYOUT: &str = include_str!("layout.css");

fn palettes() -> Value {
    serde_json::from_str(
        THEME
            .trim()
            .strip_prefix("export default ")
            .unwrap_or_default()
            .trim_end_matches(';'),
    )
    .unwrap_or_default()
}

pub(super) fn style() -> &'static str {
    static CSS: OnceLock<String> = OnceLock::new();
    CSS.get_or_init(|| {
        let design = palettes();
        let mut css = String::from(":root{");
        for (token, key) in [
            ("paper", "paper"),
            ("surface", "surface"),
            ("sidebar", "sidebar"),
            ("ink", "ink"),
            ("muted", "muted"),
            ("rule", "line"),
            ("rule-soft", "inset"),
            ("mustard", "accent"),
            ("mustard-text", "accentText"),
            ("mustard-soft", "accentSoft"),
            ("mustard-line", "accent"),
            ("code", "inset"),
            ("hl", "accentSoft"),
            ("ok", "positive"),
            ("ok-soft", "positiveSoft"),
            ("no", "warning"),
            ("no-soft", "warningSoft"),
            ("charcoal", "sidebar"),
            ("on-charcoal", "ink"),
        ] {
            let _ = write!(
                css,
                "--{token}:{};",
                design["dark"][key].as_str().unwrap_or("inherit")
            );
        }
        css.push_str("color-scheme:dark;}");
        css.push_str(LAYOUT);
        css
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn browser_and_mods_share_readable_semantic_colors_and_yellow_selection() {
        let design = palettes();
        for key in [
            "paper",
            "surface",
            "ink",
            "muted",
            "line",
            "accent",
            "accentText",
            "accentSoft",
            "positive",
            "warning",
        ] {
            let color = design["dark"][key].as_str().expect("a valid shared token");
            assert!(color.starts_with('#') && color.len() == 7, "dark.{key}");
            assert!(style().contains(color));
        }
        assert_eq!(design["font"], "Inter");
        assert_eq!(design["dark"]["accent"], "#e1ad01");
        assert!(design.get("light").is_none());
        assert!(style().contains("color-scheme:dark"));
        assert!(!style().contains("prefers-color-scheme"));
        assert!(!style().strip_suffix(LAYOUT).unwrap().contains(":inherit;"));
    }
}
