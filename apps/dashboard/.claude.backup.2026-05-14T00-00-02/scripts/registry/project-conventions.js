'use strict';

/**
 * project-conventions.js
 *
 * Single Responsibility: derive declarative project conventions from the
 * filesystem in a fully agnostic way. No technology-specific tables — every
 * value emerges from the user's own files.
 *
 * v1 — naming convention dominante (filenames classified by regex):
 *   - kebab-case      → ^[a-z][a-z0-9]*(-[a-z0-9]+)+$
 *   - camelCase       → ^[a-z][a-zA-Z0-9]*$  (and contains at least one upper)
 *   - PascalCase      → ^[A-Z][a-zA-Z0-9]*$
 *   - snake_case      → ^[a-z][a-z0-9_]*$    (and contains at least one underscore)
 *   - lowercase       → ^[a-z][a-z0-9]*$     (no boundary)
 *   - mixed           → anything else
 *
 * Output:
 *   { naming: { dominant, distribution, total } }
 *   `dominant` = bucket whose share is ≥ DOMINANCE_THRESHOLD (default 0.6),
 *   otherwise null (i.e. "mixed").
 */

const path = require('path');
const { collectFiles } = require('./file-utils');

const DOMINANCE_THRESHOLD = Math.max(
  0.5,
  Math.min(0.95, parseFloat(process.env.MUSTARD_NAMING_DOMINANCE) || 0.6)
);

/**
 * Map a stackId to its primary file extension (for filename collection).
 * Mirrors `_primaryExtForStack` in cluster-discovery.js.
 *
 * @param {string} stackId
 * @returns {string|null}
 */
function _primaryExtForStack(stackId) {
  const extMap = {
    dotnet: '.cs',
    typescript: '.ts',
    dart: '.dart',
    java: '.java',
    kotlin: '.kt',
    go: '.go',
    rust: '.rs',
    python: '.py',
    php: '.php',
  };
  return extMap[stackId] || null;
}

/**
 * Classify a basename (without extension) into a naming bucket.
 *
 * @param {string} base
 * @returns {string} - one of 'kebab-case' | 'camelCase' | 'PascalCase' | 'snake_case' | 'lowercase' | 'mixed'
 */
function classifyName(base) {
  if (!base || typeof base !== 'string') return 'mixed';

  if (/^[a-z][a-z0-9]*(-[a-z0-9]+)+$/.test(base)) return 'kebab-case';
  if (/^[A-Z][a-zA-Z0-9]*$/.test(base)) return 'PascalCase';
  if (/^[a-z][a-z0-9_]*$/.test(base) && base.includes('_')) return 'snake_case';
  if (/^[a-z][a-zA-Z0-9]*$/.test(base) && /[A-Z]/.test(base)) return 'camelCase';
  if (/^[a-z][a-z0-9]*$/.test(base)) return 'lowercase';
  return 'mixed';
}

/**
 * Compute the dominant naming convention across files of a subproject's
 * primary extension.
 *
 * @param {string} subprojectPath - absolute path to subproject root
 * @param {string} stackId
 * @returns {{ naming: { dominant: string|null, distribution: Object<string, number>, total: number } }}
 */
function computeProjectConventions(subprojectPath, stackId) {
  try {
    const ext = _primaryExtForStack(stackId);
    if (!ext) return { naming: { dominant: null, distribution: {}, total: 0 } };

    const files = collectFiles(subprojectPath, ext);
    if (!files.length) return { naming: { dominant: null, distribution: {}, total: 0 } };

    const distribution = {};
    for (const f of files) {
      const base = path.basename(f, ext);
      const bucket = classifyName(base);
      distribution[bucket] = (distribution[bucket] || 0) + 1;
    }

    let dominant = null;
    const total = files.length;
    for (const [bucket, count] of Object.entries(distribution)) {
      if (count / total >= DOMINANCE_THRESHOLD) {
        dominant = bucket;
        break;
      }
    }

    return { naming: { dominant, distribution, total } };
  } catch {
    return { naming: { dominant: null, distribution: {}, total: 0 } };
  }
}

module.exports = { computeProjectConventions, classifyName };
