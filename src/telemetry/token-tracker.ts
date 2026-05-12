/**
 * TokenTracker — Mustard 2.0 Phase 2 manual OTLP/JSON span emitter.
 *
 * Why manual emit (no `@opentelemetry/sdk-node`):
 *   - Each hook runs in a fresh Node/Bun process. AsyncLocalStorage cannot
 *     bridge PreToolUse and PostToolUse spawns — they are separate processes.
 *   - SDK cold-start (20–50 ms) × 1000+ tool uses/session is a 20–50 s
 *     accumulated tax we refuse to pay.
 *   - Hooks must have zero npm deps. The SDK would force bundling.
 *
 * Bridging Pre/Post hooks: we persist a small JSON sidecar per active span at
 *   .claude/.harness/.active-spans/{toolUseId}.json
 * `startSpan` writes the sidecar; `endSpan` reads it, computes duration,
 * emits the OTLP JSON line, and deletes the sidecar. If `endSpan` cannot
 * find the sidecar (crash, kill -9, race), it is a no-op (we cannot fake
 * a duration) and an orphan warning goes to stderr.
 *
 * Emit format: one full `resourceSpans` wrapper per JSONL line. This is the
 * shape the OTel filelog receiver expects, so the file is directly
 * collector-pluggable without preprocessing.
 *
 * Fail-open contract: every public method swallows IO/JSON errors and logs
 * to stderr. A hook bug must never block a tool dispatch.
 */

import { randomBytes } from 'node:crypto';
import * as fs from 'node:fs';
import * as path from 'node:path';
import { GEN_AI, MUSTARD } from './otel-conventions.js';
import { costUsd } from './pricing.js';

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

export interface SpanContext {
  /** 16-char lowercase hex (8 bytes per OTel spec). */
  spanId: string;
  /** 32-char lowercase hex (16 bytes per OTel spec). */
  traceId: string;
  /** 16-char lowercase hex, optional. */
  parentSpanId?: string;
}

export interface StartSpanInput {
  /** Span display name, e.g. "task.dispatch". */
  name: string;
  /** Claude Code tool_use_id — links PreToolUse / PostToolUse hooks. */
  toolUseId: string;
  /** e.g. "claude-opus-4-7". */
  model: string;
  /** e.g. "general-purpose", "Explore", "Plan". */
  agentType: string;
  spec?: string;
  phase?: string;
  wave?: number;
  /** Byte length of the prompt sent to the Task tool. */
  promptBytes: number;
  /** Optional pre-computed context to override the auto-generated one. */
  parentSpanId?: string;
}

export interface EndSpanInput {
  toolUseId: string;
  /** Byte length of the tool's response payload (text body). */
  responseBytes: number;
  isError?: boolean;
  errorType?: string;
}

interface ActiveSpanRecord {
  ctx: SpanContext;
  name: string;
  model: string;
  agentType: string;
  spec?: string;
  phase?: string;
  wave?: number;
  promptBytes: number;
  startedMs: number;
}

// OTLP attribute literal shapes (proto3 JSON mapping).
type AttrStringValue = { stringValue: string };
type AttrIntValue = { intValue: string }; // OTLP encodes int64 as decimal string in JSON.
type AttrDoubleValue = { doubleValue: number };
type AttrBoolValue = { boolValue: boolean };
type AnyAttrValue =
  | AttrStringValue
  | AttrIntValue
  | AttrDoubleValue
  | AttrBoolValue;

interface OtlpKeyValue {
  key: string;
  value: AnyAttrValue;
}

// ---------------------------------------------------------------------------
// Token estimator
// ---------------------------------------------------------------------------

/**
 * Heuristic Claude token estimator: ~4 bytes per token on English text.
 * Documented in the spec as ground truth; replace with exact usage when
 * Claude Code exposes it.
 */
function estimateTokens(bytes: number): number {
  if (!Number.isFinite(bytes) || bytes <= 0) return 0;
  return Math.ceil(bytes / 4);
}

// ---------------------------------------------------------------------------
// Hex id helpers
// ---------------------------------------------------------------------------

function newTraceId(): string {
  // 16 random bytes → 32 hex chars (OTel TraceId).
  return randomBytes(16).toString('hex');
}

function newSpanId(): string {
  // 8 random bytes → 16 hex chars (OTel SpanId).
  return randomBytes(8).toString('hex');
}

// ---------------------------------------------------------------------------
// Attribute builders
// ---------------------------------------------------------------------------

function attrString(key: string, value: string): OtlpKeyValue {
  return { key, value: { stringValue: value } };
}

function attrInt(key: string, value: number): OtlpKeyValue {
  // OTLP proto3 JSON encodes int64 as string.
  return { key, value: { intValue: String(Math.trunc(value)) } };
}

function attrDouble(key: string, value: number): OtlpKeyValue {
  return { key, value: { doubleValue: value } };
}

// ---------------------------------------------------------------------------
// TokenTracker
// ---------------------------------------------------------------------------

export class TokenTracker {
  private readonly spansJsonlPath: string;
  private readonly activeSpansDir: string;

  /**
   * @param spansJsonlPath absolute path to spans.jsonl. The active-spans
   *   sidecar directory is derived as `{dir}/.active-spans` next to it.
   */
  constructor(spansJsonlPath: string) {
    this.spansJsonlPath = spansJsonlPath;
    this.activeSpansDir = path.join(
      path.dirname(spansJsonlPath),
      '.active-spans'
    );
  }

