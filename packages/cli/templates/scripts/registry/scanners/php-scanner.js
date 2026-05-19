'use strict';

/**
 * php-scanner.js
 *
 * Stack scanner for PHP / Laravel projects.
 * Detects via composer.json + artisan file (or laravel/framework in composer.json).
 * Scans Eloquent models, PHP 8.1+ enums, interfaces, Laravel routes, Form Requests /
 * API Resources (DTOs), services, repositories, and infers patterns (framework,
 * validation style, auth guard, repository strategy).
 */

const { ScannerContract } = require('../scanner-contract');
const { collectFiles, relativePath, readFileSafe, inferCommonFolder } = require('../file-utils');
const path = require('path');
const fs = require('fs');

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/**
 * Read and parse composer.json from subprojectPath.
 * @param {string} subprojectPath
 * @returns {Object|null}
 */
function readComposer(subprojectPath) {
  try {
    const content = readFileSafe(path.join(subprojectPath, 'composer.json'));
    if (!content) return null;
    return JSON.parse(content);
  } catch { return null; }
}

/**
 * Check if a PHP file path is inside any of the given folder segments.
 * @param {string} relPath - relative forward-slash path
 * @param {string[]} segments
 * @returns {boolean}
 */
function inFolder(relPath, segments) {
  const lower = relPath.toLowerCase();
  return segments.some(s => lower.includes(`/${s.toLowerCase()}/`) || lower.startsWith(`${s.toLowerCase()}/`));
}

/**
 * Detect if a directory (or its children) with the given name exists under basePath.
 * @param {string} basePath
 * @param {string} name
 * @returns {boolean}
 */
function hasDirAnywhere(basePath, name) {
  const lower = name.toLowerCase();
  try {
    const walk = (dir) => {
      let entries;
      try { entries = fs.readdirSync(dir, { withFileTypes: true }); } catch { return false; }
      for (const e of entries) {
        if (!e.isDirectory()) continue;
        if (e.name.startsWith('.') || e.name === 'vendor' || e.name === 'node_modules') continue;
        if (e.name.toLowerCase() === lower) return true;
        if (walk(path.join(dir, e.name))) return true;
      }
      return false;
    };
    return walk(basePath);
  } catch { return false; }
}

/**
 * Extract all PHP class/trait members that look like relationship methods.
 * @param {string} content - file content
 * @returns {string[]} - relationship method names
 */
