#!/usr/bin/env node
'use strict';

/**
 * skill-generator.js
 *
 * Reads entity-registry.json v4.0 and generates skills in
 * {subproject}/.claude/skills/ based on detected _patterns.
 *
 * Closes the OODA loop: scan → registry → skills → agent implements correctly.
 *
 * Usage:
 *   node .claude/scripts/skill-generator.js                     # Generate from registry
 *   node .claude/scripts/skill-generator.js --dry-run            # Show what would be generated
 *   node .claude/scripts/skill-generator.js --subproject api     # Filter to one subproject
 *   node .claude/scripts/skill-generator.js --force              # Overwrite existing skills
 */

const fs = require('fs');
const path = require('path');

// ---------------------------------------------------------------------------
// Paths (mirror sync-registry.js convention)
// ---------------------------------------------------------------------------

const ROOT = path.resolve(__dirname, '..', '..');
const REGISTRY_PATH = path.join(ROOT, '.claude', 'entity-registry.json');
const DETECT_CACHE_PATH = path.join(ROOT, '.claude', '.detect-cache.json');

// ---------------------------------------------------------------------------
// CLI flags
// ---------------------------------------------------------------------------

const args = process.argv.slice(2);
const DRY_RUN = args.includes('--dry-run');
const FORCE = args.includes('--force');
const SUB_FILTER = (() => {
  const idx = args.indexOf('--subproject');
  return idx !== -1 && args[idx + 1] ? args[idx + 1] : null;
})();

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/**
 * Read a JSON file safely. Returns null on error.
 * @param {string} filePath
 * @returns {Object|null}
 */
function readJsonSafe(filePath) {
  try {
    return JSON.parse(fs.readFileSync(filePath, 'utf-8'));
  } catch {
    return null;
  }
}

/**
 * Read a file's first line to check for mustard:generated header.
 * @param {string} filePath
 * @returns {boolean}
 */
function isMustardGenerated(filePath) {
  try {
    const content = fs.readFileSync(filePath, 'utf-8');
    // Skills have frontmatter first, then the mustard:generated comment
    // For other files the header is on line 1
    return content.includes('<!-- mustard:generated');
  } catch {
    return false;
  }
}

/**
 * Write a file, creating parent directories as needed.
 * In dry-run mode, just prints what would be written.
 * Skips files without mustard:generated header unless --force.
 * @param {string} filePath
 * @param {string} content
 * @param {string[]} log - collector for summary lines
 */
function writeFile(filePath, content, log) {
  const relPath = path.relative(ROOT, filePath).replace(/\\/g, '/');

  if (DRY_RUN) {
    log.push(`  [dry-run] would write: ${relPath}`);
    return;
  }

  // Safety: never overwrite a manually edited file (one without the generated header)
  if (!FORCE && fs.existsSync(filePath) && !isMustardGenerated(filePath)) {
    log.push(`  [skip] manually edited: ${relPath}`);
    return;
  }

  try {
    fs.mkdirSync(path.dirname(filePath), { recursive: true });
    fs.writeFileSync(filePath, content, 'utf-8');
    log.push(`  [write] ${relPath}`);
  } catch (err) {
    process.stderr.write(`[skill-generator] Failed to write ${relPath}: ${err.message}\n`);
  }
}

/**
 * Format a date as ISO string (YYYY-MM-DDTHH:MM:SSZ).
 * @returns {string}
 */
function isoNow() {
  return new Date().toISOString().replace(/\.\d{3}Z$/, 'Z');
}

/**
 * Capitalise the first letter of a string.
 * @param {string} s
 * @returns {string}
 */
function cap(s) {
  if (!s) return s;
  return s.charAt(0).toUpperCase() + s.slice(1);
}

/**
 * Return the first non-null/non-undefined value from an array.
 * @param {...any} vals
 * @returns {any}
 */
function first(...vals) {
  for (const v of vals) {
    if (v !== null && v !== undefined && v !== '') return v;
  }
  return null;
}

/**
 * Map a role string to an agent name.
 * @param {string} role
 * @returns {string}
 */
function roleToAgent(role) {
  const map = { api: 'backend', ui: 'frontend', database: 'database', mobile: 'mobile', library: 'backend', general: 'general' };
  return map[role] || 'general';
}

/**
 * Map a stack ID to a human-readable language label used in code fences.
 * @param {string} stackId
 * @returns {string}
 */
function stackLang(stackId) {
  const map = { dotnet: 'csharp', typescript: 'typescript', dart: 'dart', php: 'php', python: 'python', java: 'java', go: 'go', rust: 'rust' };
  return map[stackId] || stackId;
}

/**
 * Map a stack ID to a human-readable framework name.
 * @param {string} stackId
 * @returns {string}
 */
function stackLabel(stackId) {
  const map = { dotnet: '.NET', typescript: 'TypeScript/Node.js', dart: 'Flutter/Dart', php: 'Laravel/PHP', python: 'Python', java: 'Spring/Java', go: 'Go', rust: 'Rust' };
  return map[stackId] || cap(stackId);
}

// ---------------------------------------------------------------------------
// Subproject resolution
// ---------------------------------------------------------------------------

/**
 * Build a map of stackId → [{subprojectName, path, role, agent}] from the detect cache.
 * Falls back to empty maps if cache is missing.
 *
 * @returns {Map<string, Array<{name: string, path: string, role: string, agent: string}>>}
 */
function buildStackSubprojectMap() {
  const cache = readJsonSafe(DETECT_CACHE_PATH);
  const subprojects = cache?.subprojects || [];
  const map = new Map(); // stackId → [{name, path, role, agent}]

  for (const sub of subprojects) {
    const subAbsPath = path.join(ROOT, sub.path);
    const stackId = detectStackFromPath(subAbsPath);
    if (!stackId) continue;

    if (!map.has(stackId)) map.set(stackId, []);
    map.get(stackId).push({
      name: sub.name,
      path: sub.path,
      role: sub.role || 'general',
      agent: sub.agent || roleToAgent(sub.role || 'general'),
    });
  }

  return map;
}

/**
 * Detect stack ID from a subproject path using file-presence heuristics.
 * Mirrors scanner-loader.js STACK_SIGNALS.
 * @param {string} absPath
 * @returns {string|null}
 */
function detectStackFromPath(absPath) {
  const signals = [
    { id: 'dotnet', exts: ['.csproj', '.sln'], files: [] },
    { id: 'dart', exts: [], files: ['pubspec.yaml'] },
    { id: 'typescript', exts: [], files: ['package.json', 'tsconfig.json'] },
    { id: 'php', exts: [], files: ['composer.json', 'artisan'] },
    { id: 'python', exts: [], files: ['pyproject.toml', 'manage.py', 'setup.py', 'requirements.txt'] },
    { id: 'java', exts: [], files: ['pom.xml', 'build.gradle', 'build.gradle.kts'] },
    { id: 'go', exts: [], files: ['go.mod'] },
    { id: 'rust', exts: [], files: ['Cargo.toml'] },
  ];

  try {
    const entries = fs.readdirSync(absPath);

    for (const sig of signals) {
      // Check extension-based patterns first
      for (const ext of sig.exts) {
        if (entries.some(e => e.endsWith(ext))) return sig.id;
      }
      // Check exact file names
      for (const f of sig.files) {
        if (entries.includes(f)) return sig.id;
      }
    }
  } catch { /* fail-open */ }

  return null;
}

// ---------------------------------------------------------------------------
// Pick a representative entity from registry.e for examples
// ---------------------------------------------------------------------------

/**
 * Find an entity entry that has the most interesting data (refs, enums, services).
 * @param {Object} entities - registry.e
 * @param {string[]} [preferredKeys] - entity names to prefer
 * @returns {{ name: string, info: Object }|null}
 */
function pickRepresentativeEntity(entities, preferredKeys = []) {
  const entries = Object.entries(entities || {});
  if (!entries.length) return null;

  // Score each entity by richness
  const scored = entries.map(([name, info]) => {
    let score = 0;
    if (info.refs?.length) score += info.refs.length * 2;
    if (info.enums?.length) score += info.enums.length * 2;
    if (info.sub?.length) score += info.sub.length;
    if (info.services?.length) score += 1;
    if (info.repositories?.length) score += 1;
    if (info.dtos?.length) score += 1;
    if (info.endpoints?.length) score += info.endpoints.length;
    // Prefer explicitly requested keys
    if (preferredKeys.includes(name)) score += 100;
    return { name, info, score };
  });

  scored.sort((a, b) => b.score - a.score);
  return { name: scored[0].name, info: scored[0].info };
}

// ---------------------------------------------------------------------------
// Skill content generators
// ---------------------------------------------------------------------------

