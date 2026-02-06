import type { ProjectInfo, Analysis, CodeSamples } from '../types.js';
export interface ContextGeneratorOptions {
    useOllama?: boolean;
    model?: string;
    verbose?: boolean;
}
/**
 * Generate context files for the .claude/context/ folder
 *
 * Note: Auto-generation of architecture.md, patterns.md, naming.md was removed
 * because the generated content was too generic and duplicated CLAUDE.md.
 * Users should create these files manually with project-specific content.
 *
 * @deprecated This function is kept for compatibility but does nothing.
 */
export declare function generateContext(_claudePath: string, _projectInfo: ProjectInfo, _analysis: Analysis, _codeSamples: CodeSamples, _options?: ContextGeneratorOptions): Promise<string[]>;
