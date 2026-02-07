#!/usr/bin/env node

/**
 * sync-registry.js
 *
 * Generates .claude/entity-registry.json automatically by scanning
 * subprojects for entity definitions based on their detected technology.
 *
 * Usage:
 *   node .claude/scripts/sync-registry.js          # Only if registry is empty/placeholder
 *   node .claude/scripts/sync-registry.js --force   # Regenerate even if populated
 *
 * Supports:
 *   - Drizzle ORM (pgTable, pgEnum)
 *   - .NET (DbSet<T>, class T : ...)
 */

const fs = require("fs");
const path = require("path");
const { execSync } = require("child_process");

// Root of the monorepo (parent of .claude/scripts/)
const ROOT = path.resolve(__dirname, "..", "..");
const REGISTRY_PATH = path.join(ROOT, ".claude", "entity-registry.json");
const DETECT_SCRIPT = path.join(ROOT, ".claude", "scripts", "sync-detect.js");

// ---------------------------------------------------------------------------
// Pluralization helpers
// ---------------------------------------------------------------------------

/**
 * Lookup for common irregular plurals (snake_case table name -> PascalCase singular).
 */
const IRREGULAR_PLURALS = {
  people: "Person",
  children: "Child",
  men: "Man",
  women: "Woman",
  mice: "Mouse",
  geese: "Goose",
  teeth: "Tooth",
  feet: "Foot",
  data: "Datum",
  indices: "Index",
  matrices: "Matrix",
  vertices: "Vertex",
  analyses: "Analysis",
  bases: "Base",
  crises: "Crisis",
  diagnoses: "Diagnosis",
  hypotheses: "Hypothesis",
  parentheses: "Parenthesis",
  theses: "Thesis",
  criteria: "Criterion",
  phenomena: "Phenomenon",
  media: "Medium",
  statuses: "Status",
  addresses: "Address",
};

/**
 * Convert a snake_case plural table name to PascalCase singular entity name.
 * Examples:
 *   contracts       -> Contract
 *   partner_types   -> PartnerType
 *   people          -> Person
 *   companies       -> Company
 *   product_categories -> ProductCategory
 *   email_queue     -> EmailQueue (already singular)
 */
function snakeToPascalSingular(snakePlural) {
  // Check irregular lookup for the full compound name
  if (IRREGULAR_PLURALS[snakePlural]) {
    return IRREGULAR_PLURALS[snakePlural];
  }

  // Split by underscore, singularize each last segment, PascalCase each part
  const parts = snakePlural.split("_");

  // Only singularize the LAST part (the noun)
  const result = parts.map((part, idx) => {
    const word = idx === parts.length - 1 ? singularize(part) : part;
    return word.charAt(0).toUpperCase() + word.slice(1);
  });

  return result.join("");
}

/**
 * Singularize a single English word (simple heuristics).
 */
function singularize(word) {
  // Check irregular
  if (IRREGULAR_PLURALS[word]) {
    return IRREGULAR_PLURALS[word].toLowerCase();
  }

  // Already singular indicators
  if (
    word.endsWith("ss") ||
    word.endsWith("us") ||
    word.endsWith("is") ||
    word === "queue"
  ) {
    return word;
  }

  // -ies -> -y (companies -> company, categories -> category)
  if (word.endsWith("ies")) {
    return word.slice(0, -3) + "y";
  }

  // -ses -> -s (bases -> base) but NOT all -ses (addresses -> address)
  if (word.endsWith("sses")) {
    return word.slice(0, -2); // addresses -> address
  }

  // -es after sh, ch, x, z, o -> remove -es
  if (word.endsWith("shes") || word.endsWith("ches") || word.endsWith("xes") || word.endsWith("zes")) {
    return word.slice(0, -2);
  }

  // Generic -s removal
  if (word.endsWith("s") && !word.endsWith("ss")) {
    return word.slice(0, -1);
  }

  return word;
}

/**
 * Convert a snake_case enum name to PascalCase.
 * Example: contract_status -> ContractStatus
 */
function snakeToPascal(snakeName) {
  return snakeName
    .split("_")
    .map((part) => part.charAt(0).toUpperCase() + part.slice(1))
    .join("");
}

// ---------------------------------------------------------------------------
// File scanning helpers
// ---------------------------------------------------------------------------

/**
 * Recursively collect files matching a pattern from a directory.
 */
