/**
 * OpenTelemetry GenAI semantic convention attribute names + Mustard custom
 * attribute names. Hard-coded constants — these strings are stable identifiers
 * in the OTel spec and not worth pulling a runtime dependency for.
 *
 * Sources:
 * - GenAI registry: https://opentelemetry.io/docs/specs/semconv/registry/attributes/gen-ai/
 * - GenAI client spans: https://opentelemetry.io/docs/specs/semconv/gen-ai/gen-ai-spans/
 *
 * Stability note (verified 2026-05): the GenAI conventions are still tracked
 * as "Development" status in the OTel spec (v1.37). The attribute *names*
 * below have been stable across releases and are accepted by every backend
 * we target (Honeycomb, Datadog, Tempo) — but if a future spec version
 * renames any of these, bump this file and the test in lock-step.
 */

// GenAI semantic conventions — attribute keys.
export const GEN_AI = {
  /** Provider name (e.g. "anthropic", "openai"). */
  SYSTEM: 'gen_ai.system',
  /** Exact model id requested (e.g. "claude-opus-4-7"). */
  REQUEST_MODEL: 'gen_ai.request.model',
  /** Input tokens (includes cached). */
  USAGE_INPUT_TOKENS: 'gen_ai.usage.input_tokens',
  /** Output tokens. */
  USAGE_OUTPUT_TOKENS: 'gen_ai.usage.output_tokens',
  /** Operation name (e.g. "chat", "task.dispatch"). */
  OPERATION_NAME: 'gen_ai.operation.name',
  /** Provider response id, if known. */
  RESPONSE_ID: 'gen_ai.response.id',
  /** Error class on failed call. */
  ERROR_TYPE: 'error.type',
} as const;

// Mustard-specific attribute keys (custom namespace).
export const MUSTARD = {
  PHASE: 'mustard.phase',
  WAVE: 'mustard.wave',
  SPEC: 'mustard.spec',
  AGENT_TYPE: 'mustard.agent_type',
  /** Derived from pricing.ts; doubleValue. */
  COST_USD: 'mustard.cost_usd',
} as const;