  /**
   * Start a span. Persists a sidecar keyed by `toolUseId` so the matching
   * PostToolUse hook (different process) can complete it. Returns the
   * generated SpanContext for callers that want to log it.
   */
  startSpan(input: StartSpanInput): SpanContext {
    const ctx: SpanContext = {
      traceId: newTraceId(),
      spanId: newSpanId(),
    };
    if (input.parentSpanId) ctx.parentSpanId = input.parentSpanId;

    const rec: ActiveSpanRecord = {
      ctx,
      name: input.name,
      model: input.model,
      agentType: input.agentType,
      promptBytes: Math.max(0, input.promptBytes | 0),
      startedMs: Date.now(),
    };
    if (input.spec !== undefined) rec.spec = input.spec;
    if (input.phase !== undefined) rec.phase = input.phase;
    if (input.wave !== undefined) rec.wave = input.wave;

    try {
      fs.mkdirSync(this.activeSpansDir, { recursive: true });
      fs.writeFileSync(
        this.sidecarPath(input.toolUseId),
        JSON.stringify(rec),
        'utf8'
      );
    } catch (err) {
      process.stderr.write(
        `[token-tracker] startSpan sidecar write failed: ${String(err)}\n`
      );
    }
    return ctx;
  }

  /**
   * End a span. Reads the sidecar, computes duration, emits OTLP JSON to
   * spans.jsonl, then deletes the sidecar. No-op (with warning) when the
   * sidecar is missing.
   */
  endSpan(input: EndSpanInput): void {
    const sidecar = this.sidecarPath(input.toolUseId);
    let active: ActiveSpanRecord | null = null;
    try {
      const raw = fs.readFileSync(sidecar, 'utf8');
      active = JSON.parse(raw) as ActiveSpanRecord;
    } catch (err) {
      process.stderr.write(
        `[token-tracker] endSpan: no active sidecar for tool_use_id=${input.toolUseId} (${String(err)})\n`
      );
      return;
    }

    const endedMs = Date.now();
    const responseBytes = Math.max(0, input.responseBytes | 0);
    const inputTokens = estimateTokens(active.promptBytes);
    const outputTokens = estimateTokens(responseBytes);

    try {
      const wrapper = this.buildOTLPSpan(
        active,
        endedMs,
        inputTokens,
        outputTokens,
        input.isError === true,
        input.errorType
      );
      fs.mkdirSync(path.dirname(this.spansJsonlPath), { recursive: true });
      fs.appendFileSync(
        this.spansJsonlPath,
        JSON.stringify(wrapper) + '\n',
        'utf8'
      );
    } catch (err) {
      process.stderr.write(
        `[token-tracker] endSpan emit failed: ${String(err)}\n`
      );
    } finally {
      try {
        fs.unlinkSync(sidecar);
      } catch {
        // already gone — fine.
      }
    }
  }

  /**
   * Build a one-line OTLP/JSON `resourceSpans` wrapper containing this
   * single span. Shape conforms to proto3 JSON mapping of OTLP v1
   * (https://opentelemetry.io/docs/specs/otlp/).
   */
  private buildOTLPSpan(
    active: ActiveSpanRecord,
    endedMs: number,
    inputTokens: number,
    outputTokens: number,
    isError: boolean,
    errorType?: string
  ): Record<string, unknown> {
    const startNs = String(BigInt(active.startedMs) * 1_000_000n);
    const endNs = String(BigInt(endedMs) * 1_000_000n);
    const cost = costUsd(active.model, inputTokens, outputTokens);

    const attributes: OtlpKeyValue[] = [
      attrString(GEN_AI.SYSTEM, 'anthropic'),
      attrString(GEN_AI.REQUEST_MODEL, active.model),
      attrInt(GEN_AI.USAGE_INPUT_TOKENS, inputTokens),
      attrInt(GEN_AI.USAGE_OUTPUT_TOKENS, outputTokens),
      attrString(GEN_AI.OPERATION_NAME, active.name),
      attrString(MUSTARD.AGENT_TYPE, active.agentType),
      attrDouble(MUSTARD.COST_USD, cost),
    ];
    if (active.spec) attributes.push(attrString(MUSTARD.SPEC, active.spec));
    if (active.phase) attributes.push(attrString(MUSTARD.PHASE, active.phase));
    if (typeof active.wave === 'number') {
      attributes.push(attrInt(MUSTARD.WAVE, active.wave));
    }
    if (isError && errorType) {
      attributes.push(attrString(GEN_AI.ERROR_TYPE, errorType));
    }

    const span: Record<string, unknown> = {
      traceId: active.ctx.traceId,
      spanId: active.ctx.spanId,
      // SPAN_KIND_CLIENT = 3 (OTLP enum). Task dispatch = outbound client call.
      kind: 3,
      name: active.name,
      startTimeUnixNano: startNs,
      endTimeUnixNano: endNs,
      attributes,
      status: { code: isError ? 2 : 1 }, // 1=OK, 2=ERROR per OTLP StatusCode.
    };
    if (active.ctx.parentSpanId) {
      span['parentSpanId'] = active.ctx.parentSpanId;
    }

    return {
      resourceSpans: [
        {
          resource: {
            attributes: [
              attrString('service.name', 'mustard'),
              attrString('service.version', '2.0-phase2'),
            ],
          },
          scopeSpans: [
            {
              scope: { name: 'mustard.telemetry', version: '1.0' },
              spans: [span],
            },
          ],
        },
      ],
    };
  }

  private sidecarPath(toolUseId: string): string {
    // Sanitize: tool_use_ids are alnum + dash from Claude Code, but be safe.
    const safe = String(toolUseId).replace(/[^A-Za-z0-9._-]/g, '_');
    return path.join(this.activeSpansDir, `${safe}.json`);
  }
}
