/**
 * Runtime assertion for Mustard 2.0+.
 *
 * Mustard is Bun-only. This module asserts the active runtime is Bun and
 * exposes version info. There is no Node fallback path.
 */

export interface RuntimeInfo {
  kind: 'bun';
  version: string;
  claudeCodeVersion?: string;
}

function isBunRuntime(): boolean {
  if (process.versions && typeof process.versions.bun === 'string') return true;
  return typeof (globalThis as { Bun?: unknown }).Bun !== 'undefined';
}

export class RuntimeError extends Error {
  constructor(message: string) {
    super(message);
    this.name = 'RuntimeError';
  }
}

/**
 * Assert Bun runtime. Throws RuntimeError if running under any other runtime.
 * Honor `MUSTARD_RUNTIME_VERBOSE=1` for stderr trace.
 */
export function detect(): RuntimeInfo {
  if (!isBunRuntime()) {
    throw new RuntimeError(
      'Mustard requires the Bun runtime (>= 1.2.0). ' +
      'Install: https://bun.sh — Windows: `scoop install bun` — Unix: `curl -fsSL https://bun.sh/install | bash`'
    );
  }

  const version = process.versions.bun ?? '';
  const claudeCodeVersion = process.env.CLAUDE_CODE_VERSION;

  if (process.env.MUSTARD_RUNTIME_VERBOSE === '1') {
    process.stderr.write(`[mustard:runtime] kind=bun version=${version}\n`);
  }

  const info: RuntimeInfo = { kind: 'bun', version };
  if (claudeCodeVersion) info.claudeCodeVersion = claudeCodeVersion;
  return info;
}
