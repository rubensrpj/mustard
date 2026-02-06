import type { ProjectInfo, Analysis, CodeSample, OllamaOptions, GeneratedPrompts } from '../types.js';
/**
 * Analyze code snippets using Ollama LLM
 */
export declare function analyzeCode(codeSnippets: CodeSample[], options?: OllamaOptions): Promise<Analysis>;
/**
 * Generate CLAUDE.md content using Ollama
 */
export declare function generateClaudeMd(projectInfo: ProjectInfo, analysis: Analysis, options?: OllamaOptions): Promise<string | null>;
/**
 * Generate specialized prompts using Ollama
 */
export declare function generatePrompts(projectInfo: ProjectInfo, analysis: Analysis, options?: OllamaOptions): Promise<Partial<GeneratedPrompts>>;
