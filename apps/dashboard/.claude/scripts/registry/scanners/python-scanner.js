'use strict';

/**
 * python-scanner.js
 *
 * Python stack scanner for sync-registry.js.
 * Scans Python projects for entities, enums, interfaces, routes, DTOs,
 * services, repositories and infers architectural patterns.
 *
 * Supports: FastAPI, Django, Flask, SQLAlchemy, SQLModel, Pydantic, Alembic.
 * Extends ScannerContract — fail-open, no external dependencies.
 */

const { ScannerContract } = require('../scanner-contract');
const { collectFiles, relativePath, readFileSafe, inferCommonFolder } = require('../file-utils');
const path = require('path');
const fs = require('fs');

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/**
 * Check if a path exists under the subproject root.
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
 * List immediate subdirectory names of a directory.
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
 * Deduplicate an array preserving order.
 * @param {string[]} arr
 * @returns {string[]}
 */
function uniq(arr) {
  return [...new Set(arr)];
}

/**
 * Detect value convention from enum member names.
 * @param {string[]} values
 * @returns {string}
 */
function detectValueConvention(values) {
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

/**
 * Normalise a route prefix segment for safe comparison.
 * @param {string} prefix
 * @returns {string}
 */
function normalisePrefix(prefix) {
  if (!prefix) return '/';
  return prefix.startsWith('/') ? prefix : '/' + prefix;
}

/**
 * Extract APIRouter prefix from a content block.
 * @param {string} content
 * @returns {string|null}
 */
function extractRouterPrefix(content) {
  const m = content.match(/APIRouter\s*\([^)]*prefix\s*=\s*["']([^"']+)["']/);
  return m ? m[1] : null;
}

// ---------------------------------------------------------------------------
// PythonScanner
// ---------------------------------------------------------------------------

class PythonScanner extends ScannerContract {
  static stackId = 'python';

  // -------------------------------------------------------------------------
  // detect
  // -------------------------------------------------------------------------

  detect() {
    try {
      return (
        existsUnder(this.subprojectPath, 'pyproject.toml') ||
        existsUnder(this.subprojectPath, 'setup.py') ||
        existsUnder(this.subprojectPath, 'requirements.txt') ||
        existsUnder(this.subprojectPath, 'manage.py')
      );
    } catch (err) {
      process.stderr.write(`[python-scanner] detect error: ${err.message}\n`);
      return false;
    }
  }

  // -------------------------------------------------------------------------
  // detectArchitecture
  // -------------------------------------------------------------------------

  detectArchitecture() {
    try {
      const dirs = new Set(listDirs(this.subprojectPath));

      // Walk one level deeper to catch src-layout projects
      const nestedDirs = new Set();
      for (const d of dirs) {
        try {
          listDirs(path.join(this.subprojectPath, d)).forEach(sub => nestedDirs.add(sub));
        } catch { /* ignore */ }
      }
      const all = new Set([...dirs, ...nestedDirs]);

      if (all.has('domain') && all.has('application') && all.has('infrastructure')) {
        return 'clean-architecture';
      }
      if (all.has('repositories') && all.has('services')) {
        return 'repository-service';
      }
      if ((all.has('routers') || all.has('views')) && all.has('models') && all.has('schemas')) {
        return 'layered';
      }
      return 'minimal';
    } catch (err) {
      process.stderr.write(`[python-scanner] detectArchitecture error: ${err.message}\n`);
      return 'unknown';
    }
  }

  // -------------------------------------------------------------------------
  // _detectFrameworks — internal helper, called once per scan
  // -------------------------------------------------------------------------

  _detectFrameworks() {
    if (this._frameworks) return this._frameworks;

    const frameworks = { fastapi: false, django: false, flask: false };
    const orm = { sqlalchemy: false, djangoOrm: false, sqlmodel: false, tortoise: false };
    const hasPydantic = { pydantic: false };

    try {
      const pyFiles = collectFiles(this.subprojectPath, '.py');
      for (const filePath of pyFiles) {
        const content = readFileSafe(filePath);
        if (!content) continue;

        if (!frameworks.fastapi && (
          /from\s+fastapi\s+import/.test(content) ||
          /@(?:app|router)\.get\s*\(/.test(content)
        )) frameworks.fastapi = true;

        if (!frameworks.django && (
          /from\s+django\.db\s+import\s+models/.test(content) ||
          /models\.Model/.test(content)
        )) frameworks.django = true;

        if (!frameworks.flask && (
          /from\s+flask\s+import/.test(content) ||
          /@\w+\.route\s*\(/.test(content)
        )) frameworks.flask = true;

        if (!orm.sqlalchemy && (
          /from\s+sqlalchemy/.test(content) ||
          /declarative_base\s*\(\s*\)/.test(content) ||
          /DeclarativeBase/.test(content)
        )) orm.sqlalchemy = true;

        if (!orm.djangoOrm && /from\s+django\.db\s+import\s+models/.test(content)) {
          orm.djangoOrm = true;
        }

        if (!orm.sqlmodel && (
          /from\s+sqlmodel\s+import/.test(content)
        )) orm.sqlmodel = true;

        if (!orm.tortoise && /from\s+tortoise/.test(content)) orm.tortoise = true;

        if (!hasPydantic.pydantic && (
          /from\s+pydantic\s+import/.test(content) ||
          /BaseModel/.test(content)
        )) hasPydantic.pydantic = true;
      }

      const hasAlembic = existsUnder(this.subprojectPath, 'alembic');

      this._frameworks = { ...frameworks, ...orm, ...hasPydantic, hasAlembic };
    } catch (err) {
      process.stderr.write(`[python-scanner] _detectFrameworks error: ${err.message}\n`);
      this._frameworks = {
        fastapi: false, django: false, flask: false,
        sqlalchemy: false, djangoOrm: false, sqlmodel: false,
        tortoise: false, pydantic: false, hasAlembic: false,
      };
    }

    return this._frameworks;
  }

  // -------------------------------------------------------------------------
  // scanEntities
  // -------------------------------------------------------------------------

  scanEntities() {
    const entities = new Map();

    try {
      const pyFiles = collectFiles(this.subprojectPath, '.py');

      for (const filePath of pyFiles) {
        const content = readFileSafe(filePath);
        if (!content) continue;

        const rel = relativePath(this.subprojectPath, filePath);

        // --- SQLAlchemy entities ---
        const sqlaRe = /class\s+(\w+)\s*\(\s*(?:Base|DeclarativeBase|Model)\s*\)/g;
        let m;
        while ((m = sqlaRe.exec(content)) !== null) {
          const name = m[1];
          const info = this._extractSqlaEntity(content, name, rel);
          if (info) entities.set(name, info);
        }

        // --- Django models ---
        const djangoRe = /class\s+(\w+)\s*\(\s*models\.Model\s*\)/g;
        djangoRe.lastIndex = 0;
        while ((m = djangoRe.exec(content)) !== null) {
          const name = m[1];
          const info = this._extractDjangoEntity(content, name, rel);
          if (info) entities.set(name, info);
        }

        // --- SQLModel entities ---
        const sqlmodelRe = /class\s+(\w+)\s*\(\s*SQLModel\s*,\s*table\s*=\s*True\s*\)/g;
        sqlmodelRe.lastIndex = 0;
        while ((m = sqlmodelRe.exec(content)) !== null) {
          const name = m[1];
          entities.set(name, { file: rel, baseClass: 'SQLModel', decorators: ['SQLModel'], properties: [], refs: [], enums: [] });
        }
      }
    } catch (err) {
      process.stderr.write(`[python-scanner] scanEntities error: ${err.message}\n`);
    }

    return entities;
  }

  /** @private */
  _extractSqlaEntity(content, name, rel) {
    try {
      const tableNameMatch = content.match(new RegExp(
        `class\\s+${name}[^:]*:[\\s\\S]*?__tablename__\\s*=\\s*['"]([^'"]+)['"]`
      ));
      const tableName = tableNameMatch ? tableNameMatch[1] : null;

      // Extract Column definitions
      const colRe = /(\w+)\s*[=:]\s*(?:mapped_column|Column)\s*\(\s*(\w+)/g;
      colRe.lastIndex = 0;
      const properties = [];
      let cm;
      while ((cm = colRe.exec(content)) !== null) {
        properties.push(`${cm[1]}: ${cm[2]}`);
      }

      // Extract relationships
      const relRe = /relationship\s*\(\s*["'](\w+)["']/g;
      relRe.lastIndex = 0;
      const refs = [];
      let rm;
      while ((rm = relRe.exec(content)) !== null) {
        refs.push(rm[1]);
      }

      return {
        file: rel,
        baseClass: 'Base',
        decorators: ['SQLAlchemy'],
        properties: uniq(properties),
        refs: uniq(refs),
        enums: [],
        ...(tableName ? { tableName } : {}),
      };
    } catch {
      return { file: rel, baseClass: 'Base', properties: [], refs: [], enums: [] };
    }
  }

  /** @private */
  _extractDjangoEntity(content, name, rel) {
    try {
      // Extract field definitions
      const fieldRe = /(\w+)\s*=\s*models\.(\w+)\s*\(/g;
      fieldRe.lastIndex = 0;
      const properties = [];
      let fm;
      while ((fm = fieldRe.exec(content)) !== null) {
        if (fm[1] !== 'class') properties.push(`${fm[1]}: ${fm[2]}`);
      }

      // Extract ForeignKey refs
      const fkRe = /models\.ForeignKey\s*\(\s*["'](\w+)["']/g;
      fkRe.lastIndex = 0;
      const refs = [];
      let fk;
      while ((fk = fkRe.exec(content)) !== null) {
        refs.push(fk[1]);
      }

      return {
        file: rel,
        baseClass: 'models.Model',
        decorators: ['Django'],
        properties: uniq(properties),
        refs: uniq(refs),
        enums: [],
      };
    } catch {
      return { file: rel, baseClass: 'models.Model', properties: [], refs: [], enums: [] };
    }
  }

  // -------------------------------------------------------------------------
  // scanEnums
  // -------------------------------------------------------------------------

  scanEnums() {
    const enums = new Map();

    try {
      const pyFiles = collectFiles(this.subprojectPath, '.py');

      const enumRe = /class\s+(\w+)\s*\(\s*(?:str\s*,\s*)?(?:Enum|IntEnum)\s*\)/g;

      for (const filePath of pyFiles) {
        const content = readFileSafe(filePath);
        if (!content) continue;

        const rel = relativePath(this.subprojectPath, filePath);

        enumRe.lastIndex = 0;
        let m;
        while ((m = enumRe.exec(content)) !== null) {
          const name = m[1];

          // Determine backed type
          const declaration = content.slice(m.index, m.index + 80);
          const isBacked = /str\s*,\s*Enum/.test(declaration);
          const isIntEnum = /IntEnum/.test(declaration);
          const backedType = isBacked ? 'str' : (isIntEnum ? 'int' : null);

          // Extract members — lines of form: MEMBER = value
          const memberRe = /^\s+(\w+)\s*=\s*['"]?(\w+)/gm;
          memberRe.lastIndex = 0;
          const values = [];
          let vm;
          while ((vm = memberRe.exec(content)) !== null) {
            // Skip dunder and non-upper members that look like class attrs
            if (!vm[1].startsWith('_')) values.push(vm[1]);
          }

          enums.set(name, {
            values: uniq(values),
            file: rel,
            valueConvention: detectValueConvention(values),
            ...(backedType ? { backedType } : {}),
          });
        }
      }
    } catch (err) {
      process.stderr.write(`[python-scanner] scanEnums error: ${err.message}\n`);
    }

    return enums;
  }

  // -------------------------------------------------------------------------
  // scanInterfaces
  // -------------------------------------------------------------------------

  scanInterfaces() {
    const interfaces = new Map();

    try {
      const pyFiles = collectFiles(this.subprojectPath, '.py');

      // Abstract classes (ABC) and Protocol types
      const abstractRe = /class\s+(\w+)\s*\(\s*(?:ABC|Protocol)\s*\)/g;

      for (const filePath of pyFiles) {
        const content = readFileSafe(filePath);
        if (!content) continue;

        const rel = relativePath(this.subprojectPath, filePath);
        const hasProtocol = /from\s+typing\s+import\s+[^#\n]*Protocol/.test(content);

        abstractRe.lastIndex = 0;
        let m;
        while ((m = abstractRe.exec(content)) !== null) {
          const name = m[1];

          // Extract abstract method signatures
          const methodRe = /(?:@abstractmethod\s+)?def\s+(\w+)\s*\(self[^)]*\)/g;
          methodRe.lastIndex = 0;
          const methods = [];
          let mm;
          while ((mm = methodRe.exec(content)) !== null) {
            if (mm[1] !== '__init__') methods.push(mm[1]);
          }

          interfaces.set(name, {
            file: rel,
            methods: uniq(methods),
            ...(hasProtocol ? { protocol: true } : {}),
          });
        }
      }
    } catch (err) {
      process.stderr.write(`[python-scanner] scanInterfaces error: ${err.message}\n`);
    }

    return interfaces;
  }

  // -------------------------------------------------------------------------
  // scanRoutes
  // -------------------------------------------------------------------------

  scanRoutes() {
    const routes = new Map();

    try {
      const pyFiles = collectFiles(this.subprojectPath, '.py');
      const fw = this._detectFrameworks();

      for (const filePath of pyFiles) {
        const content = readFileSafe(filePath);
        if (!content) continue;

        const rel = relativePath(this.subprojectPath, filePath);
        const endpoints = [];

        if (fw.fastapi) {
          // Detect APIRouter prefix
          const routerPrefix = extractRouterPrefix(content);

          const fastapiRe = /@(?:app|router)\.(get|post|put|delete|patch)\s*\(\s*["']([^"']+)["']/g;
          fastapiRe.lastIndex = 0;
          let fm;
          while ((fm = fastapiRe.exec(content)) !== null) {
            endpoints.push({
              method: fm[1].toUpperCase(),
              path: fm[2],
              name: null,
              auth: /dependencies\s*=|Depends/.test(content),
            });
          }

          if (endpoints.length > 0) {
            routes.set(rel, {
              file: rel,
              prefix: routerPrefix ? normalisePrefix(routerPrefix) : '/',
              endpoints,
            });
          }
        }

        if (fw.django) {
          // Only scan urls.py files
          if (path.basename(filePath) === 'urls.py') {
            const djangoRe = /path\s*\(\s*["']([^"']+)["']/g;
            djangoRe.lastIndex = 0;
            let dm;
            while ((dm = djangoRe.exec(content)) !== null) {
              endpoints.push({ method: 'ANY', path: dm[1], name: null, auth: false });
            }

            if (endpoints.length > 0) {
              routes.set(rel, { file: rel, prefix: '/', endpoints });
            }
          }
        }

        if (fw.flask) {
          const flaskRe = /@\w+\.route\s*\(\s*["']([^"']+)["']/g;
          flaskRe.lastIndex = 0;
          let flm;
          while ((flm = flaskRe.exec(content)) !== null) {
            // Check methods=[...] for HTTP verb
            const after = content.slice(flm.index, flm.index + 120);
            const methodsMatch = after.match(/methods\s*=\s*\[([^\]]+)\]/);
            const methods = methodsMatch
              ? methodsMatch[1].replace(/['"]/g, '').split(',').map(s => s.trim())
              : ['GET'];

            for (const method of methods) {
              endpoints.push({ method, path: flm[1], name: null, auth: false });
            }
          }

          if (endpoints.length > 0) {
            routes.set(rel, { file: rel, prefix: '/', endpoints });
          }
        }
      }
    } catch (err) {
      process.stderr.write(`[python-scanner] scanRoutes error: ${err.message}\n`);
    }

    return routes;
  }

  // -------------------------------------------------------------------------
  // scanDtos — Pydantic schemas
  // -------------------------------------------------------------------------

  scanDtos() {
    const dtos = new Map();

    try {
      const pyFiles = collectFiles(this.subprojectPath, '.py');

      const pydanticRe = /class\s+(\w+)\s*\(\s*(?:BaseModel|BaseSchema)\s*\)/g;
      // Suffixes that indicate DTO/schema intent
      const dtoSuffixRe = /(?:Schema|Request|Response|Create|Update)$/;

      for (const filePath of pyFiles) {
        const content = readFileSafe(filePath);
        if (!content) continue;

        const rel = relativePath(this.subprojectPath, filePath);

        pydanticRe.lastIndex = 0;
        let m;
        while ((m = pydanticRe.exec(content)) !== null) {
          const name = m[1];

          // Only DTO-suffix classes OR all Pydantic classes depending on context
          const isDto = dtoSuffixRe.test(name);

          // Infer linked entity name
          const entity = name
            .replace(/(?:Schema|Request|Response|Create|Update|Dto)$/, '') || null;

          dtos.set(name, {
            file: rel,
            entity: entity !== name ? entity : null,
            validationPattern: 'pydantic',
            isDtoShape: isDto,
          });
        }
      }
    } catch (err) {
      process.stderr.write(`[python-scanner] scanDtos error: ${err.message}\n`);
    }

    return dtos;
  }

  // -------------------------------------------------------------------------
  // scanServices
  // -------------------------------------------------------------------------

  scanServices() {
    const services = new Map();

    try {
      const pyFiles = collectFiles(this.subprojectPath, '.py');
      const serviceRe = /class\s+(\w+(?:Service|UseCase))/g;

      for (const filePath of pyFiles) {
        const content = readFileSafe(filePath);
        if (!content) continue;

        const rel = relativePath(this.subprojectPath, filePath);

        serviceRe.lastIndex = 0;
        let m;
        while ((m = serviceRe.exec(content)) !== null) {
          const name = m[1];

          // Extract constructor dependencies
          const initMatch = content.match(/def\s+__init__\s*\(self\s*,([^)]+)\)/);
          const dependencies = [];
          if (initMatch) {
            const params = initMatch[1].split(',');
            for (const param of params) {
              const typed = param.trim().match(/(\w+)\s*:\s*(\w+)/);
              if (typed && typed[2] !== 'str' && typed[2] !== 'int' && typed[2] !== 'bool') {
                dependencies.push(typed[2]);
              }
            }
          }

          // Infer linked entity
          const entity = name.replace(/(?:Service|UseCase)$/, '') || null;

          services.set(name, {
            file: rel,
            entity: entity !== name ? entity : null,
            dependencies: uniq(dependencies),
          });
        }
      }
    } catch (err) {
      process.stderr.write(`[python-scanner] scanServices error: ${err.message}\n`);
    }

    return services;
  }

  // -------------------------------------------------------------------------
  // scanRepositories
  // -------------------------------------------------------------------------

  scanRepositories() {
    const repos = new Map();

    try {
      const pyFiles = collectFiles(this.subprojectPath, '.py');
      const repoRe = /class\s+(\w+Repository)/g;

      for (const filePath of pyFiles) {
        const content = readFileSafe(filePath);
        if (!content) continue;

        const rel = relativePath(this.subprojectPath, filePath);

        repoRe.lastIndex = 0;
        let m;
        while ((m = repoRe.exec(content)) !== null) {
          const name = m[1];
          const entity = name.replace(/Repository$/, '') || null;

          // Detect base class
          const baseMatch = content.match(new RegExp(
            `class\\s+${name}\\s*\\(\\s*(\\w+)\\s*\\)`
          ));
          const baseClass = baseMatch ? baseMatch[1] : null;

          repos.set(name, {
            file: rel,
            entity: entity !== name ? entity : null,
            baseClass,
          });
        }
      }
    } catch (err) {
      process.stderr.write(`[python-scanner] scanRepositories error: ${err.message}\n`);
    }

    return repos;
  }

  // -------------------------------------------------------------------------
  // inferPatterns
  // -------------------------------------------------------------------------

  inferPatterns(scanResults) {
    try {
      const { entities, enums, dtos, routes, repositories } = scanResults;
      const fw = this._detectFrameworks();

      // Framework
      let framework = 'none';
      if (fw.fastapi) framework = 'fastapi';
      else if (fw.django) framework = 'django';
      else if (fw.flask) framework = 'flask';

      // ORM
      let orm = 'none';
      if (fw.sqlmodel) orm = 'sqlmodel';
      else if (fw.sqlalchemy) orm = 'sqlalchemy';
      else if (fw.djangoOrm) orm = 'django-orm';
      else if (fw.tortoise) orm = 'tortoise';

      // Entity patterns
      const entityFiles = [...entities.values()].map(e => e.file);
      const entityFolder = inferCommonFolder(entityFiles);
      const baseClasses = [...entities.values()].map(e => e.baseClass).filter(Boolean);
      const baseClass = baseClasses.length > 0
        ? baseClasses.sort((a, b) =>
          baseClasses.filter(x => x === b).length - baseClasses.filter(x => x === a).length
        )[0]
        : null;

      // Enum patterns
      const enumFiles = [...enums.values()].map(e => e.file);
      const enumFolder = inferCommonFolder(enumFiles);
      const backedEnums = [...enums.values()].filter(e => e.backedType);
      const backed = backedEnums.length > 0;
      const enumSeparateFiles = enumFolder !== entityFolder;

      // Schema / DTO patterns
      const dtoFiles = [...dtos.values()].map(d => d.file);
      const schemaFolder = inferCommonFolder(dtoFiles);
      const schemaTool = fw.pydantic ? 'pydantic' : 'none';

      // Routes
      const allEndpoints = [...routes.values()].flatMap(r => r.endpoints || []);
      const routeStyle = fw.django ? 'urlconf' : 'decorator';

      // Versioning detection
      const allPaths = allEndpoints.map(e => e.path || '');
      const hasVersioning = allPaths.some(p => /\/v\d+\//.test(p) || p.startsWith('/v'));
      const versioningStrategy = hasVersioning ? 'path-prefix' : 'none';

      // Repo base type
      const repoBases = [...repositories.values()].map(r => r.baseClass).filter(Boolean);
      const repoBaseType = repoBases.length > 0 ? repoBases[0] : 'custom';

      // Route prefix detection
      const routePrefixes = [...routes.values()].map(r => r.prefix).filter(p => p && p !== '/');
      const routePrefix = routePrefixes.length > 0 ? routePrefixes[0] : null;

      return {
        framework,
        orm,
        entity: {
          folder: entityFolder,
          tableNaming: 'snake_case',
          baseClass,
        },
        enum: {
          folder: enumFolder,
          backed,
          separateFiles: enumSeparateFiles,
        },
        schema: {
          folder: schemaFolder,
          tool: schemaTool,
        },
        routes: {
          style: routeStyle,
          prefix: routePrefix,
          versioningStrategy,
        },
        repository: {
          baseType: repoBaseType,
        },
      };
    } catch (err) {
      process.stderr.write(`[python-scanner] inferPatterns error: ${err.message}\n`);
      return {};
    }
  }
}

module.exports = PythonScanner;
