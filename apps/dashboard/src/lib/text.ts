/**
 * Middle-truncation: keep the first `start` chars, then …, then the last `end`
 * chars of the string. Preserves both the file name and enough path context.
 *
 * AC-13: never truncate at the start — always show the beginning of the path.
 */
export function midTruncate(s: string, start = 14, end = 10): string {
  if (s.length <= start + end + 1) return s;
  return `${s.slice(0, start)}…${s.slice(-end)}`;
}

const PRD_OPEN_MARKER = "<!-- PRD -->";
const PRD_CLOSE_MARKER = "<!-- PLAN -->";

/**
 * Extract the PRD layer of a `spec.md` — the text between the `<!-- PRD -->`
 * and `<!-- PLAN -->` HTML comment markers. Deterministic, no AI.
 *
 * Fail-open by design: a spec without both markers (older specs, or any draft
 * that never materialised the PRD layer) returns an empty string rather than
 * throwing. Callers render an empty state when the result is blank, matching
 * the dashboard's fault-tolerant contract (commands return empty, never throw).
 */
export function slicePrdSection(md: string): string {
  const open = md.indexOf(PRD_OPEN_MARKER);
  if (open === -1) return "";
  const start = open + PRD_OPEN_MARKER.length;
  const close = md.indexOf(PRD_CLOSE_MARKER, start);
  if (close === -1) return "";
  return md.slice(start, close).trim();
}