/**
 * Generate the entity-creation skill content.
 * @param {string} sub - subproject short name
 * @param {string} stackId
 * @param {Object} entityPattern - _patterns.{stack}.entity
 * @param {Object} enumPattern - _patterns.{stack}.enum (may be null)
 * @param {string} role
 * @param {Object} registryEntities - registry.e
 * @returns {{ skillMd: string, examplesMd: string }}
 */
function genEntityCreationSkill(sub, stackId, entityPattern, enumPattern, role, registryEntities) {
  const iso = isoNow();
  const lang = stackLang(stackId);
  const label = stackLabel(stackId);

  const folder = entityPattern.folder || '(inferred from project)';
  const baseClass = entityPattern.baseClass || null;
  const ifaces = (entityPattern.interfaces || []).filter(Boolean);
  const nsPattern = entityPattern.namespacePattern || null;
  const naming = entityPattern.namingConvention || 'PascalCase';

  const rep = pickRepresentativeEntity(registryEntities);
  const repName = rep?.name || 'Order';
  const repInfo = rep?.info || {};

  // ---- SKILL.md ----
  const ifaceLine = ifaces.length ? `- Interfaces: \`${ifaces.join(', ')}\`` : '';
  const nsLine = nsPattern ? `- Namespace: \`${nsPattern}.{Entity}\`` : '';
  const baseClassLine = baseClass ? `- Base class: \`${baseClass}\`` : '';
  const enumSeparate = enumPattern?.separateFiles
    ? `- NEVER place enums inside entity files — enums go in \`${enumPattern.folder || 'separate files'}\``
    : '';

  // Build a minimal example class body
  const exampleClass = buildEntityExample(stackId, repName, repInfo, entityPattern);

  const skillMd = `---
name: ${sub}-entity-creation
description: "Pattern for creating new ${label} entities/models in this project.
  Use when creating a new entity, model, domain object, adding a table,
  or the user says 'new entity', 'add model', 'create table', 'new domain object'.
  Even if the user just says 'I need a new X in the database'."
---
<!-- mustard:generated at:${iso} role:${role} -->

# Entity Creation

${label} entity/model pattern detected in this project.

## Pattern

- Folder: \`${folder}\`
${baseClassLine ? baseClassLine + '\n' : ''}\
${ifaceLine ? ifaceLine + '\n' : ''}\
- Naming: ${naming}
${nsLine ? nsLine + '\n' : ''}\

## Rules

- ALWAYS place entity files in \`${folder}\`
${enumSeparate ? enumSeparate + '\n' : ''}\
- Name entities in ${naming}
- Follow the base class / interface contract detected in this project

## Example

\`\`\`${lang}
${exampleClass}
\`\`\`

Ref: \`${repInfo.file || folder + '/' + repName + (lang === 'csharp' ? '.cs' : lang === 'dart' ? '.dart' : '.ts')}\`

## References

For full code examples with variants:
> Read \`references/examples.md\`
`;

  // ---- references/examples.md ----
  const secondEntity = pickRepresentativeEntity(registryEntities, []);
  // Use a second entity if available
  const allKeys = Object.keys(registryEntities || {});
  const secondKey = allKeys.find(k => k !== repName) || repName;
  const secondInfo = registryEntities?.[secondKey] || repInfo;

  const examplesMd = `<!-- mustard:generated at:${iso} role:${role} -->

# Examples: ${sub}-entity-creation

## Example 1 — Basic

\`\`\`${lang}
${buildEntityExample(stackId, repName, repInfo, entityPattern)}
\`\`\`

Ref: \`${repInfo.file || folder + '/' + repName}\`

## Example 2 — With relationships

\`\`\`${lang}
${buildEntityExample(stackId, secondKey, secondInfo, entityPattern, true)}
\`\`\`

Ref: \`${secondInfo.file || folder + '/' + secondKey}\`
`;

  return { skillMd, examplesMd };
}

/**
 * Build a minimal entity code example from detected patterns and an entity entry.
 * @param {string} stackId
 * @param {string} entityName
 * @param {Object} info - registry.e entry
 * @param {Object} pattern - _patterns.{stack}.entity
 * @param {boolean} [withRels=false]
 * @returns {string}
 */
function buildEntityExample(stackId, entityName, info, pattern, withRels = false) {
  const baseClass = info.baseClass || pattern.baseClass || null;
  const ifaces = info.interfaces || pattern.interfaces || [];
  const refs = withRels ? (info.refs || []).slice(0, 2) : [];
  const enums = withRels ? (info.enums || []).slice(0, 1) : [];

  switch (stackId) {
    case 'dotnet': {
      const ns = info.namespace || pattern.namespacePattern || 'MyApp.Domain.Entities';
      const parents = [baseClass, ...ifaces].filter(Boolean).join(', ');
      const parentStr = parents ? ` : ${parents}` : '';
      const refProps = refs.map(r => `    public ${r}? ${r} { get; set; }`).join('\n');
      const enumProps = enums.map(e => `    public ${e} ${e}Type { get; set; }`).join('\n');
      return `namespace ${ns};

public class ${entityName}${parentStr}
{
    public Guid Id { get; set; }
    public string Name { get; set; } = string.Empty;
    public DateTime CreatedAt { get; set; }
${refProps ? refProps + '\n' : ''}\
${enumProps ? enumProps + '\n' : ''}\
}`;
    }

    case 'dart': {
      const freezed = pattern.serialization === 'freezed' || (ifaces.length === 0 && baseClass === null);
      if (freezed) {
        const refFields = refs.map(r => `  final ${r}? ${r.charAt(0).toLowerCase() + r.slice(1)};`).join('\n');
        const enumFields = enums.map(e => `  final ${e}? ${e.charAt(0).toLowerCase() + e.slice(1)};`).join('\n');
        return `@freezed
class ${entityName} with _\$${entityName} {
  const factory ${entityName}({
    required String id,
    required String name,
${refFields ? refFields + '\n' : ''}\
${enumFields ? enumFields + '\n' : ''}\
  }) = _${entityName};

  factory ${entityName}.fromJson(Map<String, dynamic> json) =>
      _\$${entityName}FromJson(json);
}`;
      }
      const refFields = refs.map(r => `  final ${r}? ${r.charAt(0).toLowerCase() + r.slice(1)};`).join('\n');
      return `class ${entityName} {
  final String id;
  final String name;
${refFields ? refFields + '\n' : ''}\

  const ${entityName}({required this.id, required this.name${refs.map(r => `, this.${r.charAt(0).toLowerCase() + r.slice(1)}`).join('')}});
}`;
    }

    case 'typescript': {
      const refFields = refs.map(r => `  ${r.charAt(0).toLowerCase() + r.slice(1)}?: ${r};`).join('\n');
      const enumFields = enums.map(e => `  ${e.charAt(0).toLowerCase() + e.slice(1)}: ${e};`).join('\n');
      const extendsStr = baseClass ? ` extends ${baseClass}` : '';
      const implStr = ifaces.length ? ` implements ${ifaces.join(', ')}` : '';
      return `export class ${entityName}${extendsStr}${implStr} {
  id: string;
  name: string;
  createdAt: Date;
${refFields ? refFields + '\n' : ''}\
${enumFields ? enumFields + '\n' : ''}\
}`;
    }

    case 'php': {
      const parentStr = baseClass ? ` extends ${baseClass}` : '';
      const ifaceStr = ifaces.length ? ` implements ${ifaces.join(', ')}` : '';
      const refProps = refs.map(r => `    public ?${r} $${r.charAt(0).toLowerCase() + r.slice(1)} = null;`).join('\n');
      return `class ${entityName}${parentStr}${ifaceStr}
{
    protected $fillable = ['name'];
${refProps ? '\n' + refProps + '\n' : ''}\
}`;
    }

    case 'java': {
      const parentStr = baseClass ? ` extends ${baseClass}` : '';
      const ifaceStr = ifaces.length ? ` implements ${ifaces.join(', ')}` : '';
      const refProps = refs.map(r => `    private ${r} ${r.charAt(0).toLowerCase() + r.slice(1)};`).join('\n');
      return `@Entity
@Table(name = "${entityName.replace(/([A-Z])/g, '_$1').toLowerCase().slice(1)}s")
public class ${entityName}${parentStr}${ifaceStr} {
    @Id
    @GeneratedValue(strategy = GenerationType.UUID)
    private UUID id;
    private String name;
${refProps ? refProps + '\n' : ''}\
}`;
    }

    case 'python': {
      const parentStr = baseClass ? `(${baseClass})` : '';
      const refProps = refs.map(r => `    ${r.charAt(0).toLowerCase() + r.slice(1)}_id: Optional[UUID] = None`).join('\n');
      return `class ${entityName}${parentStr}:
    id: UUID
    name: str
${refProps ? refProps + '\n' : ''}\

    class Config:
        from_attributes = True`;
    }

    case 'go': {
      const refProps = refs.map(r => `\t${r}   *${r}`).join('\n');
      return `type ${entityName} struct {
\tID        uuid.UUID \`json:"id" db:"id"\`
\tName      string    \`json:"name" db:"name"\`
\tCreatedAt time.Time \`json:"created_at" db:"created_at"\`
${refProps ? refProps + '\n' : ''}\
}`;
    }

    case 'rust': {
      const refProps = refs.map(r => `    pub ${r.replace(/([A-Z])/g, '_$1').toLowerCase().slice(1)}: Option<${r}>,`).join('\n');
      return `#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ${entityName} {
    pub id: Uuid,
    pub name: String,
${refProps ? refProps + '\n' : ''}\
}`;
    }

    default:
      return `// ${entityName} entity — see project conventions`;
  }
}

