import type { Stack, CodeSamples } from '../types.js';
/**
 * Scan the project for real code samples without grepai
 * Returns up to 1 sample per type (service, endpoint, component, hook)
 */
export declare function scanCodeSamples(projectPath: string, stacks: Stack[], options?: {
    verbose?: boolean;
}): Promise<CodeSamples>;
