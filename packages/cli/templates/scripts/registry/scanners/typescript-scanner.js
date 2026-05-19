'use strict';

/**
 * typescript-scanner.js
 *
 * Stack scanner for TypeScript/Node.js projects.
 * Detects Drizzle ORM, Prisma, TypeORM, NestJS, Express, Hono, Next.js, Zod, class-validator.
 *
 * Extends ScannerContract — implements all scan* methods and inferPatterns.
 */

const { ScannerContract } = require('../scanner-contract');
const { collectFiles, relativePath, readFileSafe, inferCommonFolder } = require('../file-utils');
const path = require('path');
const fs = require('fs');

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/**
 * Check if a path (relative or absolute segment) exists under the subproject root.
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
 * Walk a directory and list immediate child dir names.
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
 * Detect value convention from an array of enum value strings.
 * @param {string[]} values
 * @returns {string}
 */
function detectValueConvention(values) {
  if (!values.length) return 'unknown';
  const upper = values.filter(v => /^[A-Z][A-Z0-9_]*$/.test(v.trim())).length;
  const pascal = values.filter(v => /^[A-Z][a-zA-Z0-9]*$/.test(v.trim())).length;
  const camel = values.filter(v => /^[a-z][a-zA-Z0-9]*$/.test(v.trim())).length;
  const total = values.length;
  if (upper / total > 0.6) return 'UPPER_CASE';
  if (pascal / total > 0.6) return 'PascalCase';
  if (camel / total > 0.6) return 'camelCase';
  return 'mixed';
}

/**
 * Parse a package.json and return its dependencies + devDependencies keys.
 * Returns empty Set on error.
 * @param {string} pkgPath
 * @returns {Set<string>}
 */
function readPackageDeps(pkgPath) {
  try {
    const raw = fs.readFileSync(pkgPath, 'utf-8');
    const pkg = JSON.parse(raw);
    const deps = Object.keys(pkg.dependencies || {});
    const devDeps = Object.keys(pkg.devDependencies || {});
    return new Set([...deps, ...devDeps]);
  } catch {
    return new Set();
  }
}

/**
 * Extract clean values from a bracket-delimited list (e.g., enum body or pgEnum array).
 * Handles quoted strings and bare identifiers.
 * @param {string} body
 * @returns {string[]}
 */
function parseEnumValues(body) {
  const values = [];
  // Match quoted strings first, then bare identifiers separated by commas/whitespace
  const quotedRe = /['"]([^'"]+)['"]/g;
  let m;
  while ((m = quotedRe.exec(body)) !== null) {
    values.push(m[1]);
  }
  if (values.length) return values;
  // Fall back to bare words (TS enum members)
  return body
    .split(/[,\n\r]+/)
    .map(s => s.replace(/=.*$/, '').trim())
    .filter(s => /^\w+$/.test(s));
}

// ---------------------------------------------------------------------------
// TypeScriptScanner
// ---------------------------------------------------------------------------

class TypeScriptScanner extends ScannerContract {
  static stackId = 'typescript';

  constructor(subprojectPath, subprojectMeta) {
    super(subprojectPath, subprojectMeta);

    // Cached detection flags — populated lazily by _detectFrameworks()
    this._frameworks = null;
    this._pkgDeps = null;
  }

  // -------------------------------------------------------------------------
  // detect()
  // -------------------------------------------------------------------------

  /**
   * Returns true if subprojectPath contains package.json or tsconfig.json.
   * @returns {boolean}
   */
  detect() {
    try {
      return (
        existsUnder(this.subprojectPath, 'package.json') ||
        existsUnder(this.subprojectPath, 'tsconfig.json')
      );
    } catch {
      return false;
    }
  }

  // -------------------------------------------------------------------------
  // Framework detection (private, lazy)
  // -------------------------------------------------------------------------

  /**
   * Detect all relevant frameworks from package.json deps and filesystem signals.
   * Cached after first call.
   * @returns {{ drizzle, prisma, typeorm, nestjs, express, hono, nextjs, zod, classValidator }}
   */
  _detectFrameworks() {
    if (this._frameworks) return this._frameworks;

    const pkgPath = path.join(this.subprojectPath, 'package.json');
    const deps = readPackageDeps(pkgPath);
    this._pkgDeps = deps;

    const fw = {
      drizzle: deps.has('drizzle-orm') || deps.has('drizzle-kit'),
      prisma: deps.has('@prisma/client') || deps.has('prisma') || existsUnder(this.subprojectPath, 'prisma/schema.prisma'),
      typeorm: deps.has('typeorm'),
      nestjs: deps.has('@nestjs/core') || deps.has('@nestjs/common'),
      express: deps.has('express'),
      hono: deps.has('hono'),
      nextjs: deps.has('next') || existsUnder(this.subprojectPath, 'next.config.js') || existsUnder(this.subprojectPath, 'next.config.ts') || existsUnder(this.subprojectPath, 'next.config.mjs'),
      zod: deps.has('zod'),
      classValidator: deps.has('class-validator'),
    };

    this._frameworks = fw;
    return fw;
  }

  // -------------------------------------------------------------------------
  // detectArchitecture()
  // -------------------------------------------------------------------------

