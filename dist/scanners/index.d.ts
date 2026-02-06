import type { ProjectInfo, ScanOptions } from '../types.js';
/**
 * Main scanner orchestrator
 * Scans the project directory and returns comprehensive project info
 */
export declare function scanProject(projectPath: string, options?: ScanOptions): Promise<ProjectInfo>;
