/**
 * Parser for qa.result event summaries.
 *
 * The Mustard runtime emits qa.result events with a structured payload
 * `{ spec, overall: "pass" | "fail" | "skip", criteria: [...] }`, but the
 * Rust backend currently surfaces only a flattened `summary` string to the
 * dashboard (`RecentEvent.summary`). Until the backend exposes the raw
 * payload, parse the overall verdict from the summary text.
 *
 * Recognised patterns (case-insensitive, first match wins):
 *   - `overall: pass` / `overall=pass`
 *   - bare word "fail" or "skip" before "pass" (prevents false positives
 *     like "AC pass=2 fail=1" being read as plain pass)
 *   - bare word "pass" with word boundaries
 *
 * Returns `null` when the summary is missing or none of the patterns match.
 * Callers should treat `null` as "ignore" rather than fall back to a default
 * verdict — falling back to a default falsifies the dashboard counts.
 */
export type QaOverall = "pass" | "fail" | "skip";

export function parseQaOverall(summary: string | null | undefined): QaOverall | null {
  if (!summary) return null;
  const s = summary.toLowerCase();

  // Structured pattern wins when present.
  const explicit = s.match(/overall\s*[:=]\s*(pass|fail|skip)\b/);
  if (explicit) return explicit[1] as QaOverall;

  // Failure dominates: any standalone "fail" => fail.
  if (/\bfail(?:ed)?\b/.test(s)) return "fail";

  // Skip is rarer; check before pass to avoid "passed, skipped" → pass.
  if (/\bskip(?:ped)?\b/.test(s)) return "skip";

  if (/\bpass(?:ed)?\b/.test(s)) return "pass";

  return null;
}
