import type { DependencyInfo } from '../types.js';
/**
 * Scan all package.json and .csproj files to extract real dependencies
 */
export declare function scanDependencies(projectPath: string, options?: {
    verbose?: boolean;
}): Promise<DependencyInfo>;
