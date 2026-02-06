import type { ProjectInfo, Analysis, GeneratorOptions } from '../types.js';
/**
 * Main generator orchestrator
 */
export declare function generateAll(projectPath: string, projectInfo: ProjectInfo, analysis: Analysis, options?: GeneratorOptions): Promise<string[]>;
/**
 * Generate only core files (for update command)
 * DELETES and RECREATES: prompts/, commands/mustard/, hooks/, core/, skills/, scripts/, settings.json
 * Preserves: CLAUDE.md, commands/*.md (user), context/*.md (user), docs/*
 */
export declare function generateCoreOnly(projectPath: string, projectInfo: ProjectInfo, analysis: Analysis, options?: GeneratorOptions): Promise<string[]>;
