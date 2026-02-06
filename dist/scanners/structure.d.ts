import type { StructureInfo, ScanOptions } from '../types.js';
/**
 * Scan project structure
 */
export declare function scanStructure(projectPath: string, options?: ScanOptions): Promise<StructureInfo>;
