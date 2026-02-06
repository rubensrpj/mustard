import type { SearchOptions, TraceOptions, SearchResponse, TraceResult } from '../types.js';
/**
 * Check if grepai CLI is available
 */
export declare function checkGrepaiAvailable(): Promise<boolean>;
/**
 * Get grepai index status
 */
export declare function indexStatus(): Promise<Record<string, unknown> | null>;
/**
 * Search using grepai semantic search
 */
export declare function search(query: string, options?: SearchOptions): Promise<SearchResponse>;
/**
 * Find all functions that call the specified symbol
 */
export declare function traceCallers(symbol: string, options?: TraceOptions): Promise<TraceResult>;
/**
 * Find all functions called by the specified symbol
 */
export declare function traceCallees(symbol: string, options?: TraceOptions): Promise<TraceResult>;
/**
 * Build a complete call graph around a symbol
 */
export declare function traceGraph(symbol: string, options?: TraceOptions): Promise<TraceResult>;
