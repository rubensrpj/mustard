'use strict';

/**
 * dotnet-scanner.js
 *
 * .NET stack scanner for sync-registry.js.
 * Scans C# projects for entities, enums, interfaces, routes, DTOs,
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
 * Extract the value of the first capture group from a regex applied to content.
 * Returns null when there is no match.
 * @param {string} content
 * @param {RegExp} regex
 * @returns {string|null}
 */
function firstMatch(content, regex) {
  const m = content.match(regex);
  return m ? m[1].trim() : null;
}

/**
 * Collect all captures of a global regex applied to content.
 * @param {string} content
 * @param {RegExp} regex - must have the `g` flag
 * @param {number} [group=1]
 * @returns {string[]}
 */
function allMatches(content, regex, group = 1) {
  const results = [];
  let m;
  const re = new RegExp(regex.source, regex.flags.includes('g') ? regex.flags : regex.flags + 'g');
  while ((m = re.exec(content)) !== null) {
    if (m[group]) results.push(m[group].trim());
  }
  return results;
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
 * Deduplicate an array, preserving order.
 * @param {string[]} arr
 * @returns {string[]}
 */
function uniq(arr) {
  return [...new Set(arr)];
}

/**
 * Check if a string looks like an interface identifier (I-prefix, UpperCamelCase).
 * @param {string} s
 * @returns {boolean}
 */
function isInterface(s) {
  return /^I[A-Z]/.test(s.trim());
}

/**
 * Parse the inheritance/implementation list on the right-hand side of `:` in a class/interface declaration.
 * Returns { baseClass, interfaces }.
 * Convention: first non-I-prefixed item = base class; I-prefixed items = interfaces.
 * @param {string} rhs
 * @returns {{ baseClass: string|null, interfaces: string[] }}
 */
function parseInheritance(rhs) {
  const parts = rhs.split(',').map(s => s.trim().replace(/<[^>]+>/g, '').trim()).filter(Boolean);
  let baseClass = null;
  const interfaces = [];
  for (const p of parts) {
    if (!p) continue;
    if (isInterface(p)) {
      interfaces.push(p);
    } else if (!baseClass) {
      baseClass = p;
    }
  }
  return { baseClass, interfaces };
}

/**
 * Extract class-level decorator names from content lines above a class declaration.
 * Scans backwards from classLineIndex and stops at blank lines or non-decorator lines.
 * @param {string[]} lines
 * @param {number} classLineIndex
 * @returns {string[]}
 */
function extractClassDecorators(lines, classLineIndex) {
  const decorators = [];
  for (let i = classLineIndex - 1; i >= 0; i--) {
    const line = lines[i].trim();
    if (!line) break;
    const m = line.match(/^\[(\w+)/);
    if (m) {
      decorators.unshift(m[1]);
    } else if (!line.startsWith('//') && !line.startsWith('*') && !line.startsWith('[')) {
      break;
    }
  }
  return decorators;
}

// ---------------------------------------------------------------------------
// DotNetScanner
// ---------------------------------------------------------------------------

class DotNetScanner extends ScannerContract {
  static stackId = 'dotnet';

  // -------------------------------------------------------------------------
  // detect
  // -------------------------------------------------------------------------

  /**
   * Returns true if the subproject contains *.csproj or *.sln files.
   * @returns {boolean}
   */
  detect() {
    try {
      const entries = fs.readdirSync(this.subprojectPath);
      return entries.some(e => e.endsWith('.csproj') || e.endsWith('.sln'));
    } catch {
      return false;
    }
  }

  // -------------------------------------------------------------------------
  // detectArchitecture
  // -------------------------------------------------------------------------

  /**
   * Heuristic classification of the project's architecture:
   *   - "solid"   : interfaces in separate files + DI registration in modules
   *   - "layered" : Modules/ or Controllers/ directories, but no dedicated interface files
   *   - "minimal" : flat structure, no clear separation of concerns
   * ISP is flagged inside "solid" when multiple small interfaces are found.
   * @returns {string}
   */
  detectArchitecture() {
    try {
      const csFiles = collectFiles(this.subprojectPath, '.cs');

      let hasInterfaces = false;
      let hasDiRegistration = false;
      let hasModuleOrController = false;
      let ispCandidates = 0; // interfaces whose name contains a qualifier like Query/Approval/Command

      for (const f of csFiles) {
        const rel = relativePath(this.subprojectPath, f).toLowerCase();
        const content = readFileSafe(f);
        if (!content) continue;

        if (/public\s+interface\s+I\w+/.test(content)) {
          hasInterfaces = true;
          // ISP signal: a class inheriting 3+ interfaces
          const classDecl = content.match(/public\s+(?:abstract\s+)?class\s+\w+\s*:\s*([^{]+)\{/);
          if (classDecl) {
            const ifaces = classDecl[1].split(',').filter(p => isInterface(p.trim()));
            if (ifaces.length >= 3) ispCandidates++;
          }
          // Small focused interface names (Query, Approval, Command, Read, Write)
          if (/interface\s+I\w*(Query|Approval|Command|Read|Write|Create|Update|Delete)\w*/.test(content)) {
            ispCandidates++;
          }
        }

        if (/services\.(AddScoped|AddSingleton|AddTransient)/.test(content)) {
          hasDiRegistration = true;
        }

        if (rel.includes('modules/') || rel.includes('controllers/')) {
          hasModuleOrController = true;
        }
      }

      if (hasInterfaces && hasDiRegistration) {
        return ispCandidates > 0 ? 'solid+isp' : 'solid';
      }
      if (hasModuleOrController) return 'layered';
      return 'minimal';
    } catch {
      return 'unknown';
    }
  }

  // -------------------------------------------------------------------------
  // scanEntities
  // -------------------------------------------------------------------------

  /**
   * Scan C# entity classes.
   * Looks in files under paths containing "Entities" or "entities".
   * Also reads DbContext to catch DbSet<T> references.
   * @returns {Map<string, import('../scanner-contract').EntityInfo>}
   */
  scanEntities() {
    const entities = new Map();

    try {
      const csFiles = collectFiles(this.subprojectPath, '.cs');
      const entityFiles = csFiles.filter(f => {
        const rel = relativePath(this.subprojectPath, f);
        return /[/\\]entities[/\\]/i.test(rel) || /[/\\]domain[/\\]/i.test(rel);
      });

      // --- First pass: collect known entity names from entity files ---
      const knownEntityNames = new Set();
      for (const f of entityFiles) {
        const content = readFileSafe(f);
        if (!content) continue;
        const classMatch = content.match(/public\s+(?:abstract\s+)?class\s+(\w+)/);
        if (classMatch) knownEntityNames.add(classMatch[1]);
      }

      // Also add names from DbSet<T> in DbContext
      const dbContextFiles = csFiles.filter(f => {
        const content = readFileSafe(f);
        return content && /DbSet<\w+>/.test(content);
      });
      for (const f of dbContextFiles) {
        const content = readFileSafe(f);
        if (!content) continue;
        const matches = allMatches(content, /DbSet<(\w+)>/g);
        for (const name of matches) knownEntityNames.add(name);
      }

      // --- Second pass: full extraction on entity files ---
      for (const f of entityFiles) {
        const content = readFileSafe(f);
        if (!content) continue;
        if (!/public\s+(?:abstract\s+)?class\s+\w+/.test(content)) continue;

        const lines = content.split('\n');
        const rel = relativePath(this.subprojectPath, f);
        const namespace = firstMatch(content, /^namespace\s+([\w.]+)/m);

        const classLineIdx = lines.findIndex(l =>
          /public\s+(?:abstract\s+)?class\s+\w+/.test(l)
        );
        if (classLineIdx === -1) continue;

        const classLine = lines[classLineIdx];
        const classMatch = classLine.match(/public\s+(?:abstract\s+)?class\s+(\w+)\s*(?::\s*(.+?))?(?:\s*\{|\s*$)/);
        if (!classMatch) continue;

        const className = classMatch[1];
        const inheritanceRaw = classMatch[2] || '';
        const { baseClass, interfaces } = parseInheritance(inheritanceRaw);
        const decorators = extractClassDecorators(lines, classLineIdx);

        // Navigation properties — single entity refs
        const refs = [];
        const singleNavRe = /public\s+(\w+)\??\s+\w+\s*\{[^}]*get;/g;
        let nm;
        while ((nm = singleNavRe.exec(content)) !== null) {
          const typeName = nm[1].trim();
          if (knownEntityNames.has(typeName) && typeName !== className) {
            refs.push(typeName);
          }
        }

        // Collection navigation properties
        const sub = allMatches(content, /public\s+ICollection<(\w+)>/g).filter(
          n => n !== className
        );

        // Enum usages — property types that match known enum pattern (will be enriched after scanEnums)
        // Store raw types for now; they'll be linked in inferPatterns
        const enumUsages = [];
        const propTypeRe = /public\s+(\w+)\??\s+\w+\s*\{[^}]*get;/g;
        let pm;
        while ((pm = propTypeRe.exec(content)) !== null) {
          const t = pm[1].trim();
          // Heuristic: ALL_CAPS or Status/Type/Kind suffix → likely enum
          if (/^[A-Z][A-Za-z]*(Status|Type|Kind|State|Role|Category|Level|Priority|Mode)$/.test(t)) {
            enumUsages.push(t);
          }
        }

        entities.set(className, {
          file: rel,
          namespace: namespace || undefined,
          baseClass: baseClass || undefined,
          interfaces: interfaces.length ? interfaces : undefined,
          decorators: decorators.length ? decorators : undefined,
          refs: uniq(refs).length ? uniq(refs) : undefined,
          sub: uniq(sub).length ? uniq(sub) : undefined,
          enums: uniq(enumUsages).length ? uniq(enumUsages) : undefined,
        });
      }

      // --- DbContext pass: add any DbSet<T> entities not already found ---
      for (const f of dbContextFiles) {
        const content = readFileSafe(f);
        if (!content) continue;
        const rel = relativePath(this.subprojectPath, f);
        const matches = allMatches(content, /DbSet<(\w+)>/g);
        for (const name of matches) {
          if (!entities.has(name)) {
            entities.set(name, { file: rel });
          }
        }
      }
    } catch { /* fail-open */ }

    return entities;
  }

  // -------------------------------------------------------------------------
  // scanEnums
  // -------------------------------------------------------------------------

  /**
   * Scan C# enums.
   * @returns {Map<string, import('../scanner-contract').EnumInfo>}
   */
  scanEnums() {
    const enums = new Map();

    try {
      const csFiles = collectFiles(this.subprojectPath, '.cs');

      for (const f of csFiles) {
        const content = readFileSafe(f);
        if (!content) continue;
        if (!/public\s+enum\s+\w+/.test(content)) continue;

        const lines = content.split('\n');
        const rel = relativePath(this.subprojectPath, f);
        const namespace = firstMatch(content, /^namespace\s+([\w.]+)/m);
        const isEnumFile = /[/\\]enums[/\\]/i.test(rel);

        // Find every enum declaration in the file
        for (let i = 0; i < lines.length; i++) {
          const line = lines[i];
          const enumMatch = line.match(/public\s+enum\s+(\w+)/);
          if (!enumMatch) continue;

          const enumName = enumMatch[1];
          const classDecorators = extractClassDecorators(lines, i);

          // Extract enum body
          let braceDepth = 0;
          let bodyStart = -1;
          let bodyEnd = -1;
          for (let j = i; j < lines.length; j++) {
            for (const ch of lines[j]) {
              if (ch === '{') { braceDepth++; if (bodyStart === -1) bodyStart = j; }
              else if (ch === '}') { braceDepth--; if (braceDepth === 0) { bodyEnd = j; break; } }
            }
            if (bodyEnd !== -1) break;
          }

          if (bodyStart === -1 || bodyEnd === -1) continue;

          const bodyLines = lines.slice(bodyStart + 1, bodyEnd);
          const values = [];
          const valueDecorators = new Set();

          let pendingDecorators = [];
          for (const bl of bodyLines) {
            const trimmed = bl.trim();
            if (!trimmed || trimmed.startsWith('//')) continue;

            const decMatch = trimmed.match(/^\[(\w+)/);
            if (decMatch) {
              pendingDecorators.push(decMatch[1]);
              valueDecorators.add(decMatch[1]);
              continue;
            }

            // Value line: NAME, or NAME = 1, or NAME followed by comma
            const valMatch = trimmed.match(/^(\w+)\s*[,=]?/);
            if (valMatch && valMatch[1] !== 'public' && valMatch[1] !== 'private') {
              values.push(valMatch[1]);
              pendingDecorators = [];
            }
          }

          // Value convention
          const allUpper = values.every(v => v === v.toUpperCase());
          const valueConvention = allUpper ? 'UPPER_CASE' : 'PascalCase';

          enums.set(enumName, {
            values,
            file: rel,
            namespace: namespace || undefined,
            decorators: classDecorators.length ? classDecorators : undefined,
            valueDecorators: valueDecorators.size ? [...valueDecorators] : undefined,
            valueConvention,
            separateFile: isEnumFile,
          });
        }
      }
    } catch { /* fail-open */ }

    return enums;
  }

  // -------------------------------------------------------------------------
  // scanInterfaces
  // -------------------------------------------------------------------------

  /**
   * Scan C# interfaces. Tracks parent interfaces and cross-references implementors.
   * @returns {Map<string, import('../scanner-contract').InterfaceInfo>}
   */
  scanInterfaces() {
    const interfaces = new Map();

    try {
      const csFiles = collectFiles(this.subprojectPath, '.cs');

      // First pass — collect all interfaces
      for (const f of csFiles) {
        const content = readFileSafe(f);
        if (!content) continue;
        if (!/public\s+interface\s+I\w+/.test(content)) continue;

        const rel = relativePath(this.subprojectPath, f);
        const namespace = firstMatch(content, /^namespace\s+([\w.]+)/m);
        const lines = content.split('\n');

        for (let i = 0; i < lines.length; i++) {
          const line = lines[i];
          // Match: public interface IFoo : IBar, IBaz
          const ifaceMatch = line.match(/public\s+interface\s+(I\w+)\s*(?::\s*([^{]+))?/);
          if (!ifaceMatch) continue;

          const ifaceName = ifaceMatch[1];
          const extendsRaw = ifaceMatch[2] || '';
          const extendsInterfaces = extendsRaw
            .split(',')
            .map(s => s.trim().replace(/<[^>]+>/g, '').trim())
            .filter(s => s && isInterface(s));

          // Extract method signatures from interface body
          let braceDepth = 0;
          let bodyStart = -1, bodyEnd = -1;
          for (let j = i; j < lines.length; j++) {
            for (const ch of lines[j]) {
              if (ch === '{') { braceDepth++; if (bodyStart === -1) bodyStart = j; }
              else if (ch === '}') { braceDepth--; if (braceDepth === 0) { bodyEnd = j; break; } }
            }
            if (bodyEnd !== -1) break;
          }

          const methods = [];
          if (bodyStart !== -1 && bodyEnd !== -1) {
            const bodyLines = lines.slice(bodyStart + 1, bodyEnd);
            for (const bl of bodyLines) {
              const trimmed = bl.trim();
              // Method signature: return-type MethodName(...);
              const methodMatch = trimmed.match(/(?:Task<?[^>]*>?|void|bool|int|string|Guid|[\w<>[\],\s]+)\s+(\w+)\s*\([^)]*\)\s*;/);
              if (methodMatch) {
                methods.push(trimmed.replace(/\s+/g, ' '));
              }
            }
          }

          interfaces.set(ifaceName, {
            file: rel,
            namespace: namespace || undefined,
            extends: extendsInterfaces.length ? extendsInterfaces : undefined,
            methods: methods.length ? methods : undefined,
            implementedBy: [],
          });
        }
      }

      // Second pass — find implementing classes and link them
      for (const f of csFiles) {
        const content = readFileSafe(f);
        if (!content) continue;

        const classMatch = content.match(/public\s+(?:abstract\s+)?class\s+(\w+)\s*:\s*([^{]+)\{/);
        if (!classMatch) continue;

        const className = classMatch[1];
        const rhs = classMatch[2];
        const implemented = rhs.split(',').map(s => s.trim().replace(/<[^>]+>/g, '').trim()).filter(s => isInterface(s));

        for (const iname of implemented) {
          if (interfaces.has(iname)) {
            const info = interfaces.get(iname);
            if (!info.implementedBy.includes(className)) {
              info.implementedBy.push(className);
            }
          }
        }
      }

      // Clean up empty implementedBy arrays
      for (const [, info] of interfaces) {
        if (!info.implementedBy.length) delete info.implementedBy;
      }
    } catch { /* fail-open */ }

    return interfaces;
  }

  // -------------------------------------------------------------------------
  // scanRoutes
  // -------------------------------------------------------------------------

  /**
   * Scan Minimal API module files for route groups and endpoint mappings.
   * Detects files implementing IModule or using MapGroup.
   * @returns {Map<string, import('../scanner-contract').RouteInfo>}
   */
  scanRoutes() {
    const routes = new Map();

    try {
      const csFiles = collectFiles(this.subprojectPath, '.cs');

      for (const f of csFiles) {
        const content = readFileSafe(f);
        if (!content) continue;
        if (!/(:\s*IModule|\.MapGroup\s*\()/.test(content)) continue;

        const rel = relativePath(this.subprojectPath, f);

        // Extract module class name
        const moduleClassMatch = content.match(/public\s+class\s+(\w+)\s*:\s*[^{]*IModule/);
        const moduleName = moduleClassMatch ? moduleClassMatch[1] : path.basename(f, '.cs');

        // Extract the route group prefix
        // e.g., endpoints.MapGroup("/contracts").WithTags(...)
        const groupMatch = content.match(/\.MapGroup\s*\(\s*["']([^"']+)["']\s*\)/);
        const prefix = groupMatch ? groupMatch[1] : '/';

        // Extract individual endpoint mappings
        // ep.MapGet("/", Handler).WithName("name").WithMetadata("auth:resource/action")
        const endpoints = [];
        const epRe = /\.(Map(?:Get|Post|Put|Delete|Patch))\s*\(\s*["']([^"']*)["']\s*,\s*[\w.]+\)(?:[^;]*\.WithName\s*\(\s*["']([^"']*)["']\s*\))?(?:[^;]*\.WithMetadata\s*\(\s*["']([^"']*)["']\s*\))?/g;
        let em;
        while ((em = epRe.exec(content)) !== null) {
          const method = em[1].replace('Map', '').toUpperCase();
          const epPath = em[2] || '/';
          const name = em[3] || undefined;
          const auth = em[4] || undefined;
          endpoints.push({ method, path: epPath, name, auth });
        }

        routes.set(moduleName, {
          file: rel,
          prefix,
          endpoints,
        });
      }
    } catch { /* fail-open */ }

    return routes;
  }

  // -------------------------------------------------------------------------
  // scanDtos
  // -------------------------------------------------------------------------

  /**
   * Scan DTO classes (names ending in Dto, DTO, Request, Response, ViewModel, Vm).
   * @returns {Map<string, import('../scanner-contract').DtoInfo>}
   */
  scanDtos() {
    const dtos = new Map();

    try {
      const csFiles = collectFiles(this.subprojectPath, '.cs');

      for (const f of csFiles) {
        const content = readFileSafe(f);
        if (!content) continue;

        const rel = relativePath(this.subprojectPath, f);
        const namespace = firstMatch(content, /^namespace\s+([\w.]+)/m);

        // Find all DTO-like classes
        const classRe = /public\s+(?:class|record)\s+(\w+(?:Dto|DTO|Request|Response|ViewModel|Vm))\b/g;
        let cm;
        while ((cm = classRe.exec(content)) !== null) {
          const dtoName = cm[1];

          // Link to entity: strip known DTO suffixes to get entity name
          let entity = dtoName
            .replace(/(Response|Request|Create|Update|UpSert|Upsert|List|Detail|Summary|Query)?(Dto|DTO|ViewModel|Vm)$/, '')
            .replace(/(Response|Request|Create|Update|UpSert|Upsert|List|Detail|Summary|Query)$/, '');
          if (entity === dtoName) entity = '';

          // Validation detection
          let validationPattern = null;
          if (/AbstractValidator</.test(content)) validationPattern = 'FluentValidation';
          else if (/\[Required\]|\[MaxLength\]|\[Range\]/.test(content)) validationPattern = 'DataAnnotations';

          dtos.set(dtoName, {
            file: rel,
            namespace: namespace || undefined,
            entity: entity || undefined,
            validationPattern: validationPattern || undefined,
          });
        }
      }
    } catch { /* fail-open */ }

    return dtos;
  }

  // -------------------------------------------------------------------------
  // scanServices
  // -------------------------------------------------------------------------

  /**
   * Scan service interfaces (I*Service) and their implementations (*Service).
   * @returns {Map<string, import('../scanner-contract').ServiceInfo>}
   */
  scanServices() {
    const services = new Map();

    try {
      const csFiles = collectFiles(this.subprojectPath, '.cs');

      // Collect service interfaces first
      const serviceInterfaces = new Map(); // interface name → entity
      for (const f of csFiles) {
        const content = readFileSafe(f);
        if (!content) continue;

        const ifaceRe = /public\s+interface\s+(I\w+Service)\s*(?::\s*([^{]+))?/g;
        let im;
        while ((im = ifaceRe.exec(content)) !== null) {
          const ifaceName = im[1];
          const extendsRaw = im[2] || '';

          // Extract entity from generic base: IServiceBase<Contract, ...>
          const entityMatch = extendsRaw.match(/IServiceBase<\s*(\w+)\s*[,>]/);
          const entity = entityMatch ? entityMatch[1] : null;

          serviceInterfaces.set(ifaceName, entity);
        }
      }

      // Collect service implementations
      for (const f of csFiles) {
        const content = readFileSafe(f);
        if (!content) continue;
        if (!/class\s+\w+Service\b/.test(content)) continue;

        const rel = relativePath(this.subprojectPath, f);

        const classMatch = content.match(/public\s+(?:abstract\s+)?class\s+(\w+Service)\s*(?::\s*([^{]+))?/);
        if (!classMatch) continue;

        const className = classMatch[1];
        const rhs = classMatch[2] || '';

        // Find which interface this class implements
        const implementedIface = rhs.split(',')
          .map(s => s.trim().replace(/<[^>]+>/g, '').trim())
          .find(s => isInterface(s) && s.endsWith('Service'));

        // Entity from interface map
        const entity = implementedIface ? serviceInterfaces.get(implementedIface) : null;

        // Extract constructor dependencies (injected interfaces)
        const ctorMatch = content.match(/public\s+\w+Service\s*\(([^)]+)\)/);
        const dependencies = [];
        if (ctorMatch) {
          const params = ctorMatch[1].split(',');
          for (const p of params) {
            const typeMatch = p.trim().match(/^(I\w+)/);
            if (typeMatch) dependencies.push(typeMatch[1]);
          }
        }

        services.set(className, {
          file: rel,
          interface: implementedIface || undefined,
          entity: entity || undefined,
          dependencies: dependencies.length ? dependencies : undefined,
        });
      }

      // Also register interfaces as entries with no file (interface-first pattern)
      for (const f of csFiles) {
        const content = readFileSafe(f);
        if (!content) continue;
        if (!/public\s+interface\s+I\w+Service/.test(content)) continue;

        const rel = relativePath(this.subprojectPath, f);
        const ifaceRe = /public\s+interface\s+(I\w+Service)\s*(?::\s*([^{]+))?/g;
        let im;
        while ((im = ifaceRe.exec(content)) !== null) {
          const ifaceName = im[1];
          if (!services.has(ifaceName)) {
            const entity = serviceInterfaces.get(ifaceName);
            services.set(ifaceName, {
              file: rel,
              entity: entity || undefined,
            });
          }
        }
      }
    } catch { /* fail-open */ }

    return services;
  }

  // -------------------------------------------------------------------------
  // scanRepositories
  // -------------------------------------------------------------------------

  /**
   * Scan repository interfaces (I*Repository) and their implementations (*Repository).
   * @returns {Map<string, import('../scanner-contract').RepoInfo>}
   */
  scanRepositories() {
    const repos = new Map();

    try {
      const csFiles = collectFiles(this.subprojectPath, '.cs');

      // Collect repository interfaces first
      const repoInterfaces = new Map(); // interface name → entity
      for (const f of csFiles) {
        const content = readFileSafe(f);
        if (!content) continue;

        const ifaceRe = /public\s+interface\s+(I\w+Repository)\s*(?::\s*([^{]+))?/g;
        let im;
        while ((im = ifaceRe.exec(content)) !== null) {
          const ifaceName = im[1];
          const extendsRaw = im[2] || '';

          // IRepository<Contract> → entity = Contract
          const entityMatch = extendsRaw.match(/IRepository<\s*(\w+)\s*>/);
          const entity = entityMatch ? entityMatch[1] : null;

          repoInterfaces.set(ifaceName, entity);
        }
      }

      // Collect repository implementations
      for (const f of csFiles) {
        const content = readFileSafe(f);
        if (!content) continue;
        if (!/class\s+\w+Repository\b/.test(content)) continue;

        const rel = relativePath(this.subprojectPath, f);

        const classMatch = content.match(/public\s+(?:abstract\s+)?class\s+(\w+Repository)\s*(?::\s*([^{]+))?/);
        if (!classMatch) continue;

        const className = classMatch[1];
        const rhs = classMatch[2] || '';

        const implementedIface = rhs.split(',')
          .map(s => s.trim().replace(/<[^>]+>/g, '').trim())
          .find(s => isInterface(s) && s.endsWith('Repository'));

        // Base class (non-interface entry)
        const { baseClass } = parseInheritance(rhs);

        const entity = implementedIface ? repoInterfaces.get(implementedIface) : null;

        repos.set(className, {
          file: rel,
          interface: implementedIface || undefined,
          entity: entity || undefined,
          baseClass: baseClass || undefined,
        });
      }

      // Register interfaces
      for (const f of csFiles) {
        const content = readFileSafe(f);
        if (!content) continue;
        if (!/public\s+interface\s+I\w+Repository/.test(content)) continue;

        const rel = relativePath(this.subprojectPath, f);
        const ifaceRe = /public\s+interface\s+(I\w+Repository)\s*(?::\s*([^{]+))?/g;
        let im;
        while ((im = ifaceRe.exec(content)) !== null) {
          const ifaceName = im[1];
          if (!repos.has(ifaceName)) {
            const entity = repoInterfaces.get(ifaceName);
            repos.set(ifaceName, {
              file: rel,
              entity: entity || undefined,
            });
          }
        }
      }
    } catch { /* fail-open */ }

    return repos;
  }

  // -------------------------------------------------------------------------
  // inferPatterns
  // -------------------------------------------------------------------------

  /**
   * Infer structural/naming patterns from all scan results.
   * @param {{ entities: Map, enums: Map, interfaces: Map, routes: Map, dtos: Map, services: Map, repositories: Map }} scanResults
   * @returns {Object}
   */
  inferPatterns(scanResults) {
    const patterns = {};

    try {
      const { entities, enums, dtos, services, repositories, routes } = scanResults;

      // ---- entity patterns ----
      {
        const entityFiles = [...entities.values()].map(e => e.file).filter(Boolean);
        const baseclasses = [...entities.values()].map(e => e.baseClass).filter(Boolean);
        const ifaceLists = [...entities.values()].flatMap(e => e.interfaces || []);
        const namespaces = [...entities.values()].map(e => e.namespace).filter(Boolean);

        const nsPrefix = _commonNamespacePrefix(namespaces);

        patterns.entity = {
          folder: inferCommonFolder(entityFiles),
          baseClass: mode(baseclasses) || null,
          interfaces: uniq(ifaceLists).slice(0, 5),
          namespacePattern: nsPrefix || null,
          namingConvention: 'PascalCase',
        };
      }

      // ---- enum patterns ----
      {
        const enumValues = [...enums.values()];
        const enumFiles = enumValues.map(e => e.file).filter(Boolean);
        const namespaces = enumValues.map(e => e.namespace).filter(Boolean);
        const valueConventions = enumValues.map(e => e.valueConvention).filter(Boolean);
        const allValueDecorators = enumValues.flatMap(e => e.valueDecorators || []);
        const separateCount = enumValues.filter(e => e.separateFile === true).length;
        const separateFiles = separateCount > enumValues.length * 0.7;

        // Use mode (most frequent) for enum namespace — avoids inline enums pulling prefix down
        const nsMode = mode(namespaces);

        patterns.enum = {
          folder: inferCommonFolder(enumFiles),
          namespacePattern: nsMode || _commonNamespacePrefix(namespaces) || null,
          valueConvention: mode(valueConventions) || 'UPPER_CASE',
          decorators: uniq(allValueDecorators).slice(0, 5),
          separateFiles,
        };
      }

      // ---- dto patterns ----
      {
        const dtoValues = [...dtos.values()];
        const dtoFiles = dtoValues.map(d => d.file).filter(Boolean);
        const dtoNames = [...dtos.keys()];
        const suffixes = _extractSuffixes(dtoNames, ['Dto', 'DTO', 'Request', 'Response', 'ViewModel', 'Vm']);
        const validations = dtoValues.map(d => d.validationPattern).filter(Boolean);

        patterns.dto = {
          folder: inferCommonFolder(dtoFiles),
          namingPatterns: uniq(suffixes).slice(0, 6),
          validationPattern: mode(validations) || null,
        };
      }

      // ---- service patterns ----
      {
        const serviceValues = [...services.values()];
        const hasInterfaces = serviceValues.some(s => s.interface);
        const baseInterfaces = serviceValues.flatMap(s => {
          // Extract from dependencies or cross-ref interface map
          return s.interface ? [s.interface] : [];
        });
        // Check for a common base interface (IServiceBase, ICrudService, etc.)
        const allCsFiles = collectFiles(this.subprojectPath, '.cs');
        const baseIfaceNames = [];
        for (const f of allCsFiles) {
          const content = readFileSafe(f);
          if (!content) continue;
          const m = content.match(/interface\s+I\w+Service\s*:\s*(I\w+(?:<[^>]+>)?)/);
          if (m) baseIfaceNames.push(m[1].replace(/<[^>]+>/, ''));
        }

        patterns.service = {
          interfaceFirst: hasInterfaces,
          baseInterface: mode(baseIfaceNames) || null,
        };
      }

      // ---- repository patterns ----
      {
        const repoValues = [...repositories.values()];
        const hasInterfaces = repoValues.some(r => r.interface);
        const allCsFiles = collectFiles(this.subprojectPath, '.cs');
        const baseIfaceNames = [];
        for (const f of allCsFiles) {
          const content = readFileSafe(f);
          if (!content) continue;
          const m = content.match(/interface\s+I\w+Repository\s*:\s*(I\w+(?:<[^>]+>)?)/);
          if (m) baseIfaceNames.push(m[1].replace(/<[^>]+>/, ''));
        }
        const baseclasses = repoValues.map(r => r.baseClass).filter(Boolean);

        patterns.repository = {
          interfaceFirst: hasInterfaces,
          baseInterface: mode(baseIfaceNames) || null,
          baseClass: mode(baseclasses) || null,
        };
      }

      // ---- routes patterns ----
      {
        const routeValues = [...routes.values()];
        const prefixes = routeValues.map(r => r.prefix).filter(Boolean);
        const allEndpoints = routeValues.flatMap(r => r.endpoints || []);
        const names = allEndpoints.map(e => e.name).filter(Boolean);
        const auths = allEndpoints.map(e => e.auth).filter(Boolean);

        // Detect versioning: /v1/ or /api/v1/ in prefixes
        const versioningStrategy = prefixes.some(p => /\/v\d+\//.test(p)) ? 'path-based' : 'none';

        // Naming pattern: look for {entity}_{action} pattern in WithName
        const namingPattern = names.length
          ? _inferNamingPattern(names)
          : null;

        // Auth pattern: e.g., auth:contract/read → auth:{entity}/{action}
        const authPattern = auths.length
          ? _inferAuthPattern(auths)
          : null;

        // Group prefix pattern
        const groupPattern = prefixes.length
          ? _inferGroupPrefixPattern(prefixes)
          : null;

        patterns.routes = {
          groupPrefix: groupPattern,
          namingPattern,
          authPattern,
          versioningStrategy,
        };
      }

      // ---- module patterns ----
      {
        const allCsFiles = collectFiles(this.subprojectPath, '.cs');
        let hasIModule = false;
        let registrationMethod = null;
        let diPattern = null;

        for (const f of allCsFiles) {
          const content = readFileSafe(f);
          if (!content) continue;
          if (/:\s*IModule/.test(content)) hasIModule = true;
          if (/void\s+RegisterModule\s*\(/.test(content) || /IServiceCollection\s+RegisterModule\s*\(/.test(content)) {
            registrationMethod = 'RegisterModule';
          }
          const diMatch = content.match(/services\.(AddScoped|AddSingleton|AddTransient)/);
          if (diMatch) diPattern = diMatch[1];
        }

        if (hasIModule || registrationMethod) {
          patterns.module = {
            pattern: hasIModule ? '{Entity}Module : IModule' : null,
            registrationMethod: registrationMethod || null,
            diPattern: diPattern ? `Add${diPattern.replace('Add', '')}` : null,
          };
        }
      }
    } catch { /* fail-open */ }

    return patterns;
  }
}

// ---------------------------------------------------------------------------
// Private inference helpers
// ---------------------------------------------------------------------------

/**
 * Find the common namespace prefix across an array of namespaces.
 * e.g., ["Sialia.DataAccess.Entities", "Sialia.DataAccess.Enums"] → "Sialia.DataAccess"
 * @param {string[]} namespaces
 * @returns {string|null}
 */
function _commonNamespacePrefix(namespaces) {
  if (!namespaces.length) return null;
  const parts = namespaces.map(ns => ns.split('.'));
  const first = parts[0];
  let commonLen = first.length;
  for (const p of parts.slice(1)) {
    let i = 0;
    while (i < commonLen && i < p.length && first[i] === p[i]) i++;
    commonLen = i;
  }
  return commonLen > 0 ? first.slice(0, commonLen).join('.') : null;
}

/**
 * Extract suffix tokens that appear at the end of class names.
 * @param {string[]} names
 * @param {string[]} candidates
 * @returns {string[]}
 */
function _extractSuffixes(names, candidates) {
  const found = new Set();
  for (const name of names) {
    for (const suffix of candidates) {
      if (name.endsWith(suffix)) { found.add(suffix); break; }
    }
  }
  return [...found];
}

/**
 * Infer a naming pattern like "{entity}_{action}" from a sample of route names.
 * @param {string[]} names
 * @returns {string|null}
 */
function _inferNamingPattern(names) {
  // e.g., contracts_get_all, contracts_create, contracts_by_id → {entity}_{action}
  const underscoreNames = names.filter(n => n.includes('_'));
  if (underscoreNames.length === 0) return null;
  // Count segments after entity prefix across all names
  const segCounts = underscoreNames.map(n => n.split('_').length - 1);
  const avgSegs = Math.round(segCounts.reduce((a, b) => a + b, 0) / segCounts.length);
  // Most common is 1-2 action segments → normalize to {entity}_{action}
  return avgSegs <= 1 ? '{entity}_{action}' : '{entity}_{action}';
}

/**
 * Infer an auth pattern like "auth:{entity}/{action}" from a sample of metadata values.
 * @param {string[]} auths
 * @returns {string|null}
 */
function _inferAuthPattern(auths) {
  // e.g., auth:contract/read → auth:{entity}/{action}
  const sample = auths.find(a => a.startsWith('auth:'));
  if (!sample) return null;
  return sample.replace(/:[^/]+\//, ':{entity}/').replace(/\/[^/]+$/, '/{action}');
}

/**
 * Infer the route group prefix pattern from actual prefixes.
 * e.g., ["/contracts", "/partners", "/sales-plans"] → "/{entity-plural}"
 * @param {string[]} prefixes
 * @returns {string|null}
 */
function _inferGroupPrefixPattern(prefixes) {
  if (!prefixes.length) return null;
  // If all prefixes are single-segment paths, generalise
  const allSingle = prefixes.every(p => p.split('/').filter(Boolean).length === 1);
  if (allSingle) return '/{entity-plural}';
  // Check for versioned paths like /v1/contracts
  const versioned = prefixes.some(p => /\/v\d+\//.test(p));
  if (versioned) return '/v{n}/{entity-plural}';
  return null;
}

// ---------------------------------------------------------------------------
// Exports
// ---------------------------------------------------------------------------

module.exports = DotNetScanner;