  /**
   * Infer the high-level architecture of the project.
   * @returns {string}
   */
  detectArchitecture() {
    try {
      const fw = this._detectFrameworks();
      const root = this.subprojectPath;

      // NestJS with modules + providers → solid (DI-based layered)
      if (fw.nestjs) {
        const hasModules = existsUnder(root, 'src') && this._hasNestModules();
        const hasSvcRepo = this._hasClearServiceRepoSeparation();
        if (hasModules && hasSvcRepo) return 'solid';
        if (hasModules) return 'solid';
      }

      // Plain service/repository separation with interfaces → solid
      if (!fw.nestjs && this._hasClearServiceRepoSeparation()) return 'solid';

      // Next.js App Router (app/ + route.ts or page.tsx)
      if (fw.nextjs) {
        if (existsUnder(root, 'app')) {
          const hasRouteTs = this._hasAppRouterFiles();
          if (hasRouteTs) return 'feature-based';
        }
        if (existsUnder(root, 'pages')) return 'pages-based';
        // src/app or src/pages
        if (existsUnder(root, 'src/app')) return 'feature-based';
        if (existsUnder(root, 'src/pages')) return 'pages-based';
      }

      // React/frontend atomic structure
      if (existsUnder(root, 'src/components') && existsUnder(root, 'src/hooks')) {
        // Feature-based if there are feature folders under components
        const componentDirs = listDirs(path.join(root, 'src/components'));
        const looksFeature = componentDirs.some(d => /^[A-Z]/.test(d));
        return looksFeature ? 'feature-based' : 'atomic';
      }

      // Minimal (few top-level source dirs)
      const srcDirs = existsUnder(root, 'src') ? listDirs(path.join(root, 'src')) : listDirs(root);
      if (srcDirs.length <= 2) return 'minimal';

      return 'layered';
    } catch {
      return 'unknown';
    }
  }

  /** Check if there are NestJS @Module decorated files */
  _hasNestModules() {
    try {
      const tsFiles = collectFiles(this.subprojectPath, '.ts');
      return tsFiles.some(f => {
        const content = readFileSafe(f);
        return content && content.includes('@Module(');
      });
    } catch {
      return false;
    }
  }

  /** Check if there is clear service/repository folder separation */
  _hasClearServiceRepoSeparation() {
    try {
      const root = this.subprojectPath;
      const hasSrc = existsUnder(root, 'src');
      const base = hasSrc ? path.join(root, 'src') : root;
      const dirs = listDirs(base).map(d => d.toLowerCase());
      const hasServices = dirs.includes('services') || dirs.includes('service');
      const hasRepos = dirs.includes('repositories') || dirs.includes('repository') || dirs.includes('repos');
      const hasInterfaces = dirs.includes('interfaces') || dirs.includes('contracts');
      return hasServices && (hasRepos || hasInterfaces);
    } catch {
      return false;
    }
  }

  /** Check if Next.js app/ directory contains route.ts or page.tsx files */
  _hasAppRouterFiles() {
    try {
      const root = this.subprojectPath;
      const appDir = existsUnder(root, 'app') ? path.join(root, 'app') :
                     existsUnder(root, 'src/app') ? path.join(root, 'src', 'app') : null;
      if (!appDir) return false;
      const tsFiles = collectFiles(appDir, '.ts');
      const tsxFiles = collectFiles(appDir, '.tsx');
      const all = [...tsFiles, ...tsxFiles];
      return all.some(f => {
        const base = path.basename(f);
        return base === 'route.ts' || base === 'page.tsx' || base === 'layout.tsx';
      });
    } catch {
      return false;
    }
  }

  // -------------------------------------------------------------------------
  // scanEntities()
  // -------------------------------------------------------------------------

  /**
   * Scan entities: Drizzle pgTable, Prisma models, TypeORM @Entity classes.
   * @returns {Map<string, import('../scanner-contract').EntityInfo>}
   */
  scanEntities() {
    const entities = new Map();
    try {
      const fw = this._detectFrameworks();

      if (fw.drizzle) this._scanDrizzleEntities(entities);
      if (fw.prisma) this._scanPrismaEntities(entities);
      if (fw.typeorm) this._scanTypeOrmEntities(entities);
    } catch (err) {
      process.stderr.write(`[typescript-scanner] scanEntities error: ${err.message}\n`);
    }
    return entities;
  }

