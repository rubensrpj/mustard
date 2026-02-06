import type { Analysis, EntityRegistry } from '../types.js';
/**
 * Generate entity-registry.json (v3.1 - catalog format)
 *
 * The registry is a catalog of entities, their relationships, and reference patterns.
 * All fields are populated by /sync-registry command which discovers them from actual code.
 */
export declare function generateRegistry(_projectInfo: unknown, analysis: Analysis): EntityRegistry;
