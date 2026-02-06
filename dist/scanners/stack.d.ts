import type { StackScanResult, ScanOptions } from '../types.js';
/**
 * Scan project for stacks (languages and frameworks)
 */
export declare function scanStack(projectPath: string, options?: ScanOptions): Promise<StackScanResult>;
