'use strict';

/**
 * java-scanner.js
 *
 * Java/Spring Boot stack scanner for sync-registry.js.
 * Scans Java projects for entities, enums, interfaces, routes, DTOs,
 * services, repositories and infers architectural patterns.
 *
 * Supports: Spring Boot, JPA/Hibernate, Spring Data, MapStruct, Lombok.
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
 * Recursively walk directory for all subdirectory names (up to 3 levels).
 * Returns a flat Set of all dir names found.
 * @param {string} root
 * @param {number} [depth=3]
 * @returns {Set<string>}
 */
function allDirNames(root, depth = 3) {
  const result = new Set();
  function walk(dir, remaining) {
    if (remaining <= 0) return;
    try {
      const entries = fs.readdirSync(dir, { withFileTypes: true });
      for (const e of entries) {
        if (e.isDirectory() && !e.name.startsWith('.')) {
          result.add(e.name);
          walk(path.join(dir, e.name), remaining - 1);
        }
      }
    } catch { /* ignore */ }
  }
  walk(root, depth);
  return result;
}

/**
 * Extract all annotation names from a content block (e.g. "@Entity", "@Data").
 * @param {string} content
 * @returns {Set<string>}
 */
function extractAnnotations(content) {
  const re = /@(\w+)/g;
  re.lastIndex = 0;
  const found = new Set();
  let m;
  while ((m = re.exec(content)) !== null) {
    found.add(m[1]);
  }
  return found;
}

/**
 * Extract the @RequestMapping or similar value from class-level content.
 * @param {string} classBlock - a slice of content around the class declaration
 * @returns {string|null}
 */
