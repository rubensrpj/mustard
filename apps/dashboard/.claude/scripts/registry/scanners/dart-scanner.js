'use strict';

/**
 * dart-scanner.js
 *
 * Stack scanner for Dart / Flutter projects.
 * Detects via pubspec.yaml. Scans models, enums, abstract interfaces,
 * routes (GoRouter, AutoRoute, GetX, Navigator 2.0), DTOs, services,
 * repositories, and infers patterns (state management, navigation,
 * serialization strategy).
 */

const { ScannerContract } = require('../scanner-contract');
const { collectFiles, relativePath, readFileSafe, inferCommonFolder } = require('../file-utils');
const path = require('path');
const fs = require('fs');

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/**
 * Return true if any entry inside basePath matches one of the given names
 * (case-insensitive for the last path segment / folder name).
 * @param {string} basePath
 * @param {string[]} names
 * @returns {boolean}
 */
function hasFolderOrFile(basePath, names) {
  try {
    const entries = fs.readdirSync(basePath, { withFileTypes: true });
    for (const entry of entries) {
      const lower = entry.name.toLowerCase();
      if (names.some(n => lower === n.toLowerCase())) return true;
    }
  } catch { /* ignore */ }
  return false;
}

/**
 * Recursively check if a directory path segment exists under basePath.
 * @param {string} basePath
 * @param {string} segment - folder name (case-insensitive)
 * @returns {boolean}
 */
function hasDirRecursive(basePath, segment) {
  const lower = segment.toLowerCase();
  try {
    const walk = (dir) => {
      const entries = fs.readdirSync(dir, { withFileTypes: true });
      for (const e of entries) {
        if (!e.isDirectory()) continue;
        if (e.name.startsWith('.')) continue;
        if (e.name === 'node_modules') continue;
        if (e.name.toLowerCase() === lower) return true;
        if (walk(path.join(dir, e.name))) return true;
      }
      return false;
    };
    return walk(basePath);
  } catch { return false; }
}

/**
 * Read pubspec.yaml and return its raw text (null on error).
 * @param {string} subprojectPath
 * @returns {string|null}
 */
function readPubspec(subprojectPath) {
  return readFileSafe(path.join(subprojectPath, 'pubspec.yaml'));
}

/**
 * Detect if a package name appears in pubspec.yaml dependencies.
 * @param {string} pubspecContent
 * @param {string} pkg
 * @returns {boolean}
 */
function pubspecHas(pubspecContent, pkg) {
  if (!pubspecContent) return false;
  // Match "  pkg:" or "  pkg: ^x.y.z" under dependencies / dev_dependencies
  return new RegExp(`^\\s+${pkg.replace('/', '\\/')}\\s*:`,'m').test(pubspecContent);
}

// ---------------------------------------------------------------------------
// DartScanner
// ---------------------------------------------------------------------------

class DartScanner extends ScannerContract {
  static stackId = 'dart';

  // -------------------------------------------------------------------------
  // detect
  // -------------------------------------------------------------------------

  detect() {
    try {
      return fs.existsSync(path.join(this.subprojectPath, 'pubspec.yaml'));
    } catch { return false; }
  }

  // -------------------------------------------------------------------------
  // detectArchitecture
  // -------------------------------------------------------------------------

  detectArchitecture() {
    try {
      const libPath = path.join(this.subprojectPath, 'lib');
      if (!fs.existsSync(libPath)) return 'minimal';

      // Clean Architecture: domain / data / presentation layers
      if (
        hasDirRecursive(libPath, 'domain') &&
        hasDirRecursive(libPath, 'data') &&
        hasDirRecursive(libPath, 'presentation')
      ) return 'clean-architecture';

      // BLoC / Cubit
      if (hasDirRecursive(libPath, 'bloc') || hasDirRecursive(libPath, 'cubit')) {
        return 'bloc';
      }

      // MVVM: view_model or viewmodel + model + view
      const hasViewModels =
        hasDirRecursive(libPath, 'view_model') ||
        hasDirRecursive(libPath, 'viewmodel') ||
        hasDirRecursive(libPath, 'viewmodels');
      if (hasViewModels && hasDirRecursive(libPath, 'model') && hasDirRecursive(libPath, 'view')) {
        return 'mvvm';
      }

      // MVC: controller folder
      if (hasDirRecursive(libPath, 'controller') || hasDirRecursive(libPath, 'controllers')) {
        return 'mvc';
      }

      // Flat lib/ => minimal
      return 'minimal';
    } catch { return 'unknown'; }
  }

