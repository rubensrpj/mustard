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
// Relationship inference (from .NET entities)
// ---------------------------------------------------------------------------

/**
 * Placeholder: relationship inference was previously Drizzle-based.
 * TODO: implement .NET navigation-property based inference if needed.
 */
function inferRelationships(_tables) {
  return { entities: new Map(), subItems: new Set() };
}

/**
 * Placeholder: pattern inference was previously Drizzle-based.
 */
function inferPatterns(_entities, _subItems) {
  return {};
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

    if (sub.agent === "backend") {
      console.log(`Scanning ${sub.name} (.NET)...`);
      const entities = scanDotNetEntities(subPath);
      const enums = scanDotNetEnums(subPath);

      for (const e of entities) {
        dotNetEntities.add(e);
      }
      for (const [k, v] of enums) {
        // Case-insensitive dedup: skip if an enum with the same name (different casing) already exists
        const existingKey = [...allEnums.keys()].find(
          (existing) => existing.toLowerCase() === k.toLowerCase()
        );
        if (!existingKey) {
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

  // 5. Build entity relationships (placeholder — previously Drizzle-based)
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

  // 8. Build enums output (sorted, compressed if >8 values)
  const enumEntries = {};
  const sortedEnumNames = [...allEnums.keys()].sort();
  for (const enumName of sortedEnumNames) {
    const values = allEnums.get(enumName);
    if (values.length > 8) {
      enumEntries[enumName] = [
        ...values.slice(0, 5),
        `...(${values.length} total)`,
      ];
    } else {
      enumEntries[enumName] = values;
    }
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
