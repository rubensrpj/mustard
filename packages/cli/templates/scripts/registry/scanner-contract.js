'use strict';

/**
 * scanner-contract.js
 *
 * Base contract for all stack scanners (Interface Segregation + Liskov Substitution).
 * Every scanner extends ScannerContract and implements detect() and optionally
 * the scan* methods. Each method has a single responsibility.
 *
 * Usage:
 *   const { ScannerContract } = require('./scanner-contract');
 *   class MyScanner extends ScannerContract { ... }
 */

// ---------------------------------------------------------------------------
// JSDoc typedefs
// ---------------------------------------------------------------------------

/**
 * @typedef {Object} EntityInfo
 * @property {string} file - relative path from subproject root
 * @property {string} [namespace]
 * @property {string} [baseClass]
 * @property {string[]} [interfaces]
 * @property {string[]} [decorators] - class-level decorators/attributes
 * @property {string[]} [refs] - referenced entities (FK/navigation)
 * @property {string[]} [sub] - child/collection entities
 * @property {string[]} [enums] - enum types used
 * @property {string[]} [properties] - key property names with types
 */

/**
 * @typedef {Object} EnumInfo
 * @property {string[]} values
 * @property {string} file - relative path
 * @property {string} [namespace]
 * @property {string[]} [decorators] - enum-level decorators
 * @property {string[]} [valueDecorators] - decorators found on values (e.g., Description, Display)
 * @property {string} [valueConvention] - UPPER_CASE | PascalCase | camelCase
 */

/**
 * @typedef {Object} InterfaceInfo
 * @property {string} file
 * @property {string} [namespace]
 * @property {string[]} [methods]
 * @property {string[]} [extends] - parent interfaces
 * @property {string[]} [implementedBy] - known implementing classes
 */

/**
 * @typedef {Object} RouteInfo
 * @property {string} file
 * @property {string} prefix - route group prefix (e.g., /contracts)
 * @property {Object[]} endpoints - { method, path, name, auth }
 */

/**
 * @typedef {Object} DtoInfo
 * @property {string} file
 * @property {string} [namespace]
 * @property {string} [entity] - linked entity name
 * @property {string} [validationPattern] - FluentValidation, Zod, class-validator, etc.
 */

/**
 * @typedef {Object} ServiceInfo
 * @property {string} file
 * @property {string} [interface] - interface it implements
 * @property {string} [entity] - linked entity name
 * @property {string[]} [dependencies] - injected interfaces
 */

/**
 * @typedef {Object} RepoInfo
 * @property {string} file
 * @property {string} [interface]
 * @property {string} [entity]
 * @property {string} [baseClass]
 */

// ---------------------------------------------------------------------------
// ScannerContract
// ---------------------------------------------------------------------------

/**
 * Base contract for stack scanners (Interface Segregation + Liskov Substitution).
 * Every scanner implements detect() and scan(). Each method has a single responsibility.
 */
class ScannerContract {
  /**
   * @param {string} subprojectPath - absolute path to subproject root
   * @param {Object} subprojectMeta - metadata from sync-detect.js output
   */
  constructor(subprojectPath, subprojectMeta) {
    if (new.target === ScannerContract) {
      throw new Error('ScannerContract is abstract — extend it');
    }
    this.subprojectPath = subprojectPath;
    this.meta = subprojectMeta;
  }

  /**
   * Detect if this scanner applies to the subproject.
   * @returns {boolean}
   */
  detect() { throw new Error('detect() not implemented'); }

  /**
   * Detect the architecture pattern used in the project.
   * @returns {string} - e.g., 'solid', 'layered', 'minimal', 'mvvm', 'mvc'
   */
  detectArchitecture() { return 'unknown'; }

  /**
   * Scan entities (models, domain objects).
   * @returns {Map<string, EntityInfo>}
   */
  scanEntities() { return new Map(); }

  /**
   * Scan enums/value types.
   * @returns {Map<string, EnumInfo>}
   */
  scanEnums() { return new Map(); }

  /**
   * Scan interfaces/contracts.
   * @returns {Map<string, InterfaceInfo>}
   */
  scanInterfaces() { return new Map(); }

  /**
   * Scan routes/endpoints.
   * @returns {Map<string, RouteInfo>}
   */
  scanRoutes() { return new Map(); }

  /**
   * Scan DTOs/schemas/view models.
   * @returns {Map<string, DtoInfo>}
   */
  scanDtos() { return new Map(); }

  /**
   * Scan services.
   * @returns {Map<string, ServiceInfo>}
   */
  scanServices() { return new Map(); }

  /**
   * Scan repositories.
   * @returns {Map<string, RepoInfo>}
   */
  scanRepositories() { return new Map(); }

  /**
   * Infer structural patterns from all scanned data.
   * Called AFTER all scan methods. Receives the results.
   * @param {{ entities: Map, enums: Map, interfaces: Map, routes: Map, dtos: Map, services: Map, repositories: Map }} scanResults
   * @returns {Object} - patterns object for _patterns.{stack}
   */
  inferPatterns(scanResults) { return {}; } // eslint-disable-line no-unused-vars

  /**
   * Run the full scan pipeline.
   * Calls all scan* methods in order, then inferPatterns, then returns the combined result.
   * @returns {{ entities: Map, enums: Map, interfaces: Map, routes: Map, dtos: Map, services: Map, repositories: Map, patterns: Object }}
   */
  scan() {
    const entities = this.scanEntities();
    const enums = this.scanEnums();
    const interfaces = this.scanInterfaces();
    const routes = this.scanRoutes();
    const dtos = this.scanDtos();
    const services = this.scanServices();
    const repositories = this.scanRepositories();
    const architecture = this.detectArchitecture();

    const scanResults = { entities, enums, interfaces, routes, dtos, services, repositories };
    const patterns = this.inferPatterns(scanResults);
    patterns.architecture = architecture;

    return { ...scanResults, patterns };
  }
}

module.exports = { ScannerContract };
