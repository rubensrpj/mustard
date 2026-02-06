import type { Analysis, EntityRegistry, RegistryEntity } from '../types.js';

/**
 * Generate entity-registry.json (v3.1 - catalog format)
 *
 * The registry is a catalog of entities, their relationships, and reference patterns.
 * All fields are populated by /sync-registry command which discovers them from actual code.
 */
export function generateRegistry(_projectInfo: unknown, analysis: Analysis): EntityRegistry {
  const registry: EntityRegistry = {
    _meta: {
      version: '3.1',
      generated: new Date().toISOString().split('T')[0]!,
      tool: 'mustard-cli'
    },
    _patterns: {},  // Reference entities - populated by /sync-registry
    _enums: {},     // Enum values - populated by /sync-registry
    e: generateEntities(analysis)
  };

  return registry;
}

/**
 * Generate entities map from analysis (v3.1 format with sub-entities and refs)
 */
function generateEntities(analysis: Analysis): Record<string, RegistryEntity> {
  const entities: Record<string, RegistryEntity> = {};

  // Add entities discovered by semantic analyzer
  if (analysis.entities && Array.isArray(analysis.entities)) {
    for (const entity of analysis.entities) {
      const name = toPascalCase(typeof entity === 'string' ? entity : entity.name);
      if (name && !entities[name]) {
        entities[name] = {};  // Empty - sub/refs populated by /sync-registry
      }
    }
  }

  // If no entities found, add placeholder
  if (Object.keys(entities).length === 0) {
    entities['_placeholder'] = {};
  }

  return entities;
}

/**
 * Convert string to PascalCase
 */
function toPascalCase(str: string): string {
  if (!str) return '';

  return str
    .replace(/[-_\s]+(.)?/g, (_, c: string | undefined) => (c ? c.toUpperCase() : ''))
    .replace(/^(.)/, (_, c: string) => c.toUpperCase());
}
