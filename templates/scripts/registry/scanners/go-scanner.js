'use strict';

/**
 * go-scanner.js
 *
 * Go stack scanner for sync-registry.js.
 * Scans Go projects for entities, enums, interfaces, routes, DTOs,
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
 * List immediate child directory names of a given directory.
 * Returns empty array on error.
 * @param {string} dir
 * @returns {string[]}
 */
function listDirs(dir) {
  try {
    return fs.readdirSync(dir, { withFileTypes: true })
      .filter(e => e.isDirectory())
      .map(e => e.name);
  } catch {
    return [];
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
 * Collect all captures of a global regex applied to content.
 * Resets lastIndex before iterating.
 * @param {string} content
 * @param {RegExp} regex - must have `g` flag
 * @param {number} [group=1]
 * @returns {string[]}
 */
function allMatches(content, regex, group = 1) {
  regex.lastIndex = 0;
  const results = [];
  let m;
  while ((m = regex.exec(content)) !== null) {
    if (m[group] != null) results.push(m[group].trim());
  }
  return results;
}

/**
 * Parse go.mod and return a Set of module paths (one per require line).
 * @param {string} content
 * @returns {Set<string>}
 */
function parseGoMod(content) {
  const deps = new Set();
  // Single-line: require github.com/gin-gonic/gin v1.x.x
  const singleRe = /^require\s+(\S+)\s+\S/gm;
  singleRe.lastIndex = 0;
  let m;
  while ((m = singleRe.exec(content)) !== null) {
    deps.add(m[1]);
  }
  // Block: require ( ... )
  const blockRe = /require\s*\(([^)]+)\)/gs;
  blockRe.lastIndex = 0;
  while ((m = blockRe.exec(content)) !== null) {
    const block = m[1];
    const lineRe = /^\s+(\S+)\s+\S/gm;
    lineRe.lastIndex = 0;
    let lm;
    while ((lm = lineRe.exec(block)) !== null) {
      deps.add(lm[1]);
    }
  }
  return deps;
}

/**
 * Detect which framework or ORM is present in a deps set.
 * @param {Set<string>} deps
 * @param {string} prefix
 * @returns {boolean}
 */
function hasDep(deps, prefix) {
  for (const d of deps) {
    if (d === prefix || d.startsWith(prefix + '/')) return true;
  }
  return false;
}

/**
 * Extract struct fields with a specific tag from Go source.
 * Returns array of "fieldName fieldType" strings.
 * @param {string} body - content between struct { }
 * @param {string} tagKey - e.g., "gorm" or "db"
 * @returns {string[]}
 */
function extractTaggedFields(body, tagKey) {
  const fields = [];
  // field   Type   `tagKey:"..."`
  const re = new RegExp(`(\\w+)\\s+(\\S+)\\s+\`[^\`]*${tagKey}:"([^"]*)"`, 'g');
  re.lastIndex = 0;
  let m;
  while ((m = re.exec(body)) !== null) {
    fields.push(`${m[1]} ${m[2]}`);
  }
  return fields;
}

/**
 * Extract the body between the first balanced brace pair after a position.
 * Returns null if not found.
 * @param {string} content
 * @param {number} startPos - index of the opening `{`
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

// ---------------------------------------------------------------------------
// GoScanner
// ---------------------------------------------------------------------------

class GoScanner extends ScannerContract {
  static stackId = 'go';

  // --------------------------------------------------------------------------
  // detect
  // --------------------------------------------------------------------------

  detect() {
    return existsUnder(this.subprojectPath, 'go.mod');
  }

  // --------------------------------------------------------------------------
  // detectArchitecture
  // --------------------------------------------------------------------------

  detectArchitecture() {
    try {
      const hasDomainDir = existsUnder(this.subprojectPath, 'internal/domain');
      const hasServiceDir = existsUnder(this.subprojectPath, 'internal/service');
      const hasRepoDir = existsUnder(this.subprojectPath, 'internal/repository');

      if (hasDomainDir && hasServiceDir && hasRepoDir) {
        return 'clean-architecture';
      }

      const hasHandler = existsUnder(this.subprojectPath, 'handler');
      const hasFlatService = existsUnder(this.subprojectPath, 'service');
      const hasFlatRepo = existsUnder(this.subprojectPath, 'repository');

      // Check if interfaces are used alongside handlers/services/repos
      if (hasHandler && hasFlatService && hasFlatRepo) {
        // Check for interface files as a signal of SOLID
        const goFiles = collectFiles(this.subprojectPath, '.go');
        const hasInterfaces = goFiles.some(f => {
          const content = readFileSafe(f);
          return content && /type\s+\w+\s+interface\s*\{/.test(content);
        });
        if (hasInterfaces) return 'solid';
      }

      const hasCmd = existsUnder(this.subprojectPath, 'cmd');
      const hasInternal = existsUnder(this.subprojectPath, 'internal');
      if (hasCmd && hasInternal) return 'standard-layout';

      return 'minimal';
    } catch (err) {
      process.stderr.write(`[go-scanner] detectArchitecture error: ${err.message}\n`);
      return 'unknown';
    }
  }

  // --------------------------------------------------------------------------
  // _readGoMod — cached
  // --------------------------------------------------------------------------

  _readGoMod() {
    if (this._goModDeps) return this._goModDeps;
    const goModPath = path.join(this.subprojectPath, 'go.mod');
    const content = readFileSafe(goModPath);
    this._goModDeps = content ? parseGoMod(content) : new Set();
    return this._goModDeps;
  }

  // --------------------------------------------------------------------------
  // _detectFramework
  // --------------------------------------------------------------------------

  _detectFramework() {
    const deps = this._readGoMod();
    if (hasDep(deps, 'github.com/gin-gonic/gin')) return 'gin';
    if (hasDep(deps, 'github.com/labstack/echo')) return 'echo';
    if (hasDep(deps, 'github.com/gofiber/fiber')) return 'fiber';
    if (hasDep(deps, 'github.com/go-chi/chi')) return 'chi';
    return 'stdlib';
  }

  // --------------------------------------------------------------------------
  // _detectORM
  // --------------------------------------------------------------------------

  _detectORM() {
    const deps = this._readGoMod();
    if (hasDep(deps, 'gorm.io/gorm')) return 'gorm';
    if (hasDep(deps, 'github.com/jmoiron/sqlx')) return 'sqlx';
    if (hasDep(deps, 'entgo.io/ent')) return 'ent';
    return 'none';
  }

  // --------------------------------------------------------------------------
  // scanEntities
  // --------------------------------------------------------------------------

  scanEntities() {
    const entities = new Map();
    try {
      const orm = this._detectORM();
      const goFiles = collectFiles(this.subprojectPath, '.go');

      if (orm === 'gorm') {
        this._scanGORMEntities(goFiles, entities);
      } else if (orm === 'sqlx') {
        this._scanSQLxEntities(goFiles, entities);
      } else if (orm === 'ent') {
        this._scanEntEntities(entities);
      }

      // Fallback: also scan any struct with gorm/db tags even if ORM not in go.mod
      if (entities.size === 0) {
        this._scanGORMEntities(goFiles, entities);
        if (entities.size === 0) this._scanSQLxEntities(goFiles, entities);
      }
    } catch (err) {
      process.stderr.write(`[go-scanner] scanEntities error: ${err.message}\n`);
    }
    return entities;
  }

  _scanGORMEntities(goFiles, entities) {
    const structRe = /type\s+(\w+)\s+struct\s*\{/g;

    for (const filePath of goFiles) {
      const content = readFileSafe(filePath);
      if (!content) continue;
      // Only process files that contain gorm tags
      if (!content.includes('gorm:') && !content.includes('gorm.Model')) continue;

      structRe.lastIndex = 0;
      let m;
      while ((m = structRe.exec(content)) !== null) {
        const name = m[1];
        const bodyStart = content.indexOf('{', m.index + m[0].length - 1);
        if (bodyStart === -1) continue;
        const body = extractBraceBody(content, bodyStart);
        if (!body) continue;

        // Only include structs with gorm tags or gorm.Model
        const hasGormTag = /gorm:"[^"]*"/.test(body);
        const hasGormModel = /gorm\.Model/.test(body);
        if (!hasGormTag && !hasGormModel) continue;

        const properties = extractTaggedFields(body, 'gorm');
        const refs = [];
        const sub = [];

        // Detect relationships: field is another struct type or []StructType
        const relRe = /\w+\s+(\*?(\w+)|\[\]\*?(\w+))\s+`[^`]*gorm:"[^"]*"[^`]*`/g;
        relRe.lastIndex = 0;
        let rm;
        while ((rm = relRe.exec(body)) !== null) {
          const refName = rm[2] || rm[3];
          if (refName && refName !== name && /^[A-Z]/.test(refName)) {
            if (rm[0].includes('[]')) {
              sub.push(refName);
            } else {
              refs.push(refName);
            }
          }
        }

        const rel = relativePath(this.subprojectPath, filePath);
        entities.set(name, {
          file: rel,
          baseClass: hasGormModel ? 'gorm.Model' : undefined,
          refs: uniq(refs),
          sub: uniq(sub),
          properties,
        });
      }
    }
  }

  _scanSQLxEntities(goFiles, entities) {
    const structRe = /type\s+(\w+)\s+struct\s*\{/g;

    for (const filePath of goFiles) {
      const content = readFileSafe(filePath);
      if (!content) continue;
      if (!content.includes('db:"')) continue;

      structRe.lastIndex = 0;
      let m;
      while ((m = structRe.exec(content)) !== null) {
        const name = m[1];
        const bodyStart = content.indexOf('{', m.index + m[0].length - 1);
        if (bodyStart === -1) continue;
        const body = extractBraceBody(content, bodyStart);
        if (!body) continue;

        const hasDbTag = /db:"[^"]*"/.test(body);
        if (!hasDbTag) continue;

        const properties = extractTaggedFields(body, 'db');
        const rel = relativePath(this.subprojectPath, filePath);
        entities.set(name, {
          file: rel,
          properties,
        });
      }
    }
  }

  _scanEntEntities(entities) {
    const entDir = path.join(this.subprojectPath, 'ent', 'schema');
    if (!existsUnder(this.subprojectPath, 'ent/schema')) return;

    const goFiles = collectFiles(entDir, '.go');
    const methodRe = /func\s+\((\w+)\)\s+Fields\(\)/g;

    for (const filePath of goFiles) {
      const content = readFileSafe(filePath);
      if (!content) continue;

      methodRe.lastIndex = 0;
      let m;
      while ((m = methodRe.exec(content)) !== null) {
        const name = m[1];
        const rel = relativePath(this.subprojectPath, filePath);
        entities.set(name, {
          file: rel,
          decorators: ['ent.Schema'],
        });
      }
    }
  }

  // --------------------------------------------------------------------------
  // scanEnums
  // --------------------------------------------------------------------------

  scanEnums() {
    const enums = new Map();
    try {
      const goFiles = collectFiles(this.subprojectPath, '.go');

      for (const filePath of goFiles) {
        const content = readFileSafe(filePath);
        if (!content) continue;

        this._extractGoEnums(content, filePath, enums);
      }
    } catch (err) {
      process.stderr.write(`[go-scanner] scanEnums error: ${err.message}\n`);
    }
    return enums;
  }

  _extractGoEnums(content, filePath, enums) {
    // Match: type StatusType int|string|uint
    const typeRe = /type\s+(\w+)\s+(int|int8|int16|int32|int64|uint|uint8|uint16|uint32|uint64|string)\s*\n/g;
    typeRe.lastIndex = 0;
    let tm;
    while ((tm = typeRe.exec(content)) !== null) {
      const typeName = tm[1];
      const baseType = tm[2];
      const typePos = tm.index + tm[0].length;

      // Look for a const block after this type declaration
      const afterType = content.slice(typePos);
      const constBlockRe = /const\s*\(\s*([\s\S]*?)\)/;
      const cbm = afterType.match(constBlockRe);
      if (!cbm) continue;

      const block = cbm[1];
      const values = [];

      if (baseType === 'string') {
        // string-based: ConstName TypeName = "value"
        const strRe = new RegExp(`(\\w+)\\s+${typeName}\\s*=\\s*"([^"]*)"`, 'g');
        strRe.lastIndex = 0;
        let sm;
        while ((sm = strRe.exec(block)) !== null) {
          values.push(sm[1]);
        }
      } else {
        // iota-based: ConstName TypeName = iota  OR  ConstName (subsequent)
        const iotaRe = new RegExp(`(\\w+)\\s+${typeName}\\s*=\\s*iota`, 'g');
        iotaRe.lastIndex = 0;
        let im = iotaRe.exec(block);
        if (im) {
          // First value has = iota
          values.push(im[1]);
          // Subsequent values are bare identifiers on their own line
          const afterIota = block.slice(im.index + im[0].length);
          const bareRe = /^\s{1,}(\w+)\s*$/gm;
          bareRe.lastIndex = 0;
          let bm;
          while ((bm = bareRe.exec(afterIota)) !== null) {
            if (bm[1] !== typeName) values.push(bm[1]);
          }
        }
      }

      if (values.length === 0) continue;

      const rel = relativePath(this.subprojectPath, filePath);
      const style = baseType === 'string' ? 'string-const' : 'iota';
      enums.set(typeName, {
        values,
        file: rel,
        valueConvention: this._detectGoValueConvention(values),
        decorators: [`base:${baseType}`, `style:${style}`],
      });
    }
  }

  _detectGoValueConvention(values) {
    if (!values.length) return 'unknown';
    const upper = values.filter(v => /^[A-Z][A-Z0-9_]*$/.test(v)).length;
    const pascal = values.filter(v => /^[A-Z][a-zA-Z0-9]*$/.test(v)).length;
    const camel = values.filter(v => /^[a-z][a-zA-Z0-9]*$/.test(v)).length;
    const total = values.length;
    if (upper / total > 0.6) return 'UPPER_CASE';
    if (pascal / total > 0.6) return 'PascalCase';
    if (camel / total > 0.6) return 'camelCase';
    return 'mixed';
  }

  // --------------------------------------------------------------------------
  // scanInterfaces
  // --------------------------------------------------------------------------

  scanInterfaces() {
    const interfaces = new Map();
    try {
      const goFiles = collectFiles(this.subprojectPath, '.go');
      // interface regex: type Name interface { body }
      const ifaceRe = /type\s+(\w+)\s+interface\s*\{([^}]*)\}/gs;

      for (const filePath of goFiles) {
        const content = readFileSafe(filePath);
        if (!content) continue;

        ifaceRe.lastIndex = 0;
        let m;
        while ((m = ifaceRe.exec(content)) !== null) {
          const name = m[1];
          const body = m[2];
          const methods = this._extractInterfaceMethods(body);
          const rel = relativePath(this.subprojectPath, filePath);
          interfaces.set(name, {
            file: rel,
            methods,
          });
        }
      }
    } catch (err) {
      process.stderr.write(`[go-scanner] scanInterfaces error: ${err.message}\n`);
    }
    return interfaces;
  }

  _extractInterfaceMethods(body) {
    const methods = [];
    // Lines like: MethodName(args) ReturnType
    const methodRe = /^\s*([A-Z]\w*)\s*\([^)]*\)[^{}\n]*/gm;
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
      const goFiles = collectFiles(this.subprojectPath, '.go');

      for (const filePath of goFiles) {
        const content = readFileSafe(filePath);
        if (!content) continue;

        const endpoints = this._extractRoutes(content, framework);
        if (endpoints.length === 0) continue;

        const rel = relativePath(this.subprojectPath, filePath);
        const prefix = this._detectRoutePrefix(content, framework);

        const existing = routes.get(rel);
        if (existing) {
          existing.endpoints.push(...endpoints);
        } else {
          routes.set(rel, { file: rel, prefix, endpoints });
        }
      }
    } catch (err) {
      process.stderr.write(`[go-scanner] scanRoutes error: ${err.message}\n`);
    }
    return routes;
  }

  _extractRoutes(content, framework) {
    const endpoints = [];

    const patterns = {
      gin: /(?:r|router|group|g)\.(GET|POST|PUT|DELETE|PATCH)\s*\(\s*["']([^"']+)["']/gi,
      echo: /e\.(GET|POST|PUT|DELETE|PATCH)\s*\(\s*["']([^"']+)["']/gi,
      fiber: /app\.(Get|Post|Put|Delete|Patch)\s*\(\s*["']([^"']+)["']/gi,
      chi: /r\.(Get|Post|Put|Delete|Patch)\s*\(\s*["']([^"']+)["']/gi,
      stdlib: /http\.Handle(?:Func)?\s*\(\s*["']([^"']+)["']/gi,
    };

    const re = patterns[framework] || patterns.gin;
    re.lastIndex = 0;
    let m;

    if (framework === 'stdlib') {
      while ((m = re.exec(content)) !== null) {
        endpoints.push({ method: 'ANY', path: m[1] });
      }
    } else {
      while ((m = re.exec(content)) !== null) {
        endpoints.push({ method: m[1].toUpperCase(), path: m[2] });
      }
    }

    return endpoints;
  }

  _detectRoutePrefix(content, framework) {
    const groupPatterns = {
      gin: /(?:r|router)\.Group\s*\(\s*["']([^"']+)["']/,
      echo: /e\.Group\s*\(\s*["']([^"']+)["']/,
      fiber: /app\.Group\s*\(\s*["']([^"']+)["']/,
      chi: /r\.Route\s*\(\s*["']([^"']+)["']/,
    };
    const re = groupPatterns[framework];
    if (!re) return '/';
    const m = content.match(re);
    return m ? m[1] : '/';
  }

  // --------------------------------------------------------------------------
  // scanDtos
  // --------------------------------------------------------------------------

  scanDtos() {
    const dtos = new Map();
    try {
      const goFiles = collectFiles(this.subprojectPath, '.go');
      const dtoSuffixes = /(?:Request|Response|DTO|Dto|Input|Output)$/;
      const structRe = /type\s+(\w+)\s+struct\s*\{/g;

      for (const filePath of goFiles) {
        const content = readFileSafe(filePath);
        if (!content) continue;

        structRe.lastIndex = 0;
        let m;
        while ((m = structRe.exec(content)) !== null) {
          const name = m[1];
          if (!dtoSuffixes.test(name)) continue;

          const bodyStart = content.indexOf('{', m.index + m[0].length - 1);
          if (bodyStart === -1) continue;
          const body = extractBraceBody(content, bodyStart);
          if (!body) continue;

          // Extract JSON fields
          const jsonFields = extractTaggedFields(body, 'json');
          const rel = relativePath(this.subprojectPath, filePath);

          // Infer linked entity by stripping suffix
          const entity = name
            .replace(/Request$|Response$|DTO$|Dto$|Input$|Output$/, '') || undefined;

          dtos.set(name, {
            file: rel,
            entity: entity || undefined,
            properties: jsonFields,
          });
        }
      }
    } catch (err) {
      process.stderr.write(`[go-scanner] scanDtos error: ${err.message}\n`);
    }
    return dtos;
  }

  // --------------------------------------------------------------------------
  // scanServices
  // --------------------------------------------------------------------------

  scanServices() {
    const services = new Map();
    try {
      const goFiles = collectFiles(this.subprojectPath, '.go');
      const serviceSuffixes = /(?:Service|UseCase|UseCases)$/;
      const structRe = /type\s+(\w+)\s+struct\s*\{/g;
      const ctorRe = /func\s+New(\w+)\s*\(([^)]*)\)\s*(?:\*\w+|[A-Z]\w+)/g;

      for (const filePath of goFiles) {
        const content = readFileSafe(filePath);
        if (!content) continue;

        structRe.lastIndex = 0;
        let m;
        while ((m = structRe.exec(content)) !== null) {
          const name = m[1];
          if (!serviceSuffixes.test(name)) continue;

          const rel = relativePath(this.subprojectPath, filePath);
          const entity = name
            .replace(/Service$|UseCase$|UseCases$/, '') || undefined;

          // Look for constructor
          ctorRe.lastIndex = 0;
          let cm;
          const dependencies = [];
          while ((cm = ctorRe.exec(content)) !== null) {
            if (cm[1] === name) {
              // Extract interface deps from constructor params
              const params = cm[2];
              const paramRe = /\w+\s+([A-Z]\w+)/g;
              paramRe.lastIndex = 0;
              let pm;
              while ((pm = paramRe.exec(params)) !== null) {
                dependencies.push(pm[1]);
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
    } catch (err) {
      process.stderr.write(`[go-scanner] scanServices error: ${err.message}\n`);
    }
    return services;
  }

  // --------------------------------------------------------------------------
  // scanRepositories
  // --------------------------------------------------------------------------

  scanRepositories() {
    const repositories = new Map();
    try {
      const goFiles = collectFiles(this.subprojectPath, '.go');
      const repoSuffixes = /(?:Repository|Repo|Store)$/;
      const structRe = /type\s+(\w+)\s+struct\s*\{/g;

      for (const filePath of goFiles) {
        const content = readFileSafe(filePath);
        if (!content) continue;

        structRe.lastIndex = 0;
        let m;
        while ((m = structRe.exec(content)) !== null) {
          const name = m[1];
          if (!repoSuffixes.test(name)) continue;

          const rel = relativePath(this.subprojectPath, filePath);
          const entity = name
            .replace(/Repository$|Repo$|Store$/, '') || undefined;

          // Look for implemented interface (same file, type Name interface)
          const ifaceRe = /type\s+(I\w*(?:Repository|Repo|Store)\w*)\s+interface/g;
          ifaceRe.lastIndex = 0;
          let im;
          let iface;
          while ((im = ifaceRe.exec(content)) !== null) {
            if (im[1].includes(entity || name)) {
              iface = im[1];
              break;
            }
          }

          repositories.set(name, {
            file: rel,
            entity: entity || undefined,
            interface: iface,
          });
        }
      }
    } catch (err) {
      process.stderr.write(`[go-scanner] scanRepositories error: ${err.message}\n`);
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

      // Entity folder
      const entityFiles = [...entities.values()].map(e => e.file).filter(Boolean);
      const entityFolder = inferCommonFolder(entityFiles);

      // Entity base struct
      const entityBases = [...entities.values()].map(e => e.baseClass).filter(Boolean);
      const baseStruct = mode(entityBases) || 'none';

      // Tag style
      const tagStyle = orm === 'gorm' ? 'gorm' : orm === 'sqlx' ? 'db' : 'none';

      // Enum style
      const enumDecorators = [...enums.values()].flatMap(e => e.decorators || []);
      const hasIota = enumDecorators.some(d => d.includes('iota'));
      const hasStringConst = enumDecorators.some(d => d.includes('string-const'));
      const enumStyle = hasIota ? 'iota' : hasStringConst ? 'string-const' : 'none';
      const enumFiles = [...enums.values()].map(e => e.file).filter(Boolean);
      const uniqueEnumFiles = uniq(enumFiles);

      // Routes style
      const allEndpoints = [...routes.values()].flatMap(r => r.endpoints || []);
      const prefixes = [...routes.values()].map(r => r.prefix).filter(p => p && p !== '/');
      const commonPrefix = mode(prefixes) || '/';

      // Project layout
      const hasCmd = existsUnder(this.subprojectPath, 'cmd');
      const hasInternal = existsUnder(this.subprojectPath, 'internal');
      const projectLayout = (hasCmd && hasInternal) ? 'standard' : 'flat';

      return {
        framework: framework === 'stdlib' ? 'stdlib' : framework,
        orm,
        entity: {
          folder: entityFolder,
          baseStruct,
          tagStyle,
        },
        enum: {
          style: enumStyle,
          separateFiles: uniqueEnumFiles.length > 1,
        },
        routes: {
          style: framework,
          prefix: commonPrefix,
          groupPattern: prefixes.length > 0,
        },
        projectLayout,
      };
    } catch (err) {
      process.stderr.write(`[go-scanner] inferPatterns error: ${err.message}\n`);
      return {};
    }
  }
}

module.exports = GoScanner;
