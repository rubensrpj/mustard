import type { ProjectInfo, Analysis, GeneratedPrompts, PromptGeneratorOptions, DiscoveredPatterns } from '../types.js';
/**
 * Generate prompt files
 */
export declare function generatePrompts(projectInfo: ProjectInfo, analysis: Analysis, options?: PromptGeneratorOptions): Promise<GeneratedPrompts>;
/**
 * Generate auto-populated context section for a prompt
 */
export declare function generateAutoSection(promptType: string, projectInfo: ProjectInfo, analysis: Analysis, patterns: DiscoveredPatterns): string;
/**
 * Merge auto-generated content with existing prompt, preserving user content
 */
export declare function mergePromptContext(existingContent: string, newAutoSection: string): string;