  // -------------------------------------------------------------------------
  // scanEntities
  // -------------------------------------------------------------------------

  scanEntities() {
    const result = new Map();
    try {
      const dartFiles = collectFiles(this.subprojectPath, '.dart', ['.dart_tool', 'build']);

      // Focus on model/entity/domain files but also scan all .dart
      const classRe = /class\s+(\w+)\s*(?:extends\s+(\w+))?\s*(?:with\s+([^{]+?))?\s*(?:implements\s+([^{]+?))?\s*\{/g;

      for (const filePath of dartFiles) {
        const rel = relativePath(this.subprojectPath, filePath);
        const relLower = rel.toLowerCase();

        // Focus heuristic: files in models/, entities/, domain/
        const isFocused =
          relLower.includes('/models/') ||
          relLower.includes('/model/') ||
          relLower.includes('/entities/') ||
          relLower.includes('/entity/') ||
          relLower.includes('/domain/');

        const content = readFileSafe(filePath);
        if (!content) continue;

        // Skip abstract class files (handled in scanInterfaces)
        let match;
        classRe.lastIndex = 0;
        while ((match = classRe.exec(content)) !== null) {
          const [, name, baseClass, withClause, implementsClause] = match;

          // Skip abstract classes here (they go to scanInterfaces)
          const lineStart = content.lastIndexOf('\n', match.index);
          const lineEnd = content.indexOf('\n', match.index);
          const surroundingLines = content.slice(
            Math.max(0, lineStart - 100),
            lineEnd === -1 ? content.length : lineEnd
          );
          if (/abstract\s+(?:class|interface)/.test(surroundingLines)) continue;

          // Decorators / mixins
          const decorators = [];
          const isFreezed = content.includes('@freezed') && content.includes('factory');
          const isJsonSerializable = content.includes('@JsonSerializable()');
          const isEquatable = baseClass === 'Equatable' || baseClass === 'EquatableMixin';
          if (isFreezed) decorators.push('@freezed');
          if (isJsonSerializable) decorators.push('@JsonSerializable');
          if (isEquatable) decorators.push('Equatable');

          // Mixins from "with" clause
          const mixins = withClause
            ? withClause.split(',').map(s => s.trim()).filter(Boolean)
            : [];

          // Interfaces from "implements" clause
          const interfaces = implementsClause
            ? implementsClause.split(',').map(s => s.trim()).filter(Boolean)
            : [];

          // Only register if in a focused path OR it's a meaningful model class
          const isModelClass =
            isFocused ||
            name.endsWith('Model') ||
            name.endsWith('Entity') ||
            name.endsWith('Dto') ||
            name.endsWith('Request') ||
            name.endsWith('Response') ||
            isFreezed ||
            isEquatable;

          if (!isModelClass) continue;

          result.set(name, {
            file: rel,
            baseClass: baseClass || undefined,
            interfaces: interfaces.length ? interfaces : undefined,
            decorators: decorators.length ? decorators : undefined,
            mixins: mixins.length ? mixins : undefined,
          });
        }
      }
    } catch (err) {
      process.stderr.write(`[dart-scanner] scanEntities error: ${err.message}\n`);
    }
    return result;
  }

  // -------------------------------------------------------------------------
  // scanEnums
  // -------------------------------------------------------------------------

  scanEnums() {
    const result = new Map();
    try {
      const dartFiles = collectFiles(this.subprojectPath, '.dart', ['.dart_tool', 'build']);
      // gs flag via separate approach — iterate file by file
      const enumRe = /enum\s+(\w+)\s*\{([^}]*)\}/gs;

      for (const filePath of dartFiles) {
        const content = readFileSafe(filePath);
        if (!content) continue;

        const rel = relativePath(this.subprojectPath, filePath);

        enumRe.lastIndex = 0;
        let match;
        while ((match = enumRe.exec(content)) !== null) {
          const [, name, body] = match;

          // Parse values — stop at constructor / method markers
          const rawValues = body
            .split(/[,;]/)
            .map(v => v.trim())
            .filter(v => v && !v.startsWith('//') && !v.startsWith('const ') && !v.startsWith('final ') && !v.includes('(') && !v.includes('{'))
            .map(v => v.replace(/\/\/.*$/, '').trim())
            .filter(v => /^\w+/.test(v));

          // Enhanced enum detection (Dart 3): has constructors or methods inside
          const enhanced =
            body.includes('const ') ||
            /\w+\s+get\s+\w+/.test(body) ||
            /\w+\s+\w+\s*\(/.test(body);

          result.set(name, {
            values: rawValues,
            file: rel,
            enhanced,
          });
        }
      }
    } catch (err) {
      process.stderr.write(`[dart-scanner] scanEnums error: ${err.message}\n`);
    }
    return result;
  }

  // -------------------------------------------------------------------------
  // scanInterfaces
  // -------------------------------------------------------------------------

  scanInterfaces() {
    const result = new Map();
    try {
      const dartFiles = collectFiles(this.subprojectPath, '.dart', ['.dart_tool', 'build']);
      const abstractRe = /abstract\s+(?:class|interface)\s+(\w+)(?:\s+extends\s+([\w, ]+?))?\s*(?:implements\s+([\w, ]+?))?\s*\{/g;

      for (const filePath of dartFiles) {
        const content = readFileSafe(filePath);
        if (!content) continue;

        const rel = relativePath(this.subprojectPath, filePath);

        abstractRe.lastIndex = 0;
        let match;
        while ((match = abstractRe.exec(content)) !== null) {
          const [, name, extendsClause, implementsClause] = match;

          const extendsArr = extendsClause
            ? extendsClause.split(',').map(s => s.trim()).filter(Boolean)
            : [];
          const implementsArr = implementsClause
            ? implementsClause.split(',').map(s => s.trim()).filter(Boolean)
            : [];

          result.set(name, {
            file: rel,
            extends: extendsArr.length ? extendsArr : undefined,
            interfaces: implementsArr.length ? implementsArr : undefined,
          });
        }
      }
    } catch (err) {
      process.stderr.write(`[dart-scanner] scanInterfaces error: ${err.message}\n`);
    }
    return result;
  }

  // -------------------------------------------------------------------------
  // scanRoutes
  // -------------------------------------------------------------------------

  scanRoutes() {
    const result = new Map();
    try {
      const dartFiles = collectFiles(this.subprojectPath, '.dart', ['.dart_tool', 'build']);

      // GoRouter: GoRoute(path: '/foo')
      const goRouterRe = /GoRoute\s*\(\s*path:\s*['"]([^'"]+)['"]/g;
      // GetX: GetPage(name: '/foo')
      const getxRe = /GetPage\s*\(\s*name:\s*['"]([^'"]+)['"]/g;
      // AutoRoute: @RoutePage() on a class
      const autoRouteClassRe = /@RoutePage\(\)\s*(?:\n\s*)*class\s+(\w+)/g;
      // Navigator 2.0 named routes: '/' : (context) => ...  or routes: { '/foo': ... }
      const namedRouteRe = /['"](\/{1}[^'"]+)['"]\s*:/g;
      // MaterialApp routes map entries
      const materialRouteRe = /['"](\/{1}[^'"]+)['"]\s*:\s*(?:\(|[\w$])/g;

      const goRoutes = [];
      const getxRoutes = [];
      const autoRoutePages = [];
      const namedRoutes = [];

      for (const filePath of dartFiles) {
        const content = readFileSafe(filePath);
        if (!content) continue;
        const rel = relativePath(this.subprojectPath, filePath);

        // GoRouter
        goRouterRe.lastIndex = 0;
        let m;
        while ((m = goRouterRe.exec(content)) !== null) {
          goRoutes.push({ method: 'GET', path: m[1], file: rel });
        }

        // GetX
        getxRe.lastIndex = 0;
        while ((m = getxRe.exec(content)) !== null) {
          getxRoutes.push({ method: 'GET', path: m[1], file: rel });
        }

        // AutoRoute
        autoRouteClassRe.lastIndex = 0;
        while ((m = autoRouteClassRe.exec(content)) !== null) {
          autoRoutePages.push({ method: 'GET', path: `/${m[1]}`, name: m[1], file: rel });
        }

        // Named routes (MaterialApp)
        if (content.includes('routes:') || content.includes('onGenerateRoute')) {
          materialRouteRe.lastIndex = 0;
          while ((m = materialRouteRe.exec(content)) !== null) {
            const route = m[1];
            if (!namedRoutes.find(r => r.path === route)) {
              namedRoutes.push({ method: 'GET', path: route, file: rel });
            }
          }
        }
      }

      // Determine which router is in use and emit accordingly
      if (goRoutes.length > 0) {
        result.set('go_router', { file: 'lib/', prefix: '/', endpoints: goRoutes });
      }
      if (getxRoutes.length > 0) {
        result.set('getx', { file: 'lib/', prefix: '/', endpoints: getxRoutes });
      }
      if (autoRoutePages.length > 0) {
        result.set('auto_route', { file: 'lib/', prefix: '/', endpoints: autoRoutePages });
      }
      if (namedRoutes.length > 0 && goRoutes.length === 0 && getxRoutes.length === 0) {
        result.set('navigator', { file: 'lib/', prefix: '/', endpoints: namedRoutes });
      }
    } catch (err) {
      process.stderr.write(`[dart-scanner] scanRoutes error: ${err.message}\n`);
    }
    return result;
  }

  // -------------------------------------------------------------------------
  // scanDtos
  // -------------------------------------------------------------------------

  scanDtos() {
    const result = new Map();
    try {
      const dartFiles = collectFiles(this.subprojectPath, '.dart', ['.dart_tool', 'build']);
      const classRe = /class\s+(\w+)\s*(?:extends\s+(\w+))?/g;

      for (const filePath of dartFiles) {
        const content = readFileSafe(filePath);
        if (!content) continue;
        const rel = relativePath(this.subprojectPath, filePath);

        classRe.lastIndex = 0;
        let match;
        while ((match = classRe.exec(content)) !== null) {
          const [, name, baseClass] = match;
          const isDtoClass =
            name.endsWith('Dto') ||
            name.endsWith('Model') ||
            name.endsWith('Request') ||
            name.endsWith('Response');

          if (!isDtoClass) continue;

          // Infer linked entity from class name
          let entity;
          for (const suffix of ['Dto', 'Request', 'Response', 'Model']) {
            if (name.endsWith(suffix)) {
              entity = name.slice(0, -suffix.length) || undefined;
              break;
            }
          }

          const validationPattern = content.includes('@JsonSerializable()') ? 'json_serializable'
            : content.includes('@freezed') ? 'freezed'
            : 'manual';

          result.set(name, {
            file: rel,
            entity: entity || undefined,
            baseClass: baseClass || undefined,
            validationPattern,
          });
        }
      }
    } catch (err) {
      process.stderr.write(`[dart-scanner] scanDtos error: ${err.message}\n`);
    }
    return result;
  }

  // -------------------------------------------------------------------------
  // scanServices
  // -------------------------------------------------------------------------

  scanServices() {
    const result = new Map();
    try {
      const dartFiles = collectFiles(this.subprojectPath, '.dart', ['.dart_tool', 'build']);
      const classRe = /class\s+(\w+)\s*(?:extends\s+(\w+))?\s*(?:implements\s+([\w, ]+?))?[\s{]/g;

      for (const filePath of dartFiles) {
        const content = readFileSafe(filePath);
        if (!content) continue;
        const rel = relativePath(this.subprojectPath, filePath);

        classRe.lastIndex = 0;
        let match;
        while ((match = classRe.exec(content)) !== null) {
          const [, name, baseClass, implementsClause] = match;
          const isService =
            name.endsWith('Service') ||
            name.endsWith('UseCase') ||
            name.endsWith('Interactor');

          if (!isService) continue;

          const interfaces = implementsClause
            ? implementsClause.split(',').map(s => s.trim()).filter(Boolean)
            : [];

          // Linked entity: strip suffix
          let entity;
          for (const suffix of ['Service', 'UseCase', 'Interactor']) {
            if (name.endsWith(suffix)) {
              entity = name.slice(0, -suffix.length) || undefined;
              break;
            }
          }

          result.set(name, {
            file: rel,
            interface: interfaces.length === 1 ? interfaces[0] : undefined,
            entity: entity || undefined,
            baseClass: baseClass || undefined,
          });
        }
      }
    } catch (err) {
      process.stderr.write(`[dart-scanner] scanServices error: ${err.message}\n`);
    }
    return result;
  }

  // -------------------------------------------------------------------------
  // scanRepositories
  // -------------------------------------------------------------------------

  scanRepositories() {
    const result = new Map();
    try {
      const dartFiles = collectFiles(this.subprojectPath, '.dart', ['.dart_tool', 'build']);
      const classRe = /class\s+(\w+)\s*(?:extends\s+(\w+))?\s*(?:implements\s+([\w, ]+?))?[\s{]/g;

      for (const filePath of dartFiles) {
        const content = readFileSafe(filePath);
        if (!content) continue;
        const rel = relativePath(this.subprojectPath, filePath);

        classRe.lastIndex = 0;
        let match;
        while ((match = classRe.exec(content)) !== null) {
          const [, name, baseClass, implementsClause] = match;
          const isRepo =
            name.endsWith('Repository') ||
            name.endsWith('DataSource') ||
            name.endsWith('Repo');

          if (!isRepo) continue;

          const interfaces = implementsClause
            ? implementsClause.split(',').map(s => s.trim()).filter(Boolean)
            : [];

          // Linked entity
          let entity;
          for (const suffix of ['Repository', 'DataSource', 'Repo']) {
            if (name.endsWith(suffix)) {
              entity = name.slice(0, -suffix.length) || undefined;
              break;
            }
          }

          result.set(name, {
            file: rel,
            interface: interfaces.length === 1 ? interfaces[0] : undefined,
            entity: entity || undefined,
            baseClass: baseClass || undefined,
          });
        }
      }
    } catch (err) {
      process.stderr.write(`[dart-scanner] scanRepositories error: ${err.message}\n`);
    }
    return result;
  }

  // -------------------------------------------------------------------------
  // inferPatterns
  // -------------------------------------------------------------------------

  inferPatterns(scanResults) {
    try {
      const pubspec = readPubspec(this.subprojectPath) || '';

      // ---- State management ----
      let stateManagement = 'none';
      if (pubspecHas(pubspec, 'flutter_bloc') || pubspecHas(pubspec, 'bloc')) {
        stateManagement = 'bloc';
      } else if (
        pubspecHas(pubspec, 'flutter_riverpod') ||
        pubspecHas(pubspec, 'riverpod') ||
        pubspecHas(pubspec, 'hooks_riverpod')
      ) {
        stateManagement = 'riverpod';
      } else if (pubspecHas(pubspec, 'provider')) {
        stateManagement = 'provider';
      } else if (pubspecHas(pubspec, 'get')) {
        stateManagement = 'getx';
      } else if (pubspecHas(pubspec, 'mobx') || pubspecHas(pubspec, 'flutter_mobx')) {
        stateManagement = 'mobx';
      } else {
        // Fallback: check for BLoC/Cubit class patterns in scanned files
        const dartFiles = collectFiles(this.subprojectPath, '.dart', ['.dart_tool', 'build']);
        for (const filePath of dartFiles) {
          const content = readFileSafe(filePath);
          if (!content) continue;
          if (/extends\s+Bloc</.test(content) || /extends\s+Cubit</.test(content)) {
            stateManagement = 'bloc';
            break;
          }
          if (/ChangeNotifier/.test(content) || /Provider</.test(content)) {
            stateManagement = 'provider';
            break;
          }
          if (/GetxController/.test(content) || /Obx\(/.test(content)) {
            stateManagement = 'getx';
            break;
          }
          if (/@observable/.test(content) || /@action/.test(content)) {
            stateManagement = 'mobx';
            break;
          }
        }
      }

      // ---- Navigation ----
      let navigation = 'none';
      if (scanResults.routes.has('go_router') || pubspecHas(pubspec, 'go_router')) {
        navigation = 'go_router';
      } else if (scanResults.routes.has('auto_route') || pubspecHas(pubspec, 'auto_route')) {
        navigation = 'auto_route';
      } else if (scanResults.routes.has('getx') || stateManagement === 'getx') {
        navigation = 'getx';
      } else if (scanResults.routes.has('navigator')) {
        navigation = 'navigator';
      }

      // ---- Serialization ----
      let serialization = 'none';
      const hasFreezed = pubspecHas(pubspec, 'freezed') || pubspecHas(pubspec, 'freezed_annotation');
      const hasJsonSer = pubspecHas(pubspec, 'json_serializable');
      if (hasFreezed) {
        serialization = 'freezed';
      } else if (hasJsonSer) {
        serialization = 'json_serializable';
      } else {
        // Check entity decorators
        for (const [, info] of scanResults.entities) {
          if (info.decorators?.includes('@freezed')) { serialization = 'freezed'; break; }
          if (info.decorators?.includes('@JsonSerializable')) { serialization = 'json_serializable'; break; }
        }
        if (serialization === 'none' && scanResults.entities.size > 0) {
          serialization = 'manual';
        }
      }

      // ---- Entity patterns ----
      const entityFiles = [...scanResults.entities.values()].map(e => e.file).filter(Boolean);
      const entityFolder = inferCommonFolder(entityFiles);

      let basePattern = 'plain';
      for (const [, info] of scanResults.entities) {
        if (info.decorators?.includes('@freezed')) { basePattern = 'freezed'; break; }
        if (info.decorators?.includes('Equatable')) { basePattern = 'equatable'; break; }
      }

      // ---- Enum patterns ----
      const enumFiles = [...scanResults.enums.values()].map(e => e.file).filter(Boolean);
      const enumFolder = inferCommonFolder(enumFiles);
      const hasEnhancedEnums = [...scanResults.enums.values()].some(e => e.enhanced);
      // Separate files: more than half enums in dedicated enum files
      const enumInEnumDir = enumFiles.filter(f => f.toLowerCase().includes('enum')).length;
      const enumSeparateFiles = enumFiles.length > 0 && (enumInEnumDir / enumFiles.length) > 0.5;

      // ---- Repository patterns ----
      const repoFiles = [...scanResults.repositories.values()].map(r => r.file).filter(Boolean);
      const repoFolder = inferCommonFolder(repoFiles);
      // Interface-first: most repos implement an interface
      const reposWithInterface = [...scanResults.repositories.values()].filter(r => r.interface).length;
      const interfaceFirst = scanResults.repositories.size > 0
        ? (reposWithInterface / scanResults.repositories.size) > 0.5
        : false;

      return {
        stateManagement,
        navigation,
        serialization,
        entity: {
          folder: entityFolder,
          basePattern,
          namingConvention: 'PascalCase',
        },
        enum: {
          folder: enumFolder,
          enhanced: hasEnhancedEnums,
          separateFiles: enumSeparateFiles,
        },
        repository: {
          interfaceFirst,
          folder: repoFolder,
        },
      };
    } catch (err) {
      process.stderr.write(`[dart-scanner] inferPatterns error: ${err.message}\n`);
      return {};
    }
  }
}

module.exports = DartScanner;
