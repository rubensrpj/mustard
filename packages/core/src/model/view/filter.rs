//! Filter and window types accepted by the reader trait.
//!
//! Kept tiny on purpose — these are arguments, not ViewModels. They serialize
//! so a Tauri command can accept them straight from the React frontend without
//! a hand-rolled converter.

use serde::{Deserialize, Serialize};

/// Time window for queries that span "recent" data. Designed to map 1:1 to
/// the SQL window expressions used by the SQLite reader.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TimeWindow {
    /// Calendar today (UTC).
    Today,
    /// Last 7 days.
    SevenDays,
    /// Last 30 days.
    ThirtyDays,
    /// All-time, no filter.
    All,
}

impl Default for TimeWindow {
    fn default() -> Self {
        Self::Today
    }
}

impl TimeWindow {
    /// SQL fragment that selects this window on the `events` table. Used by
    /// the SQLite reader. Returns the empty string for `All`.
    ///
    /// The boundary expressions match `datetime('now', '-X')` so they work on
    /// ISO-8601 timestamps stored as TEXT.
    #[must_use]
    pub fn sql_filter(self) -> &'static str {
        match self {
            Self::Today => "ts >= datetime('now', 'start of day')",
            Self::SevenDays => "ts >= datetime('now', '-7 days')",
            Self::ThirtyDays => "ts >= datetime('now', '-30 days')",
            Self::All => "1=1",
        }
    }
}

/// Filter on a list of specs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SpecStatusFilter {
    /// Specs whose status is `is_active()`.
    Active,
    /// Specs whose status is `is_terminal()`.
    Closed,
    /// No filter.
    Any,
}

impl Default for SpecStatusFilter {
    fn default() -> Self {
        Self::Any
    }
}

/// Composite filter for `SpecReader::list_specs`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct SpecFilter {
    /// Optional status restriction. `Any` returns everything.
    pub status: Option<SpecStatusFilter>,
    /// Time window — specs whose most recent event falls outside the window
    /// are excluded.
    pub window: TimeWindow,
    /// Free-text substring search on the spec name. None disables search.
    pub search: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn time_window_default_is_today() {
        assert_eq!(TimeWindow::default(), TimeWindow::Today);
    }

    #[test]
    fn time_window_sql_fragments_are_distinct() {
        assert_ne!(TimeWindow::Today.sql_filter(), TimeWindow::All.sql_filter());
        assert!(TimeWindow::All.sql_filter().contains("1=1"));
        assert!(TimeWindow::SevenDays.sql_filter().contains("-7 days"));
        assert!(TimeWindow::ThirtyDays.sql_filter().contains("-30 days"));
    }

    #[test]
    fn default_spec_filter_is_unrestricted() {
        let f = SpecFilter::default();
        assert!(f.status.is_none());
        assert_eq!(f.window, TimeWindow::Today);
        assert!(f.search.is_none());
    }
}
