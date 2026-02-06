import { execSync } from 'child_process';
import type { SearchOptions, TraceOptions, SearchResponse, TraceResult } from '../types.js';

/**
 * Check if grepai CLI is available
 */
export async function checkGrepaiAvailable(): Promise<boolean> {
  try {
    execSync('grepai --help', { stdio: 'pipe' });
    return true;
  } catch {
    return false;
  }
}

/**
 * Get grepai index status
 */
export async function indexStatus(): Promise<Record<string, unknown> | null> {
  try {
    const result = execSync('grepai status --json', {
      encoding: 'utf-8',
      maxBuffer: 10 * 1024 * 1024
    });
    return JSON.parse(result) as Record<string, unknown>;
  } catch {
    return null;
  }
}

/**
 * Search using grepai semantic search
 */
export async function search(query: string, options: SearchOptions = {}): Promise<SearchResponse> {
  const { limit = 10, compact = false } = options;

  try {
    const compactFlag = compact ? '--compact' : '';
    const result = execSync(
      `grepai search "${query}" --limit ${limit} --json ${compactFlag}`,
      {
        encoding: 'utf-8',
        maxBuffer: 10 * 1024 * 1024
      }
    );
    return JSON.parse(result) as SearchResponse;
  } catch (error: unknown) {
    const message = error instanceof Error ? error.message : 'Unknown error';
    console.error('grepai search failed:', message);
    return { results: [] };
  }
}

/**
 * Find all functions that call the specified symbol
 */
export async function traceCallers(symbol: string, options: TraceOptions = {}): Promise<TraceResult> {
  const { compact = false } = options;

  try {
    const compactFlag = compact ? '--compact' : '';
    const result = execSync(
      `grepai trace callers "${symbol}" --json ${compactFlag}`,
      { encoding: 'utf-8' }
    );
    return JSON.parse(result) as TraceResult;
  } catch {
    return { callers: [] };
  }
}

/**
 * Find all functions called by the specified symbol
 */
export async function traceCallees(symbol: string, options: TraceOptions = {}): Promise<TraceResult> {
  const { compact = false } = options;

  try {
    const compactFlag = compact ? '--compact' : '';
    const result = execSync(
      `grepai trace callees "${symbol}" --json ${compactFlag}`,
      { encoding: 'utf-8' }
    );
    return JSON.parse(result) as TraceResult;
  } catch {
    return { callees: [] };
  }
}

/**
 * Build a complete call graph around a symbol
 */
export async function traceGraph(symbol: string, options: TraceOptions = {}): Promise<TraceResult> {
  const { depth = 2 } = options;

  try {
    const result = execSync(
      `grepai trace graph "${symbol}" --depth ${depth} --json`,
      { encoding: 'utf-8' }
    );
    return JSON.parse(result) as TraceResult;
  } catch {
    return { graph: {} };
  }
}
