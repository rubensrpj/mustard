// Wave 6 — Trace viewer DTOs. Mirrors the serde shape of
// `apps/dashboard/src-tauri/src/telemetry.rs::TraceNode` (`#[serde(rename_all
// = "snake_case")]`). Keep these aligned; a field rename on the Rust side
// must land here in the same commit.

export type TraceKind =
  | "spec"
  | "wave"
  | "agent"
  | "tool"
  | "session"
  | "prompt";

export interface TokenBreakdown {
  input: number;
  output: number;
  cache_read: number;
  cache_creation: number;
  /** Cost in micro-USD (USD × 10⁶). `null` when no spans observed. */
  cost_usd_micros: number | null;
}

/**
 * Real shape emitted by the `tool.use` hook (post-followup-2 fix
 * `2026-05-21-economia-followup-2-trace-rich`). The `target` field is the
 * structured surface the hook actually writes — `command` for Bash,
 * `file_path` for Edit/Write/MultiEdit/Read, `description` as a human
 * fallback. The optional `result` is the paired `tool.result` payload
 * spliced in by `pair_tool_results` on the Rust side; it carries the
 * captured side-effects (stdout, stderr, file diff content) for the
 * variants that the post-tool hook knows how to capture.
 */
export interface ToolUseTarget {
  command?: string;
  file_path?: string;
  /** Legacy alias for `file_path` kept by some hook versions. */
  file?: string;
  description?: string;
}

export interface ToolResultPayload {
  /** Echoed from the `tool.use` so the renderer can confirm the pairing. */
  tool_use_id?: string;
  tool?: string;
  file_path?: string;
  stdout_excerpt?: string;
  stderr_excerpt?: string;
  exit_code?: number;
  /** Snapshot of the file BEFORE an Edit/Write/MultiEdit applied. */
  file_before?: string;
  /** Snapshot of the file AFTER an Edit/Write/MultiEdit applied. */
  file_after?: string;
  /** Truncated body of a Read result. */
  content_excerpt?: string;
}

export interface ToolUsePayload {
  tool?: string;
  target?: ToolUseTarget;
  phase?: string | null;
  tool_use_id?: string;
  /** Spliced in by `telemetry.rs::pair_tool_results` when a `tool.result`
   *  event was paired with this `tool.use`. */
  result?: ToolResultPayload;
  /** The assistant narration that MOTIVATED this tool — the `text` block that
   *  preceded the `tool_use` in the session transcript. Spliced onto the node
   *  by `telemetry.rs::build_trace_tree` (matched by `tool_use_id`) for session
   *  traces; absent for most tools (no narration) and for spec traces. The same
   *  field is also spliced onto `kind:"agent"` nodes (keyed by the dispatch's
   *  spawn `tool_use_id`) so the spawning "why" rides under the agent label. */
  motivation?: string;
}

export interface TraceNode {
  kind: TraceKind;
  label: string;
  /**
   * Only populated for `kind === "agent"`. The dispatch's subagent type
   * (`Explore` / `general-purpose` / `mustard-review` / …), rendered as a small
   * muted badge beside the agent label. `null`/absent for the orchestrator and
   * every non-agent node.
   */
  subagent_type?: string | null;
  tokens: TokenBreakdown | null;
  duration_ms: number | null;
  ts: string | null;
  /**
   * Only populated for `kind === "tool"`. Carries the original
   * `tool.use` payload verbatim — see `ToolUsePayload` for the real
   * shape and the optional `result` field added by the Rust pairing.
   * Typed as a loose record so legacy events (with extra/missing
   * fields) still deserialize without breaking the tree.
   */
  payload: Record<string, unknown> | null;
  children: TraceNode[];
}