function collectFiles(dir, extension, ignore = []) {
  const results = [];
  try {
    const entries = fs.readdirSync(dir, { withFileTypes: true });
    for (const entry of entries) {
      const fullPath = path.join(dir, entry.name);

      // Skip ignored directories
      if (entry.isDirectory()) {
        if (
          ignore.includes(entry.name) ||
          entry.name.startsWith(".") ||
          entry.name === "node_modules" ||
          entry.name === "bin" ||
          entry.name === "obj" ||
          entry.name === "dist" ||
          entry.name === ".next"
        ) {
          continue;
        }
        results.push(...collectFiles(fullPath, extension, ignore));
      } else if (entry.name.endsWith(extension)) {
        results.push(fullPath);
      }
    }
  } catch {
    // ignore permission errors, etc.
  }
  return results;
}

// ---------------------------------------------------------------------------
// Drizzle scanning (database agent)
// ---------------------------------------------------------------------------

/**
 * Scan Drizzle schema files for pgTable definitions.
 * Returns a map: tableName (snake_case) -> { entityName, file, refs: Set, columns: string[] }
 */
function scanDrizzleTables(subprojectPath) {
  const schemaDir = path.join(subprojectPath, "src", "db", "schema");
  if (!fs.existsSync(schemaDir)) {
    // Fallback: try src/schema
    const altDir = path.join(subprojectPath, "src", "schema");
    if (!fs.existsSync(altDir)) return new Map();
    return _scanDrizzleDir(altDir);
  }
  return _scanDrizzleDir(schemaDir);
}