// ---------------------------------------------------------------------------

/**
 * Generate the enum-placement skill.
 * @param {string} sub
 * @param {string} stackId
 * @param {Object} enumPattern
 * @param {string} role
 * @param {Object} registryEnums - registry._enums
 * @returns {{ skillMd: string, examplesMd: string }}
 */
function genEnumPlacementSkill(sub, stackId, enumPattern, role, registryEnums) {
  const iso = isoNow();
  const lang = stackLang(stackId);

  const folder = enumPattern.folder || '(project folder)';
  const valueConvention = enumPattern.valueConvention || 'PascalCase';
  const decorators = (enumPattern.decorators || []).filter(Boolean);
  const nsPattern = enumPattern.namespacePattern || null;

  // Pick a real enum from registry
  const enumKeys = Object.keys(registryEnums || {});
  const exampleEnum = enumKeys[0] || 'Status';
  const exampleEnumInfo = registryEnums?.[exampleEnum] || { values: ['Active', 'Inactive'] };
  const exampleValues = Array.isArray(exampleEnumInfo)
    ? exampleEnumInfo.filter(v => !v.startsWith('...'))
    : (exampleEnumInfo.values || ['Active', 'Inactive']).filter(v => !v.startsWith('...'));

  const exampleCode = buildEnumExample(stackId, exampleEnum, exampleValues, valueConvention, decorators, nsPattern || undefined);

  const skillMd = `---
name: ${sub}-enum-placement
description: "Pattern for enum/value type placement and conventions in this project.
  Use when creating new enums, status types, adding enum values,
  or the user says 'add enum', 'new status', 'add type'.
  Even if the user just says 'I need a status field'."
---
<!-- mustard:generated at:${iso} role:${role} -->

# Enum Conventions

Enum placement and naming rules detected in this project.

## Pattern

- Folder: \`${folder}\`
${nsPattern ? `- Namespace: \`${nsPattern}.{EnumName}\`\n` : ''}\
- Value naming: ${valueConvention}
${decorators.length ? `- Value decorators: ${decorators.map(d => `\`${d}\``).join(', ')}\n` : ''}\
- ALWAYS create enums in separate files in \`${folder}\`
- NEVER define enums inside entity/model files

## Example

\`\`\`${lang}
${exampleCode}
\`\`\`

## References

For full code examples with variants:
> Read \`references/examples.md\`
`;

  // Second enum for examples
  const secondEnumKey = enumKeys[1] || enumKeys[0] || 'Category';
  const secondEnumInfo = registryEnums?.[secondEnumKey] || { values: ['Alpha', 'Beta'] };
  const secondValues = Array.isArray(secondEnumInfo)
    ? secondEnumInfo.filter(v => !v.startsWith('...'))
    : (secondEnumInfo.values || []).filter(v => !v.startsWith('...'));

  const examplesMd = `<!-- mustard:generated at:${iso} role:${role} -->

# Examples: ${sub}-enum-placement

## Example 1 — Basic enum

\`\`\`${lang}
${exampleCode}
\`\`\`

## Example 2 — Second example

\`\`\`${lang}
${buildEnumExample(stackId, secondEnumKey, secondValues, valueConvention, decorators, nsPattern || undefined)}
\`\`\`
`;

  return { skillMd, examplesMd };
}

/**
 * Build enum code example.
 * @param {string} stackId
 * @param {string} enumName
 * @param {string[]} values
 * @param {string} valueConvention
 * @param {string[]} decorators
 * @param {string} [namespace]
 * @returns {string}
 */
function buildEnumExample(stackId, enumName, values, valueConvention, decorators, namespace) {
  const displayValues = values.slice(0, 5);
  const decoratorLine = decorators.length ? `    [${decorators[0]}("...")] ` : '    ';

  switch (stackId) {
    case 'dotnet': {
      const ns = namespace || 'MyApp.Domain.Enums';
      const valueLines = displayValues
        .map((v, i) => `    ${decoratorLine.trim() ? '[' + decorators[0] + '("...")]' + ' ' : ''}${v}${i < displayValues.length - 1 ? ',' : ''}`)
        .join('\n');
      return `namespace ${ns};

public enum ${enumName}
{
${valueLines}
}`;
    }

    case 'dart':
      return `enum ${enumName} {
  ${displayValues.join(',\n  ')};
}`;

    case 'typescript':
      return `export enum ${enumName} {
  ${displayValues.map(v => `${v} = '${v}'`).join(',\n  ')},
}`;

    case 'php':
      return `enum ${enumName}: string
{
    ${displayValues.map(v => `case ${v} = '${v.toLowerCase()}';`).join('\n    ')}
}`;

    case 'java':
      return `public enum ${enumName} {
    ${displayValues.join(', ')};
}`;

    case 'python':
      return `class ${enumName}(str, Enum):
    ${displayValues.map(v => `${v.toUpperCase()} = "${v.toLowerCase()}"`).join('\n    ')}`;

    case 'go':
      return `type ${enumName} string

const (
\t${displayValues.map((v, i) => `${enumName}${cap(v.toLowerCase())}${i === 0 ? ' ' + enumName + ' = "' + v.toLowerCase() + '"' : ' = "' + v.toLowerCase() + '"'}`).join('\n\t')}
)`;

    case 'rust':
      return `#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ${enumName} {
    ${displayValues.join(',\n    ')},
}`;

    default:
      return `// ${enumName} enum values: ${displayValues.join(', ')}`;
  }
}

// ---------------------------------------------------------------------------

/**
 * Generate the route-conventions skill.
 * @param {string} sub
 * @param {string} stackId
 * @param {Object} routesPattern
 * @param {string} role
 * @param {Object} registryEntities
 * @returns {{ skillMd: string, examplesMd: string }}
 */
function genRouteConventionsSkill(sub, stackId, routesPattern, role, registryEntities) {
  const iso = isoNow();
  const lang = stackLang(stackId);

  const groupPrefix = routesPattern.groupPrefix || '/api/{entity}';
  const namingPattern = routesPattern.namingPattern || null;
  const authPattern = routesPattern.authPattern || null;
  const versioningStrategy = routesPattern.versioningStrategy || 'none';

  const rep = pickRepresentativeEntity(registryEntities);
  const repName = rep?.name || 'Contract';
  const repInfo = rep?.info || {};
  const repPrefix = repInfo.routePrefix || `/${repName.toLowerCase()}s`;

  const exampleCode = buildRouteExample(stackId, repName, repPrefix, routesPattern);

  const skillMd = `---
name: ${sub}-route-conventions
description: "Pattern for route/endpoint naming and structure in this project.
  Use when creating routes, endpoints, controllers, API actions,
  or the user says 'new route', 'add API', 'create endpoint'.
  Even if the user just says 'expose X via API'."
---
<!-- mustard:generated at:${iso} role:${role} -->

# Route Conventions

Route/endpoint naming patterns detected in this project.

## Pattern

- Group prefix: \`${groupPrefix}\`
${namingPattern ? `- Endpoint naming: \`${namingPattern}\`\n` : ''}\
${authPattern ? `- Auth: \`${authPattern}\`\n` : ''}\
- Versioning: ${versioningStrategy}
- Standard CRUD: GET (list), GET /{id} (single), POST (create), PUT /{id} (update), DELETE /{id} (delete)

## Example

\`\`\`${lang}
${exampleCode}
\`\`\`

Ref: \`${repInfo.file || '(route file)'}\`

## References

For full code examples with variants:
> Read \`references/examples.md\`
`;

  const examplesMd = `<!-- mustard:generated at:${iso} role:${role} -->

# Examples: ${sub}-route-conventions

## Example 1 — CRUD routes for ${repName}

\`\`\`${lang}
${exampleCode}
\`\`\`

## Example 2 — Route with auth

\`\`\`${lang}
${buildRouteExample(stackId, repName, repPrefix, routesPattern, true)}
\`\`\`
`;

  return { skillMd, examplesMd };
}