function extractClassMapping(classBlock) {
  const m = classBlock.match(/@(?:RequestMapping)\s*\(\s*(?:value\s*=\s*)?["']([^"']+)["']/);
  return m ? m[1] : null;
}

/**
 * Extract the @Table name annotation value.
 * @param {string} content
 * @returns {string|null}
 */
function extractTableName(content) {
  const m = content.match(/@Table\s*\(\s*(?:name\s*=\s*)["']([^"']+)["']/);
  return m ? m[1] : null;
}

/**
 * Detect whether jakarta or javax validation is used.
 * @param {string} content
 * @returns {'jakarta-validation'|'javax-validation'|'none'}
 */
function detectValidation(content) {
  if (/jakarta\.validation/.test(content) || /import jakarta\.validation/.test(content)) {
    return 'jakarta-validation';
  }
  if (/javax\.validation/.test(content) || /import javax\.validation/.test(content)) {
    return 'javax-validation';
  }
  // Annotation-based hint
  if (/@(?:NotNull|NotBlank|Valid|Size|Min|Max)\b/.test(content)) {
    // Default to jakarta for modern Spring Boot 3+
    return 'jakarta-validation';
  }
  return 'none';
}

/**
 * Detect table naming convention from a list of table names.
 * @param {string[]} tableNames
 * @returns {string}
 */
function detectTableNaming(tableNames) {
  if (!tableNames.length) return 'snake_case';
  const snakeCount = tableNames.filter(t => /^[a-z][a-z0-9_]*$/.test(t)).length;
  const camelCount = tableNames.filter(t => /^[a-z][a-zA-Z0-9]*$/.test(t)).length;
  if (snakeCount >= camelCount) return 'snake_case';
  return 'camelCase';
}

/**
 * Extract the entity type from a JpaRepository/CrudRepository generic parameter.
 * e.g. "extends JpaRepository<User, Long>" → "User"
 * @param {string} content
 * @returns {string|null}
 */
function extractRepoEntity(content) {
  const m = content.match(/extends\s+(?:JpaRepository|CrudRepository|PagingAndSortingRepository)\s*<\s*(\w+)\s*,/);
  return m ? m[1] : null;
}

// ---------------------------------------------------------------------------
// JavaScanner
// ---------------------------------------------------------------------------

class JavaScanner extends ScannerContract {
  static stackId = 'java';

  // -------------------------------------------------------------------------
  // detect
  // -------------------------------------------------------------------------

  detect() {
    try {
      return (
        existsUnder(this.subprojectPath, 'pom.xml') ||
        existsUnder(this.subprojectPath, 'build.gradle') ||
        existsUnder(this.subprojectPath, 'build.gradle.kts')
      );
    } catch (err) {
      process.stderr.write(`[java-scanner] detect error: ${err.message}\n`);
      return false;
    }
  }

  // -------------------------------------------------------------------------
  // detectArchitecture
  // -------------------------------------------------------------------------

  detectArchitecture() {
    try {
      const dirs = allDirNames(this.subprojectPath, 4);

      if (
        dirs.has('domain') && dirs.has('application') &&
        dirs.has('infrastructure') && dirs.has('ports')
      ) {
        return 'hexagonal';
      }

      if (dirs.has('domain') && dirs.has('application') && dirs.has('infrastructure')) {
        // Check for interfaces indicating SOLID
        const hasInterfaces = this._hasInterfaceFiles();
        return hasInterfaces ? 'solid' : 'layered';
      }

      if (dirs.has('controller') && dirs.has('service') && dirs.has('repository')) {
        const hasInterfaces = this._hasInterfaceFiles();
        return hasInterfaces ? 'solid' : 'layered';
      }

      return 'minimal';
    } catch (err) {
      process.stderr.write(`[java-scanner] detectArchitecture error: ${err.message}\n`);
      return 'unknown';
    }
  }

  /** @private */
  _hasInterfaceFiles() {
    try {
      const javaFiles = collectFiles(this.subprojectPath, '.java');
      return javaFiles.some(f => {
        const content = readFileSafe(f);
        return content && /public\s+interface\s+\w+/.test(content);
      });
    } catch {
      return false;
    }
  }

  // -------------------------------------------------------------------------
  // _detectFrameworks — internal helper, called once per scan
  // -------------------------------------------------------------------------

  _detectFrameworks() {
    if (this._frameworks) return this._frameworks;

    const result = {
      springBoot: false,
      jpa: false,
      springData: false,
      mapstruct: false,
      lombok: false,
      buildTool: 'unknown',
    };

    try {
      // Build tool detection
      if (existsUnder(this.subprojectPath, 'pom.xml')) {
        result.buildTool = 'maven';
        const pomContent = readFileSafe(path.join(this.subprojectPath, 'pom.xml'));
        if (pomContent) {
          result.springBoot = /spring-boot-starter/.test(pomContent);
          result.springData = /spring-data/.test(pomContent) || /spring-boot-starter-data/.test(pomContent);
          result.mapstruct = /mapstruct/.test(pomContent);
          result.lombok = /lombok/.test(pomContent);
        }
      } else if (existsUnder(this.subprojectPath, 'build.gradle')) {
        result.buildTool = 'gradle';
        const gradleContent = readFileSafe(path.join(this.subprojectPath, 'build.gradle'));
        if (gradleContent) {
          result.springBoot = /spring-boot/.test(gradleContent);
          result.springData = /spring-data/.test(gradleContent);
          result.mapstruct = /mapstruct/.test(gradleContent);
          result.lombok = /lombok/.test(gradleContent);
        }
      } else if (existsUnder(this.subprojectPath, 'build.gradle.kts')) {
        result.buildTool = 'gradle';
        const gradleContent = readFileSafe(path.join(this.subprojectPath, 'build.gradle.kts'));
        if (gradleContent) {
          result.springBoot = /spring-boot/.test(gradleContent);
          result.springData = /spring-data/.test(gradleContent);
          result.mapstruct = /mapstruct/.test(gradleContent);
          result.lombok = /lombok/.test(gradleContent);
        }
      }

      // Verify by scanning .java files for JPA annotations
      const javaFiles = collectFiles(this.subprojectPath, '.java');
      for (const filePath of javaFiles) {
        const content = readFileSafe(filePath);
        if (!content) continue;

        if (!result.jpa && (/@Entity\b/.test(content) || /@Table\b/.test(content))) {
          result.jpa = true;
        }
        if (!result.springBoot && /@SpringBootApplication/.test(content)) {
          result.springBoot = true;
        }
        if (!result.springData && /extends\s+(?:JpaRepository|CrudRepository)/.test(content)) {
          result.springData = true;
        }
        if (!result.mapstruct && /@Mapper\b/.test(content)) {
          result.mapstruct = true;
        }
        if (!result.lombok && (/@Data\b/.test(content) || /@Builder\b/.test(content) || /@Getter\b/.test(content))) {
          result.lombok = true;
        }

        if (result.jpa && result.springBoot && result.springData && result.mapstruct && result.lombok) break;
      }
    } catch (err) {
      process.stderr.write(`[java-scanner] _detectFrameworks error: ${err.message}\n`);
    }

    this._frameworks = result;
    return this._frameworks;
  }

  // -------------------------------------------------------------------------
  // scanEntities
  // -------------------------------------------------------------------------

  scanEntities() {
    const entities = new Map();

    try {
      const javaFiles = collectFiles(this.subprojectPath, '.java');

      // Multiline: @Entity (possibly other annotations) then class ClassName
      const entityRe = /@Entity[\s\S]*?class\s+(\w+)/g;

      for (const filePath of javaFiles) {
        const content = readFileSafe(filePath);
        if (!content) continue;

        const rel = relativePath(this.subprojectPath, filePath);

        entityRe.lastIndex = 0;
        let m;
        while ((m = entityRe.exec(content)) !== null) {
          const name = m[1];

          // Extract @Table name
          const tableName = extractTableName(content);

          // Extract base class
          const baseMatch = content.match(new RegExp(
            `class\\s+${name}\\s+extends\\s+(\\w+)`
          ));
          const baseClass = baseMatch ? baseMatch[1] : null;

          // Extract interfaces
          const implMatch = content.match(new RegExp(
            `class\\s+${name}[^{]*implements\\s+([^{]+)`
          ));
          const interfaces = implMatch
            ? implMatch[1].split(',').map(s => s.trim().replace(/<[^>]+>/g, '').trim()).filter(Boolean)
            : [];

          // Extract class-level annotations (decorators)
          const blockStart = Math.max(0, m.index - 300);
          const blockSlice = content.slice(blockStart, m.index + 100);
          const annotations = extractAnnotations(blockSlice);
          const decorators = [...annotations].filter(a =>
            ['Entity', 'Table', 'Data', 'Builder', 'NoArgsConstructor', 'AllArgsConstructor',
              'Getter', 'Setter', 'ToString', 'EqualsAndHashCode'].includes(a)
          );

          // Extract relationship refs
          const refs = [];
          const sub = [];
          const relAnnotations = [
            { re: /@ManyToOne[\s\S]*?(?:private|protected)\s+(\w+)\s+\w+\s*;/g, type: 'ref' },
            { re: /@OneToOne[\s\S]*?(?:private|protected)\s+(\w+)\s+\w+\s*;/g, type: 'ref' },
            { re: /@OneToMany[\s\S]*?(?:private|protected)\s+\w+<(\w+)>/g, type: 'sub' },
            { re: /@ManyToMany[\s\S]*?(?:private|protected)\s+\w+<(\w+)>/g, type: 'sub' },
          ];

          for (const { re, type } of relAnnotations) {
            re.lastIndex = 0;
            let rm;
            while ((rm = re.exec(content)) !== null) {
              const target = rm[1];
              if (target && target !== name) {
                if (type === 'ref') refs.push(target);
                else sub.push(target);
              }
            }
          }

          // Extract field names and types
          const fieldRe = /(?:private|protected)\s+(\w+(?:<[^>]+>)?)\s+(\w+)\s*;/g;
          fieldRe.lastIndex = 0;
          const properties = [];
          let fm;
          while ((fm = fieldRe.exec(content)) !== null) {
            properties.push(`${fm[2]}: ${fm[1]}`);
          }

          entities.set(name, {
            file: rel,
            ...(baseClass ? { baseClass } : {}),
            interfaces: uniq(interfaces),
            decorators: uniq(decorators),
            refs: uniq(refs),
            sub: uniq(sub),
            enums: [],
            properties: uniq(properties),
            ...(tableName ? { tableName } : {}),
          });
        }
      }
    } catch (err) {
      process.stderr.write(`[java-scanner] scanEntities error: ${err.message}\n`);
    }

    return entities;
  }

  // -------------------------------------------------------------------------
  // scanEnums
  // -------------------------------------------------------------------------

  scanEnums() {
    const enums = new Map();

    try {
      const javaFiles = collectFiles(this.subprojectPath, '.java');
      const enumRe = /public\s+enum\s+(\w+)\s*(?:implements\s+([^{]+))?\s*\{/g;

      for (const filePath of javaFiles) {
        const content = readFileSafe(filePath);
        if (!content) continue;

        const rel = relativePath(this.subprojectPath, filePath);

        enumRe.lastIndex = 0;
        let m;
        while ((m = enumRe.exec(content)) !== null) {
          const name = m[1];
          const implementsRaw = m[2] ? m[2].trim() : null;
          const implementsList = implementsRaw
            ? implementsRaw.split(',').map(s => s.trim()).filter(Boolean)
            : [];

          // Extract enum constants — first block before any ';' or method
          const bodyStart = content.indexOf('{', m.index);
          if (bodyStart === -1) continue;
          const bodyEnd = content.indexOf(';', bodyStart);
          const constantsBlock = bodyEnd !== -1
            ? content.slice(bodyStart + 1, bodyEnd)
            : content.slice(bodyStart + 1, bodyStart + 500);

          // Constants are lines: CONSTANT_NAME or CONSTANT_NAME(args)
          const constRe = /^\s*([A-Z][A-Z0-9_]*(?:\([^)]*\))?)\s*(?:,|$)/gm;
          constRe.lastIndex = 0;
          const values = [];
          let cm;
          while ((cm = constRe.exec(constantsBlock)) !== null) {
            // Strip constructor args
            values.push(cm[1].replace(/\([^)]*\)/, '').trim());
          }

          // Check if enum has fields (constructor args present)
          const hasFields = /\(\s*\w/.test(constantsBlock);

          enums.set(name, {
            values: uniq(values),
            file: rel,
            ...(implementsList.length ? { interfaces: implementsList } : {}),
            hasFields,
          });
        }
      }
    } catch (err) {
      process.stderr.write(`[java-scanner] scanEnums error: ${err.message}\n`);
    }

    return enums;
  }

  // -------------------------------------------------------------------------
  // scanInterfaces
  // -------------------------------------------------------------------------

  scanInterfaces() {
    const interfaces = new Map();

    try {
      const javaFiles = collectFiles(this.subprojectPath, '.java');
      const interfaceRe = /public\s+interface\s+(\w+)(?:\s+extends\s+([^{]+))?\s*\{/g;

      for (const filePath of javaFiles) {
        const content = readFileSafe(filePath);
        if (!content) continue;

        const rel = relativePath(this.subprojectPath, filePath);

        interfaceRe.lastIndex = 0;
        let m;
        while ((m = interfaceRe.exec(content)) !== null) {
          const name = m[1];
          const extendsRaw = m[2] ? m[2].trim() : null;
          const extendsList = extendsRaw
            ? extendsRaw.split(',').map(s => s.trim().replace(/<[^>]+>/g, '').trim()).filter(Boolean)
            : [];

          // Extract method signatures from interface body
          const bodyStart = content.indexOf('{', m.index);
          if (bodyStart === -1) continue;
          const bodyEnd = this._findMatchingBrace(content, bodyStart);
          const body = bodyEnd !== -1
            ? content.slice(bodyStart + 1, bodyEnd)
            : content.slice(bodyStart + 1, bodyStart + 2000);

          const methodRe = /(?:[\w<>\[\]]+\s+)+(\w+)\s*\([^)]*\)\s*(?:throws[^;{]+)?[;{]/g;
          methodRe.lastIndex = 0;
          const methods = [];
          let mm;
          while ((mm = methodRe.exec(body)) !== null) {
            if (mm[1] && !['if', 'for', 'while', 'switch', 'return'].includes(mm[1])) {
              methods.push(mm[1]);
            }
          }

          // Find implementing classes
          const implRe = new RegExp(`class\\s+(\\w+)[^{]*implements[^{]*\\b${name}\\b`, 'g');
          implRe.lastIndex = 0;
          const implementedBy = [];
          let im;
          while ((im = implRe.exec(content)) !== null) {
            implementedBy.push(im[1]);
          }

          interfaces.set(name, {
            file: rel,
            extends: extendsList,
            methods: uniq(methods),
            implementedBy: uniq(implementedBy),
          });
        }
      }
    } catch (err) {
      process.stderr.write(`[java-scanner] scanInterfaces error: ${err.message}\n`);
    }

    return interfaces;
  }

  /**
   * Find the matching closing brace for an opening brace at position start.
   * @param {string} content
   * @param {number} start - index of '{'
   * @returns {number} index of matching '}', or -1
   * @private
   */
  _findMatchingBrace(content, start) {
    let depth = 0;
    for (let i = start; i < content.length; i++) {
      if (content[i] === '{') depth++;
      else if (content[i] === '}') {
        depth--;
        if (depth === 0) return i;
      }
    }
    return -1;
  }

  // -------------------------------------------------------------------------
  // scanRoutes
  // -------------------------------------------------------------------------

  scanRoutes() {
    const routes = new Map();

    try {
      const javaFiles = collectFiles(this.subprojectPath, '.java');

      for (const filePath of javaFiles) {
        const content = readFileSafe(filePath);
        if (!content) continue;

        const rel = relativePath(this.subprojectPath, filePath);

        // Only files with @RestController or @Controller
        if (!/@(?:RestController|Controller)\b/.test(content)) continue;

        // Extract class-level prefix from @RequestMapping
        const classPrefix = extractClassMapping(content) || '';

        const endpoints = [];

        // @GetMapping, @PostMapping, @PutMapping, @DeleteMapping, @PatchMapping
        const verbRe = /@(Get|Post|Put|Delete|Patch)Mapping\s*(?:\(\s*(?:value\s*=\s*)?["']([^"']+)["']\s*\))?/g;
        verbRe.lastIndex = 0;
        let vm;
        while ((vm = verbRe.exec(content)) !== null) {
          const method = vm[1].toUpperCase();
          const subPath = vm[2] || '';
          const fullPath = classPrefix + subPath;

          // Try to get method name from next def
          const after = content.slice(vm.index, vm.index + 200);
          const methodNameMatch = after.match(/public\s+\S+\s+(\w+)\s*\(/);
          const name = methodNameMatch ? methodNameMatch[1] : null;

          endpoints.push({
            method,
            path: fullPath || '/',
            name,
            auth: /@(?:PreAuthorize|Secured|RolesAllowed)\b/.test(content),
          });
        }

        // Legacy @RequestMapping with method=RequestMethod.GET etc.
        const legacyRe = /@RequestMapping\s*\([^)]*value\s*=\s*["']([^"']+)["'][^)]*method\s*=\s*RequestMethod\.(\w+)/g;
        legacyRe.lastIndex = 0;
        let lm;
        while ((lm = legacyRe.exec(content)) !== null) {
          endpoints.push({
            method: lm[2].toUpperCase(),
            path: classPrefix + lm[1],
            name: null,
            auth: false,
          });
        }

        // Reverse: method first then value
        const legacyRe2 = /@RequestMapping\s*\([^)]*method\s*=\s*RequestMethod\.(\w+)[^)]*value\s*=\s*["']([^"']+)["']/g;
        legacyRe2.lastIndex = 0;
        let lm2;
        while ((lm2 = legacyRe2.exec(content)) !== null) {
          endpoints.push({
            method: lm2[1].toUpperCase(),
            path: classPrefix + lm2[2],
            name: null,
            auth: false,
          });
        }

        if (endpoints.length > 0) {
          routes.set(rel, {
            file: rel,
            prefix: classPrefix || '/',
            endpoints,
          });
        }
      }
    } catch (err) {
      process.stderr.write(`[java-scanner] scanRoutes error: ${err.message}\n`);
    }

    return routes;
  }

  // -------------------------------------------------------------------------
  // scanDtos
  // -------------------------------------------------------------------------

  scanDtos() {
    const dtos = new Map();

    try {
      const javaFiles = collectFiles(this.subprojectPath, '.java');
      const dtoSuffixRe = /(?:Dto|DTO|Request|Response|Command|Query)$/;

      for (const filePath of javaFiles) {
        const content = readFileSafe(filePath);
        if (!content) continue;

        const rel = relativePath(this.subprojectPath, filePath);

        // Match classes with DTO-like suffixes
        const classRe = /(?:public\s+)?(?:class|record)\s+(\w+(?:Dto|DTO|Request|Response|Command|Query))\b/g;
        classRe.lastIndex = 0;
        let m;
        while ((m = classRe.exec(content)) !== null) {
          const name = m[1];

          // Infer entity name by stripping suffix
          const entity = name
            .replace(/(?:Dto|DTO|Request|Response|Command|Query)$/, '') || null;

          // Detect validation
          const validationPattern = detectValidation(content);

          // Detect if it has a MapStruct mapper
          const mapperRe = new RegExp(`@Mapper[\\s\\S]*?${entity}`, 'g');
          mapperRe.lastIndex = 0;
          const hasMapper = mapperRe.test(content);

          dtos.set(name, {
            file: rel,
            entity: entity !== name ? entity : null,
            validationPattern,
            hasMapper,
          });
        }

        // Also pick up @Mapper-annotated classes (MapStruct)
        if (/@Mapper\b/.test(content)) {
          const mapperClassRe = /(?:public\s+)?(?:interface|class)\s+(\w+Mapper)\b/g;
          mapperClassRe.lastIndex = 0;
          let mm;
          while ((mm = mapperClassRe.exec(content)) !== null) {
            const name = mm[1];
            if (!dtos.has(name)) {
              const entity = name.replace(/Mapper$/, '') || null;
              dtos.set(name, {
                file: rel,
                entity: entity !== name ? entity : null,
                validationPattern: 'none',
                isMapper: true,
              });
            }
          }
        }
      }
    } catch (err) {
      process.stderr.write(`[java-scanner] scanDtos error: ${err.message}\n`);
    }

    return dtos;
  }

  // -------------------------------------------------------------------------
  // scanServices
  // -------------------------------------------------------------------------

  scanServices() {
    const services = new Map();

    try {
      const javaFiles = collectFiles(this.subprojectPath, '.java');

      for (const filePath of javaFiles) {
        const content = readFileSafe(filePath);
        if (!content) continue;

        const rel = relativePath(this.subprojectPath, filePath);

        if (!/@Service\b/.test(content)) continue;

        const classRe = /(?:public\s+)?class\s+(\w+)\b/g;
        classRe.lastIndex = 0;
        let m;
        while ((m = classRe.exec(content)) !== null) {
          const name = m[1];

          // Check interface it implements
          const implMatch = content.match(new RegExp(
            `class\\s+${name}[^{]*implements\\s+([\\w,\\s<>]+)`
          ));
          const implementsList = implMatch
            ? implMatch[1].split(',').map(s => s.trim().replace(/<[^>]+>/g, '').trim()).filter(Boolean)
            : [];
          const serviceInterface = implementsList.length > 0 ? implementsList[0] : null;

          // Extract constructor-injected dependencies (private final fields)
          const ctorRe = new RegExp(
            `public\\s+${name}\\s*\\(([^)]+)\\)`, 'g'
          );
          ctorRe.lastIndex = 0;
          const dependencies = [];
          let ctorm;
          while ((ctorm = ctorRe.exec(content)) !== null) {
            const params = ctorm[1].split(',');
            for (const param of params) {
              const typed = param.trim().match(/(\w+(?:<[^>]+>)?)\s+\w+$/);
              if (typed && !['String', 'int', 'long', 'boolean', 'Integer', 'Long', 'Boolean'].includes(typed[1])) {
                dependencies.push(typed[1]);
              }
            }
          }

          // Fallback: private final fields
          if (!dependencies.length) {
            const fieldRe = /private\s+final\s+(\w+)\s+\w+\s*;/g;
            fieldRe.lastIndex = 0;
            let fm;
            while ((fm = fieldRe.exec(content)) !== null) {
              if (!['String', 'int', 'long', 'boolean', 'Integer', 'Long', 'Boolean'].includes(fm[1])) {
                dependencies.push(fm[1]);
              }
            }
          }

          // Infer linked entity
          const entity = name.replace(/(?:Service|ServiceImpl)$/, '') || null;

          services.set(name, {
            file: rel,
            interface: serviceInterface,
            entity: entity !== name ? entity : null,
            dependencies: uniq(dependencies),
          });
        }
      }
    } catch (err) {
      process.stderr.write(`[java-scanner] scanServices error: ${err.message}\n`);
    }

    return services;
  }

  // -------------------------------------------------------------------------
  // scanRepositories
  // -------------------------------------------------------------------------

  scanRepositories() {
    const repos = new Map();

    try {
      const javaFiles = collectFiles(this.subprojectPath, '.java');

      for (const filePath of javaFiles) {
        const content = readFileSafe(filePath);
        if (!content) continue;

        const rel = relativePath(this.subprojectPath, filePath);

        const isAnnotated = /@Repository\b/.test(content);
        const extendsRepo = /extends\s+(?:JpaRepository|CrudRepository|PagingAndSortingRepository)/.test(content);

        if (!isAnnotated && !extendsRepo) continue;

        // Extract interface/class name
        const nameRe = /(?:public\s+)?(?:interface|class)\s+(\w+)\b/g;
        nameRe.lastIndex = 0;
        let m;
        while ((m = nameRe.exec(content)) !== null) {
          const name = m[1];

          // Detect base type
          let baseType = 'custom';
          const baseMatch = content.match(/extends\s+(JpaRepository|CrudRepository|PagingAndSortingRepository)\s*</);
          if (baseMatch) baseType = baseMatch[1];

          // Detect entity
          const entity = extractRepoEntity(content) || name.replace(/(?:Repository|Repo)(?:Impl)?$/, '') || null;

          // Detect implementing interface
          const implMatch = content.match(new RegExp(
            `class\\s+${name}[^{]*implements\\s+([\\w,\\s<>]+)`
          ));
          const iface = implMatch
            ? implMatch[1].split(',')[0].trim().replace(/<[^>]+>/g, '').trim()
            : null;

          // Detect custom @Query methods
          const hasCustomQuery = /@Query\s*\(/.test(content);

          repos.set(name, {
            file: rel,
            interface: iface,
            entity: entity !== name ? entity : null,
            baseClass: baseType,
            hasCustomQuery,
          });
        }
      }
    } catch (err) {
      process.stderr.write(`[java-scanner] scanRepositories error: ${err.message}\n`);
    }

    return repos;
  }

  // -------------------------------------------------------------------------
  // inferPatterns
  // -------------------------------------------------------------------------

  inferPatterns(scanResults) {
    try {
      const { entities, enums, dtos, routes, repositories, services } = scanResults;
      const fw = this._detectFrameworks();

      // Entity patterns
      const entityFiles = [...entities.values()].map(e => e.file);
      const entityFolder = inferCommonFolder(entityFiles);
      const baseClasses = [...entities.values()].map(e => e.baseClass).filter(Boolean);
      const baseClass = baseClasses.length > 0
        ? baseClasses.sort((a, b) =>
          baseClasses.filter(x => x === b).length - baseClasses.filter(x => x === a).length
        )[0]
        : null;

      // Table naming
      const tableNames = [...entities.values()]
        .map(e => e.tableName)
        .filter(Boolean);
      const tableNaming = detectTableNaming(tableNames);

      // Lombok detection
      const usesLombok = fw.lombok || [...entities.values()].some(e =>
        e.decorators && (
          e.decorators.includes('Data') ||
          e.decorators.includes('Builder') ||
          e.decorators.includes('Getter')
        )
      );

      // Enum patterns
      const enumFiles = [...enums.values()].map(e => e.file);
      const enumFolder = inferCommonFolder(enumFiles);
      const hasFields = [...enums.values()].some(e => e.hasFields);
      const enumSeparateFiles = enumFolder !== entityFolder;

      // Route patterns
      const allEndpoints = [...routes.values()].flatMap(r => r.endpoints || []);
      const allPaths = allEndpoints.map(e => e.path || '');
      const hasVersioning = allPaths.some(p => /\/v\d+\//.test(p) || p.startsWith('/v'));
      const versioningStrategy = hasVersioning ? 'path-prefix' : 'none';

      // Route prefix
      const routePrefixes = [...routes.values()]
        .map(r => r.prefix)
        .filter(p => p && p !== '/');
      const routePrefix = routePrefixes.length > 0 ? routePrefixes[0] : null;

      // DTO patterns
      const dtoFiles = [...dtos.values()].map(d => d.file);
      const dtoFolder = inferCommonFolder(dtoFiles);
      const usesMapstruct = fw.mapstruct || [...dtos.values()].some(d => d.isMapper);
      const validationPatterns = [...dtos.values()].map(d => d.validationPattern).filter(Boolean);
      const validationPattern = validationPatterns.length > 0
        ? (validationPatterns.includes('jakarta-validation') ? 'jakarta-validation'
          : validationPatterns.includes('javax-validation') ? 'javax-validation'
            : 'none')
        : 'none';

      // Repository base type
      const repoBaseTypes = [...repositories.values()].map(r => r.baseClass).filter(Boolean);
      const repoBaseType = repoBaseTypes.length > 0
        ? (repoBaseTypes.includes('JpaRepository') ? 'JpaRepository'
          : repoBaseTypes.includes('CrudRepository') ? 'CrudRepository'
            : 'custom')
        : 'custom';

      // Interface-first pattern for repos and services
      const repoInterfaceFirst = [...repositories.values()].some(r => r.interface);
      const serviceInterfaceFirst = [...services.values()].some(s => s.interface);

      return {
        framework: 'spring-boot',
        buildTool: fw.buildTool,
        entity: {
          folder: entityFolder,
          baseClass,
          namingConvention: 'PascalCase',
          tableNaming,
          lombok: usesLombok,
        },
        enum: {
          folder: enumFolder,
          separateFiles: enumSeparateFiles,
          hasFields,
        },
        routes: {
          style: 'annotation',
          prefix: routePrefix,
          versioningStrategy,
        },
        dto: {
          folder: dtoFolder,
          mapstruct: usesMapstruct,
          validationPattern,
        },
        repository: {
          baseType: repoBaseType,
          interfaceFirst: repoInterfaceFirst || serviceInterfaceFirst,
        },
      };
    } catch (err) {
      process.stderr.write(`[java-scanner] inferPatterns error: ${err.message}\n`);
      return {};
    }
  }
}

module.exports = JavaScanner;
