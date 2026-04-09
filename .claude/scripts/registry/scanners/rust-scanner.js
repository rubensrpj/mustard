'use strict';

/**
 * rust-scanner.js
 *
 * Rust stack scanner for sync-registry.js.
 * Scans Rust projects for entities, enums, traits, routes, DTOs,
 * services, repositories and infers architectural patterns.
 *
 * Extends ScannerContract — fail-open, no external dependencies.
 */

const path = require('path');
const fs = require('fs');
const { ScannerContract } = require('../scanner-contract');
const { collectFiles, relativePath, readFileSafe, inferCommonFolder } = require('../file-utils');

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/**
 * Check whether a path exists under the subproject root.
 * @param {string} root
 * @param {string} rel
 * @returns {boolean}
 */
function existsUnder(root, rel) {
  try {
    return fs.existsSync(path.join(root, rel));
  } catch {
    return false;
  }
}

/**
 * Mode of an array of strings — most frequent item.
 * @param {string[]} arr
 * @returns {string|null}
 */
function mode(arr) {
  if (!arr.length) return null;
  const counts = new Map();
  for (const v of arr) counts.set(v, (counts.get(v) || 0) + 1);
  let top = null, topCount = 0;
  for (const [v, c] of counts) {
    if (c > topCount) { top = v; topCount = c; }
  }
  return top;
}

/**
 * Deduplicate an array preserving order.
 * @param {string[]} arr
 * @returns {string[]}
 */
function uniq(arr) {
  return [...new Set(arr)];
}

/**
 * Parse Cargo.toml and return a set of dependency names (lower-cased).
 * Handles both `[dependencies]` and `[dev-dependencies]` sections.
 * Simple line-by-line parser — no external TOML library.
 *
 * @param {string} content
 * @returns {Set<string>}
 */
function parseCargoToml(content) {
  const deps = new Set();
  let inDepsSection = false;

  for (const rawLine of content.split('\n')) {
    const line = rawLine.trim();

    // Section headers
    if (/^\[.*\]$/.test(line)) {
      inDepsSection =
        line === '[dependencies]' ||
        line === '[dev-dependencies]' ||
        line === '[build-dependencies]';
      continue;
    }

    if (!inDepsSection) continue;
    if (!line || line.startsWith('#')) continue;

    // package = "version"  OR  package = { version = "...", ... }
    const m = line.match(/^([\w-]+)\s*=/);
    if (m) deps.add(m[1].toLowerCase());
  }
  return deps;
}

/**
 * Check if a deps set contains a given package name (exact).
 * @param {Set<string>} deps
 * @param {string} pkg
 * @returns {boolean}
 */
function hasDep(deps, pkg) {
  return deps.has(pkg.toLowerCase());
}

/**
 * Extract the body between the first balanced brace pair starting at or after startPos.
 * Returns null if not found.
 * @param {string} content
 * @param {number} startPos
 * @returns {string|null}
 */
function extractBraceBody(content, startPos) {
  let depth = 0;
  let start = -1;
  for (let i = startPos; i < content.length; i++) {
    if (content[i] === '{') {
      if (depth === 0) start = i + 1;
      depth++;
    } else if (content[i] === '}') {
      depth--;
      if (depth === 0) return content.slice(start, i);
    }
  }
  return null;
}

/**
 * Extract derive macro names from a #[derive(...)] attribute string.
 * @param {string} attrBlock - text up to and including the derive attr
 * @returns {string[]}
 */
