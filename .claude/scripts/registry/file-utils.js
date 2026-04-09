'use strict';

/**
 * file-utils.js
 *
 * Single Responsibility: file collection and path helpers shared across scanners.
 * No scanning logic, no schema building — only filesystem utilities.
 *
 * Usage:
 *   const { collectFiles, relativePath, readFileSafe } = require('./registry/file-utils');
 */

const fs = require('fs');
const path = require('path');

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const DEFAULT_IGNORE = [
  'node_modules', 'bin', 'obj', 'dist', '.next',
  '__pycache__', '.venv', 'venv', 'target', 'build',
  '.git', 'migrations', 'Migrations',
];

// ---------------------------------------------------------------------------
// collectFiles
// ---------------------------------------------------------------------------

/**
 * Recursively collect all files with the given extension under a directory.
 * Skips DEFAULT_IGNORE directories, dot-directories, and any extra ignore dirs.
 *
 * @param {string} dir - root directory to walk
 * @param {string} extension - file extension including dot, e.g. '.cs', '.ts'
 * @param {string[]} [ignore] - additional directory names to skip
 * @returns {string[]} - absolute file paths
 */
function collectFiles(dir, extension, ignore = []) {
  const allIgnore = new Set([...DEFAULT_IGNORE, ...ignore]);
  const results = [];

  function walk(currentDir) {
    try {
      const entries = fs.readdirSync(currentDir, { withFileTypes: true });
      for (const entry of entries) {
        if (entry.isDirectory()) {
          if (allIgnore.has(entry.name) || entry.name.startsWith('.')) continue;
          walk(path.join(currentDir, entry.name));
        } else if (entry.name.endsWith(extension)) {
          results.push(path.join(currentDir, entry.name));
        }
      }
    } catch { /* ignore permission errors */ }
  }

  walk(dir);
  return results;
}

// ---------------------------------------------------------------------------
// relativePath
// ---------------------------------------------------------------------------

/**
 * Get a relative path from a base directory, normalised with forward slashes.
 *
 * @param {string} base - absolute base directory
 * @param {string} filePath - absolute file path
 * @returns {string} - relative path with forward slashes
 */
function relativePath(base, filePath) {
  return path.relative(base, filePath).replace(/\\/g, '/');
}

// ---------------------------------------------------------------------------
// readFileSafe
// ---------------------------------------------------------------------------

/**
 * Read a file as UTF-8 string. Returns null on any error.
 *
 * @param {string} filePath - absolute path to file
 * @returns {string|null}
 */
function readFileSafe(filePath) {
  try {
    return fs.readFileSync(filePath, 'utf-8');
  } catch {
    return null;
  }
}

// ---------------------------------------------------------------------------
// inferCommonFolder
// ---------------------------------------------------------------------------

/**
 * Detect the most common parent folder from a list of relative file paths.
 * Useful for pattern inference (e.g., "most entities live in Domain/Entities/").
 *
 * @param {string[]} filePaths - relative paths (forward slashes)
 * @returns {string|null} - most common parent folder with trailing slash, or null
 */
function inferCommonFolder(filePaths) {
  if (!filePaths.length) return null;

  const counts = new Map();
  for (const fp of filePaths) {
    const dir = path.dirname(fp).replace(/\\/g, '/');
    counts.set(dir, (counts.get(dir) || 0) + 1);
  }

  let maxDir = null;
  let maxCount = 0;
  for (const [dir, count] of counts) {
    if (count > maxCount) {
      maxDir = dir;
      maxCount = count;
    }
  }

  return maxDir ? maxDir + '/' : null;
}

module.exports = { collectFiles, relativePath, readFileSafe, inferCommonFolder, DEFAULT_IGNORE };