/**
 * Build a route code example.
 * @param {string} stackId
 * @param {string} entityName
 * @param {string} prefix
 * @param {Object} pattern
 * @param {boolean} [withAuth=false]
 * @returns {string}
 */
function buildRouteExample(stackId, entityName, prefix, pattern, withAuth = false) {
  const authLine = withAuth && pattern.authPattern ? pattern.authPattern : null;
  const lower = entityName.charAt(0).toLowerCase() + entityName.slice(1);

  switch (stackId) {
    case 'dotnet': {
      const authAttr = authLine ? `\n        .RequireAuthorization("${authLine.replace('{entity}', lower).replace('{action}', 'read')}")` : '';
      return `var group = app.MapGroup("${prefix}");

group.MapGet("/", ${entityName}EndPoints.GetAllAsync)
     .WithName("${lower}_list")${authAttr};

group.MapGet("/{id}", ${entityName}EndPoints.GetByIdAsync)
     .WithName("${lower}_get")${authAttr};

group.MapPost("/", ${entityName}EndPoints.CreateAsync)
     .WithName("${lower}_create");

group.MapPut("/{id}", ${entityName}EndPoints.UpdateAsync)
     .WithName("${lower}_update");

group.MapDelete("/{id}", ${entityName}EndPoints.DeleteAsync)
     .WithName("${lower}_delete");`;
    }

    case 'typescript': {
      const authMiddleware = authLine ? `\nrouter.use(authenticate);` : '';
      return `const router = express.Router();
${authMiddleware}
router.get('${prefix}', ${lower}Controller.getAll);
router.get('${prefix}/:id', ${lower}Controller.getById);
router.post('${prefix}', ${lower}Controller.create);
router.put('${prefix}/:id', ${lower}Controller.update);
router.delete('${prefix}/:id', ${lower}Controller.delete);`;
    }

    case 'php': {
      return `Route::prefix('${prefix}')->group(function () {
    Route::get('/', [${entityName}Controller::class, 'index']);
    Route::post('/', [${entityName}Controller::class, 'store']);
    Route::get('/{id}', [${entityName}Controller::class, 'show']);
    Route::put('/{id}', [${entityName}Controller::class, 'update']);
    Route::delete('/{id}', [${entityName}Controller::class, 'destroy']);
});`;
    }

    case 'java': {
      return `@RestController
@RequestMapping("${prefix}")
public class ${entityName}Controller {
    @GetMapping public ResponseEntity<List<${entityName}Response>> getAll() { ... }
    @GetMapping("/{id}") public ResponseEntity<${entityName}Response> getById(@PathVariable UUID id) { ... }
    @PostMapping public ResponseEntity<${entityName}Response> create(@RequestBody @Valid ${entityName}Request req) { ... }
    @PutMapping("/{id}") public ResponseEntity<${entityName}Response> update(@PathVariable UUID id, @RequestBody @Valid ${entityName}Request req) { ... }
    @DeleteMapping("/{id}") public ResponseEntity<Void> delete(@PathVariable UUID id) { ... }
}`;
    }

    case 'python': {
      return `router = APIRouter(prefix="${prefix}", tags=["${lower}"])

@router.get("/", response_model=list[${entityName}Response])
async def get_all(db: AsyncSession = Depends(get_db)): ...

@router.get("/{id}", response_model=${entityName}Response)
async def get_by_id(id: UUID, db: AsyncSession = Depends(get_db)): ...

@router.post("/", response_model=${entityName}Response, status_code=201)
async def create(data: ${entityName}Create, db: AsyncSession = Depends(get_db)): ...`;
    }

    case 'go': {
      return `func (h *${entityName}Handler) RegisterRoutes(r chi.Router) {
\tr.Get("${prefix}", h.GetAll)
\tr.Get("${prefix}/{id}", h.GetByID)
\tr.Post("${prefix}", h.Create)
\tr.Put("${prefix}/{id}", h.Update)
\tr.Delete("${prefix}/{id}", h.Delete)
}`;
    }

    case 'rust': {
      return `pub fn ${lower}_routes() -> Router {
    Router::new()
        .route("${prefix}", get(get_all_${lower}s).post(create_${lower}))
        .route("${prefix}/:id", get(get_${lower}).put(update_${lower}).delete(delete_${lower}))
}`;
    }

    default:
      return `// ${entityName} routes at ${prefix}`;
  }
}

// ---------------------------------------------------------------------------

/**
 * Generate the service-pattern skill.
 * @param {string} sub
 * @param {string} stackId
 * @param {Object} servicePattern
 * @param {string} role
 * @param {Object} registryEntities
 * @returns {{ skillMd: string, examplesMd: string }}
 */
function genServicePatternSkill(sub, stackId, servicePattern, role, registryEntities) {
  const iso = isoNow();
  const lang = stackLang(stackId);
  const label = stackLabel(stackId);

  const interfaceFirst = servicePattern.interfaceFirst === true;
  const baseInterface = servicePattern.baseInterface || null;

  const rep = pickRepresentativeEntity(registryEntities);
  const repName = rep?.name || 'Contract';
  const exampleCode = buildServiceExample(stackId, repName, servicePattern);

  const skillMd = `---
name: ${sub}-service-pattern
description: "Pattern for service classes and interfaces in this project.
  Use when creating a new service, implementing business logic, adding use cases,
  or the user says 'add service', 'new use case', 'implement business logic'.
  Even if the user says 'I need logic for X'."
---
<!-- mustard:generated at:${iso} role:${role} -->

# Service Pattern

${label} service pattern detected in this project.

## Pattern

- Interface-first: ${interfaceFirst ? 'YES — always define I{Entity}Service interface first' : 'NO — concrete class only'}
${baseInterface ? `- Base interface: \`${baseInterface}\`\n` : ''}\
- Naming: \`I{Entity}Service\` (interface), \`{Entity}Service\` (implementation)
- Registration: injected via constructor (DI)

## Rules

- ${interfaceFirst ? 'ALWAYS create the interface before the implementation' : 'Create the service class directly'}
- ${interfaceFirst ? 'Register both interface and implementation in DI container' : 'Register service in DI container'}
- Service must NOT access the database directly — go through repositories

## Example

\`\`\`${lang}
${exampleCode}
\`\`\`

## References

For full code examples with variants:
> Read \`references/examples.md\`
`;

  const examplesMd = `<!-- mustard:generated at:${iso} role:${role} -->

# Examples: ${sub}-service-pattern

## Example 1 — Basic service

\`\`\`${lang}
${exampleCode}
\`\`\`

## Example 2 — With dependency injection

\`\`\`${lang}
${buildServiceExample(stackId, repName, servicePattern, true)}
\`\`\`
`;

  return { skillMd, examplesMd };
}

/**
 * Build a service code example.
 * @param {string} stackId
 * @param {string} entityName
 * @param {Object} pattern
 * @param {boolean} [withDeps=false]
 * @returns {string}
 */