function extractRelationships(content) {
  const rels = [];
  const relRe = /public\s+function\s+(\w+)\s*\(\s*\)[^{]*\{[^}]*\b(hasOne|hasMany|belongsTo|belongsToMany|morphTo|morphMany|morphOne|hasManyThrough|hasOneThrough)\s*\(/g;
  let m;
  while ((m = relRe.exec(content)) !== null) {
    rels.push(m[1]);
  }
  return rels;
}

/**
 * Extract the first value of a PHP property from file content.
 * Handles array form: protected $prop = ['a', 'b', ...];
 * @param {string} content
 * @param {string} propName - e.g., 'table', 'fillable'
 * @returns {string|string[]|null}
 */
function extractProp(content, propName) {
  // Single-value: protected $table = 'users';
  const singleRe = new RegExp(`\\$${propName}\\s*=\\s*['"]([^'"]+)['"]`);
  const sm = singleRe.exec(content);
  if (sm) return sm[1];

  // Array form
  const arrayRe = new RegExp(`\\$${propName}\\s*=\\s*\\[([^\\]]*?)\\]`, 's');
  const am = arrayRe.exec(content);
  if (am) {
    return am[1]
      .split(',')
      .map(v => v.trim().replace(/['"]/g, ''))
      .filter(Boolean);
  }
  return null;
}

// ---------------------------------------------------------------------------
// PhpScanner
// ---------------------------------------------------------------------------

class PhpScanner extends ScannerContract {
  static stackId = 'php';

  // -------------------------------------------------------------------------
  // detect
  // -------------------------------------------------------------------------

  detect() {
    try {
      const composerPath = path.join(this.subprojectPath, 'composer.json');
      if (!fs.existsSync(composerPath)) return false;

      // artisan file → definitely Laravel
      if (fs.existsSync(path.join(this.subprojectPath, 'artisan'))) return true;

      // Check composer.json for laravel/framework
      const composer = readComposer(this.subprojectPath);
      if (!composer) return false;

      const require = { ...((composer.require) || {}), ...((composer['require-dev']) || {}) };
      return Object.keys(require).some(k => k === 'laravel/framework' || k.startsWith('laravel/'));
    } catch { return false; }
  }

  // -------------------------------------------------------------------------
  // detectArchitecture
  // -------------------------------------------------------------------------

  detectArchitecture() {
    try {
      const appPath = path.join(this.subprojectPath, 'app');

      // DDD: Domain / Application / Infrastructure
      if (
        hasDirAnywhere(appPath, 'Domain') &&
        hasDirAnywhere(appPath, 'Application') &&
        hasDirAnywhere(appPath, 'Infrastructure')
      ) return 'ddd';

      // Modular: Modules/ with self-contained sub-folders
      if (hasDirAnywhere(this.subprojectPath, 'Modules')) return 'modular';

      // Repository-service pattern
      const hasRepos = hasDirAnywhere(appPath, 'Repositories') || hasDirAnywhere(appPath, 'Repository');
      const hasSvcs = hasDirAnywhere(appPath, 'Services') || hasDirAnywhere(appPath, 'Service');
      if (hasRepos && hasSvcs) return 'repository-service';

      // Standard Laravel MVC
      if (
        hasDirAnywhere(appPath, 'Models') &&
        hasDirAnywhere(appPath, 'Controllers')
      ) return 'mvc';

      return 'minimal';
    } catch { return 'unknown'; }
  }

  // -------------------------------------------------------------------------
  // scanEntities — Eloquent Models
  // -------------------------------------------------------------------------

  scanEntities() {
    const result = new Map();
    try {
      const phpFiles = collectFiles(this.subprojectPath, '.php', ['vendor', 'storage', 'bootstrap/cache']);
      // Eloquent: extends Model | Authenticatable | Pivot
      const modelRe = /class\s+(\w+)\s+extends\s+(Model|Authenticatable|Pivot|MorphPivot)\b/g;

      for (const filePath of phpFiles) {
        const content = readFileSafe(filePath);
        if (!content) continue;

        const rel = relativePath(this.subprojectPath, filePath);

        modelRe.lastIndex = 0;
        let match;
        while ((match = modelRe.exec(content)) !== null) {
          const [, name, baseClass] = match;

          // Traits
          const traitRe = /use\s+([\w,\s\\]+);/g;
          const decorators = [];
          let tm;
          while ((tm = traitRe.exec(content)) !== null) {
            const traits = tm[1].split(',').map(t => t.trim().replace(/\\/g, '\\'));
            for (const trait of traits) {
              const shortName = trait.split('\\').pop();
              if (['HasFactory', 'SoftDeletes', 'HasUuids', 'HasTimestamps', 'Notifiable'].includes(shortName)) {
                decorators.push(shortName);
              }
            }
          }

          // Relationships
          const rels = extractRelationships(content);

          // $table
          const table = extractProp(content, 'table');
          // $fillable
          const fillable = extractProp(content, 'fillable');

          result.set(name, {
            file: rel,
            baseClass,
            decorators: decorators.length ? [...new Set(decorators)] : undefined,
            refs: rels.length ? [...new Set(rels)] : undefined,
            properties: Array.isArray(fillable)
              ? fillable.slice(0, 10)
              : (typeof fillable === 'string' ? [fillable] : undefined),
            table: typeof table === 'string' ? table : undefined,
          });
        }
      }
    } catch (err) {
      process.stderr.write(`[php-scanner] scanEntities error: ${err.message}\n`);
    }
    return result;
  }

  // -------------------------------------------------------------------------
  // scanEnums — PHP 8.1+ Enums
  // -------------------------------------------------------------------------

  scanEnums() {
    const result = new Map();
    try {
      const phpFiles = collectFiles(this.subprojectPath, '.php', ['vendor', 'storage', 'bootstrap/cache']);
      const enumRe = /enum\s+(\w+)(?:\s*:\s*(string|int))?\s*(?:implements\s+([^{]+?))?\s*\{/g;
      const caseRe = /case\s+(\w+)\s*(?:=\s*(['"]?)([^'";]+)\2)?\s*;/g;

      for (const filePath of phpFiles) {
        const content = readFileSafe(filePath);
        if (!content) continue;

        const rel = relativePath(this.subprojectPath, filePath);

        enumRe.lastIndex = 0;
        let match;
        while ((match = enumRe.exec(content)) !== null) {
          const [, name, backingType, implementsClause] = match;

          // Collect cases: find the enum body
          const enumBodyStart = match.index + match[0].length;
          // Find matching closing brace
          let depth = 1;
          let i = enumBodyStart;
          while (i < content.length && depth > 0) {
            if (content[i] === '{') depth++;
            else if (content[i] === '}') depth--;
            i++;
          }
          const body = content.slice(enumBodyStart, i - 1);

          const values = [];
          caseRe.lastIndex = 0;
          let cm;
          while ((cm = caseRe.exec(body)) !== null) {
            values.push(cm[1]);
          }

          // Filament-style interfaces
          const interfaces = implementsClause
            ? implementsClause.split(',').map(s => s.trim().split('\\').pop()).filter(Boolean)
            : [];

          // Detect value convention
          let valueConvention = 'PascalCase';
          if (values.length > 0) {
            if (values.every(v => v === v.toUpperCase())) valueConvention = 'UPPER_CASE';
            else if (values.every(v => v[0] === v[0].toLowerCase())) valueConvention = 'camelCase';
          }

          result.set(name, {
            values,
            file: rel,
            backed: !!backingType,
            backingType: backingType || null,
            interfaces: interfaces.length ? interfaces : undefined,
            valueConvention,
          });
        }
      }
    } catch (err) {
      process.stderr.write(`[php-scanner] scanEnums error: ${err.message}\n`);
    }
    return result;
  }

  // -------------------------------------------------------------------------
  // scanInterfaces
  // -------------------------------------------------------------------------

  scanInterfaces() {
    const result = new Map();
    try {
      const phpFiles = collectFiles(this.subprojectPath, '.php', ['vendor', 'storage', 'bootstrap/cache']);
      const ifaceRe = /interface\s+(\w+)(?:\s+extends\s+([^{]+?))?\s*\{/g;

      for (const filePath of phpFiles) {
        const content = readFileSafe(filePath);
        if (!content) continue;

        const rel = relativePath(this.subprojectPath, filePath);

        ifaceRe.lastIndex = 0;
        let match;
        while ((match = ifaceRe.exec(content)) !== null) {
          const [, name, extendsClause] = match;

          const parents = extendsClause
            ? extendsClause.split(',').map(s => s.trim().split('\\').pop()).filter(Boolean)
            : [];

          // Extract method signatures from body
          const methodRe = /public\s+function\s+(\w+)\s*\(/g;
          const methods = [];
          let mm;
          while ((mm = methodRe.exec(content)) !== null) {
            methods.push(mm[1]);
          }

          result.set(name, {
            file: rel,
            extends: parents.length ? parents : undefined,
            methods: methods.length ? methods.slice(0, 15) : undefined,
          });
        }
      }
    } catch (err) {
      process.stderr.write(`[php-scanner] scanInterfaces error: ${err.message}\n`);
    }
    return result;
  }

  // -------------------------------------------------------------------------
  // scanRoutes
  // -------------------------------------------------------------------------

  scanRoutes() {
    const result = new Map();
    try {
      const routesDir = path.join(this.subprojectPath, 'routes');
      let routeFiles = [];

      // Prefer routes/api.php and routes/web.php
      for (const name of ['api.php', 'web.php']) {
        const p = path.join(routesDir, name);
        if (fs.existsSync(p)) routeFiles.push(p);
      }
      // Also look for any other .php in routes/
      if (fs.existsSync(routesDir)) {
        try {
          for (const entry of fs.readdirSync(routesDir)) {
            if (entry.endsWith('.php') && !routeFiles.find(f => f.endsWith(entry))) {
              routeFiles.push(path.join(routesDir, entry));
            }
          }
        } catch { /* ignore */ }
      }

      // Explicit routes: Route::get('/path', ...)
      const explicitRe = /Route::(get|post|put|patch|delete|options|any)\s*\(\s*['"]([^'"]+)['"]\s*,\s*(?:\[([^\]]+)\]|(\w+::class)|(\w+))/g;
      // Resource routes: Route::resource('posts', PostController::class)
      const resourceRe = /Route::(?:api)?[Rr]esource\s*\(\s*['"]([^'"]+)['"]\s*,\s*(\w+)::class/g;
      // Route groups with prefix
      const groupPrefixRe = /Route::(?:prefix|group)\s*\(\s*['"]([^'"]+)['"]/g;

      const endpoints = [];
      let detectedPrefix = null;

      for (const filePath of routeFiles) {
        const content = readFileSafe(filePath);
        if (!content) continue;
        const rel = relativePath(this.subprojectPath, filePath);
        const isApi = path.basename(filePath) === 'api.php';

        // Detect API prefix
        if (isApi && !detectedPrefix) detectedPrefix = '/api';

        // Group prefix detection
        groupPrefixRe.lastIndex = 0;
        let gm;
        while ((gm = groupPrefixRe.exec(content)) !== null) {
          if (gm[1] !== 'api' && !detectedPrefix) detectedPrefix = `/${gm[1]}`;
        }

        // Explicit methods
        explicitRe.lastIndex = 0;
        let m;
        while ((m = explicitRe.exec(content)) !== null) {
          const [, method, routePath, controllerArr, controllerClass] = m;
          // Extract controller name
          let name = controllerArr
            ? controllerArr.split(',')[0].trim().replace(/::class/, '').split('\\').pop()
            : (controllerClass ? controllerClass.replace(/::class/, '') : undefined);

          endpoints.push({
            method: method.toUpperCase(),
            path: routePath,
            name: name || undefined,
            file: rel,
          });
        }

        // Resource routes — expand to standard 7 CRUD routes
        resourceRe.lastIndex = 0;
        while ((m = resourceRe.exec(content)) !== null) {
          const [, resourcePath, controller] = m;
          for (const ep of [
            { method: 'GET',    path: `/${resourcePath}` },
            { method: 'POST',   path: `/${resourcePath}` },
            { method: 'GET',    path: `/${resourcePath}/{id}` },
            { method: 'PUT',    path: `/${resourcePath}/{id}` },
            { method: 'PATCH',  path: `/${resourcePath}/{id}` },
            { method: 'DELETE', path: `/${resourcePath}/{id}` },
          ]) {
            endpoints.push({ ...ep, name: controller, file: rel });
          }
        }
      }

      if (endpoints.length > 0 || routeFiles.length > 0) {
        result.set('laravel', {
          file: 'routes/',
          prefix: detectedPrefix || '/',
          endpoints,
        });
      }
    } catch (err) {
      process.stderr.write(`[php-scanner] scanRoutes error: ${err.message}\n`);
    }
    return result;
  }

  // -------------------------------------------------------------------------
  // scanDtos — Form Requests + API Resources
  // -------------------------------------------------------------------------

  scanDtos() {
    const result = new Map();
    try {
      const phpFiles = collectFiles(this.subprojectPath, '.php', ['vendor', 'storage', 'bootstrap/cache']);
      // FormRequest or JsonResource base classes
      const classRe = /class\s+(\w+)\s+extends\s+(FormRequest|JsonResource|Resource)\b/g;

      for (const filePath of phpFiles) {
        const content = readFileSafe(filePath);
        if (!content) continue;
        const rel = relativePath(this.subprojectPath, filePath);

        classRe.lastIndex = 0;
        let match;
        while ((match = classRe.exec(content)) !== null) {
          const [, name, baseClass] = match;

          // Infer linked entity from name
          let entity;
          for (const suffix of ['Request', 'Resource', 'Dto', 'Response']) {
            if (name.endsWith(suffix)) {
              entity = name.slice(0, -suffix.length) || undefined;
              break;
            }
          }

          const validationPattern = baseClass === 'FormRequest' ? 'form-request'
            : baseClass === 'JsonResource' || baseClass === 'Resource' ? 'api-resource'
            : undefined;

          result.set(name, {
            file: rel,
            entity: entity || undefined,
            baseClass,
            validationPattern,
          });
        }
      }
    } catch (err) {
      process.stderr.write(`[php-scanner] scanDtos error: ${err.message}\n`);
    }
    return result;
  }

  // -------------------------------------------------------------------------
  // scanServices
  // -------------------------------------------------------------------------

  scanServices() {
    const result = new Map();
    try {
      const phpFiles = collectFiles(this.subprojectPath, '.php', ['vendor', 'storage', 'bootstrap/cache']);
      const classRe = /class\s+(\w+Service)\s*(?:extends\s+(\w+))?\s*(?:implements\s+([^{]+?))?\s*\{/g;

      for (const filePath of phpFiles) {
        const content = readFileSafe(filePath);
        if (!content) continue;
        const rel = relativePath(this.subprojectPath, filePath);

        classRe.lastIndex = 0;
        let match;
        while ((match = classRe.exec(content)) !== null) {
          const [, name, baseClass, implementsClause] = match;

          const interfaces = implementsClause
            ? implementsClause.split(',').map(s => s.trim().split('\\').pop()).filter(Boolean)
            : [];

          // Constructor injection detection
          const ctorRe = /public\s+function\s+__construct\s*\(([^)]*)\)/;
          const ctorMatch = ctorRe.exec(content);
          const dependencies = [];
          if (ctorMatch) {
            // Extract type-hinted parameters
            const params = ctorMatch[1].split(',');
            for (const param of params) {
              const typeMatch = /(?:readonly\s+)?(?:\w+\s+)?(\w+)\s+\$\w+/.exec(param.trim());
              if (typeMatch && typeMatch[1] !== 'string' && typeMatch[1] !== 'int' && typeMatch[1] !== 'bool') {
                dependencies.push(typeMatch[1]);
              }
            }
          }

          const entity = name.endsWith('Service')
            ? (name.slice(0, -7) || undefined)
            : undefined;

          result.set(name, {
            file: rel,
            interface: interfaces.length === 1 ? interfaces[0] : undefined,
            entity: entity || undefined,
            baseClass: baseClass || undefined,
            dependencies: dependencies.length ? dependencies : undefined,
          });
        }
      }
    } catch (err) {
      process.stderr.write(`[php-scanner] scanServices error: ${err.message}\n`);
    }
    return result;
  }

  // -------------------------------------------------------------------------
  // scanRepositories
  // -------------------------------------------------------------------------

  scanRepositories() {
    const result = new Map();
    try {
      const phpFiles = collectFiles(this.subprojectPath, '.php', ['vendor', 'storage', 'bootstrap/cache']);
      const classRe = /class\s+(\w+Repository)\s*(?:extends\s+(\w+))?\s*(?:implements\s+([^{]+?))?\s*\{/g;

      for (const filePath of phpFiles) {
        const content = readFileSafe(filePath);
        if (!content) continue;
        const rel = relativePath(this.subprojectPath, filePath);

        classRe.lastIndex = 0;
        let match;
        while ((match = classRe.exec(content)) !== null) {
          const [, name, baseClass, implementsClause] = match;

          const interfaces = implementsClause
            ? implementsClause.split(',').map(s => s.trim().split('\\').pop()).filter(Boolean)
            : [];

          // Eloquent-based detection: uses $this->model->
          const isEloquentBased = /\$this->model->/.test(content);

          const entity = name.endsWith('Repository')
            ? (name.slice(0, -10) || undefined)
            : undefined;

          result.set(name, {
            file: rel,
            interface: interfaces.length === 1 ? interfaces[0]
              : interfaces.find(i => i.startsWith('I') && i.endsWith('Repository')) || undefined,
            entity: entity || undefined,
            baseClass: baseClass || undefined,
            eloquentBased: isEloquentBased || undefined,
          });
        }
      }
    } catch (err) {
      process.stderr.write(`[php-scanner] scanRepositories error: ${err.message}\n`);
    }
    return result;
  }

  // -------------------------------------------------------------------------
  // inferPatterns
  // -------------------------------------------------------------------------

  inferPatterns(scanResults) {
    try {
      const composer = readComposer(this.subprojectPath) || {};
      const require = { ...(composer.require || {}), ...(composer['require-dev'] || {}) };

      // PHP version
      const phpVersion = (require['php'] || '')
        .replace(/[^0-9.]/g, '')
        .split('.')
        .slice(0, 2)
        .join('.') || null;

      // Laravel version
      const laravelVersion = (require['laravel/framework'] || '')
        .replace(/[^\d.]/g, '')
        .split('.')
        .slice(0, 2)
        .join('.') || null;

      // Auth guard detection
      let authGuard = 'none';
      if ('laravel/sanctum' in require) authGuard = 'sanctum';
      else if ('laravel/passport' in require) authGuard = 'passport';
      else if ('laravel/jetstream' in require) authGuard = 'jetstream';

      // Entity patterns
      const entityFiles = [...scanResults.entities.values()].map(e => e.file).filter(Boolean);
      const entityFolder = inferCommonFolder(entityFiles);

      // Common traits across entities
      const traitCounts = new Map();
      for (const [, info] of scanResults.entities) {
        for (const d of (info.decorators || [])) {
          traitCounts.set(d, (traitCounts.get(d) || 0) + 1);
        }
      }
      const dominantTraits = [...traitCounts.entries()]
        .sort((a, b) => b[1] - a[1])
        .slice(0, 3)
        .map(([t]) => t);

      // Enum patterns
      const enumFiles = [...scanResults.enums.values()].map(e => e.file).filter(Boolean);
      const enumFolder = inferCommonFolder(enumFiles);
      const backedEnums = [...scanResults.enums.values()].filter(e => e.backed);
      const hasBacked = backedEnums.length > 0;
      // Determine dominant backing type
      const strBacked = backedEnums.filter(e => e.backingType === 'string').length;
      const intBacked = backedEnums.filter(e => e.backingType === 'int').length;
      const backingType = hasBacked
        ? (strBacked >= intBacked ? 'string' : 'int')
        : 'none';
      const enumInEnumDir = enumFiles.filter(f => f.toLowerCase().includes('enum')).length;
      const enumSeparateFiles = enumFiles.length > 0 && (enumInEnumDir / enumFiles.length) > 0.5;

      // Routes pattern
      let routeStyle = 'explicit';
      const routeEntry = scanResults.routes.get('laravel');
      let routePrefix = '/api';
      if (routeEntry) {
        routePrefix = routeEntry.prefix || '/api';
        const resourceLike = routeEntry.endpoints.filter(e =>
          ['GET', 'POST', 'PUT', 'PATCH', 'DELETE'].includes(e.method) &&
          /\{id\}/.test(e.path)
        ).length;
        const total = routeEntry.endpoints.length;
        if (resourceLike > 0 && total > 0) {
          routeStyle = resourceLike / total > 0.4 ? 'resource' : 'mixed';
        }
      }

      // API versioning: detect /v1/, /v2/ prefixes
      const allPaths = routeEntry
        ? routeEntry.endpoints.map(e => e.path)
        : [];
      const versioningStrategy = allPaths.some(p => /\/v\d+\//.test(p)) ? 'prefix' : 'none';

      // Validation style
      const formRequestCount = [...scanResults.dtos.values()].filter(d => d.validationPattern === 'form-request').length;
      const totalDtos = scanResults.dtos.size;
      const validationStyle = totalDtos === 0
        ? 'inline'
        : formRequestCount / totalDtos > 0.5
        ? 'form-request'
        : formRequestCount > 0
        ? 'mixed'
        : 'inline';

      // Repository DI registration: check AppServiceProvider
      const appServiceProviderPath = path.join(this.subprojectPath, 'app', 'Providers', 'AppServiceProvider.php');
      const appSPContent = readFileSafe(appServiceProviderPath) || '';
      const repoRegisteredInSP = scanResults.repositories.size > 0 && appSPContent.includes('Repository');
      const diRegistration = repoRegisteredInSP ? 'AppServiceProvider' : 'none';

      // Repository interface-first
      const reposWithInterface = [...scanResults.repositories.values()].filter(r => r.interface).length;
      const interfaceFirst = scanResults.repositories.size > 0
        ? (reposWithInterface / scanResults.repositories.size) > 0.5
        : false;

      // Database driver from .env.example
      const envExample = readFileSafe(path.join(this.subprojectPath, '.env.example')) || '';
      const dbDriverMatch = /DB_CONNECTION\s*=\s*(\w+)/.exec(envExample);
      const dbDriver = dbDriverMatch ? dbDriverMatch[1] : null;

      return {
        framework: 'laravel',
        phpVersion: phpVersion || undefined,
        laravelVersion: laravelVersion || undefined,
        dbDriver: dbDriver || undefined,
        packages: {
          spatie_permission: 'spatie/laravel-permission' in require,
          filament: 'filament/filament' in require,
          livewire: 'livewire/livewire' in require || 'livewire/volt' in require,
          inertia: 'inertiajs/inertia-laravel' in require,
        },
        entity: {
          folder: entityFolder,
          traits: dominantTraits.length ? dominantTraits : undefined,
          namingConvention: 'PascalCase',
          tableNaming: 'snake_case_plural',
        },
        enum: {
          folder: enumFolder,
          backed: hasBacked,
          backingType,
          separateFiles: enumSeparateFiles,
        },
        routes: {
          style: routeStyle,
          prefix: routePrefix,
          versioningStrategy,
          namingPattern: 'resource.action',
        },
        validation: {
          style: validationStyle,
        },
        auth: {
          guard: authGuard,
        },
        repository: {
          interfaceFirst,
          diRegistration,
        },
      };
    } catch (err) {
      process.stderr.write(`[php-scanner] inferPatterns error: ${err.message}\n`);
      return {};
    }
  }
}

module.exports = PhpScanner;