  /** Drizzle: export const tableName = pgTable('table_name', { ... }) */
  _scanDrizzleEntities(entities) {
    const tsFiles = collectFiles(this.subprojectPath, '.ts');
    const re = /export\s+const\s+(\w+)\s*=\s*pgTable\s*\(\s*['"](\w+)['"]/g;

    for (const file of tsFiles) {
      const content = readFileSafe(file);
      if (!content || !content.includes('pgTable')) continue;

      let m;
      re.lastIndex = 0;
      while ((m = re.exec(content)) !== null) {
        const varName = m[1];
        const tableName = m[2];
        // Derive a PascalCase entity name from the variable name or table name
        const entityName = this._toPascalCase(varName);

        // Extract column names from the table definition body
        const props = this._extractDrizzleColumns(content, m.index);
        // Detect referenced enum names used in the table
        const enumsUsed = this._extractDrizzleEnumRefs(content, m.index);

        entities.set(entityName, {
          file: relativePath(this.subprojectPath, file),
          decorators: ['pgTable'],
          properties: props,
          enums: enumsUsed.length ? enumsUsed : undefined,
          _tableName: tableName,
        });
      }
    }
  }

  /** Extract column names from a Drizzle pgTable(..., { ... }) call body */
  _extractDrizzleColumns(content, matchIndex) {
    try {
      // Find the opening brace of the columns object
      const afterMatch = content.indexOf('{', matchIndex);
      if (afterMatch === -1) return [];
      // Count braces to find the closing brace of the columns object
      let depth = 0;
      let start = -1;
      let end = -1;
      for (let i = afterMatch; i < content.length; i++) {
        if (content[i] === '{') {
          if (depth === 0) start = i;
          depth++;
        } else if (content[i] === '}') {
          depth--;
          if (depth === 0) { end = i; break; }
        }
      }
      if (start === -1 || end === -1) return [];
      const body = content.slice(start + 1, end);
      // Match column definitions: colName: type(...)
      const colRe = /^\s*(\w+)\s*:/gm;
      const cols = [];
      let cm;
      while ((cm = colRe.exec(body)) !== null) {
        cols.push(cm[1]);
      }
      return cols;
    } catch {
      return [];
    }
  }

  /** Find enum variable names referenced via .$type<...> or pgEnum calls near a table */
  _extractDrizzleEnumRefs(content, matchIndex) {
    try {
      const snippet = content.slice(matchIndex, matchIndex + 2000);
      const refs = new Set();
      // Detect references like: someEnumCol: statusEnum(...) or similar patterns
      const enumRefRe = /(\w+Enum|\w+Status|\w+Type)\s*\(/g;
      let m;
      while ((m = enumRefRe.exec(snippet)) !== null) {
        refs.add(this._toPascalCase(m[1]));
      }
      return [...refs];
    } catch {
      return [];
    }
  }

  /** Prisma: parse schema.prisma for model declarations */
  _scanPrismaEntities(entities) {
    const schemaPath = path.join(this.subprojectPath, 'prisma', 'schema.prisma');
    const content = readFileSafe(schemaPath);
    if (!content) return;

    const modelRe = /^model\s+(\w+)\s*\{([^}]+)\}/gm;
    let m;
    while ((m = modelRe.exec(content)) !== null) {
      const modelName = m[1];
      const body = m[2];
      const props = this._parsePrismaFields(body);
      const refs = this._parsePrismaRefs(body);
      const enumsUsed = this._parsePrismaEnumRefs(body);

      entities.set(modelName, {
        file: relativePath(this.subprojectPath, schemaPath),
        decorators: ['prisma-model'],
        properties: props,
        refs: refs.length ? refs : undefined,
        enums: enumsUsed.length ? enumsUsed : undefined,
      });
    }
  }

  /** Extract field names from a Prisma model body */
  _parsePrismaFields(body) {
    const fields = [];
    const fieldRe = /^\s+(\w+)\s+\w/gm;
    let m;
    while ((m = fieldRe.exec(body)) !== null) {
      const name = m[1];
      if (!['@@', '@'].includes(name)) fields.push(name);
    }
    return fields;
  }

  /** Extract relation references from Prisma model body (field types that look like models) */
  _parsePrismaRefs(body) {
    const refs = new Set();
    // Fields whose type is a relation: fieldName  ModelName (not scalar type)
    const relRe = /^\s+\w+\s+([A-Z]\w+)(\[\])?\s*[?@\n]/gm;
    let m;
    while ((m = relRe.exec(body)) !== null) {
      const typeName = m[1];
      // Skip Prisma scalar types
      if (!['String', 'Int', 'Float', 'Boolean', 'DateTime', 'Json', 'Bytes', 'Decimal', 'BigInt'].includes(typeName)) {
        refs.add(typeName);
      }
    }
    return [...refs];
  }

  /** Extract enum type references from a Prisma model body */
  _parsePrismaEnumRefs(body) {
    const enums = new Set();
    // Enum fields: fieldName EnumType
    const re = /^\s+\w+\s+([A-Z][a-z]\w*)\s*[?@\n]/gm;
    let m;
    while ((m = re.exec(body)) !== null) {
      enums.add(m[1]);
    }
    return [...enums];
  }

  /** TypeORM: scan @Entity() decorated classes */
  _scanTypeOrmEntities(entities) {
    const tsFiles = collectFiles(this.subprojectPath, '.ts');
    // Match @Entity() optionally followed by more decorators then class declaration
    const re = /@Entity\s*\([^)]*\)[\s\S]*?class\s+(\w+)/g;

    for (const file of tsFiles) {
      const content = readFileSafe(file);
      if (!content || !content.includes('@Entity')) continue;

      let m;
      re.lastIndex = 0;
      while ((m = re.exec(content)) !== null) {
        const className = m[1];
        const props = this._extractTypeOrmColumns(content, m.index);
        const refs = this._extractTypeOrmRelations(content, m.index);

        entities.set(className, {
          file: relativePath(this.subprojectPath, file),
          decorators: ['@Entity'],
          properties: props,
          refs: refs.length ? refs : undefined,
        });
      }
    }
  }

  /** Extract @Column property names from a TypeORM entity class */
  _extractTypeOrmColumns(content, classStart) {
    try {
      const body = content.slice(classStart, classStart + 5000);
      const cols = [];
      const colRe = /@Column[^)]*\)\s*\n\s*(\w+)/g;
      let m;
      while ((m = colRe.exec(body)) !== null) {
        cols.push(m[1]);
      }
      // Also capture PrimaryGeneratedColumn
      const pkRe = /@PrimaryGeneratedColumn[^)]*\)\s*\n\s*(\w+)/g;
      while ((m = pkRe.exec(body)) !== null) {
        cols.push(m[1]);
      }
      return cols;
    } catch {
      return [];
    }
  }

  /** Extract relation entity names from TypeORM decorators */
  _extractTypeOrmRelations(content, classStart) {
    try {
      const body = content.slice(classStart, classStart + 5000);
      const refs = new Set();
      // @ManyToOne(() => User, ...) etc.
      const relRe = /@(?:ManyToOne|OneToMany|OneToOne|ManyToMany)\s*\(\s*\(\)\s*=>\s*(\w+)/g;
      let m;
      while ((m = relRe.exec(body)) !== null) {
        refs.add(m[1]);
      }
      return [...refs];
    } catch {
      return [];
    }
  }

  // -------------------------------------------------------------------------
  // scanEnums()
  // -------------------------------------------------------------------------

  /**
   * Scan enums: TS enums, Drizzle pgEnum, Prisma enums, const-as-enum objects.
   * @returns {Map<string, import('../scanner-contract').EnumInfo>}
   */
  scanEnums() {
    const enums = new Map();
    try {
      const fw = this._detectFrameworks();

      // TypeScript enum declarations (all TS projects)
      this._scanTsEnums(enums);

      // Drizzle pgEnum
      if (fw.drizzle) this._scanDrizzlePgEnums(enums);

      // Prisma enums
      if (fw.prisma) this._scanPrismaEnums(enums);

      // Object-as-enum (export const X = { ... } as const)
      this._scanConstObjectEnums(enums);
    } catch (err) {
      process.stderr.write(`[typescript-scanner] scanEnums error: ${err.message}\n`);
    }
    return enums;
  }

  /** TS enum: export enum Name { A, B } or export const enum Name { A, B } */
  _scanTsEnums(enums) {
    const tsFiles = collectFiles(this.subprojectPath, '.ts');
    const re = /export\s+(?:const\s+)?enum\s+(\w+)\s*\{([^}]*)\}/gs;

    for (const file of tsFiles) {
      const content = readFileSafe(file);
      if (!content || !content.includes('enum ')) continue;

      let m;
      re.lastIndex = 0;
      while ((m = re.exec(content)) !== null) {
        const enumName = m[1];
        const body = m[2];
        const values = parseEnumValues(body);

        enums.set(enumName, {
          values,
          file: relativePath(this.subprojectPath, file),
          valueConvention: detectValueConvention(values),
        });
      }
    }
  }

  /** Drizzle pgEnum: export const statusEnum = pgEnum('status', ['a', 'b']) */
  _scanDrizzlePgEnums(enums) {
    const tsFiles = collectFiles(this.subprojectPath, '.ts');
    const re = /export\s+const\s+(\w+)\s*=\s*pgEnum\s*\(\s*['"](\w+)['"]\s*,\s*\[([^\]]+)\]\)/g;

    for (const file of tsFiles) {
      const content = readFileSafe(file);
      if (!content || !content.includes('pgEnum')) continue;

      let m;
      re.lastIndex = 0;
      while ((m = re.exec(content)) !== null) {
        const varName = m[1];
        const dbName = m[2];
        const body = m[3];
        const values = parseEnumValues(body);
        const enumName = this._toPascalCase(varName);

        enums.set(enumName, {
          values,
          file: relativePath(this.subprojectPath, file),
          decorators: ['pgEnum'],
          valueConvention: detectValueConvention(values),
          _dbName: dbName,
        });
      }
    }
  }

  /** Prisma enums: enum Name { VALUE1 VALUE2 } in .prisma files */
  _scanPrismaEnums(enums) {
    const schemaPath = path.join(this.subprojectPath, 'prisma', 'schema.prisma');
    const content = readFileSafe(schemaPath);
    if (!content) return;

    const re = /^enum\s+(\w+)\s*\{([^}]+)\}/gm;
    let m;
    while ((m = re.exec(content)) !== null) {
      const enumName = m[1];
      const body = m[2];
      const values = body
        .split(/\s+/)
        .map(s => s.trim())
        .filter(s => s && /^\w+$/.test(s) && !s.startsWith('@') && !s.startsWith('//'));

      enums.set(enumName, {
        values,
        file: relativePath(this.subprojectPath, schemaPath),
        decorators: ['prisma-enum'],
        valueConvention: detectValueConvention(values),
      });
    }
  }

  /** Object-as-enum: export const Status = { A: 'a', B: 'b' } as const */
  _scanConstObjectEnums(enums) {
    const tsFiles = collectFiles(this.subprojectPath, '.ts');
    const re = /export\s+const\s+(\w+)\s*=\s*\{[^}]+\}\s+as\s+const/g;

    for (const file of tsFiles) {
      const content = readFileSafe(file);
      if (!content || !content.includes('as const')) continue;

      let m;
      re.lastIndex = 0;
      while ((m = re.exec(content)) !== null) {
        const varName = m[1];
        // Only treat it as enum if name looks like an enum (PascalCase or all-caps)
        if (!/^[A-Z]/.test(varName)) continue;

        // Extract the object body
        const objStart = content.indexOf('{', m.index);
        if (objStart === -1) continue;
        let depth = 0;
        let objEnd = -1;
        for (let i = objStart; i < content.length; i++) {
          if (content[i] === '{') depth++;
          else if (content[i] === '}') { depth--; if (depth === 0) { objEnd = i; break; } }
        }
        if (objEnd === -1) continue;
        const body = content.slice(objStart + 1, objEnd);

        // Extract keys
        const keyRe = /^\s*(\w+)\s*:/gm;
        const keys = [];
        let km;
        while ((km = keyRe.exec(body)) !== null) {
          keys.push(km[1]);
        }
        if (!keys.length) continue;

        enums.set(varName, {
          values: keys,
          file: relativePath(this.subprojectPath, file),
          decorators: ['const-object'],
          valueConvention: detectValueConvention(keys),
        });
      }
    }
  }

  // -------------------------------------------------------------------------
  // scanInterfaces()
  // -------------------------------------------------------------------------

  /**
   * Scan TypeScript interfaces and complex type aliases.
   * @returns {Map<string, import('../scanner-contract').InterfaceInfo>}
   */
  scanInterfaces() {
    const interfaces = new Map();
    try {
      const tsFiles = collectFiles(this.subprojectPath, '.ts');
      const ifaceRe = /export\s+(?:default\s+)?interface\s+(\w+)(?:\s+extends\s+([^{]+))?\s*\{/g;
      const typeRe = /export\s+type\s+(\w+)\s*=/g;

      for (const file of tsFiles) {
        const content = readFileSafe(file);
        if (!content) continue;

        const rel = relativePath(this.subprojectPath, file);

        // Interfaces
        ifaceRe.lastIndex = 0;
        let m;
        while ((m = ifaceRe.exec(content)) !== null) {
          const name = m[1];
          const extendsClause = m[2];
          const parentInterfaces = extendsClause
            ? extendsClause.split(',').map(s => s.trim()).filter(Boolean)
            : [];

          // Extract method signatures from interface body
          const bodyStart = content.indexOf('{', m.index);
          const methods = bodyStart !== -1 ? this._extractInterfaceMethods(content, bodyStart) : [];

          interfaces.set(name, {
            file: rel,
            extends: parentInterfaces.length ? parentInterfaces : undefined,
            methods: methods.length ? methods : undefined,
          });
        }

        // Complex type aliases (skip simple primitives)
        typeRe.lastIndex = 0;
        while ((m = typeRe.exec(content)) !== null) {
          const name = m[1];
          // Only include if name suggests it is a structural type (not a simple alias)
          // Heuristic: includes object braces or union with object shapes
          const afterEq = content.slice(m.index + m[0].length, m.index + m[0].length + 200);
          if (afterEq.includes('{') || afterEq.includes('|') || afterEq.includes('&')) {
            interfaces.set(name, { file: rel });
          }
        }
      }
    } catch (err) {
      process.stderr.write(`[typescript-scanner] scanInterfaces error: ${err.message}\n`);
    }
    return interfaces;
  }

  /** Extract method names from an interface body */
  _extractInterfaceMethods(content, bodyStart) {
    try {
      let depth = 0;
      let end = -1;
      for (let i = bodyStart; i < content.length; i++) {
        if (content[i] === '{') depth++;
        else if (content[i] === '}') { depth--; if (depth === 0) { end = i; break; } }
      }
      if (end === -1) return [];
      const body = content.slice(bodyStart + 1, end);
      const methodRe = /^\s*(\w+)\s*[(<:]/gm;
      const methods = [];
      let m;
      while ((m = methodRe.exec(body)) !== null) {
        const name = m[1];
        if (!['constructor', 'new'].includes(name)) methods.push(name);
      }
      return methods;
    } catch {
      return [];
    }
  }

  // -------------------------------------------------------------------------
  // scanRoutes()
  // -------------------------------------------------------------------------

  /**
   * Scan routes: Express/Hono router methods, NestJS controller decorators, Next.js App Router.
   * @returns {Map<string, import('../scanner-contract').RouteInfo>}
   */
  scanRoutes() {
    const routes = new Map();
    try {
      const fw = this._detectFrameworks();

      if (fw.nextjs) this._scanNextJsRoutes(routes);
      if (fw.nestjs) this._scanNestJsRoutes(routes);
      if (fw.express || fw.hono) this._scanExpressHonoRoutes(routes);
    } catch (err) {
      process.stderr.write(`[typescript-scanner] scanRoutes error: ${err.message}\n`);
    }
    return routes;
  }

  /** Next.js App Router: detect route.ts files and infer paths from directory structure */
  _scanNextJsRoutes(routes) {
    const root = this.subprojectPath;
    const appDir = existsUnder(root, 'app') ? path.join(root, 'app') :
                   existsUnder(root, 'src/app') ? path.join(root, 'src', 'app') : null;
    if (!appDir) return;

    const tsFiles = collectFiles(appDir, '.ts');
    const tsxFiles = collectFiles(appDir, '.tsx');

    for (const file of [...tsFiles, ...tsxFiles]) {
      const basename = path.basename(file);
      if (basename !== 'route.ts' && basename !== 'route.tsx') continue;

      const content = readFileSafe(file);
      if (!content) continue;

      // Infer route path from directory structure relative to appDir
      const dir = path.dirname(file);
      const routePath = '/' + path.relative(appDir, dir).replace(/\\/g, '/');

      // Detect exported HTTP method handlers
      const endpoints = [];
      const methodRe = /export\s+(?:async\s+)?(?:function\s+)?(GET|POST|PUT|DELETE|PATCH|HEAD|OPTIONS)\s*[(\{]/g;
      let m;
      while ((m = methodRe.exec(content)) !== null) {
        endpoints.push({ method: m[1], path: routePath, name: m[1] });
      }

      if (endpoints.length) {
        const key = routePath || '/';
        routes.set(key, {
          file: relativePath(this.subprojectPath, file),
          prefix: routePath,
          endpoints,
        });
      }
    }
  }

  /** NestJS: scan @Controller + @Get/@Post etc. decorators */
  _scanNestJsRoutes(routes) {
    const tsFiles = collectFiles(this.subprojectPath, '.ts');
    const controllerRe = /@Controller\s*\(\s*['"]?([^'")\s]*)['"]?\s*\)/g;
    const methodRe = /@(Get|Post|Put|Delete|Patch|Head|Options)\s*\(\s*['"]?([^'")\s]*)['"]?\s*\)/g;

    for (const file of tsFiles) {
      const content = readFileSafe(file);
      if (!content || !content.includes('@Controller')) continue;

      const rel = relativePath(this.subprojectPath, file);

      controllerRe.lastIndex = 0;
      let cm;
      while ((cm = controllerRe.exec(content)) !== null) {
        const prefix = cm[1] ? '/' + cm[1].replace(/^\//, '') : '/';
        const endpoints = [];

        // Scan for HTTP method decorators after the controller declaration
        const afterCtrl = content.slice(cm.index);
        methodRe.lastIndex = 0;
        let mm;
        while ((mm = methodRe.exec(afterCtrl)) !== null) {
          const method = mm[1].toUpperCase();
          const subPath = mm[2] ? '/' + mm[2].replace(/^\//, '') : '';
          const fullPath = subPath ? prefix + subPath : prefix;

          // Try to get the handler function name (next identifier after decorator)
          const handlerMatch = afterCtrl.slice(mm.index + mm[0].length).match(/\s*(?:async\s+)?(\w+)\s*\(/);
          const handlerName = handlerMatch ? handlerMatch[1] : undefined;

          endpoints.push({ method, path: fullPath, name: handlerName });
        }

        if (endpoints.length) {
          routes.set(prefix + ':' + rel, {
            file: rel,
            prefix,
            endpoints,
          });
        }
      }
    }
  }

  /** Express/Hono: scan router.get/post/put/delete/patch calls */
  _scanExpressHonoRoutes(routes) {
    const tsFiles = [...collectFiles(this.subprojectPath, '.ts'), ...collectFiles(this.subprojectPath, '.js')];
    const re = /(?:router|app)\.(get|post|put|delete|patch)\s*\(\s*['"]([^'"]+)['"]/g;

    for (const file of tsFiles) {
      const content = readFileSafe(file);
      if (!content) continue;
      if (!content.includes('router.') && !content.includes('app.')) continue;

      const rel = relativePath(this.subprojectPath, file);
      const endpoints = [];

      re.lastIndex = 0;
      let m;
      while ((m = re.exec(content)) !== null) {
        endpoints.push({
          method: m[1].toUpperCase(),
          path: m[2],
          name: undefined,
        });
      }

      if (endpoints.length) {
        // Detect a common prefix for this file's routes
        const prefix = this._inferRoutePrefix(endpoints.map(e => e.path));
        routes.set(rel, {
          file: rel,
          prefix,
          endpoints,
        });
      }
    }
  }

  /** Infer a common route prefix from a list of paths */
  _inferRoutePrefix(paths) {
    if (!paths.length) return '/';
    const parts = paths[0].split('/').filter(Boolean);
    let common = '';
    for (const segment of parts) {
      if (paths.every(p => p.startsWith('/' + segment))) {
        common += '/' + segment;
      } else {
        break;
      }
    }
    return common || '/';
  }

  // -------------------------------------------------------------------------
  // scanDtos()
  // -------------------------------------------------------------------------

  /**
   * Scan DTOs, request/response shapes, and Zod schemas.
   * @returns {Map<string, import('../scanner-contract').DtoInfo>}
   */
  scanDtos() {
    const dtos = new Map();
    try {
      const fw = this._detectFrameworks();
      const tsFiles = collectFiles(this.subprojectPath, '.ts');

      // Classes/interfaces ending in Dto/Request/Response/Input/Output
      const classRe = /export\s+(?:class|interface|type)\s+(\w+(?:Dto|Request|Response|Input|Output))/g;
      // Zod schemas
      const zodRe = /export\s+const\s+(\w+(?:Schema|Input|Output))\s*=\s*z\./g;

      for (const file of tsFiles) {
        const content = readFileSafe(file);
        if (!content) continue;
        const rel = relativePath(this.subprojectPath, file);

        // Determine validation pattern
        let validationPattern = 'none';
        if (fw.zod && content.includes('z.')) validationPattern = 'zod';
        else if (fw.classValidator && content.includes('@Is')) validationPattern = 'class-validator';

        classRe.lastIndex = 0;
        let m;
        while ((m = classRe.exec(content)) !== null) {
          const name = m[1];
          // Infer linked entity: strip suffix
          const entity = name.replace(/(?:Dto|Request|Response|Input|Output)$/, '') || undefined;
          dtos.set(name, {
            file: rel,
            entity: entity !== name ? entity : undefined,
            validationPattern,
          });
        }

        if (fw.zod) {
          zodRe.lastIndex = 0;
          while ((m = zodRe.exec(content)) !== null) {
            const name = m[1];
            dtos.set(name, {
              file: rel,
              validationPattern: 'zod',
            });
          }
        }
      }
    } catch (err) {
      process.stderr.write(`[typescript-scanner] scanDtos error: ${err.message}\n`);
    }
    return dtos;
  }

  // -------------------------------------------------------------------------
  // scanServices()
  // -------------------------------------------------------------------------

  /**
   * Scan service classes: NestJS @Injectable services and plain *Service exports.
   * @returns {Map<string, import('../scanner-contract').ServiceInfo>}
   */
  scanServices() {
    const services = new Map();
    try {
      const fw = this._detectFrameworks();
      const tsFiles = collectFiles(this.subprojectPath, '.ts');

      // NestJS: @Injectable class *Service
      const nestRe = /@Injectable\s*\(\s*\)[\s\S]*?class\s+(\w+Service)/g;
      // Generic: export class *Service
      const genericRe = /export\s+class\s+(\w+Service)/g;

      for (const file of tsFiles) {
        const content = readFileSafe(file);
        if (!content) continue;

        const rel = relativePath(this.subprojectPath, file);

        if (fw.nestjs && content.includes('@Injectable')) {
          nestRe.lastIndex = 0;
          let m;
          while ((m = nestRe.exec(content)) !== null) {
            const className = m[1];
            const entity = className.replace(/Service$/, '');
            const deps = this._extractConstructorDeps(content, m.index);

            services.set(className, {
              file: rel,
              entity,
              dependencies: deps.length ? deps : undefined,
            });
          }
        } else if (content.includes('Service')) {
          genericRe.lastIndex = 0;
          let m;
          while ((m = genericRe.exec(content)) !== null) {
            const className = m[1];
            // Skip if already added by NestJS path
            if (!services.has(className)) {
              const entity = className.replace(/Service$/, '');
              services.set(className, { file: rel, entity });
            }
          }
        }
      }
    } catch (err) {
      process.stderr.write(`[typescript-scanner] scanServices error: ${err.message}\n`);
    }
    return services;
  }

  /** Extract constructor parameter types (injected interfaces/services) */
  _extractConstructorDeps(content, classStart) {
    try {
      const body = content.slice(classStart, classStart + 3000);
      const ctorMatch = body.match(/constructor\s*\(([^)]*)\)/);
      if (!ctorMatch) return [];
      const params = ctorMatch[1];
      const deps = [];
      // Extract type annotations: private readonly foo: FooService
      const paramRe = /:\s*([A-Z]\w+)/g;
      let m;
      while ((m = paramRe.exec(params)) !== null) {
        deps.push(m[1]);
      }
      return deps;
    } catch {
      return [];
    }
  }

  // -------------------------------------------------------------------------
  // scanRepositories()
  // -------------------------------------------------------------------------

  /**
   * Scan repository classes: NestJS @Injectable repos and plain *Repository exports.
   * @returns {Map<string, import('../scanner-contract').RepoInfo>}
   */
  scanRepositories() {
    const repos = new Map();
    try {
      const fw = this._detectFrameworks();
      const tsFiles = collectFiles(this.subprojectPath, '.ts');

      const nestRe = /@Injectable\s*\(\s*\)[\s\S]*?class\s+(\w+Repository)/g;
      const genericRe = /export\s+class\s+(\w+Repository)/g;

      for (const file of tsFiles) {
        const content = readFileSafe(file);
        if (!content) continue;

        const rel = relativePath(this.subprojectPath, file);

        if (fw.nestjs && content.includes('@Injectable')) {
          nestRe.lastIndex = 0;
          let m;
          while ((m = nestRe.exec(content)) !== null) {
            const className = m[1];
            const entity = className.replace(/Repository$/, '');
            // Detect base class (extends ...)
            const baseMatch = content.slice(m.index, m.index + 300).match(/extends\s+(\w+)/);
            const baseClass = baseMatch ? baseMatch[1] : undefined;

            repos.set(className, { file: rel, entity, baseClass });
          }
        } else if (content.includes('Repository')) {
          genericRe.lastIndex = 0;
          let m;
          while ((m = genericRe.exec(content)) !== null) {
            const className = m[1];
            if (!repos.has(className)) {
              const entity = className.replace(/Repository$/, '');
              const baseMatch = content.slice(m.index, m.index + 300).match(/extends\s+(\w+)/);
              const baseClass = baseMatch ? baseMatch[1] : undefined;
              repos.set(className, { file: rel, entity, baseClass });
            }
          }
        }
      }
    } catch (err) {
      process.stderr.write(`[typescript-scanner] scanRepositories error: ${err.message}\n`);
    }
    return repos;
  }

  // -------------------------------------------------------------------------
  // inferPatterns()
  // -------------------------------------------------------------------------

  /**
   * Infer structural patterns from all scanned data for _patterns.typescript.
   * @param {{ entities: Map, enums: Map, interfaces: Map, routes: Map, dtos: Map, services: Map, repositories: Map }} scanResults
   * @returns {Object}
   */
  inferPatterns(scanResults) {
    try {
      const fw = this._detectFrameworks();
      const { entities, enums, dtos, routes, services } = scanResults;

      // ---- ORM ----
      let orm = 'none';
      if (fw.drizzle) orm = 'drizzle';
      else if (fw.prisma) orm = 'prisma';
      else if (fw.typeorm) orm = 'typeorm';

      // ---- Framework ----
      let framework = 'none';
      if (fw.nestjs) framework = 'nestjs';
      else if (fw.nextjs) framework = 'nextjs';
      else if (fw.express) framework = 'express';
      else if (fw.hono) framework = 'hono';

      // ---- Entity patterns ----
      const entityFiles = [...entities.values()].map(e => e.file).filter(Boolean);
      const entityFolder = inferCommonFolder(entityFiles);
      let defStyle = 'none';
      if (fw.drizzle) defStyle = 'pgTable';
      else if (fw.typeorm) defStyle = 'decorator';
      else if (fw.prisma) defStyle = 'prisma-model';
      const entityNaming = this._detectNamingConvention([...entities.keys()]);

      // ---- Enum patterns ----
      const enumFiles = [...enums.values()].map(e => e.file).filter(Boolean);
      const enumFolder = inferCommonFolder(enumFiles);
      const enumStyles = [...enums.values()].map(e => e.decorators?.[0]).filter(Boolean);
      const enumDefStyle = this._mostCommon(enumStyles) || 'ts-enum';
      const allEnumValues = [...enums.values()].flatMap(e => e.values || []);
      const enumValueConvention = detectValueConvention(allEnumValues);
      const enumSeparateFiles = enumFolder !== null && enumFolder !== entityFolder;

      // ---- Route patterns ----
      let routeStyle = 'none';
      if (fw.nextjs) routeStyle = 'file-based';
      else if (fw.nestjs) routeStyle = 'decorator';
      else if (fw.express || fw.hono) routeStyle = 'minimal-api';
      const allPrefixes = [...routes.values()].map(r => r.prefix).filter(Boolean);
      const routePrefix = this._detectCommonPrefix(allPrefixes);
      const routePaths = [...routes.values()].flatMap(r => r.endpoints || []).map(e => e.path).filter(Boolean);
      const routeNaming = this._detectRouteNamingPattern(routePaths);

      // ---- DTO patterns ----
      const dtoFiles = [...dtos.values()].map(d => d.file).filter(Boolean);
      const dtoFolder = inferCommonFolder(dtoFiles);
      let validationTool = 'none';
      if (fw.zod) validationTool = 'zod';
      else if (fw.classValidator) validationTool = 'class-validator';
      else {
        // Check from actual dtos
        const patterns = [...dtos.values()].map(d => d.validationPattern).filter(p => p && p !== 'none');
        validationTool = this._mostCommon(patterns) || 'none';
      }

      return {
        orm,
        framework,
        entity: {
          folder: entityFolder,
          defStyle,
          namingConvention: entityNaming,
        },
        enum: {
          folder: enumFolder,
          defStyle: enumDefStyle,
          separateFiles: enumSeparateFiles,
          valueConvention: enumValueConvention,
        },
        routes: {
          style: routeStyle,
          prefix: routePrefix,
          namingPattern: routeNaming,
        },
        dto: {
          folder: dtoFolder,
          validationTool,
        },
      };
    } catch (err) {
      process.stderr.write(`[typescript-scanner] inferPatterns error: ${err.message}\n`);
      return {};
    }
  }

  // -------------------------------------------------------------------------
  // Pattern inference helpers
  // -------------------------------------------------------------------------

  /** Convert camelCase/snake_case identifier to PascalCase */
  _toPascalCase(name) {
    if (!name) return name;
    // Already PascalCase
    if (/^[A-Z]/.test(name)) return name;
    return name.charAt(0).toUpperCase() + name.slice(1);
  }

  /** Detect naming convention from a list of names */
  _detectNamingConvention(names) {
    if (!names.length) return 'PascalCase';
    const pascal = names.filter(n => /^[A-Z][a-zA-Z0-9]*$/.test(n)).length;
    const camel = names.filter(n => /^[a-z][a-zA-Z0-9]*$/.test(n)).length;
    return pascal >= camel ? 'PascalCase' : 'camelCase';
  }

  /** Get the most common element in an array */
  _mostCommon(arr) {
    if (!arr.length) return null;
    const counts = new Map();
    for (const v of arr) counts.set(v, (counts.get(v) || 0) + 1);
    let max = 0;
    let result = null;
    for (const [v, c] of counts) {
      if (c > max) { max = c; result = v; }
    }
    return result;
  }

  /** Detect shared prefix across all route paths */
  _detectCommonPrefix(paths) {
    if (!paths.length) return '/';
    // Find the most common first segment
    const firstSegments = paths.map(p => {
      const parts = p.replace(/^\//, '').split('/');
      return parts[0] || '';
    }).filter(Boolean);
    const common = this._mostCommon(firstSegments);
    return common ? '/' + common : '/';
  }

  /** Detect route naming pattern: kebab-case, camelCase, or mixed */
  _detectRouteNamingPattern(paths) {
    if (!paths.length) return 'unknown';
    const segments = paths.flatMap(p =>
      p.split('/').filter(s => s && !s.startsWith(':') && !s.startsWith('{'))
    );
    if (!segments.length) return 'unknown';
    const kebab = segments.filter(s => /^[a-z][a-z0-9]*(-[a-z0-9]+)*$/.test(s)).length;
    const camel = segments.filter(s => /^[a-z][a-zA-Z0-9]*$/.test(s) && /[A-Z]/.test(s)).length;
    const total = segments.length;
    if (kebab / total > 0.5) return 'kebab-case';
    if (camel / total > 0.5) return 'camelCase';
    return 'mixed';
  }
}

module.exports = TypeScriptScanner;
