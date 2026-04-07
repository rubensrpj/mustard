'use strict';

/**
 * schema-builder.js
 *
 * Single Responsibility: builds the entity-registry.json v4.0 output object
 * from normalized scanner results. No scanning, no file I/O.
 *
 * Usage:
 *   const { buildRegistry, computeSourceHash } = require('./registry/schema-builder');
 *   const registry = buildRegistry({ scanResults });
 */

const crypto = require('crypto');
const fs = require('fs');

// ---------------------------------------------------------------------------
// buildRegistry
// ---------------------------------------------------------------------------

/**
 * Build entity-registry.json v4.0 from scanner results.
 *
 * @param {Object} options
 * @param {Map<string, Object>} options.scanResults - stackId → scan() result
 * @param {string} [options.sourceHash] - SHA256 of all scanned files (first 16 hex chars)
 * @returns {Object} - the registry JSON object
 */
function buildRegistry({ scanResults, sourceHash }) {
  const registry = {
    _meta: {
      version: '4.0',
      generated: new Date().toISOString().split('T')[0],
      generator: 'sync-registry.js',
    },
    _patterns: {},
    _enums: {},
    e: {},
  };

  if (sourceHash) registry._meta.sourceHash = sourceHash;

  for (const [stackId, result] of scanResults) {
    // Patterns per stack
    if (result.patterns && Object.keys(result.patterns).length > 0) {
      registry._patterns[stackId] = result.patterns;
    }

    // Enums — merge across stacks, richer info where available
    if (result.enums) {
      for (const [name, info] of result.enums) {
        // Rich info (has file, decorators, namespace) → keep as object
        if (info.file || info.decorators?.length || info.namespace) {
          const entry = { values: _compressValues(info.values) };
          if (info.file) entry.file = info.file;
          if (info.namespace) entry.namespace = info.namespace;
          if (info.decorators?.length) entry.decorators = info.decorators;
          if (info.valueDecorators?.length) entry.valueDecorators = info.valueDecorators;
          if (info.valueConvention) entry.valueConvention = info.valueConvention;
          registry._enums[name] = entry;
        } else {
          // Bare array for backward compat (v3.1 style)
          const vals = Array.isArray(info) ? info : (info.values || []);
          registry._enums[name] = _compressValues(vals);
        }
      }
    }

    // Entities — merge across stacks, compact (omit empty fields)
    if (result.entities) {
      for (const [name, info] of result.entities) {
        const entry = {};
        if (info.file) entry.file = info.file;
        if (info.namespace) entry.namespace = info.namespace;
        if (info.baseClass) entry.baseClass = info.baseClass;
        if (info.interfaces?.length) entry.interfaces = info.interfaces;
        if (info.decorators?.length) entry.decorators = info.decorators;
        if (info.refs?.length) entry.refs = [...info.refs].sort();
        if (info.sub?.length) entry.sub = [...info.sub].sort();
        if (info.enums?.length) entry.enums = [...info.enums].sort();
        if (info.dtos?.length) entry.dtos = info.dtos;
        if (info.services?.length) entry.services = info.services;
        if (info.repositories?.length) entry.repositories = info.repositories;
        if (info.routePrefix) entry.routePrefix = info.routePrefix;
        if (info.endpoints?.length) entry.endpoints = info.endpoints;
        registry.e[name] = entry;
      }
    }
  }

  // Sort keys for deterministic output
  registry._enums = sortKeys(registry._enums);
  registry.e = sortKeys(registry.e);

  return registry;
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/**
 * Compress an enum values array: if >8 values, keep first 5 + count summary.
 * @param {string[]} values
 * @returns {string[]}
 */
function _compressValues(values) {
  if (!Array.isArray(values)) return [];
  if (values.length > 8) {
    return [...values.slice(0, 5), `...(${values.length} total)`];
  }
  return values;
}

/**
 * Sort an object's keys alphabetically, returning a new object.
 * @param {Object} obj
 * @returns {Object}
 */
function sortKeys(obj) {
  const sorted = {};
  for (const key of Object.keys(obj).sort()) {
    sorted[key] = obj[key];
  }
  return sorted;
}

/**
 * Compute a short SHA256 hash (first 16 hex chars) over a set of file contents.
 * Files are sorted before hashing for determinism.
 *
 * @param {string[]} filePaths - absolute paths to files to include in the hash
 * @returns {string} - 16-character hex string
 */
function computeSourceHash(filePaths) {
  const hash = crypto.createHash('sha256');
  for (const fp of [...filePaths].sort()) {
    try {
      hash.update(fs.readFileSync(fp));
    } catch { /* skip unreadable files */ }
  }
  return hash.digest('hex').slice(0, 16);
}

module.exports = { buildRegistry, computeSourceHash, sortKeys };