function buildServiceExample(stackId, entityName, pattern, withDeps = false) {
  const ifaceFirst = pattern.interfaceFirst === true;
  const base = pattern.baseInterface;

  switch (stackId) {
    case 'dotnet': {
      const extendsStr = base ? ` : ${base.replace(/</g, '<' + entityName + ', ').replace(/>/, '>')}` : '';
      const iface = `public interface I${entityName}Service${extendsStr}
{
    Task<IEnumerable<${entityName}ResponseDto>> GetAllAsync();
    Task<${entityName}ResponseDto> GetByIdAsync(Guid id);
    Task<${entityName}ResponseDto> CreateAsync(${entityName}UpSertDto dto);
}`;
      const impl = `public class ${entityName}Service : I${entityName}Service
{
    private readonly I${entityName}Repository _repo;
    ${withDeps ? 'private readonly IMapper _mapper;' : ''}

    public ${entityName}Service(I${entityName}Repository repo${withDeps ? ', IMapper mapper' : ''})
    {
        _repo = repo;
        ${withDeps ? '_mapper = mapper;' : ''}
    }

    public async Task<IEnumerable<${entityName}ResponseDto>> GetAllAsync()
        => (await _repo.GetAllAsync()).Adapt<IEnumerable<${entityName}ResponseDto>>();
}`;
      return ifaceFirst ? iface + '\n\n' + impl : impl;
    }

    case 'typescript': {
      const ifaceCode = `export interface I${entityName}Service {
  getAll(): Promise<${entityName}[]>;
  getById(id: string): Promise<${entityName} | null>;
  create(data: Create${entityName}Dto): Promise<${entityName}>;
}`;
      const implCode = `@Injectable()
export class ${entityName}Service implements I${entityName}Service {
  constructor(
    private readonly ${entityName.charAt(0).toLowerCase() + entityName.slice(1)}Repo: I${entityName}Repository,
  ) {}

  async getAll(): Promise<${entityName}[]> {
    return this.${entityName.charAt(0).toLowerCase() + entityName.slice(1)}Repo.findAll();
  }
}`;
      return ifaceFirst ? ifaceCode + '\n\n' + implCode : implCode;
    }

    case 'php': {
      return `class ${entityName}Service
{
    public function __construct(
        private readonly ${entityName}Repository $repository
    ) {}

    public function getAll(): Collection
    {
        return $this->repository->all();
    }

    public function create(array $data): ${entityName}
    {
        return $this->repository->create($data);
    }
}`;
    }

    case 'java': {
      const iface = `public interface ${entityName}Service {
    List<${entityName}Response> findAll();
    ${entityName}Response findById(UUID id);
    ${entityName}Response create(${entityName}Request request);
}`;
      const impl = `@Service
@RequiredArgsConstructor
public class ${entityName}ServiceImpl implements ${entityName}Service {
    private final ${entityName}Repository repository;

    @Override
    public List<${entityName}Response> findAll() {
        return repository.findAll().stream()
            .map(${entityName}Response::from)
            .toList();
    }
}`;
      return ifaceFirst ? iface + '\n\n' + impl : impl;
    }

    case 'python': {
      return `class ${entityName}Service:
    def __init__(self, repo: ${entityName}Repository):
        self._repo = repo

    async def get_all(self) -> list[${entityName}]:
        return await self._repo.find_all()

    async def create(self, data: ${entityName}Create) -> ${entityName}:
        return await self._repo.create(data)`;
    }

    case 'go': {
      return `type ${entityName}Service interface {
\tGetAll(ctx context.Context) ([]${entityName}, error)
\tGetByID(ctx context.Context, id uuid.UUID) (*${entityName}, error)
\tCreate(ctx context.Context, input Create${entityName}Input) (*${entityName}, error)
}

type ${entityName.charAt(0).toLowerCase() + entityName.slice(1)}Service struct {
\trepo ${entityName}Repository
}

func New${entityName}Service(repo ${entityName}Repository) ${entityName}Service {
\treturn &${entityName.charAt(0).toLowerCase() + entityName.slice(1)}Service{repo: repo}
}`;
    }

    case 'rust': {
      return `pub struct ${entityName}Service {
    repo: Arc<dyn ${entityName}Repository>,
}

impl ${entityName}Service {
    pub fn new(repo: Arc<dyn ${entityName}Repository>) -> Self {
        Self { repo }
    }

    pub async fn get_all(&self) -> Result<Vec<${entityName}>, AppError> {
        self.repo.find_all().await
    }
}`;
    }

    default:
      return `// ${entityName}Service — see project conventions`;
  }
}

// ---------------------------------------------------------------------------

/**
 * Generate the repository-pattern skill.
 * @param {string} sub
 * @param {string} stackId
 * @param {Object} repoPattern
 * @param {string} role
 * @param {Object} registryEntities
 * @returns {{ skillMd: string, examplesMd: string }}
 */
function genRepositoryPatternSkill(sub, stackId, repoPattern, role, registryEntities) {
  const iso = isoNow();
  const lang = stackLang(stackId);
  const label = stackLabel(stackId);

  const interfaceFirst = repoPattern.interfaceFirst === true;
  const baseInterface = repoPattern.baseInterface || null;
  const baseClass = repoPattern.baseClass || null;

  const rep = pickRepresentativeEntity(registryEntities);
  const repName = rep?.name || 'Contract';
  const exampleCode = buildRepoExample(stackId, repName, repoPattern);

  const skillMd = `---
name: ${sub}-repository-pattern
description: "Pattern for repository classes in this project.
  Use when creating data access layer, adding a repository, wiring database access,
  or the user says 'add repository', 'new data access', 'query the database'.
  Even if the user just says 'store/fetch X from database'."
---
<!-- mustard:generated at:${iso} role:${role} -->

# Repository Pattern

${label} repository pattern detected in this project.

## Pattern

- Interface-first: ${interfaceFirst ? 'YES' : 'NO'}
${baseInterface ? `- Base interface: \`${baseInterface}\`\n` : ''}\
${baseClass ? `- Base class: \`${baseClass}\`\n` : ''}\
- Naming: \`I{Entity}Repository\` (interface), \`{Entity}Repository\` (implementation)

## Rules

- ${interfaceFirst ? 'ALWAYS define the interface before the implementation' : 'Create the repository class directly'}
- Repositories handle ONLY data persistence — no business logic
- ${baseClass ? 'Extend ' + baseClass + ' for the base CRUD methods' : 'Implement all CRUD methods explicitly'}

## Example

\`\`\`${lang}
${exampleCode}
\`\`\`

## References

For full code examples with variants:
> Read \`references/examples.md\`
`;

  const examplesMd = `<!-- mustard:generated at:${iso} role:${role} -->

# Examples: ${sub}-repository-pattern

## Example 1 — Basic repository

\`\`\`${lang}
${exampleCode}
\`\`\`
`;

  return { skillMd, examplesMd };
}

/**
 * Build a repository code example.
 * @param {string} stackId
 * @param {string} entityName
 * @param {Object} pattern
 * @returns {string}
 */
function buildRepoExample(stackId, entityName, pattern) {
  const ifaceFirst = pattern.interfaceFirst === true;
  const base = pattern.baseInterface;
  const baseClass = pattern.baseClass;

  switch (stackId) {
    case 'dotnet': {
      const baseExt = base ? ` : ${base.replace(/<.*>/, '')}<${entityName}>` : '';
      const iface = `public interface I${entityName}Repository${baseExt}
{
    Task<IEnumerable<${entityName}>> GetAllAsync();
    Task<${entityName}?> GetByIdAsync(Guid id);
    Task<${entityName}> CreateAsync(${entityName} entity);
    Task UpdateAsync(${entityName} entity);
    Task DeleteAsync(Guid id);
}`;
      const implBase = baseClass ? ` : ${baseClass}<${entityName}>` : ` : I${entityName}Repository`;
      const impl = `public class ${entityName}Repository${implBase}
{
    private readonly AppDbContext _context;

    public ${entityName}Repository(AppDbContext context)
    {
        _context = context;
    }

    public async Task<IEnumerable<${entityName}>> GetAllAsync()
        => await _context.${entityName}s.ToListAsync();
}`;
      return ifaceFirst ? iface + '\n\n' + impl : impl;
    }

    case 'typescript': {
      return `export interface I${entityName}Repository {
  findAll(): Promise<${entityName}[]>;
  findById(id: string): Promise<${entityName} | null>;
  create(data: Partial<${entityName}>): Promise<${entityName}>;
  update(id: string, data: Partial<${entityName}>): Promise<${entityName}>;
  delete(id: string): Promise<void>;
}

export class ${entityName}Repository implements I${entityName}Repository {
  constructor(private readonly db: DatabaseClient) {}

  async findAll(): Promise<${entityName}[]> {
    return this.db.${entityName.toLowerCase()}.findMany();
  }
}`;
    }

    case 'php': {
      return `class ${entityName}Repository
{
    public function all(): Collection
    {
        return ${entityName}::all();
    }

    public function findById(string $id): ?${entityName}
    {
        return ${entityName}::find($id);
    }

    public function create(array $data): ${entityName}
    {
        return ${entityName}::create($data);
    }
}`;
    }

    case 'java': {
      return `public interface ${entityName}Repository extends JpaRepository<${entityName}, UUID> {
    List<${entityName}> findByStatus(${entityName}Status status);
}`;
    }

    case 'python': {
      return `class ${entityName}Repository:
    def __init__(self, db: AsyncSession):
        self._db = db

    async def find_all(self) -> list[${entityName}]:
        result = await self._db.execute(select(${entityName}))
        return result.scalars().all()

    async def create(self, data: ${entityName}Create) -> ${entityName}:
        entity = ${entityName}(**data.model_dump())
        self._db.add(entity)
        await self._db.commit()
        return entity`;
    }

    case 'go': {
      return `type ${entityName}Repository interface {
\tFindAll(ctx context.Context) ([]${entityName}, error)
\tFindByID(ctx context.Context, id uuid.UUID) (*${entityName}, error)
\tCreate(ctx context.Context, entity *${entityName}) error
\tUpdate(ctx context.Context, entity *${entityName}) error
\tDelete(ctx context.Context, id uuid.UUID) error
}`;
    }

    case 'rust': {
      return `#[async_trait]
pub trait ${entityName}Repository: Send + Sync {
    async fn find_all(&self) -> Result<Vec<${entityName}>, AppError>;
    async fn find_by_id(&self, id: Uuid) -> Result<Option<${entityName}>, AppError>;
    async fn create(&self, input: Create${entityName}Input) -> Result<${entityName}, AppError>;
}`;
    }

    default:
      return `// ${entityName}Repository — see project conventions`;
  }
}

