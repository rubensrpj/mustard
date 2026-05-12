/**
 * Anthropic public model pricing snapshot. Keep in sync with
 * https://www.anthropic.com/pricing — values are $/MTok (per million tokens).
 *
 * Last reviewed: 2026-05. When prices change, bump this file; consumers
 * recompute cost_usd on the next span emit (no migration needed for
 * historical spans — they keep the cost computed at emission time).
 */

export interface ModelPrice {
  /** USD per 1M input tokens. */
  input: number;
  /** USD per 1M output tokens. */
  output: number;
}

export const PRICING: Record<string, ModelPrice> = {
  'claude-opus-4-7': { input: 15, output: 75 },
  'claude-opus-4-6': { input: 15, output: 75 },
  'claude-sonnet-4-6': { input: 3, output: 15 },
  'claude-sonnet-4-5': { input: 3, output: 15 },
  'claude-haiku-4-5': { input: 1, output: 5 },
};

/**
 * Compute USD cost for a token usage tuple. Returns 0 for unknown models —
 * callers should treat 0 as "unpriced" rather than "free".
 */
export function costUsd(
  model: string,
  inputTokens: number,
  outputTokens: number
): number {
  const p = PRICING[model];
  if (!p) return 0;
  return (inputTokens * p.input + outputTokens * p.output) / 1_000_000;
}
