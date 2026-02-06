import type { ProjectInfo, Analysis, GeneratorOptions } from '../types.js';
/**
 * Main generator orchestrator
 */
export declare function generateAll(projectPath: string, projectInfo: ProjectInfo, analysis: Analysis, options?: GeneratorOptions): Promise<string[]>;
/**
 * Generate only core files (for update command)
 * Preserves: CLAUDE.md, context/*.md (except README), docs/*
 * Updates: commands/, prompts/, hooks/, core/, scripts/, settings.json, entity-registry.json
 */
export declare function generateCoreOnly(projectPath: string, projectInfo: ProjectInfo, analysis: Analysis, options?: GeneratorOptions): Promise<string[]>;