// ---------------------------------------------------------------------------

/**
 * Generate the DTO-conventions skill.
 * @param {string} sub
 * @param {string} stackId
 * @param {Object} dtoPattern
 * @param {string} role
 * @param {Object} registryEntities
 * @returns {{ skillMd: string, examplesMd: string }}
 */
function genDtoConventionsSkill(sub, stackId, dtoPattern, role, registryEntities) {
  const iso = isoNow();
  const lang = stackLang(stackId);
  const label = stackLabel(stackId);

  const folder = dtoPattern.folder || '(project DTOs folder)';
  const namingPatterns = (dtoPattern.namingPatterns || []).filter(Boolean);
  const validationPattern = dtoPattern.validationPattern || null;

  const rep = pickRepresentativeEntity(registryEntities);
  const repName = rep?.name || 'Contract';

  const exampleCode = buildDtoExample(stackId, repName, dtoPattern);

  const skillMd = `---
name: ${sub}-dto-conventions
description: "Pattern for DTOs, request/response objects and validation in this project.
  Use when creating DTOs, request bodies, API payloads, response models,
  or the user says 'add DTO', 'request model', 'response type', 'input validation'.
  Even if the user just says 'what shape should the API body be'."
---
<!-- mustard:generated at:${iso} role:${role} -->

# DTO Conventions

${label} DTO/schema pattern detected in this project.

## Pattern

- Folder: \`${folder}\`
${namingPatterns.length ? `- Naming suffixes: ${namingPatterns.map(p => '`' + p + '`').join(', ')}\n` : ''}\
${validationPattern ? `- Validation: \`${validationPattern}\`\n` : ''}\
- Standard types per entity: \`{Entity}UpSertDto\` (create/update), \`{Entity}ResponseDto\` (read)

## Rules

- Keep DTOs separate from entity classes
- ${validationPattern ? 'Use ' + validationPattern + ' for all input validation' : 'Validate all input fields'}
- Response DTOs omit sensitive fields (passwords, internal IDs where applicable)

## Example

\`\`\`${lang}
${exampleCode}
\`\`\`

## References

For full code examples with variants:
> Read \`references/examples.md\`
`;

  const examplesMd = `<!-- mustard:generated at:${iso} role:${role} -->

# Examples: ${sub}-dto-conventions

## Example 1 — Create/Update DTO

\`\`\`${lang}
${exampleCode}
\`\`\`

## Example 2 — Response DTO

\`\`\`${lang}
${buildDtoExample(stackId, repName, dtoPattern, 'response')}
\`\`\`
`;

  return { skillMd, examplesMd };
}

/**
 * Build a DTO code example.
 * @param {string} stackId
 * @param {string} entityName
 * @param {Object} pattern
 * @param {string} [variant='upsert']
 * @returns {string}
 */
function buildDtoExample(stackId, entityName, pattern, variant = 'upsert') {
  const validationPat = pattern.validationPattern;
  const isResponse = variant === 'response';
  const suffix = isResponse ? 'ResponseDto' : 'UpSertDto';
  const dtoName = entityName + suffix;

  switch (stackId) {
    case 'dotnet': {
      if (isResponse) {
        return `public record ${dtoName}(
    Guid Id,
    string Name,
    DateTime CreatedAt
);`;
      }
      if (validationPat === 'FluentValidation') {
        return `public record ${dtoName}(
    string Name,
    string? Description
);

public class ${entityName}UpSertDtoValidator : AbstractValidator<${dtoName}>
{
    public ${entityName}UpSertDtoValidator()
    {
        RuleFor(x => x.Name).NotEmpty().MaximumLength(200);
    }
}`;
      }
      return `public record ${dtoName}(
    [Required] string Name,
    string? Description
);`;
    }

    case 'typescript': {
      if (isResponse) {
        return `export interface ${entityName}Response {
  id: string;
  name: string;
  createdAt: string;
}`;
      }
      if (validationPat === 'Zod') {
        return `export const ${entityName.charAt(0).toLowerCase() + entityName.slice(1)}Schema = z.object({
  name: z.string().min(1).max(200),
  description: z.string().optional(),
});

export type Create${entityName}Dto = z.infer<typeof ${entityName.charAt(0).toLowerCase() + entityName.slice(1)}Schema>;`;
      }
      return `export interface Create${entityName}Dto {
  name: string;
  description?: string;
}`;
    }

    case 'php': {
      return isResponse
        ? `class ${entityName}Resource extends JsonResource
{
    public function toArray(Request $request): array
    {
        return [
            'id'   => $this->id,
            'name' => $this->name,
        ];
    }
}`
        : `class ${dtoName} extends FormRequest
{
    public function rules(): array
    {
        return [
            'name'        => 'required|string|max:200',
            'description' => 'nullable|string',
        ];
    }
}`;
    }

    case 'java': {
      return isResponse
        ? `public record ${entityName}Response(UUID id, String name, LocalDateTime createdAt) {
    public static ${entityName}Response from(${entityName} entity) {
        return new ${entityName}Response(entity.getId(), entity.getName(), entity.getCreatedAt());
    }
}`
        : `public record ${entityName}Request(
    @NotBlank @Size(max = 200) String name,
    String description
) {}`;
    }

    case 'python': {
      return isResponse
        ? `class ${entityName}Response(BaseModel):
    id: UUID
    name: str
    created_at: datetime

    model_config = ConfigDict(from_attributes=True)`
        : `class ${entityName}Create(BaseModel):
    name: str = Field(..., min_length=1, max_length=200)
    description: Optional[str] = None`;
    }

    case 'go': {
      return isResponse
        ? `type ${entityName}Response struct {
\tID        uuid.UUID \`json:"id"\`
\tName      string    \`json:"name"\`
\tCreatedAt time.Time \`json:"created_at"\`
}`
        : `type Create${entityName}Request struct {
\tName        string  \`json:"name" validate:"required,max=200"\`
\tDescription *string \`json:"description,omitempty"\`
}`;
    }

    case 'rust': {
      return isResponse
        ? `#[derive(Serialize)]
pub struct ${entityName}Response {
    pub id: Uuid,
    pub name: String,
    pub created_at: DateTime<Utc>,
}`
        : `#[derive(Deserialize, Validate)]
pub struct Create${entityName}Request {
    #[validate(length(min = 1, max = 200))]
    pub name: String,
    pub description: Option<String>,
}`;
    }

    default:
      return `// ${dtoName} — see project conventions`;
  }
}

// ---------------------------------------------------------------------------

/**
 * Generate the module-registration skill (dotnet-specific).
 * @param {string} sub
 * @param {string} stackId
 * @param {Object} modulePattern
 * @param {string} role
 * @returns {{ skillMd: string, examplesMd: string }}
 */
function genModuleRegistrationSkill(sub, stackId, modulePattern, role) {
  const iso = isoNow();
  const lang = stackLang(stackId);

  const pattern = modulePattern.pattern || '{Entity}Module : IModule';
  const regMethod = modulePattern.registrationMethod || 'RegisterModule';
  const diPattern = modulePattern.diPattern || 'AddScoped';

  const skillMd = `---
name: ${sub}-module-registration
description: "Pattern for module/DI registration in this project.
  Use when creating a new domain module, wiring services, adding DI registrations,
  or the user says 'add module', 'register service', 'wire dependency'.
  Even if the user says 'I need a new feature module'."
---
<!-- mustard:generated at:${iso} role:${role} -->

# Module Registration

Module pattern detected in this project.

## Pattern

- Module class: \`${pattern}\`
- Registration method: \`${regMethod}\`
- DI scope: \`${diPattern}\`

## Rules

- Every new domain module MUST implement IModule
- Services registered via \`${regMethod}\` on the module class
- Use \`${diPattern}\` for services; use \`AddSingleton\` only for stateless singletons

## Example

\`\`\`${lang}
${buildModuleExample(stackId, 'Contract', modulePattern)}
\`\`\`

## References

For full code examples with variants:
> Read \`references/examples.md\`
`;

  const examplesMd = `<!-- mustard:generated at:${iso} role:${role} -->

# Examples: ${sub}-module-registration

## Example 1 — Domain module

\`\`\`${lang}
${buildModuleExample(stackId, 'Contract', modulePattern)}
\`\`\`

## Example 2 — With additional services

\`\`\`${lang}
${buildModuleExample(stackId, 'Invoice', modulePattern)}
\`\`\`
`;

  return { skillMd, examplesMd };
}

