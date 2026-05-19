'use strict';

/**
 * scanner-loader.js
 *
 * Dependency Inversion: sync-registry.js depends on this loader, not on concrete scanners.
 * Open/Closed: adding a new stack scanner = dropping a new file in scanners/, zero other changes.
 *
 * Usage:
 *   const { loadScanner, detectStack } = require('./registry/scanner-loader');
 *   const scanner = loadScanner(subprojectPath, subprojectMeta);
 *   if (scanner) { const result = scanner.scan(); }
 */

const fs = require('fs');
const path = require('path');

const SCANNERS_DIR = path.join(__dirname, 'scanners');

// ---------------------------------------------------------------------------
// Stack detection signals
// Each key is the stack ID that maps to a scanner file: {stackId}-scanner.js
// ---------------------------------------------------------------------------

const STACK_SIGNALS = {
  dotnet: { files: ['*.csproj', '*.sln'], dirs: [] },
  typescript: { files: ['package.json', 'tsconfig.json'], dirs: [] },
  dart: { files: ['pubspec.yaml'], dirs: ['lib'] },
  php: { files: ['composer.json', 'artisan'], dirs: [] },
  python: { files: ['pyproject.toml', 'setup.py', 'requirements.txt', 'manage.py'], dirs: [] },
  java: { files: ['pom.xml', 'build.gradle', 'build.gradle.kts'], dirs: [] },
  go: { files: ['go.mod'], dirs: [] },
  rust: { files: ['Cargo.toml'], dirs: [] },
};

// ---------------------------------------------------------------------------
// detectStack
// ---------------------------------------------------------------------------

/**
 * Detect which stack a subproject uses via file-presence heuristics.
 * Iterates STACK_SIGNALS in definition order (most specific first).
 *
 * @param {string} subprojectPath - absolute path to subproject root
 * @returns {string|null} - stack ID (e.g., 'dotnet') or null if unrecognised
 */
function detectStack(subprojectPath) {
  for (const [stackId, signals] of Object.entries(STACK_SIGNALS)) {
    for (const pattern of signals.files) {
      // Handle glob-like patterns (*.ext) — match any file with that extension
      if (pattern.startsWith('*')) {
        const ext = pattern.slice(1); // e.g., '.csproj'
        try {
          const entries = fs.readdirSync(subprojectPath);
          if (entries.some(e => e.endsWith(ext))) return stackId;
        } catch { /* ignore unreadable dirs */ }
      } else {
        if (fs.existsSync(path.join(subprojectPath, pattern))) return stackId;
      }
    }
  }
  return null;
}

// ---------------------------------------------------------------------------
// loadScanner
// ---------------------------------------------------------------------------

/**
 * Load the appropriate scanner class for a subproject and return an instance,
 * or null if no scanner is available or detect() returns false.
 *
 * Resolution order:
 *   1. Use subprojectMeta.stack if provided
 *   2. Fall back to detectStack()
 *   3. Look for scanners/{stackId}-scanner.js
 *   4. Instantiate and call detect() — return null if detect() is false
 *
 * @param {string} subprojectPath - absolute path to subproject root
 * @param {Object} subprojectMeta - metadata from sync-detect.js output
 * @returns {import('./scanner-contract').ScannerContract|null}
 */
function loadScanner(subprojectPath, subprojectMeta) {
  const stackId = subprojectMeta.stack || detectStack(subprojectPath);
  if (!stackId) return null;

  const scannerFile = path.join(SCANNERS_DIR, `${stackId}-scanner.js`);
  if (!fs.existsSync(scannerFile)) return null;

  try {
    const ScannerClass = require(scannerFile);
    const scanner = new ScannerClass(subprojectPath, subprojectMeta);
    if (scanner.detect()) return scanner;
  } catch (err) {
    process.stderr.write(`[scanner-loader] Failed to load ${stackId} scanner: ${err.message}\n`);
  }

  return null;
}

// ---------------------------------------------------------------------------
// listAvailableScanners
// ---------------------------------------------------------------------------

/**
 * List all scanner files currently present in the scanners/ directory.
 * Useful for diagnostics and --list-scanners flag.
 * @returns {string[]} - array of stack IDs with scanners installed
 */
function listAvailableScanners() {
  try {
    return fs.readdirSync(SCANNERS_DIR)
      .filter(f => f.endsWith('-scanner.js'))
      .map(f => f.replace('-scanner.js', ''));
  } catch {
    return [];
  }
}

module.exports = { detectStack, loadScanner, listAvailableScanners, STACK_SIGNALS };
