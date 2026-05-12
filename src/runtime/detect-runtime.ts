/**
 * Runtime detection for Mustard 2.0 Phase 0.
 *
 * Pure module — zero side effects, zero I/O. Detects whether the current
 * process is running under Bun or Node, reports versions, and probes for
 * `bun:sqlite` availability when running under Bun.
 *
 * Detection rules:
 * - `process.versions.bun` is the canonical Bun marker (works without
 *   `bun-types` and is recommended by the Bun team).
 * - `typeof Bun !== 'undefined'` is checked as a fallback signal.
 * - `MUSTARD_RUNTIME=node|bun` overrides detection (force mode).
 */

import { createRequire } from 'node:module';

export type RuntimeKind = 'bun' | 'node';

export interface RuntimeInfo {
  kind: RuntimeKind;
  version: string;
  bunSqliteAvailable: boolean;
  claudeCodeVersion?: string;
}

function isBunRuntime(): boolean {
  if (process.versions && typeof process.versions.bun === 'string') {
    return true;
  }
  return typeof (globalThis as { Bun?: unknown }).Bun !== 'undefined';
}

function probeBunSqlite(): boolean {
  try {
    // Use createRequire for consistency with src/cli.ts. Resolving `bun:sqlite`
    // throws on Node (unknown specifier) and succeeds on Bun, which is exactly
    // the signal we want.
    const req = createRequire(import.meta.url);
    req('bun:sqlite');
    return true;
  } catch {
    return false;
  }
}

function readOverride(): RuntimeKind | null {
  const raw = process.env.MUSTARD_RUNTIME;
  if (raw === 'node' || raw === 'bun') return raw;
  return null;
}

function verbose(): boolean {
  return process.env.MUSTARD_RUNTIME_VERBOSE === '1';
}

/**
 * Detect the active JavaScript runtime.
 *
 * Honors `MUSTARD_RUNTIME=node|bun` as a forced override. When verbose mode
 * is enabled (`MUSTARD_RUNTIME_VERBOSE=1`), logs the chosen runtime to stderr.
 */
export function detect(): RuntimeInfo {
  const override = readOverride();
  const detectedBun = isBunRuntime();
  const kind: RuntimeKind = override ?? (detectedBun ? 'bun' : 'node');

  const version = kind === 'bun'
    ? (process.versions.bun ?? '')
    : process.versions.node;

  const bunSqliteAvailable = kind === 'bun' ? probeBunSqlite() : false;

  const claudeCodeVersion = process.env.CLAUDE_CODE_VERSION;

  if (verbose()) {
    const src = override ? `override=${override}` : `detected=${detectedBun ? 'bun' : 'node'}`;
    process.stderr.write(
      `[mustard:runtime] kind=${kind} version=${version} bunSqlite=${bunSqliteAvailable} ${src}\n`
    );
  }

  const info: RuntimeInfo = { kind, version, bunSqliteAvailable };
  if (claudeCodeVersion) info.claudeCodeVersion = claudeCodeVersion;
  return info;
}