/**
 * Build a module registration example.
 * @param {string} stackId
 * @param {string} entityName
 * @param {Object} pattern
 * @returns {string}
 */
function buildModuleExample(stackId, entityName, pattern) {
  const regMethod = pattern.registrationMethod || 'RegisterModule';
  const diPat = pattern.diPattern || 'AddScoped';

  if (stackId !== 'dotnet') return `// Module registration for ${entityName} — see project conventions`;

  return `public class ${entityName}Module : IModule
{
    public IServiceCollection ${regMethod}(IServiceCollection services)
    {
        services.${diPat}<I${entityName}Service, ${entityName}Service>();
        services.${diPat}<I${entityName}Repository, ${entityName}Repository>();
        return services;
    }

    public IEndpointRouteBuilder MapEndpoints(IEndpointRouteBuilder endpoints)
    {
        endpoints.MapGroup("/v1/${entityName.toLowerCase()}s")
            .MapGet("/", ${entityName}EndPoints.GetAllAsync)
            .MapPost("/", ${entityName}EndPoints.CreateAsync);
        return endpoints;
    }
}`;
}

// ---------------------------------------------------------------------------
// Stack-specific: Dart extras
// ---------------------------------------------------------------------------

/**
 * Generate the state-management skill (Dart/Flutter).
 * @param {string} sub
 * @param {Object} dartPatterns - full _patterns.dart
 * @param {string} role
 * @param {Object} registryEntities
 * @returns {{ skillMd: string, examplesMd: string }}
 */
function genStateManagementSkill(sub, dartPatterns, role, registryEntities) {
  const iso = isoNow();
  const sm = dartPatterns.stateManagement || {};
  const framework = sm.framework || 'Riverpod';
  const pattern = sm.pattern || 'AsyncNotifier';
  const fileConvention = sm.fileConvention || '{feature}_provider.dart';

  const rep = pickRepresentativeEntity(registryEntities);
  const repName = rep?.name || 'Contract';
  const lower = repName.charAt(0).toLowerCase() + repName.slice(1);

  const skillMd = `---
name: ${sub}-state-management
description: "Pattern for state management in this Flutter project.
  Use when adding state, creating providers/blocs, managing UI state,
  or the user says 'add provider', 'new bloc', 'manage state', 'state for X'.
  Even if the user just says 'I need state for the ${repName} screen'."
---
<!-- mustard:generated at:${iso} role:${role} -->

# State Management

${framework} state management pattern detected in this project.

## Pattern

- Framework: ${framework}
- Pattern: ${pattern}
- File naming: \`${fileConvention}\`

## Rules

- ALWAYS use ${framework} for state — no setState outside of local ephemeral UI
- One provider/notifier per feature/entity
- Async operations return AsyncValue — handle loading/error states in UI

## Example

\`\`\`dart
${buildStateExample(framework, repName, lower)}
\`\`\`

## References

For full code examples with variants:
> Read \`references/examples.md\`
`;

  const examplesMd = `<!-- mustard:generated at:${iso} role:${role} -->

# Examples: ${sub}-state-management

## Example 1 — ${framework} provider for ${repName}

\`\`\`dart
${buildStateExample(framework, repName, lower)}
\`\`\`
`;

  return { skillMd, examplesMd };
}

/**
 * Build a state management example for Dart.
 * @param {string} framework
 * @param {string} entityName
 * @param {string} lower
 * @returns {string}
 */
function buildStateExample(framework, entityName, lower) {
  switch (framework) {
    case 'Riverpod':
      return `@riverpod
class ${entityName}Notifier extends _\$${entityName}Notifier {
  @override
  Future<List<${entityName}>> build() async {
    return ref.watch(${lower}RepositoryProvider).getAll();
  }

  Future<void> create(Create${entityName}Input input) async {
    state = const AsyncLoading();
    state = await AsyncValue.guard(() async {
      await ref.read(${lower}RepositoryProvider).create(input);
      return ref.read(${lower}RepositoryProvider).getAll();
    });
  }
}`;

    case 'BLoC':
      return `class ${entityName}Bloc extends Bloc<${entityName}Event, ${entityName}State> {
  final ${entityName}Repository _repo;

  ${entityName}Bloc(this._repo) : super(${entityName}Initial()) {
    on<Load${entityName}s>(_onLoad);
    on<Create${entityName}>(_onCreate);
  }

  Future<void> _onLoad(Load${entityName}s event, Emitter<${entityName}State> emit) async {
    emit(${entityName}Loading());
    try {
      final items = await _repo.getAll();
      emit(${entityName}Loaded(items));
    } catch (e) {
      emit(${entityName}Error(e.toString()));
    }
  }
}`;

    default:
      return `// State management for ${entityName} — uses ${framework}`;
  }
}

// ---------------------------------------------------------------------------

/**
 * Generate the navigation-pattern skill (Dart/Flutter).
 * @param {string} sub
 * @param {Object} dartPatterns
 * @param {string} role
 * @returns {{ skillMd: string, examplesMd: string }}
 */
function genNavigationPatternSkill(sub, dartPatterns, role) {
  const iso = isoNow();
  const nav = dartPatterns.navigation || {};
  const framework = nav.framework || 'GoRouter';
  const routeFileConvention = nav.fileConvention || 'router.dart';

  const skillMd = `---
name: ${sub}-navigation-pattern
description: "Pattern for navigation/routing in this Flutter project.
  Use when adding a new screen, route, deep link, navigation action,
  or the user says 'add screen', 'new route', 'navigate to X'.
  Even if the user just says 'I need a page for X'."
---
<!-- mustard:generated at:${iso} role:${role} -->

# Navigation Pattern

${framework} navigation pattern detected in this project.

## Pattern

- Framework: ${framework}
- Router file: \`${routeFileConvention}\`
- Route naming: lowercase-kebab-case paths

## Rules

- ALWAYS define routes in the central router file
- Use named routes — never push raw MaterialPageRoute
- Pass data via route params, not shared state

## Example

\`\`\`dart
${buildNavigationExample(framework)}
\`\`\`

## References

For full code examples with variants:
> Read \`references/examples.md\`
`;

  const examplesMd = `<!-- mustard:generated at:${iso} role:${role} -->

# Examples: ${sub}-navigation-pattern

## Example 1 — Route definition

\`\`\`dart
${buildNavigationExample(framework)}
\`\`\`
`;

  return { skillMd, examplesMd };
}

/**
 * Build a navigation example for Dart.
 * @param {string} framework
 * @returns {string}
 */
function buildNavigationExample(framework) {
  switch (framework) {
    case 'GoRouter':
      return `final router = GoRouter(
  routes: [
    GoRoute(
      path: '/contracts',
      builder: (context, state) => const ContractListPage(),
    ),
    GoRoute(
      path: '/contracts/:id',
      builder: (context, state) {
        final id = state.pathParameters['id']!;
        return ContractDetailPage(id: id);
      },
    ),
  ],
);`;

    case 'AutoRoute':
      return `@AutoRouterConfig()
class AppRouter extends \$AppRouter {
  @override
  List<AutoRoute> get routes => [
    AutoRoute(page: ContractListRoute.page, path: '/contracts'),
    AutoRoute(page: ContractDetailRoute.page, path: '/contracts/:id'),
  ];
}`;

    default:
      return `// Navigation for ${framework} — see router.dart`;
  }
}

// ---------------------------------------------------------------------------
// Main generation logic
// ---------------------------------------------------------------------------

/**
 * Determine if a pattern has enough data to generate a useful skill.
 * @param {Object|null|undefined} pattern
 * @param {string[]} requiredFields - at least one must be non-null
 * @returns {boolean}
 */
function patternHasData(pattern, requiredFields) {
  if (!pattern || typeof pattern !== 'object') return false;
  return requiredFields.some(f => {
    const v = pattern[f];
    if (v === null || v === undefined || v === '') return false;
    if (Array.isArray(v)) return v.length > 0;
    return true;
  });
}

/**
 * Generate all skills for a given stack and its subproject(s).
 *
 * @param {string} stackId
 * @param {Object} stackPatterns - _patterns[stackId]
 * @param {Array<{name: string, path: string, role: string, agent: string}>} subprojects
 * @param {Object} registry - full registry
 * @returns {Array<{filePath: string, content: string}>}
 */