function extractDerives(attrBlock) {
  const m = attrBlock.match(/#\[derive\(([^)]+)\)\]/);
  if (!m) return [];
  return m[1].split(',').map(s => s.trim()).filter(Boolean);
}

/**
 * Collect all occurrences of a global regex in content.
 * Resets lastIndex before use.
 * @param {string} content
 * @param {RegExp} re
 * @param {number} [group=1]
 * @returns {string[]}
 */
function allMatches(content, re, group = 1) {
  re.lastIndex = 0;
  const out = [];
  let m;
  while ((m = re.exec(content)) !== null) {
    if (m[group] != null) out.push(m[group].trim());
  }
  return out;
}

/**
 * Get the attribute block (attributes + whitespace) immediately before a keyword position.
 * Scans backwards from pos for #[...] lines.
 * @param {string} content
 * @param {number} pos
 * @returns {string}
 */
function getAttrBlock(content, pos) {
  const before = content.slice(0, pos);
  // Walk backwards over whitespace and #[...] blocks
  const attrRe = /((?:\s*#\[[^\]]*\])+\s*)$/;
  const m = before.match(attrRe);
  return m ? m[1] : '';
}

// ---------------------------------------------------------------------------
// RustScanner
// ---------------------------------------------------------------------------

class RustScanner extends ScannerContract {
  static stackId = 'rust';

  // --------------------------------------------------------------------------
  // detect
  // --------------------------------------------------------------------------

  detect() {
    return existsUnder(this.subprojectPath, 'Cargo.toml');
  }

  // --------------------------------------------------------------------------
  // detectArchitecture
  // --------------------------------------------------------------------------

  detectArchitecture() {
    try {
      const srcDir = path.join(this.subprojectPath, 'src');

      const hasDomain = existsUnder(srcDir, 'domain') || existsUnder(this.subprojectPath, 'domain');
      const hasApplication = existsUnder(srcDir, 'application') || existsUnder(this.subprojectPath, 'application');
      const hasInfra = existsUnder(srcDir, 'infrastructure') || existsUnder(this.subprojectPath, 'infrastructure');

      if (hasDomain && hasApplication && hasInfra) return 'clean-architecture';

      const hasHandlers = existsUnder(srcDir, 'handlers') || existsUnder(this.subprojectPath, 'handlers');
      const hasServices = existsUnder(srcDir, 'services') || existsUnder(this.subprojectPath, 'services');
      const hasRepositories = existsUnder(srcDir, 'repositories') || existsUnder(this.subprojectPath, 'repositories');

      if (hasHandlers && hasServices && hasRepositories) return 'layered';

      // Check for mod.rs with module declarations indicating clear separation
      const mainRsPath = path.join(srcDir, 'main.rs');
      const libRsPath = path.join(srcDir, 'lib.rs');
      const mainContent = readFileSafe(mainRsPath) || readFileSafe(libRsPath) || '';
      const modCount = (mainContent.match(/^mod\s+\w+/gm) || []).length;
      if (modCount >= 3) return 'modular';

      return 'minimal';
    } catch (err) {
      process.stderr.write(`[rust-scanner] detectArchitecture error: ${err.message}\n`);
      return 'unknown';
    }
  }

  // --------------------------------------------------------------------------
  // _readCargoToml — cached
  // --------------------------------------------------------------------------

  _readCargoToml() {
    if (this._cargoDeps) return this._cargoDeps;
    const cargoPath = path.join(this.subprojectPath, 'Cargo.toml');
    const content = readFileSafe(cargoPath);
    this._cargoContent = content || '';
    this._cargoDeps = content ? parseCargoToml(content) : new Set();
    return this._cargoDeps;
  }

  // --------------------------------------------------------------------------
  // _detectFramework
  // --------------------------------------------------------------------------

  _detectFramework() {
    const deps = this._readCargoToml();
    if (hasDep(deps, 'axum')) return 'axum';
    if (hasDep(deps, 'actix-web')) return 'actix';
    if (hasDep(deps, 'rocket')) return 'rocket';
    return 'none';
  }

  // --------------------------------------------------------------------------
  // _detectORM
  // --------------------------------------------------------------------------

  _detectORM() {
    const deps = this._readCargoToml();
    if (hasDep(deps, 'diesel')) return 'diesel';
    if (hasDep(deps, 'sqlx')) return 'sqlx';
    if (hasDep(deps, 'sea-orm')) return 'sea-orm';
    return 'none';
  }

  // --------------------------------------------------------------------------
  // _detectErrorHandling
  // --------------------------------------------------------------------------

  _detectErrorHandling() {
    const deps = this._readCargoToml();
    if (hasDep(deps, 'thiserror')) return 'thiserror';
    if (hasDep(deps, 'anyhow')) return 'anyhow';
    // Check for custom error types
    const rsFiles = collectFiles(this.subprojectPath, '.rs');
    for (const f of rsFiles) {
      const c = readFileSafe(f);
      if (c && /pub\s+enum\s+\w+Error/.test(c)) return 'custom';
    }
    return 'none';
  }

  // --------------------------------------------------------------------------
  // scanEntities
  // --------------------------------------------------------------------------

  scanEntities() {
    const entities = new Map();
    try {
      const orm = this._detectORM();
      const rsFiles = collectFiles(this.subprojectPath, '.rs');

      // Regex for struct declarations with preceding attributes
      // We scan each file individually to properly associate attributes with structs
      for (const filePath of rsFiles) {
        const content = readFileSafe(filePath);
        if (!content) continue;

        this._extractRustEntities(content, filePath, orm, entities);
      }
    } catch (err) {
      process.stderr.write(`[rust-scanner] scanEntities error: ${err.message}\n`);
    }
    return entities;
  }

  _extractRustEntities(content, filePath, orm, entities) {
    // Match pub struct declarations
    const structRe = /pub\s+struct\s+(\w+)\s*(?:<[^>]+>)?\s*\{/g;
    structRe.lastIndex = 0;
    let m;

    while ((m = structRe.exec(content)) !== null) {
      const name = m[1];
      const attrBlock = getAttrBlock(content, m.index);
      const derives = extractDerives(attrBlock);

      // Determine if this is an entity based on ORM-specific markers
      const isEntityByOrm = this._isOrmEntity(attrBlock, content, name, orm);
      if (!isEntityByOrm) continue;

      const bodyStart = content.indexOf('{', m.index + m[0].length - 1);
      if (bodyStart === -1) continue;
      const body = extractBraceBody(content, bodyStart);

      const properties = body ? this._extractStructFields(body) : [];
      const rel = relativePath(this.subprojectPath, filePath);

      // Detect table name from diesel/sea-orm attributes
      const tableAttr = attrBlock.match(/diesel\s*\(\s*table_name\s*=\s*"([^"]+)"/) ||
                        attrBlock.match(/sea_orm\s*\(\s*table_name\s*=\s*"([^"]+)"/) ||
                        attrBlock.match(/table_name\s*=\s*"([^"]+)"/);
      const tableName = tableAttr ? tableAttr[1] : undefined;

      entities.set(name, {
        file: rel,
        decorators: derives,
        properties,
        ...(tableName ? { namespace: tableName } : {}),
      });
    }
  }

  _isOrmEntity(attrBlock, content, name, orm) {
    if (orm === 'diesel') {
      return /Queryable|Insertable|AsChangeset|Identifiable/.test(attrBlock) ||
             /diesel\s*\(/.test(attrBlock);
    }
    if (orm === 'sqlx') {
      return /sqlx::FromRow|FromRow/.test(attrBlock);
    }
    if (orm === 'sea-orm') {
      return /DeriveEntityModel|DeriveModel|DeriveActiveModel/.test(attrBlock);
    }
    // Generic: any struct with Serialize/Deserialize might be an entity
    return /Queryable|Insertable|FromRow|DeriveEntityModel/.test(attrBlock);
  }

  _extractStructFields(body) {
    const fields = [];
    // pub field_name: Type,
    const fieldRe = /(?:pub\s+)?(\w+)\s*:\s*([^,\n]+)/g;
    fieldRe.lastIndex = 0;
    let m;
    while ((m = fieldRe.exec(body)) !== null) {
      const fieldName = m[1].trim();
      const fieldType = m[2].trim().replace(/,\s*$/, '');
      if (fieldName === 'pub' || fieldName === 'fn') continue;
      fields.push(`${fieldName}: ${fieldType}`);
    }
    return fields;
  }

  // --------------------------------------------------------------------------
  // scanEnums
  // --------------------------------------------------------------------------

  scanEnums() {
    const enums = new Map();
    try {
      const rsFiles = collectFiles(this.subprojectPath, '.rs');

      for (const filePath of rsFiles) {
        const content = readFileSafe(filePath);
        if (!content) continue;

        this._extractRustEnums(content, filePath, enums);
      }
    } catch (err) {
      process.stderr.write(`[rust-scanner] scanEnums error: ${err.message}\n`);
    }
    return enums;
  }

  _extractRustEnums(content, filePath, enums) {
    // Match pub enum declarations with optional attributes before them
    // Uses multiline scanning to capture attributes + enum body
    const enumRe = /pub\s+enum\s+(\w+)\s*(?:<[^>]+>)?\s*\{([^}]*)\}/gs;
    enumRe.lastIndex = 0;
    let m;

    while ((m = enumRe.exec(content)) !== null) {
      const name = m[1];
      const body = m[2];
      const attrBlock = getAttrBlock(content, m.index);
      const derives = extractDerives(attrBlock);

      // Extract variants (lines that look like variant declarations)
      const variants = this._extractEnumVariants(body);
      if (variants.length === 0) continue;

      // Detect serde rename_all
      const renameAll = attrBlock.match(/serde\s*\(\s*rename_all\s*=\s*"([^"]+)"/);

      const rel = relativePath(this.subprojectPath, filePath);

      const decoratorList = derives.slice();
      if (renameAll) decoratorList.push(`serde:rename_all:${renameAll[1]}`);

      enums.set(name, {
        values: variants,
        file: rel,
        decorators: decoratorList,
        valueDecorators: this._extractVariantAttributes(body),
        valueConvention: this._detectEnumConvention(derives, renameAll ? renameAll[1] : null),
      });
    }
  }

  _extractEnumVariants(body) {
    const variants = [];
    // Lines: VariantName, or VariantName(Type), or VariantName { fields }
    const variantRe = /^\s{0,8}([A-Z]\w*)\s*(?:\([^)]*\)|\{[^}]*\})?\s*,?\s*$/gm;
    variantRe.lastIndex = 0;
    let m;
    while ((m = variantRe.exec(body)) !== null) {
      variants.push(m[1]);
    }
    return variants;
  }

  _extractVariantAttributes(body) {
    const attrs = [];
    const attrRe = /#\[serde\(([^)]+)\)\]/g;
    attrRe.lastIndex = 0;
    let m;
    while ((m = attrRe.exec(body)) !== null) {
      attrs.push(`serde:${m[1].trim()}`);
    }
    return attrs;
  }

  _detectEnumConvention(derives, renameAll) {
    if (renameAll) return renameAll;
    // Default Rust convention is PascalCase
    return 'PascalCase';
  }

  // --------------------------------------------------------------------------
  // scanInterfaces — Rust traits
  // --------------------------------------------------------------------------

  scanInterfaces() {
    const interfaces = new Map();
    try {
      const rsFiles = collectFiles(this.subprojectPath, '.rs');
      const traitRe = /pub\s+trait\s+(\w+)(?:\s*:\s*([^{]+))?\s*\{/g;
      const implRe = /impl\s+(\w+)\s+for\s+(\w+)/g;

      // First pass: collect traits
      for (const filePath of rsFiles) {
        const content = readFileSafe(filePath);
        if (!content) continue;

        traitRe.lastIndex = 0;
        let m;
        while ((m = traitRe.exec(content)) !== null) {
          const name = m[1];
          const supertraits = m[2]
            ? m[2].split('+').map(s => s.trim()).filter(Boolean)
            : [];

          const bodyStart = content.indexOf('{', m.index + m[0].length - 1);
          const body = bodyStart !== -1 ? extractBraceBody(content, bodyStart) : '';
          const methods = body ? this._extractTraitMethods(body) : [];

          const rel = relativePath(this.subprojectPath, filePath);
          interfaces.set(name, {
            file: rel,
            methods,
            extends: supertraits,
            implementedBy: [],
          });
        }
      }

      // Second pass: collect implementations
      for (const filePath of rsFiles) {
        const content = readFileSafe(filePath);
        if (!content) continue;

        implRe.lastIndex = 0;
        let m;
        while ((m = implRe.exec(content)) !== null) {
          const traitName = m[1];
          const implName = m[2];
          if (interfaces.has(traitName)) {
            interfaces.get(traitName).implementedBy.push(implName);
          }
        }
      }
    } catch (err) {
      process.stderr.write(`[rust-scanner] scanInterfaces error: ${err.message}\n`);
    }
    return interfaces;
  }

  _extractTraitMethods(body) {
    const methods = [];
    // fn method_name(...) -> ReturnType;  or  fn method_name(...) -> ReturnType { ... }
    const methodRe = /fn\s+(\w+)\s*(?:<[^>]*>)?\s*\([^)]*\)\s*(?:->[^;{]+)?/g;
    methodRe.lastIndex = 0;
    let m;
    while ((m = methodRe.exec(body)) !== null) {
      methods.push(m[0].trim());
    }
    return methods;
  }

  // --------------------------------------------------------------------------
  // scanRoutes
  // --------------------------------------------------------------------------

  scanRoutes() {
    const routes = new Map();
    try {
      const framework = this._detectFramework();
      const rsFiles = collectFiles(this.subprojectPath, '.rs');

      for (const filePath of rsFiles) {
        const content = readFileSafe(filePath);
        if (!content) continue;

        const endpoints = this._extractRustRoutes(content, framework);
        if (endpoints.length === 0) continue;

        const rel = relativePath(this.subprojectPath, filePath);
        const prefix = this._detectRustRoutePrefix(content, framework);

        const existing = routes.get(rel);
        if (existing) {
          existing.endpoints.push(...endpoints);
        } else {
          routes.set(rel, { file: rel, prefix, endpoints });
        }
      }
    } catch (err) {
      process.stderr.write(`[rust-scanner] scanRoutes error: ${err.message}\n`);
    }
    return routes;
  }

  _extractRustRoutes(content, framework) {
    const endpoints = [];

    if (framework === 'axum') {
      // .route("/path", get(handler)) or .route("/path", post(handler))
      const axumRe = /\.route\s*\(\s*["']([^"']+)["']\s*,\s*(get|post|put|delete|patch)\s*\(/gi;
      axumRe.lastIndex = 0;
      let m;
      while ((m = axumRe.exec(content)) !== null) {
        endpoints.push({ method: m[2].toUpperCase(), path: m[1] });
      }
    } else if (framework === 'actix') {
      // #[get("/path")] / #[post("/path")] etc.
      const actixAttrRe = /#\[(get|post|put|delete|patch)\s*\(\s*["']([^"']+)["']\s*\)\]/gi;
      actixAttrRe.lastIndex = 0;
      let m;
      while ((m = actixAttrRe.exec(content)) !== null) {
        endpoints.push({ method: m[1].toUpperCase(), path: m[2] });
      }
      // web::resource("/path").route(web::get().to(handler))
      const actixResRe = /web::resource\s*\(\s*["']([^"']+)["']\s*\)/g;
      actixResRe.lastIndex = 0;
      while ((m = actixResRe.exec(content)) !== null) {
        endpoints.push({ method: 'ANY', path: m[1] });
      }
    } else if (framework === 'rocket') {
      // #[get("/path")] / #[post("/path")] etc.
      const rocketRe = /#\[(get|post|put|delete|patch)\s*\(\s*["']([^"']+)["']\s*\)\]/gi;
      rocketRe.lastIndex = 0;
      let m;
      while ((m = rocketRe.exec(content)) !== null) {
        endpoints.push({ method: m[1].toUpperCase(), path: m[2] });
      }
    }

    return endpoints;
  }

  _detectRustRoutePrefix(content, framework) {
    if (framework === 'axum') {
      // .nest("/prefix", router)
      const nestRe = /\.nest\s*\(\s*["']([^"']+)["']/;
      const m = content.match(nestRe);
      if (m) return m[1];
    } else if (framework === 'actix') {
      // web::scope("/prefix")
      const scopeRe = /web::scope\s*\(\s*["']([^"']+)["']/;
      const m = content.match(scopeRe);
      if (m) return m[1];
    }
    return '/';
  }

  // --------------------------------------------------------------------------
  // scanDtos
  // --------------------------------------------------------------------------

  scanDtos() {
    const dtos = new Map();
    try {
      const rsFiles = collectFiles(this.subprojectPath, '.rs');
      const dtoSuffixes = /(?:Request|Response|Dto|DTO|Input|Output)$/;
      const structRe = /pub\s+struct\s+(\w+)\s*(?:<[^>]+>)?\s*\{/g;

      for (const filePath of rsFiles) {
        const content = readFileSafe(filePath);
        if (!content) continue;

        structRe.lastIndex = 0;
        let m;
        while ((m = structRe.exec(content)) !== null) {
          const name = m[1];
          if (!dtoSuffixes.test(name)) continue;

          const attrBlock = getAttrBlock(content, m.index);
          const derives = extractDerives(attrBlock);

          const bodyStart = content.indexOf('{', m.index + m[0].length - 1);
          const body = bodyStart !== -1 ? extractBraceBody(content, bodyStart) : '';
          const properties = body ? this._extractStructFields(body) : [];

          // Detect direction: Deserialize = input, Serialize = output
          const hasDeserialize = derives.includes('Deserialize');
          const hasSerialize = derives.includes('Serialize');

          // Detect validation crate usage
          const hasValidation = body ? /#\[validate/.test(body) || attrBlock.includes('validate') : false;

          const entity = name
            .replace(/Request$|Response$|Dto$|DTO$|Input$|Output$/, '') || undefined;

          const rel = relativePath(this.subprojectPath, filePath);

          dtos.set(name, {
            file: rel,
            entity: entity || undefined,
            validationPattern: hasValidation ? 'validator' : undefined,
            decorators: derives,
            properties,
            ...(hasDeserialize && !hasSerialize ? { direction: 'input' } : {}),
            ...(hasSerialize && !hasDeserialize ? { direction: 'output' } : {}),
          });
        }
      }
    } catch (err) {
      process.stderr.write(`[rust-scanner] scanDtos error: ${err.message}\n`);
    }
    return dtos;
  }

  // --------------------------------------------------------------------------
  // scanServices
  // --------------------------------------------------------------------------

  scanServices() {
    const services = new Map();
    try {
      const rsFiles = collectFiles(this.subprojectPath, '.rs');
      const serviceSuffixes = /(?:Service|UseCase)$/;
      const structRe = /pub\s+struct\s+(\w+)\s*(?:<[^>]+>)?\s*[\{(;]/g;
      const implTraitRe = /impl\s+(\w+)\s+for\s+(\w+)/g;

      // First pass: find service structs
      for (const filePath of rsFiles) {
        const content = readFileSafe(filePath);
        if (!content) continue;

        structRe.lastIndex = 0;
        let m;
        while ((m = structRe.exec(content)) !== null) {
          const name = m[1];
          if (!serviceSuffixes.test(name)) continue;

          const rel = relativePath(this.subprojectPath, filePath);
          const entity = name.replace(/Service$|UseCase$/, '') || undefined;

          // Look for constructor: pub fn new(...) -> Self
          const ctorRe = /pub\s+fn\s+new\s*\(([^)]*)\)/;
          const ctorM = content.match(ctorRe);
          const dependencies = [];
          if (ctorM) {
            const params = ctorM[1];
            // Extract type names from params: name: impl TraitName or name: Arc<TraitName>
            const paramRe = /:\s*(?:impl\s+|Arc<|Box<)?(\w+)/g;
            paramRe.lastIndex = 0;
            let pm;
            while ((pm = paramRe.exec(params)) !== null) {
              const typeName = pm[1];
              if (/^[A-Z]/.test(typeName) && typeName !== 'Self') {
                dependencies.push(typeName);
              }
            }
          }

          services.set(name, {
            file: rel,
            entity: entity || undefined,
            dependencies: uniq(dependencies),
          });
        }
      }

      // Second pass: detect trait implementations for services
      for (const filePath of rsFiles) {
        const content = readFileSafe(filePath);
        if (!content) continue;

        implTraitRe.lastIndex = 0;
        let m;
        while ((m = implTraitRe.exec(content)) !== null) {
          const traitName = m[1];
          const implName = m[2];
          if (services.has(implName)) {
            services.get(implName).interface = traitName;
          }
        }
      }
    } catch (err) {
      process.stderr.write(`[rust-scanner] scanServices error: ${err.message}\n`);
    }
    return services;
  }

  // --------------------------------------------------------------------------
  // scanRepositories
  // --------------------------------------------------------------------------

  scanRepositories() {
    const repositories = new Map();
    try {
      const rsFiles = collectFiles(this.subprojectPath, '.rs');
      const repoSuffixes = /(?:Repository|Repo|Store)$/;
      const structRe = /pub\s+struct\s+(\w+)\s*(?:<[^>]+>)?\s*[\{(;]/g;
      const implTraitRe = /impl\s+(\w+)\s+for\s+(\w+)/g;

      // First pass: find repository structs
      for (const filePath of rsFiles) {
        const content = readFileSafe(filePath);
        if (!content) continue;

        structRe.lastIndex = 0;
        let m;
        while ((m = structRe.exec(content)) !== null) {
          const name = m[1];
          if (!repoSuffixes.test(name)) continue;

          const rel = relativePath(this.subprojectPath, filePath);
          const entity = name.replace(/Repository$|Repo$|Store$/, '') || undefined;

          repositories.set(name, {
            file: rel,
            entity: entity || undefined,
          });
        }
      }

      // Second pass: detect trait implementations for repositories
      for (const filePath of rsFiles) {
        const content = readFileSafe(filePath);
        if (!content) continue;

        implTraitRe.lastIndex = 0;
        let m;
        while ((m = implTraitRe.exec(content)) !== null) {
          const traitName = m[1];
          const implName = m[2];
          if (repositories.has(implName)) {
            repositories.get(implName).interface = traitName;
          }
        }
      }
    } catch (err) {
      process.stderr.write(`[rust-scanner] scanRepositories error: ${err.message}\n`);
    }
    return repositories;
  }

  // --------------------------------------------------------------------------
  // inferPatterns
  // --------------------------------------------------------------------------

  inferPatterns(scanResults) {
    try {
      const { entities, enums, routes } = scanResults;
      const framework = this._detectFramework();
      const orm = this._detectORM();
      const deps = this._readCargoToml();
      const hasSerde = hasDep(deps, 'serde');

      // Entity folder + derive macros
      const entityFiles = [...entities.values()].map(e => e.file).filter(Boolean);
      const entityFolder = inferCommonFolder(entityFiles);
      const allDeriveLists = [...entities.values()].map(e => e.decorators || []);
      const allDerives = allDeriveLists.flat();
      const deriveMacros = mode(allDerives) ? uniq(allDerives).slice(0, 5) : [];

      // Detect table naming convention from entity namespaces (diesel/sea-orm table_name)
      const tableNames = [...entities.values()].map(e => e.namespace).filter(Boolean);
      const tableNaming = tableNames.length > 0 ? 'explicit' : 'inferred';

      // Enum patterns
      const enumFiles = [...enums.values()].map(e => e.file).filter(Boolean);
      const uniqueEnumFiles = uniq(enumFiles);
      const enumFolder = inferCommonFolder(enumFiles);
      const allEnumDerives = [...enums.values()].flatMap(e => e.decorators || []);
      const enumDeriveMacros = mode(allEnumDerives) ? uniq(allEnumDerives).slice(0, 5) : [];

      // Detect most common serde rename_all pattern in enums
      const renameAllPatterns = [...enums.values()]
        .flatMap(e => (e.decorators || []))
        .filter(d => d.startsWith('serde:rename_all:'))
        .map(d => d.replace('serde:rename_all:', ''));
      const renameAll = mode(renameAllPatterns) || null;

      // Routes
      const allEndpoints = [...routes.values()].flatMap(r => r.endpoints || []);
      const prefixes = [...routes.values()].map(r => r.prefix).filter(p => p && p !== '/');
      const commonPrefix = mode(prefixes) || '/';
      const hasNest = [...routes.values()].some(r => r.prefix && r.prefix !== '/');

      // Error handling
      const errorHandling = this._detectErrorHandling();

      // Cargo.toml metadata
      const cargoContent = this._cargoContent || '';
      const editionMatch = cargoContent.match(/edition\s*=\s*"(\d+)"/);
      const edition = editionMatch ? editionMatch[1] : '2021';

      return {
        framework,
        orm,
        serialization: hasSerde ? 'serde' : 'none',
        edition,
        entity: {
          folder: entityFolder,
          deriveMacros,
          tableNaming,
        },
        enum: {
          folder: enumFolder,
          deriveMacros: enumDeriveMacros,
          renameAll,
          separateFiles: uniqueEnumFiles.length > 1,
        },
        routes: {
          style: framework,
          prefix: commonPrefix,
          nestPattern: hasNest,
        },
        errorHandling,
      };
    } catch (err) {
      process.stderr.write(`[rust-scanner] inferPatterns error: ${err.message}\n`);
      return {};
    }
  }
}

module.exports = RustScanner;
