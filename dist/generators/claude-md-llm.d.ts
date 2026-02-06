import type { ProjectInfo, Analysis, GeneratorOptions } from '../types.js';
/**
 * Generate CLAUDE.md using Ollama LLM
 */
export declare function generateClaudeMd(projectInfo: ProjectInfo, analysis: Analysis, options?: GeneratorOptions): Promise<string | null>;
