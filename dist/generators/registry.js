/**
 * Generate entity-registry.json (v3.1 - catalog format)
 *
 * The registry is a catalog of entities, their relationships, and reference patterns.
 * All fields are populated by /sync-registry command which discovers them from actual code.
 */
export function generateRegistry(_projectInfo, analysis) {
    const registry = {
        _meta: {
            version: '3.1',
            generated: new Date().toISOString().split('T')[0],
            tool: 'mustard-cli'
        },
        _patterns: {}, // Reference entities - populated by /sync-registry
        _enums: {}, // Enum values - populated by /sync-registry
        e: generateEntities(analysis)
    };
    return registry;
}
/**
 * Generate entities map from analysis (v3.1 format with sub-entities and refs)
 */
function generateEntities(analysis) {
    const entities = {};
    // Add entities discovered by semantic analyzer
    if (analysis.entities && Array.isArray(analysis.entities)) {
        for (const entity of analysis.entities) {
            const name = toPascalCase(typeof entity === 'string' ? entity : entity.name);
            if (name && !entities[name]) {
                entities[name] = {}; // Empty - sub/refs populated by /sync-registry
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
function toPascalCase(str) {
    if (!str)
        return '';
    return str
        .replace(/[-_\s]+(.)?/g, (_, c) => (c ? c.toUpperCase() : ''))
        .replace(/^(.)/, (_, c) => c.toUpperCase());
}
//# sourceMappingURL=registry.js.map