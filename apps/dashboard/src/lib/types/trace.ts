// Wave 6 — Trace viewer DTOs. Mirrors the serde shape of
// `apps/dashboard/src-tauri/src/telemetry.rs::TraceNode` (`#[serde(rename_all
// = "snake_case")]`). Keep these aligned; a field rename on the Rust side
// must land here in the same commit.

export type TraceKind = "spec" | "wave" | "agent" | "tool";

export interface TokenBreakdown {
  input: number;
  output: number;
  cache_read: number;
  cache_creation: number;
  /** Cost in micro-USD (USD × 10⁶). `null` when no spans observed. */
  cost_usd_micros: number | null;
}

export interface TraceNode {
  kind: TraceKind;
  label: string;
  tokens: TokenBreakdown | null;
  duration_ms: number | null;
  ts: string | null;
  /**
   * Only populated for `kind === "tool"`. Carries the original
   * `tool.use` payload verbatim — typical fields: `tool_name`,
   * `tool_input`, `tool_response`, `file_path`, `command`, `before`,
   * `after`, `content`, `stdout`.
   */
  payload: Record<string, unknown> | null;
  children: TraceNode[];
}
