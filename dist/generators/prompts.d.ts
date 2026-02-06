import type { ProjectInfo, Analysis, GeneratedPrompts, DiscoveredPatterns } from '../types.js';
/**
 * Generate prompt files from templates
 */
export declare function generatePrompts(projectInfo: ProjectInfo, _analysis: Analysis): Promise<GeneratedPrompts>;
/**
 * Generate auto-populated context section for a prompt
 */
export declare function generateAutoSection(_promptType: string, projectInfo: ProjectInfo, analysis: Analysis, patterns: DiscoveredPatterns): string;
/**
 * Merge auto-generated content with existing prompt, preserving user content
 */
export declare function mergePromptContext(existingContent: string, newAutoSection: string): string;
