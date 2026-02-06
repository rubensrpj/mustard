import type { Stack, Entity, DiscoveredPatterns, CodeSamples, DiscoverOptions, CodeSampleOptions } from '../types.js';
/**
 * Discover patterns using semantic search
 */
export declare function discoverPatterns(options?: DiscoverOptions): Promise<DiscoveredPatterns>;
/**
 * Discover entities in the codebase
 */
export declare function discoverEntities(stacks?: Stack[]): Promise<Entity[]>;
/**
 * Get code samples for each pattern type
 */
export declare function getCodeSamples(patterns: DiscoveredPatterns, options?: CodeSampleOptions): Promise<CodeSamples>;
