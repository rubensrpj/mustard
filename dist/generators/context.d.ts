import type { ProjectInfo, Analysis, CodeSamples } from '../types.js';
export interface ContextGeneratorOptions {
    useOllama?: boolean;
    model?: string;
    verbose?: boolean;
}
/**
 * Generate context files for the .claude/context/ folder
 * These files provide instant context to agents during implementations
 */
export declare function generateContext(claudePath: string, projectInfo: ProjectInfo, analysis: Analysis, codeSamples: CodeSamples, options?: ContextGeneratorOptions): Promise<string[]>;