function _scanDrizzleDir(schemaDir) {
  const tables = new Map();
  const tsFiles = collectFiles(schemaDir, ".ts");

  for (const filePath of tsFiles) {
    const fileName = path.basename(filePath);
    // Skip index.ts, views.ts, enums.ts
    if (fileName === "index.ts" || fileName === "views.ts" || fileName === "enums.ts") {
      continue;
    }

    let content;
    try {
      content = fs.readFileSync(filePath, "utf-8");
    } catch {
      continue;
    }

    // Find all pgTable('table_name', ...) definitions
    const tableRegex = /pgTable\(\s*['"](\w+)['"]/g;
    let match;
    while ((match = tableRegex.exec(content)) !== null) {
      const tableName = match[1]; // e.g., "contracts", "partner_types"
      const entityName = snakeToPascalSingular(tableName);

      // Extract the table definition block (from pgTable( to the matching export)
      // We'll use the full content since one file may have multiple tables
      const tableStartIdx = match.index;

      // Find the columns defined in this table by looking for references
      const refs = new Set();
      let selfRef = false;

      // Count "own" columns (non-FK, non-audit columns)
      // We approximate by looking at the block after this pgTable until the next pgTable or end
      const nextTableMatch = content.indexOf("pgTable(", tableStartIdx + 10);
      const tableBlock =
        nextTableMatch > 0
          ? content.substring(tableStartIdx, nextTableMatch)
          : content.substring(tableStartIdx);

      // Find .references(() => otherTable.id) patterns
      const refRegex = /\.references\(\s*\(\s*\)\s*(?::\s*\w+)?\s*=>\s*(\w+)\.(\w+)/g;
      let refMatch;
      while ((refMatch = refRegex.exec(tableBlock)) !== null) {
        const refVarName = refMatch[1]; // e.g., "partners", "tenants", "financialAccounts"
        refs.add(refVarName);
      }

      // Check for self-reference: parentId references this same table
      // Pattern: parentId ... references(() => thisTableVar.id)
      const exportMatch = content.match(
        new RegExp(`export\\s+const\\s+(\\w+)\\s*=\\s*pgTable\\(\\s*['"]${tableName}['"]`)
      );
      const exportVarName = exportMatch ? exportMatch[1] : null;

      if (exportVarName && refs.has(exportVarName)) {
        selfRef = true;
        refs.delete(exportVarName); // Don't count self-ref as external ref
      }

      // Count columns (rough: count lines with key: something(...) pattern)
      const columnCount = (tableBlock.match(/^\s+\w+:\s+\w+\(/gm) || []).length;

      // Count FK columns specifically
      const fkCount = (tableBlock.match(/\.references\(/g) || []).length;

      tables.set(tableName, {
        entityName,
        file: filePath,
        refVars: refs, // Variable names like "partners", "contracts"
        selfRef,
        columnCount,
        fkCount,
        exportVarName,
      });
    }
  }

  return tables;
}

/**
 * Scan Drizzle schema files for pgEnum definitions.
 * Returns a map: enumName (PascalCase) -> string[]
 */
function scanDrizzleEnums(subprojectPath) {
  const enums = new Map();

  // Check standard enum file location
  const enumFile = path.join(subprojectPath, "src", "db", "schema", "enums.ts");
  if (!fs.existsSync(enumFile)) {
    // Fallback
    const altFile = path.join(subprojectPath, "src", "schema", "enums.ts");
    if (!fs.existsSync(altFile)) return enums;
    return _scanDrizzleEnumFile(altFile);
  }
  return _scanDrizzleEnumFile(enumFile);
}

function _scanDrizzleEnumFile(filePath) {
  const enums = new Map();
  let content;
  try {
    content = fs.readFileSync(filePath, "utf-8");
  } catch {
    return enums;
  }

  // Match pgEnum('enum_name', ['val1', 'val2', ...])
  // The values array can span multiple lines
  const enumRegex = /pgEnum\(\s*['"](\w+)['"]\s*,\s*\[([^\]]*)\]/gs;
  let match;
  while ((match = enumRegex.exec(content)) !== null) {
    const enumSnakeName = match[1]; // e.g., "contract_status"
    const valuesBlock = match[2];

    // Extract quoted values
    const values = [];
    const valRegex = /['"]([^'"]+)['"]/g;
    let valMatch;
    while ((valMatch = valRegex.exec(valuesBlock)) !== null) {
      values.push(valMatch[1]);
    }

    const enumPascalName = snakeToPascal(enumSnakeName);
    enums.set(enumPascalName, values);
  }

  return enums;
}

// ---------------------------------------------------------------------------
// .NET scanning (backend agent)
// ---------------------------------------------------------------------------

/**
 * Scan .NET files for DbSet<EntityName> and entity class definitions.
 * Returns a set of entity names.
 */
function scanDotNetEntities(subprojectPath) {
  const entities = new Set();
  const csFiles = collectFiles(subprojectPath, ".cs", ["migrations", "Migrations"]);

  for (const filePath of csFiles) {
    let content;
    try {
      content = fs.readFileSync(filePath, "utf-8");
    } catch {
      continue;
    }

    // DbSet<EntityName>
    const dbSetRegex = /DbSet<(\w+)>/g;
    let match;
    while ((match = dbSetRegex.exec(content)) !== null) {
      entities.add(match[1]);
    }

    // class EntityName : BaseEntity (in Entities folder)
    if (filePath.includes("Entities") || filePath.includes("entities")) {
      const classRegex = /class\s+(\w+)\s*(?::|extends)/g;
      while ((match = classRegex.exec(content)) !== null) {
        entities.add(match[1]);
      }
    }
  }

  return entities;
}

/**
 * Scan .NET files for enum definitions.
 * Returns a map: enumName -> string[]
 */
function scanDotNetEnums(subprojectPath) {
  const enums = new Map();
  const csFiles = collectFiles(subprojectPath, ".cs");

  for (const filePath of csFiles) {
    let content;
    try {
      content = fs.readFileSync(filePath, "utf-8");
    } catch {
      continue;
    }

    // Strip XML doc comments (/// ...) before parsing
    const cleaned = content.replace(/\/\/\/.*$/gm, "");

    // public enum EnumName { Val1, Val2, ... }
    const enumRegex = /public\s+enum\s+(\w+)\s*\{([^}]*)\}/gs;
    let match;
    while ((match = enumRegex.exec(cleaned)) !== null) {
      const enumName = match[1];
      const body = match[2];

      // Strip single-line comments and multi-line comments from body
      const cleanBody = body
        .replace(/\/\/.*$/gm, "")
        .replace(/\/\*[\s\S]*?\*\//g, "");

      const values = [];
      // Match enum member names (identifier at start of meaningful content)
      const valRegex = /^\s*([A-Z]\w*)\s*(?:=\s*\d+)?\s*,?\s*$/gm;
      let valMatch;
      while ((valMatch = valRegex.exec(cleanBody)) !== null) {
        const val = valMatch[1].trim();
        if (val) values.push(val);
      }

      if (values.length > 0) {
        enums.set(enumName, values);
      }
    }
  }

  return enums;
}

// ---------------------------------------------------------------------------
// Relationship inference
// ---------------------------------------------------------------------------

/**
 * Build a variable-name -> entity-name lookup from Drizzle tables.
 * e.g., "partners" -> "Partner", "financialAccounts" -> "FinancialAccount"
 */
function buildVarToEntityMap(tables) {
  const map = new Map();
  for (const [, tableInfo] of tables) {
    if (tableInfo.exportVarName) {
      map.set(tableInfo.exportVarName, tableInfo.entityName);
    }
  }
  return map;
}

/**
 * Given the scanned tables, infer parent-child (sub) and reference (refs) relationships.
 * Also detect which entities are sub-items of others.
 *
 * Returns:
 *   entities: Map<entityName, { sub: string[], refs: string[] }>
 *   subItems: Set<entityName> (entities that are children of another)
 */
function inferRelationships(tables) {
  const varToEntity = buildVarToEntityMap(tables);
  const entities = new Map();
  const subItems = new Set();

  // Common multi-tenant / audit columns to ignore when counting "own" fields
  const IGNORE_REFS = new Set(["tenants", "companies"]);

  // First pass: create all entities and resolve refs
  for (const [tableName, tableInfo] of tables) {
    const entityName = tableInfo.entityName;
    const refs = [];

    for (const refVar of tableInfo.refVars) {
      if (IGNORE_REFS.has(refVar)) continue;
      const refEntity = varToEntity.get(refVar);
      if (refEntity) {
        refs.push(refEntity);
      }
    }

    entities.set(entityName, {
      sub: [],
      refs: [...new Set(refs)], // deduplicate
      selfRef: tableInfo.selfRef,
      columnCount: tableInfo.columnCount,
      fkCount: tableInfo.fkCount,
      tableName,
    });
  }

  // Second pass: detect parent-child relationships
  // Heuristic: if entity B references entity A, and B's name starts with A's name
  // (e.g., ContractItem -> Contract, PartnerContact -> Partner)
  // then B is a sub-item of A.
  for (const [entityName, entityInfo] of entities) {
    for (const refEntityName of entityInfo.refs) {
      // Check if this entity name starts with the referenced entity name
      if (
        entityName !== refEntityName &&
        entityName.startsWith(refEntityName) &&
        entityName.length > refEntityName.length
      ) {
        // This entity is likely a sub-item of the referenced entity
        const parent = entities.get(refEntityName);
        if (parent) {
          if (!parent.sub.includes(entityName)) {
            parent.sub.push(entityName);
          }
          subItems.add(entityName);
        }
      }
    }
  }

  // Special case: UserSalesChannelOverride -> User (starts with "User")
  // Already handled by the general heuristic above

  // Special case: GatewayLog -> linked to both Contract and Receivable
  // Check if the current entity-registry has it as sub of Receivable
  // We'll check if entity is explicitly a child by looking at the naming convention

  return { entities, subItems };
}

// ---------------------------------------------------------------------------
// Pattern inference
// ---------------------------------------------------------------------------

/**
 * Infer pattern examples from entities.
 */
function inferPatterns(entities, subItems) {
  const patterns = {};

  for (const [entityName, entityInfo] of entities) {
    // selfReferencing: entity that references itself (skip sub-items)
    if (!subItems.has(entityName) && entityInfo.selfRef && !patterns.selfReferencing) {
      patterns.selfReferencing = entityName;
    }

    // manyToMany: junction table with 2+ FKs and few own columns
    // Heuristic: refs.length >= 2 AND total columns are small (junction tables)
    // manyToMany can be a sub-item, so don't skip sub-items here
    const nonTenantRefs = entityInfo.refs;
    if (
      nonTenantRefs.length >= 2 &&
      entityInfo.columnCount <= 12 &&
      !patterns.manyToMany
    ) {
      patterns.manyToMany = entityName;
    }

    if (subItems.has(entityName)) continue; // Skip sub-items for remaining patterns

    // withSubItems: entity that has sub-items
    if (entityInfo.sub.length > 0 && !patterns.withSubItems) {
      patterns.withSubItems = entityName;
    }

    // simple: entity without sub, without refs (or minimal refs), not self-referencing
    if (
      entityInfo.sub.length === 0 &&
      entityInfo.refs.length === 0 &&
      !entityInfo.selfRef &&
      !patterns.simple
    ) {
      patterns.simple = entityName;
    }
  }

  // If no "simple" found, pick one with empty sub and minimal refs
  if (!patterns.simple) {
    for (const [entityName, entityInfo] of entities) {
      if (subItems.has(entityName)) continue;
      if (entityInfo.sub.length === 0 && !entityInfo.selfRef) {
        patterns.simple = entityName;
        break;
      }
    }
  }

  return patterns;
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

function main() {
  const forceFlag = process.argv.includes("--force");

  // 1. Read current registry
  let currentRegistry = null;
  if (fs.existsSync(REGISTRY_PATH)) {
    try {
      currentRegistry = JSON.parse(fs.readFileSync(REGISTRY_PATH, "utf-8"));
    } catch {
      // Invalid JSON, will regenerate
    }
  }

  // 2. Check if already populated (has real entities, not just _placeholder)
  if (currentRegistry && !forceFlag) {
    const entities = currentRegistry.e || {};
    const entityNames = Object.keys(entities).filter(
      (k) => !k.startsWith("_")
    );
    if (entityNames.length > 0) {
      console.log(
        `Registry already populated (${entityNames.length} entities). Use --force to regenerate.`
      );
      process.exit(0);
    }
  }

  // 3. Run sync-detect.js to get subprojects
  let detectResult;
  try {
    const output = execSync(`node "${DETECT_SCRIPT}"`, {
      cwd: ROOT,
      encoding: "utf-8",
      stdio: ["pipe", "pipe", "pipe"],
    });
    detectResult = JSON.parse(output);
  } catch (err) {
    console.error("Failed to run sync-detect.js:", err.message);
    process.exit(1);
  }

  const subprojects = detectResult.subprojects || [];
  console.log(
    `Detected ${subprojects.length} subproject(s): ${subprojects.map((s) => s.name).join(", ")}`
  );

  // 4. Scan each subproject
  let allTables = new Map();
  let allEnums = new Map();
  const dotNetEntities = new Set();

  for (const sub of subprojects) {
    const subPath = path.join(ROOT, sub.path);

    if (sub.agent === "database") {
      console.log(`Scanning ${sub.name} (Drizzle)...`);
      const tables = scanDrizzleTables(subPath);
      const enums = scanDrizzleEnums(subPath);

      // Merge tables
      for (const [k, v] of tables) {
        allTables.set(k, v);
      }
      // Merge enums
      for (const [k, v] of enums) {
        allEnums.set(k, v);
      }

      console.log(
        `  Found ${tables.size} table(s), ${enums.size} enum(s)`
      );
    } else if (sub.agent === "backend") {
      console.log(`Scanning ${sub.name} (.NET)...`);
      const entities = scanDotNetEntities(subPath);
      const enums = scanDotNetEnums(subPath);

      for (const e of entities) {
        dotNetEntities.add(e);
      }
      for (const [k, v] of enums) {
        if (!allEnums.has(k)) {
          allEnums.set(k, v);
        }
      }

      console.log(
        `  Found ${entities.size} entity class(es), ${enums.size} enum(s)`
      );
    } else if (sub.agent === "frontend") {
      console.log(`Skipping ${sub.name} (frontend - does not define entities)`);
    } else {
      console.log(`Skipping ${sub.name} (agent: ${sub.agent})`);
    }
  }

  // 5. Build entity relationships from Drizzle tables
  const { entities, subItems } = inferRelationships(allTables);

  console.log(
    `\nInferred ${entities.size} entities, ${subItems.size} sub-items`
  );

  // 6. Infer patterns
  const patterns = inferPatterns(entities, subItems);
  console.log(`Patterns: ${JSON.stringify(patterns)}`);

  // 7. Build the registry output
  const entityEntries = {};

  // Sort entity names alphabetically
  const sortedNames = [...entities.keys()].sort();

  for (const entityName of sortedNames) {
    const info = entities.get(entityName);

    // Sub-items still get their own entry (as the current registry does)
    // but they also appear in parent's "sub" array
    const entry = {};

    if (info.sub.length > 0) {
      entry.sub = info.sub.sort();
    }
    if (info.refs.length > 0) {
      // Filter out self from refs (self-ref is tracked separately in patterns)
      const filteredRefs = info.refs.filter((r) => r !== entityName);
      if (filteredRefs.length > 0) {
        entry.refs = filteredRefs;
      }
    }
    if (info.selfRef) {
      // Add self to refs for self-referencing entities
      if (!entry.refs) entry.refs = [];
      entry.refs.unshift(entityName);
    }

    entityEntries[entityName] = entry;
  }

  // 8. Build enums output (sorted)
  const enumEntries = {};
  const sortedEnumNames = [...allEnums.keys()].sort();
  for (const enumName of sortedEnumNames) {
    enumEntries[enumName] = allEnums.get(enumName);
  }

  // 9. Assemble final registry
  const registry = {
    _meta: {
      version: "3.1",
      generated: new Date().toISOString().split("T")[0],
      generator: "sync-registry.js",
    },
    _patterns: patterns,
    _enums: enumEntries,
    e: entityEntries,
  };

  // 10. Write output
  const output = JSON.stringify(registry, null, 2) + "\n";
  fs.writeFileSync(REGISTRY_PATH, output, "utf-8");

  console.log(`\nGenerated ${REGISTRY_PATH}`);
  console.log(
    `  ${Object.keys(entityEntries).length} entities, ${Object.keys(enumEntries).length} enums`
  );
}

main();
