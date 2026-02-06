import type { ProjectInfo, Analysis, CodeSample, OllamaOptions } from '../types.js';
/**
 * Analyze code snippets using Ollama LLM
 */
export declare function analyzeCode(codeSnippets: CodeSample[], options?: OllamaOptions): Promise<Analysis>;
/**
 * Generate CLAUDE.md content using Ollama
 */
export declare function generateClaudeMd(projectInfo: ProjectInfo, analysis: Analysis, options?: OllamaOptions): Promise<string | null>;