function generateSkillsForStack(stackId, stackPatterns, subprojects, registry) {
  if (!subprojects.length) return [];

  const files = [];
  const registryEntities = registry.e || {};
  const registryEnums = registry._enums || {};

  // Use the first matching subproject as the primary target for skill output
  // (skills are generated into each subproject's .claude/skills/)
  for (const sub of subprojects) {
    const subPath = path.join(ROOT, sub.path);
    const skillsDir = path.join(subPath, '.claude', 'skills');
    const role = sub.role || 'general';
    // Use agent role as skill prefix (e.g., "api-", "app-") instead of full subproject name
    const subName = sub.agent || role;

    // 1. entity-creation
    if (patternHasData(stackPatterns.entity, ['folder', 'baseClass', 'interfaces', 'namespacePattern'])) {
      const { skillMd, examplesMd } = genEntityCreationSkill(
        subName, stackId, stackPatterns.entity,
        stackPatterns.enum || null, role, registryEntities
      );
      files.push({ filePath: path.join(skillsDir, `${subName}-entity-creation`, 'SKILL.md'), content: skillMd });
      files.push({ filePath: path.join(skillsDir, `${subName}-entity-creation`, 'references', 'examples.md'), content: examplesMd });
    }

    // 2. enum-placement (only if enum pattern with specific folder or separateFiles)
    if (patternHasData(stackPatterns.enum, ['folder', 'valueConvention']) &&
        (stackPatterns.enum.separateFiles || stackPatterns.enum.folder)) {
      const { skillMd, examplesMd } = genEnumPlacementSkill(
        subName, stackId, stackPatterns.enum, role, registryEnums
      );
      files.push({ filePath: path.join(skillsDir, `${subName}-enum-placement`, 'SKILL.md'), content: skillMd });
      files.push({ filePath: path.join(skillsDir, `${subName}-enum-placement`, 'references', 'examples.md'), content: examplesMd });
    }

    // 3. route-conventions
    if (patternHasData(stackPatterns.routes, ['groupPrefix', 'namingPattern', 'authPattern', 'versioningStrategy'])) {
      const { skillMd, examplesMd } = genRouteConventionsSkill(
        subName, stackId, stackPatterns.routes, role, registryEntities
      );
      files.push({ filePath: path.join(skillsDir, `${subName}-route-conventions`, 'SKILL.md'), content: skillMd });
      files.push({ filePath: path.join(skillsDir, `${subName}-route-conventions`, 'references', 'examples.md'), content: examplesMd });
    }

    // 4. service-pattern (only if interfaceFirst is detected)
    if (patternHasData(stackPatterns.service, ['interfaceFirst', 'baseInterface']) &&
        stackPatterns.service.interfaceFirst === true) {
      const { skillMd, examplesMd } = genServicePatternSkill(
        subName, stackId, stackPatterns.service, role, registryEntities
      );
      files.push({ filePath: path.join(skillsDir, `${subName}-service-pattern`, 'SKILL.md'), content: skillMd });
      files.push({ filePath: path.join(skillsDir, `${subName}-service-pattern`, 'references', 'examples.md'), content: examplesMd });
    }

    // 5. repository-pattern
    if (patternHasData(stackPatterns.repository, ['interfaceFirst', 'baseInterface', 'baseClass'])) {
      const { skillMd, examplesMd } = genRepositoryPatternSkill(
        subName, stackId, stackPatterns.repository, role, registryEntities
      );
      files.push({ filePath: path.join(skillsDir, `${subName}-repository-pattern`, 'SKILL.md'), content: skillMd });
      files.push({ filePath: path.join(skillsDir, `${subName}-repository-pattern`, 'references', 'examples.md'), content: examplesMd });
    }

    // 6. dto-conventions (only if validationPattern is present)
    if (patternHasData(stackPatterns.dto, ['folder', 'validationPattern', 'namingPatterns'])) {
      const { skillMd, examplesMd } = genDtoConventionsSkill(
        subName, stackId, stackPatterns.dto, role, registryEntities
      );
      files.push({ filePath: path.join(skillsDir, `${subName}-dto-conventions`, 'SKILL.md'), content: skillMd });
      files.push({ filePath: path.join(skillsDir, `${subName}-dto-conventions`, 'references', 'examples.md'), content: examplesMd });
    }

    // 7. module-registration (only if module pattern exists)
    if (patternHasData(stackPatterns.module, ['pattern', 'registrationMethod'])) {
      const { skillMd, examplesMd } = genModuleRegistrationSkill(
        subName, stackId, stackPatterns.module, role
      );
      files.push({ filePath: path.join(skillsDir, `${subName}-module-registration`, 'SKILL.md'), content: skillMd });
      files.push({ filePath: path.join(skillsDir, `${subName}-module-registration`, 'references', 'examples.md'), content: examplesMd });
    }

    // ---- Stack-specific extras ----

    // Dart: state management
    if (stackId === 'dart' && patternHasData(stackPatterns.stateManagement, ['framework'])) {
      const { skillMd, examplesMd } = genStateManagementSkill(
        subName, stackPatterns, role, registryEntities
      );
      files.push({ filePath: path.join(skillsDir, `${subName}-state-management`, 'SKILL.md'), content: skillMd });
      files.push({ filePath: path.join(skillsDir, `${subName}-state-management`, 'references', 'examples.md'), content: examplesMd });
    }

    // Dart: navigation pattern
    if (stackId === 'dart' && patternHasData(stackPatterns.navigation, ['framework'])) {
      const { skillMd, examplesMd } = genNavigationPatternSkill(
        subName, stackPatterns, role
      );
      files.push({ filePath: path.join(skillsDir, `${subName}-navigation-pattern`, 'SKILL.md'), content: skillMd });
      files.push({ filePath: path.join(skillsDir, `${subName}-navigation-pattern`, 'references', 'examples.md'), content: examplesMd });
    }
  }

  return files;
}

// ---------------------------------------------------------------------------
// main
// ---------------------------------------------------------------------------

function main() {
  // 1. Read registry
  const registry = readJsonSafe(REGISTRY_PATH);
  if (!registry) {
    console.error('Error: entity-registry.json not found at', REGISTRY_PATH);
    console.error('Run: node .claude/scripts/sync-registry.js');
    process.exit(1);
  }

  const version = registry._meta?.version || '?';
  if (version < '4.0') {
    console.warn(`Warning: registry at v${version} — some patterns may be missing. Run sync-registry.js --force to upgrade.`);
  }

  const patterns = registry._patterns || {};
  const patternStacks = Object.keys(patterns);

  if (patternStacks.length === 0) {
    console.log('No patterns in registry. Run sync-registry.js first.');
    process.exit(0);
  }

  console.log(`Registry v${version} — patterns: [${patternStacks.join(', ')}]`);
  console.log(`Entities: ${Object.keys(registry.e || {}).length}, Enums: ${Object.keys(registry._enums || {}).length}`);

  // 2. Build stack → subprojects map from detect cache
  const stackSubMap = buildStackSubprojectMap();

  // 3. For each stack with patterns, generate skills
  const log = [];
  let totalSkills = 0;

  for (const stackId of patternStacks) {
    const stackPatterns = patterns[stackId];
    if (!stackPatterns || Object.keys(stackPatterns).length === 0) continue;

    // Get subprojects for this stack
    let subs = stackSubMap.get(stackId) || [];

    // If no cache match, try to infer from directory names
    if (subs.length === 0) {
      // Try common naming heuristics
      const guessNames = { dotnet: ['api', 'backend', 'server'], typescript: ['frontend', 'ui', 'web', 'app'], dart: ['mobile', 'app'] };
      const guesses = guessNames[stackId] || [];
      for (const guess of guesses) {
        const guessPath = path.join(ROOT, guess);
        if (fs.existsSync(guessPath)) {
          subs.push({ name: guess, path: guess, role: 'general', agent: 'general' });
          break;
        }
      }

      // Last resort: use stackId itself as subproject name (single-subproject monorepo)
      if (subs.length === 0) {
        console.log(`  No subproject found for stack "${stackId}" — skipping`);
        continue;
      }
    }

    // Apply --subproject filter
    if (SUB_FILTER) {
      subs = subs.filter(s => s.name === SUB_FILTER);
      if (subs.length === 0) continue;
    }

    console.log(`\nStack: ${stackId} → subproject(s): ${subs.map(s => s.name).join(', ')}`);

    const files = generateSkillsForStack(stackId, stackPatterns, subs, registry);

    for (const { filePath, content } of files) {
      writeFile(filePath, content, log);
      totalSkills++;
    }
  }

  // 4. Report
  console.log('\n' + log.join('\n'));
  console.log(`\nDone: ${totalSkills} file(s) processed.`);

  if (DRY_RUN) {
    console.log('\n(dry-run — no files written)');
  }
}

// Fail-open: never crash the calling process
try {
  main();
} catch (err) {
  process.stderr.write(`[skill-generator] Fatal error: ${err.message}\n${err.stack}\n`);
  process.exit(0); // fail-open
}
