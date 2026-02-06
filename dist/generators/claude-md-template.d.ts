import type { ProjectInfo, Analysis } from '../types.js';
/**
 * Generate CLAUDE.md using templates (fallback when Ollama is unavailable)
 */
export declare function generateClaudeMd(projectInfo: ProjectInfo, analysis: Analysis): string;
