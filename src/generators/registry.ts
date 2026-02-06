import type { ProjectInfo, Analysis, EntityRegistry, RegistryPatterns, Entity } from '../types.js';

/**
 * Generate entity-registry.json
 */
export function generateRegistry(projectInfo: ProjectInfo, analysis: Analysis): EntityRegistry {
  const registry: EntityRegistry = {
    _meta: {
      version: '2.1',
      generated: new Date().toISOString().split('T')[0]!,
      tool: 'mustard-cli'
    },
    _p: generatePatterns(projectInfo),
    e: generateEntities(projectInfo, analysis)
  };

  return registry;
}

/**
 * Generate path patterns based on detected stacks
 */
function generatePatterns(projectInfo: ProjectInfo): RegistryPatterns {
  const patterns: RegistryPatterns = {};

  // Find database stack
  const dbStack = projectInfo.stacks.find(s =>
    ['drizzle', 'prisma', 'typeorm', 'sequelize'].includes(s.name)
  );
  if (dbStack) {
    const basePath = dbStack.path === '.' ? '' : `${dbStack.path}/`;
    if (dbStack.name === 'drizzle') {
      patterns.db = `${basePath}src/schema/{e}.ts`;
    } else if (dbStack.name === 'prisma') {
      patterns.db = `${basePath}prisma/schema.prisma`;
    } else {
      patterns.db = `${basePath}src/entities/{e}.ts`;
    }
  }

  // Find backend stack
  const beStack = projectInfo.stacks.find(s =>
    ['dotnet', 'node', 'python', 'java', 'go', 'rust'].includes(s.name)
  );
  if (beStack) {
    const basePath = beStack.path === '.' ? '' : `${beStack.path}/`;
    if (beStack.name === 'dotnet') {
      patterns.be = `${basePath}Modules/{E}/`;
    } else if (beStack.name === 'python') {
      patterns.be = `${basePath}app/modules/{e}/`;
    } else {
      patterns.be = `${basePath}src/modules/{e}/`;
    }
  }

  // Find frontend stack
  const feStack = projectInfo.stacks.find(s =>
    ['react', 'nextjs', 'vue', 'angular', 'svelte'].includes(s.name)
  );
  if (feStack) {
    const basePath = feStack.path === '.' ? '' : `${feStack.path}/`;
    if (feStack.name === 'nextjs') {
      patterns.fe = `${basePath}src/features/{e}/`;
    } else {
      patterns.fe = `${basePath}src/features/{e}/`;
    }
  }

  return patterns;
}

/**
 * Generate entities map from analysis
 */
function generateEntities(projectInfo: ProjectInfo, analysis: Analysis): Record<string, number> {
  const entities: Record<string, number> = {};

  // Add entities discovered by semantic analyzer
  if (analysis.entities && Array.isArray(analysis.entities)) {
    for (const entity of analysis.entities) {
      // Normalize entity name to PascalCase
      const name = toPascalCase(typeof entity === 'string' ? entity : entity.name);
      if (name && !entities[name]) {
        entities[name] = 1;
      }
    }
  }

  // If no entities found, add placeholder
  if (Object.keys(entities).length === 0) {
    entities['_placeholder'] = 0;
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
