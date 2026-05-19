'use strict';

/**
 * cluster-discovery.js
 *
 * Generic, technology-agnostic cluster discovery for sync-registry.js.
 *
 * Discovers structural patterns by examining the filesystem — folder layout,
 * shared file suffixes, and shared base classes — without any knowledge of
 * specific technology names (no vendor-specific keywords are hardcoded).
 *
 * The words that appear in cluster.suffix / cluster.label are derived
 * exclusively from what was found on the user's filesystem.
 *
 * Cluster types emitted:
 *   - 'folder-cluster'  : ≥5 files in the SAME folder share a PascalCase suffix
 *   - 'suffix-cluster'  : same suffix appears across MULTIPLE folders (consolidated)
 *   - 'base-class-cluster' : ≥MIN_BASE_CLASS_INHERITORS classes share a common base
 *                           (C# `:`, TypeScript `extends`, Python `class X(Y)`)
 *   - 'decorator-cluster' : ≥MIN_DECORATOR_USAGE files share a decorator/annotation
 *                           (TS/Python/Java/Kotlin `@Name`, C# `[Name]`)
 *   - 'function-prefix-cluster' : ≥MIN_FUNCTION_PREFIX_USAGE top-level functions
 *                                share a camelCase/snake_case prefix (TS/Python)
 *
 * Cache: results are persisted at `<subproject>/.claude/.cluster-cache.json`,
 * keyed by stackId and a SHA-256 of (file path, size, mtime) over the scanned
 * file-set. Disabled when MUSTARD_CLUSTER_CACHE=off.
 *
 * Limits: at most MAX_CLUSTERS (default 15, env MUSTARD_CLUSTER_MAX) per call,
 * ranked by fileCount desc. Clusters dropped above the cap are logged to stderr.
 * Other tunable thresholds: MUSTARD_CLUSTER_MIN_FILES, MUSTARD_CLUSTER_MIN_SUFFIX_LEN,
 * MUSTARD_CLUSTER_MIN_BASE_INHERITORS.
 */

const crypto = require('crypto');
const fs = require('fs');
const path = require('path');
const { collectFiles, relativePath, readFileSafe } = require('./file-utils');

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/** Directories to skip during cluster discovery. */
const SKIP_DIRS = new Set([
  'node_modules', 'bin', 'obj', '.git', 'dist', 'build',
  'migrations', 'Migrations', '__pycache__', '.venv', 'venv',
  'target', '.next',
]);

/** Minimum number of files sharing a suffix to qualify as a cluster. */
const MIN_FILES_PER_SUFFIX = Math.max(2, parseInt(process.env.MUSTARD_CLUSTER_MIN_FILES, 10) || 5);

/** Minimum suffix length in characters to avoid false positives (e.g. "s", "ed"). */
const MIN_SUFFIX_LENGTH    = Math.max(2, parseInt(process.env.MUSTARD_CLUSTER_MIN_SUFFIX_LEN, 10) || 6);

/** Minimum number of classes extending the same base for a base-class-cluster. */
const MIN_BASE_CLASS_INHERITORS = Math.max(2, parseInt(process.env.MUSTARD_CLUSTER_MIN_BASE_INHERITORS, 10) || 3);

/** Minimum number of files using the same decorator/annotation for a decorator-cluster. */
const MIN_DECORATOR_USAGE = Math.max(2, parseInt(process.env.MUSTARD_DECORATOR_MIN, 10) || 3);

/** Minimum number of top-level functions sharing a prefix for a function-prefix-cluster. */
const MIN_FUNCTION_PREFIX_USAGE = Math.max(2, parseInt(process.env.MUSTARD_FN_PREFIX_MIN, 10) || 5);

/** Minimum prefix length (characters) — discards single-letter noise like `f`, `a`. */
const MIN_FUNCTION_PREFIX_LEN = Math.max(2, parseInt(process.env.MUSTARD_FN_PREFIX_MIN_LEN, 10) || 2);

/** Minimum number of distinct folders sharing the same basename for a filename-cluster.
 *  Catches Next.js feature-folder patterns (e.g. `detail.tsx` in N feature folders) that
 *  PascalCase suffix matchers miss because the basename is a single lowercase word. */
const MIN_FILENAME_FOLDERS = Math.max(2, parseInt(process.env.MUSTARD_FILENAME_MIN_FOLDERS, 10) || 3);

/** Basenames considered structural — skipped from filename-cluster detection. */
const STRUCTURAL_BASENAMES = new Set([
  // Next.js conventions
  'page', 'layout', 'loading', 'error', 'not-found', 'route',
  'middleware', 'template', 'default', 'global-error',
  // JS/TS module conventions
  'index', 'main',
  // Common config noise
  'config', 'types', 'constants',
]);

/**
 * Universal comment-line prefixes — covers most modern languages without
 * being technology-specific. Used to skip comments during enrichment.
 */
const COMMENT_PREFIXES = ['//', '#', '--', '/*', ';', '%'];

/** Maximum samples read per cluster during enrichment. */
const MAX_ENRICHMENT_SAMPLES = Math.max(1, parseInt(process.env.MUSTARD_ENRICHMENT_MAX, 10) || 5);

/** Maximum clusters returned per discovery call (before skill-generator applies its own top-10 cap). */
const MAX_CLUSTERS         = Math.max(1, parseInt(process.env.MUSTARD_CLUSTER_MAX, 10) || 30);

/** Bypass cluster cache (per-discovery). Set MUSTARD_CLUSTER_CACHE=off to disable. */
const CLUSTER_CACHE_DISABLED = String(process.env.MUSTARD_CLUSTER_CACHE || '').toLowerCase() === 'off';

/** Cache schema version — bump when cluster shape changes.
 *  v2 (2026-05-03): added enrichment fields (namingPattern, declarationKeywords,
 *  declarationSuffix, topOfFileLines, memberSuffixes).
 *  v3 (2026-05-03): added subprojectName tag for orchestrator slicing. */
const CLUSTER_CACHE_VERSION = 3;

// ---------------------------------------------------------------------------
// Cache helpers
// ---------------------------------------------------------------------------

/**
 * Compute a deterministic hash of a file-set, scoped by stackId. Includes
 * size and mtime for each file so that any content-mutating change invalidates
 * the cache. Files are sorted by absolute path before hashing.
 *
 * @param {string} stackId
 * @param {string[]} files - absolute file paths
 * @returns {string} - 16-char hex digest (or '' on error)
 */
function _computeFileSetHash(stackId, files) {
  try {
    const h = crypto.createHash('sha256');
    // Cache key includes all tunables that affect the kept output, so a tuning
    // change (env var or default bump) invalidates stale entries automatically.
    const tunables = [
      MIN_FILES_PER_SUFFIX,
      MIN_SUFFIX_LENGTH,
      MIN_BASE_CLASS_INHERITORS,
      MAX_CLUSTERS,
      MIN_DECORATOR_USAGE,
      MIN_FUNCTION_PREFIX_USAGE,
      MIN_FUNCTION_PREFIX_LEN,
      MIN_FILENAME_FOLDERS,
    ].join(',');
    h.update(`v${CLUSTER_CACHE_VERSION}|${stackId}|t=${tunables}|`);
    const sorted = [...files].sort();
    for (const f of sorted) {
      try {
        const st = fs.statSync(f);
        h.update(`${f}|${st.size}|${st.mtimeMs}\n`);
      } catch {
        h.update(`${f}|missing\n`);
      }
    }
    return h.digest('hex').slice(0, 16);
  } catch {
    return '';
  }
}

/**
 * Resolve the per-subproject cluster cache file path.
 *
 * @param {string} subprojectPath
 * @returns {string}
 */
function _clusterCachePath(subprojectPath) {
  return path.join(subprojectPath, '.claude', '.cluster-cache.json');
}

/**
 * Read the cluster cache file (or return null on miss/error).
 *
 * @param {string} subprojectPath
 * @returns {Object|null}
 */
function _readClusterCache(subprojectPath) {
  try {
    const p = _clusterCachePath(subprojectPath);
    if (!fs.existsSync(p)) return null;
    const raw = fs.readFileSync(p, 'utf-8');
    const parsed = JSON.parse(raw);
    if (!parsed || parsed.cacheVersion !== CLUSTER_CACHE_VERSION) return null;
    return parsed;
  } catch {
    return null;
  }
}

/**
 * Atomically write the cluster cache file (best-effort; fails silently).
 *
 * @param {string} subprojectPath
 * @param {Object} payload
 */
function _writeClusterCache(subprojectPath, payload) {
  try {
    const p = _clusterCachePath(subprojectPath);
    fs.mkdirSync(path.dirname(p), { recursive: true });
    fs.writeFileSync(p, JSON.stringify(payload), 'utf-8');
  } catch { /* fail-open */ }
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/**
 * Discover structural clusters in a subproject.
 *
 * @param {string} subprojectPath - absolute path to the subproject root
 * @param {string} stackId - detected stack id (e.g. 'dotnet', 'typescript')
 * @param {string} [subprojectName] - optional subproject name to tag each cluster with
 * @returns {Array<Object>} - array of cluster descriptors
 */
function discoverClusters(subprojectPath, stackId, subprojectName) {
  try {
    const ext = _primaryExtForStack(stackId);
    if (!ext) return [];

    const allFiles = collectFiles(subprojectPath, ext);

    // ---- Cache lookup ---------------------------------------------------
    let fileSetHash = '';
    if (!CLUSTER_CACHE_DISABLED) {
      fileSetHash = _computeFileSetHash(stackId, allFiles);
      if (fileSetHash) {
        const cached = _readClusterCache(subprojectPath);
        if (cached
          && cached.entries
          && cached.entries[stackId]
          && cached.entries[stackId].hash === fileSetHash
          && Array.isArray(cached.entries[stackId].clusters)
        ) {
          return cached.entries[stackId].clusters.slice(0, MAX_CLUSTERS);
        }
      }
    }
    // ---------------------------------------------------------------------

    // Step 1: global suffix scan across ALL files (catches 1-per-folder patterns like
    // "one QueryResolver per module folder" that per-folder grouping would miss)
    const globalClusters = _discoverGlobalSuffixClusters(subprojectPath, allFiles, ext);

    // Step 2: per-folder suffix clusters (catches dense folders with ≥5 same-suffix files)
    const folderClusters = _discoverFolderClusters(subprojectPath, allFiles, ext);

    // Step 3: consolidate per-folder clusters into cross-folder suffix clusters
    const { consolidated, remaining } = _consolidateClusters(folderClusters);

    // Step 4: discover base-class / extends clusters per stack (cross-folder)
    let baseClassClusters = [];
    if (stackId === 'dotnet') {
      baseClassClusters = _discoverBaseClassClustersDotnet(subprojectPath, allFiles);
    } else if (stackId === 'typescript') {
      baseClassClusters = _discoverBaseClassClustersTypeScript(subprojectPath, allFiles);
    } else if (stackId === 'python') {
      baseClassClusters = _discoverBaseClassClustersPython(subprojectPath, allFiles);
    }

    // Step 5: decorator / annotation clusters (agnostic; names emerge from source)
    const decoratorClusters = _discoverDecoratorClusters(subprojectPath, allFiles, stackId);

    // Step 6: function-prefix clusters (camelCase/snake_case shared prefixes)
    const fnPrefixClusters = _discoverFunctionPrefixClusters(subprojectPath, allFiles, stackId);

    // Step 7: filename clusters (same basename repeated across folders — Next.js feature-folder pattern).
    // For typescript, also scan .tsx — that's where React/Next.js components live, and the rest of
    // cluster-discovery only sees .ts. Without this, `detail.tsx` in N feature folders is invisible.
    const extraFilenameFiles = stackId === 'typescript'
      ? collectFiles(subprojectPath, '.tsx')
      : [];
    const filenameClusters = _discoverFilenameClusters(subprojectPath, allFiles, ext, extraFilenameFiles);

    // Merge all clusters; deduplicate by (kind, suffix, ext) so same name across kinds coexist.
    const all = _mergeClusters([
      ...globalClusters,
      ...consolidated,
      ...remaining,
      ...baseClassClusters,
      ...decoratorClusters,
      ...fnPrefixClusters,
      ...filenameClusters,
    ]);
    all.sort((a, b) => b.fileCount - a.fileCount);

    const ranked = all;
    const kept = ranked.slice(0, MAX_CLUSTERS);
    const dropped = ranked.slice(MAX_CLUSTERS);
    if (dropped.length > 0) {
      try {
        const summary = dropped.map(c => `${c.kind}:${c.label || c.suffix || c.commonBaseClass || '?'}(${c.fileCount})`).join(', ');
        process.stderr.write(`[cluster-discovery] dropped ${dropped.length} cluster(s) above MAX_CLUSTERS=${MAX_CLUSTERS}: ${summary}\n`);
      } catch { /* fail-open */ }
    }

    // ---- Enrichment: extract universal metadata from samples ------------
    // Done once per cluster, results cached. Keeps subsequent /scan agents
    // from re-reading the same files for Convention fields.
    for (const cluster of kept) {
      _enrichCluster(cluster, subprojectPath);
      if (subprojectName) cluster.subprojectName = subprojectName;
    }

    // ---- Cache write ----------------------------------------------------
    if (!CLUSTER_CACHE_DISABLED && fileSetHash) {
      const existing = _readClusterCache(subprojectPath) || { cacheVersion: CLUSTER_CACHE_VERSION, entries: {} };
      existing.cacheVersion = CLUSTER_CACHE_VERSION;
      if (!existing.entries) existing.entries = {};
      existing.entries[stackId] = {
        hash: fileSetHash,
        clusters: kept,
        savedAt: new Date().toISOString(),
      };
      _writeClusterCache(subprojectPath, existing);
    }
    // ---------------------------------------------------------------------

    return kept;
  } catch {
    return []; // fail-open
  }
}

// ---------------------------------------------------------------------------
// Step 1: per-folder suffix discovery
// ---------------------------------------------------------------------------

/**
 * Group files by their immediate parent folder, then find shared PascalCase
 * suffixes (≥5 files, ≥6 chars) within each folder.
 *
 * @param {string} subprojectPath
 * @param {string[]} allFiles - absolute file paths
 * @param {string} ext - file extension including dot
 * @returns {Array<Object>} - raw folder-cluster descriptors (may have fileCount < 5 per folder)
 */
function _discoverFolderClusters(subprojectPath, allFiles, ext) {
  // Group files by folder (relative path)
  const byFolder = new Map();
  for (const f of allFiles) {
    const rel = relativePath(subprojectPath, f);
    const dir = path.dirname(rel).replace(/\\/g, '/');
    if (!byFolder.has(dir)) byFolder.set(dir, []);
    byFolder.get(dir).push({ abs: f, rel, base: path.basename(f, ext) });
  }

  const clusters = [];

  for (const [folder, files] of byFolder) {
    if (files.length < 2) continue; // not enough to find a shared suffix

    // Extract PascalCase words from each basename and find shared trailing words
    const suffixMap = _groupBySuffix(files.map(f => f.base));

    for (const [suffix, matchingBases] of suffixMap) {
      if (suffix.length < MIN_SUFFIX_LENGTH) continue;
      if (matchingBases.length < 2) continue; // need ≥2 to track; consolidation handles ≥5

      const samples = matchingBases.slice(0, 3).map(b => b + ext);
      clusters.push({
        kind: 'folder-cluster',
        folder,
        suffix,
        ext,
        fileCount: matchingBases.length,
        samples,
        label: suffix,
      });
    }
  }

  return clusters;
}

// ---------------------------------------------------------------------------
// Step 1b: global suffix scan (catches 1-per-folder patterns)
// ---------------------------------------------------------------------------

/**
 * Scan ALL files in the subproject for shared PascalCase suffixes, regardless
 * of which folder they live in. This catches patterns where each folder has
 * only 1-2 files but the suffix repeats across many folders (e.g., one
 * "*Resolver" per module).
 *
 * @param {string} subprojectPath
 * @param {string[]} allFiles - absolute file paths
 * @param {string} ext - file extension including dot
 * @returns {Array<Object>} - suffix-cluster descriptors for qualifying suffixes
 */
function _discoverGlobalSuffixClusters(subprojectPath, allFiles, ext) {
  // Build: suffix → [{basename, folder}]
  const suffixToFiles = new Map();

  for (const f of allFiles) {
    const rel = relativePath(subprojectPath, f);
    const dir = path.dirname(rel).replace(/\\/g, '/');
    const base = path.basename(f, ext);
    const words = _splitPascalCase(base);
    if (words.length < 2) continue;

    // Try each possible trailing word group
    for (let wordCount = 1; wordCount < words.length; wordCount++) {
      const suffix = words.slice(words.length - wordCount).join('');
      if (suffix.length < MIN_SUFFIX_LENGTH) continue;

      if (!suffixToFiles.has(suffix)) suffixToFiles.set(suffix, []);
      suffixToFiles.get(suffix).push({ base, folder: dir, file: path.basename(rel) });
    }
  }

  // Prune: keep only suffixes with ≥ MIN_FILES_PER_SUFFIX total files
  for (const [suffix, files] of suffixToFiles) {
    if (files.length < MIN_FILES_PER_SUFFIX) suffixToFiles.delete(suffix);
  }

  // Prune subset suffixes globally (same logic as per-folder)
  const suffixNames = [...suffixToFiles.keys()];
  const toDelete = new Set();
  for (const shorter of suffixNames) {
    for (const longer of suffixNames) {
      if (longer === shorter) continue;
      if (!longer.endsWith(shorter)) continue;
      const shorterFiles = suffixToFiles.get(shorter);
      const longerFiles = suffixToFiles.get(longer);
      if (!shorterFiles || !longerFiles) continue;
      // If every file in longer is also in shorter, shorter is redundant
      const shorterBases = new Set(shorterFiles.map(f => f.base));
      const longerBases = new Set(longerFiles.map(f => f.base));
      const allInShorter = [...longerBases].every(b => shorterBases.has(b));
      if (allInShorter && longerBases.size === shorterBases.size) {
        toDelete.add(shorter);
      }
    }
  }
  for (const s of toDelete) suffixToFiles.delete(s);

  // Build cluster descriptors
  const clusters = [];
  for (const [suffix, files] of suffixToFiles) {
    const folders = [...new Set(files.map(f => f.folder))];
    const sharedParent = _commonFolderSegment(folders);
    const folderPattern = folders.length === 1
      ? folders[0] + '/'
      : (sharedParent ? `**/${sharedParent}/` : '(multiple)');
    const samples = files.slice(0, 3).map(f => f.file);

    clusters.push({
      kind: 'suffix-cluster',
      suffix,
      ext,
      fileCount: files.length,
      folders,
      folderPattern,
      samples,
      label: suffix,
    });
  }

  return clusters;
}

/**
 * Merge cluster arrays, deduplicating by kind+suffix. When the same kind+suffix appears
 * in multiple sources (global + per-folder), keep the one with the highest fileCount.
 *
 * @param {Array<Object>} clusters
 * @returns {Array<Object>}
 */
function _mergeClusters(clusters) {
  const bySuffix = new Map();
  for (const c of clusters) {
    const key = `${c.kind}|${c.suffix || c.commonBaseClass || c.decorator || ''}|${c.ext}`;
    const existing = bySuffix.get(key);
    if (!existing || c.fileCount > existing.fileCount) {
      bySuffix.set(key, c);
    }
  }
  return [...bySuffix.values()];
}

// ---------------------------------------------------------------------------
// Step 2: consolidation across folders
// ---------------------------------------------------------------------------

/**
 * Groups folder-clusters by suffix. If the same suffix appears in multiple
 * folders with a combined count ≥ MIN_FILES_PER_SUFFIX, emit one
 * 'suffix-cluster'. Folder-clusters with unique suffixes that still
 * individually meet MIN_FILES_PER_SUFFIX are kept as-is.
 *
 * @param {Array<Object>} folderClusters
 * @returns {{ consolidated: Array<Object>, remaining: Array<Object> }}
 */
function _consolidateClusters(folderClusters) {
  // Group by (suffix + ext)
  const bySuffix = new Map();
  for (const c of folderClusters) {
    const key = c.suffix + c.ext;
    if (!bySuffix.has(key)) bySuffix.set(key, []);
    bySuffix.get(key).push(c);
  }

  const consolidated = [];
  const remaining = [];

  for (const [, group] of bySuffix) {
    const totalFiles = group.reduce((sum, c) => sum + c.fileCount, 0);
    const folders = group.map(c => c.folder);
    const ext = group[0].ext;
    const suffix = group[0].suffix;

    if (totalFiles < MIN_FILES_PER_SUFFIX) continue; // below threshold globally

    if (group.length > 1) {
      // Multiple folders → emit consolidated suffix-cluster
      const allSamples = group.flatMap(c => c.samples);
      const uniqueSamples = [...new Set(allSamples)].slice(0, 3);

      // Derive folderPattern: find the shared parent segment among folders
      const sharedParent = _commonFolderSegment(folders);
      const folderPattern = sharedParent ? `**/${sharedParent}/` : '(multiple)';

      consolidated.push({
        kind: 'suffix-cluster',
        suffix,
        ext,
        fileCount: totalFiles,
        folders,
        folderPattern,
        samples: uniqueSamples,
        label: suffix,
      });
    } else {
      // Single folder, meets threshold → keep as folder-cluster
      const c = group[0];
      if (c.fileCount >= MIN_FILES_PER_SUFFIX) {
        remaining.push(c);
      }
    }
  }

  return { consolidated, remaining };
}

// ---------------------------------------------------------------------------
// Step 3: base-class cluster discovery (.cs only)
// ---------------------------------------------------------------------------

/**
 * Find classes that share a common base class by parsing `: BaseClass` patterns.
 * Groups by base class name; emits clusters when ≥ MIN_BASE_CLASS_INHERITORS.
 *
 * Only looks at base classes (non-interface, non-I-prefixed) to avoid
 * conflating interface implementations with inheritance clusters.
 *
 * @param {string} subprojectPath
 * @param {string[]} allFiles - absolute .cs file paths
 * @returns {Array<Object>}
 */
function _discoverBaseClassClustersDotnet(subprojectPath, allFiles) {
  // Map: baseClass → [{className, file, folder}]
  const inheritors = new Map();

  for (const f of allFiles) {
    const content = readFileSafe(f);
    if (!content) continue;

    const rel = relativePath(subprojectPath, f);
    const folder = path.dirname(rel).replace(/\\/g, '/');

    // Match: public class Foo : BaseClass, IFoo
    // We want the first non-interface entry after the colon
    const classRe = /public\s+(?:abstract\s+)?class\s+(\w+)\s*:\s*([^{]+)/g;
    let m;
    while ((m = classRe.exec(content)) !== null) {
      const className = m[1];
      const rhs = m[2];
      // Split on comma, strip generics, find first non-I-prefix entry
      const parts = rhs.split(',').map(s => s.trim().replace(/<[^>]+>/g, '').trim());
      const baseClass = parts.find(p => p && !/^I[A-Z]/.test(p) && /^[A-Z]/.test(p));
      if (!baseClass) continue;

      if (!inheritors.has(baseClass)) inheritors.set(baseClass, []);
      inheritors.get(baseClass).push({ className, file: rel, folder });
    }
  }

  return _materializeBaseClassClusters(inheritors, '.cs');
}

// ---------------------------------------------------------------------------
// Step 3b: base-class cluster discovery (TypeScript)
// ---------------------------------------------------------------------------

/**
 * Find TypeScript classes that share a common base via `extends`.
 * Regex picks up: (export)? (abstract)? class Foo extends BaseClass<...> { ... }
 * Generics on the base are stripped. Threshold: MIN_BASE_CLASS_INHERITORS.
 *
 * @param {string} subprojectPath
 * @param {string[]} allFiles - absolute .ts file paths
 * @returns {Array<Object>}
 */
function _discoverBaseClassClustersTypeScript(subprojectPath, allFiles) {
  const inheritors = new Map();

  for (const f of allFiles) {
    const content = readFileSafe(f);
    if (!content) continue;

    const rel = relativePath(subprojectPath, f);
    const folder = path.dirname(rel).replace(/\\/g, '/');

    const classRe = /(?:export\s+)?(?:abstract\s+)?class\s+(\w+)\s+extends\s+([\w.]+)/g;
    let m;
    while ((m = classRe.exec(content)) !== null) {
      const className = m[1];
      const baseClass = m[2];
      // Strip namespace dot path — keep last segment
      const bareBase = baseClass.split('.').pop();
      if (!bareBase) continue;

      if (!inheritors.has(bareBase)) inheritors.set(bareBase, []);
      inheritors.get(bareBase).push({ className, file: rel, folder });
    }
  }

  return _materializeBaseClassClusters(inheritors, '.ts');
}

// ---------------------------------------------------------------------------
// Step 3c: base-class cluster discovery (Python)
// ---------------------------------------------------------------------------

/** Trivial Python bases that should not produce a cluster. */
const PYTHON_TRIVIAL_BASES = new Set([
  'object', 'Exception', 'BaseException', 'TypedDict',
  'Enum', 'IntEnum', 'StrEnum', 'Flag', 'IntFlag',
  'NamedTuple', 'Protocol', 'ABC',
]);

/**
 * Find Python classes that share a common base.
 * Regex matches top-level: `class Foo(Base):` (multi-base separated by comma — first base used).
 * Trivial stdlib bases are filtered. Threshold: MIN_BASE_CLASS_INHERITORS.
 *
 * @param {string} subprojectPath
 * @param {string[]} allFiles - absolute .py file paths
 * @returns {Array<Object>}
 */
function _discoverBaseClassClustersPython(subprojectPath, allFiles) {
  const inheritors = new Map();

  for (const f of allFiles) {
    const content = readFileSafe(f);
    if (!content) continue;

    const rel = relativePath(subprojectPath, f);
    const folder = path.dirname(rel).replace(/\\/g, '/');

    const classRe = /^class\s+(\w+)\s*\(\s*([\w.,\s]+?)\s*\)\s*:/gm;
    let m;
    while ((m = classRe.exec(content)) !== null) {
      const className = m[1];
      const bases = m[2].split(',').map(s => s.trim()).filter(Boolean);
      const firstBase = bases[0];
      if (!firstBase) continue;

      const bareBase = firstBase.split('.').pop();
      if (!bareBase || PYTHON_TRIVIAL_BASES.has(bareBase)) continue;

      if (!inheritors.has(bareBase)) inheritors.set(bareBase, []);
      inheritors.get(bareBase).push({ className, file: rel, folder });
    }
  }

  return _materializeBaseClassClusters(inheritors, '.py');
}

// ---------------------------------------------------------------------------
// Shared materialiser: inheritors → cluster descriptors
// ---------------------------------------------------------------------------

/**
 * Convert a `baseClass → inheritors[]` map into base-class-cluster descriptors,
 * applying MIN_BASE_CLASS_INHERITORS threshold and folder-pattern derivation.
 *
 * @param {Map<string, Array<{className, file, folder}>>} inheritors
 * @param {string} ext - extension to embed in the cluster
 * @returns {Array<Object>}
 */
function _materializeBaseClassClusters(inheritors, ext) {
  const clusters = [];

  for (const [baseClass, classes] of inheritors) {
    if (classes.length < MIN_BASE_CLASS_INHERITORS) continue;

    const folders = [...new Set(classes.map(c => c.folder))];
    const samples = classes.slice(0, 3).map(c => path.basename(c.file));
    const folderPattern = folders.length === 1
      ? folders[0] + '/'
      : (_commonFolderSegment(folders) ? `**/${_commonFolderSegment(folders)}/` : '(multiple)');

    clusters.push({
      kind: 'base-class-cluster',
      commonBaseClass: baseClass,
      suffix: baseClass,
      ext,
      fileCount: classes.length,
      folders,
      folderPattern,
      samples,
      label: baseClass,
    });
  }

  return clusters;
}

// ---------------------------------------------------------------------------
// Step 5: decorator / annotation cluster discovery (sintaxe agnóstica)
// ---------------------------------------------------------------------------

/**
 * Decorator regex tuned per language.
 *  - TS/JS:    `@Name` or `@Name(...)` immediately before class/function.
 *  - Python:   `@name` or `@module.name(...)` immediately before class/def (any indent).
 *  - Java/Kt:  `@Name` or `@Name(...)` before class/fun (with optional modifiers).
 *  - C#:       `[Name]` or `[Name(...)]` (single-bracket attribute) before class.
 *
 * Mustard knows zero specific names — the names emerge from the user's source.
 *
 * @returns {RegExp|null}
 */
function _decoratorRegexFor(stackId) {
  switch (stackId) {
    case 'typescript':
      // @Foo or @Foo(...) → maybe newline/whitespace → maybe export/abstract → class|function
      return /@(\w+)(?:\([^)]*\))?\s*\n?\s*(?:export\s+)?(?:default\s+)?(?:abstract\s+)?(?:class|function)\b/g;
    case 'python':
      return /^[ \t]*@([\w.]+)(?:\([^)]*\))?\s*\n[ \t]*(?:async\s+)?(?:class|def)\b/gm;
    case 'java':
    case 'kotlin':
      return /@(\w+)(?:\([^)]*\))?\s+(?:public\s+|private\s+|internal\s+|protected\s+|open\s+)?(?:abstract\s+|final\s+|sealed\s+|data\s+)?(?:class|fun|interface)\b/g;
    case 'dotnet':
      // Match every [Name] or [Name(...)] attribute — proximity to `class` is
      // checked in the JS loop (handles stacked attributes and nested brackets
      // like [Route("api/[controller]")] without regex catastrophic backtracking).
      return /\[(\w+)(?:\([^)]*\))?\]/g;
    default:
      return null;
  }
}

/**
 * Find files annotated with the same decorator (across the whole subproject).
 * Threshold: MIN_DECORATOR_USAGE distinct files per decorator.
 *
 * @param {string} subprojectPath
 * @param {string[]} allFiles - absolute file paths for the stack's primary extension
 * @param {string} stackId
 * @returns {Array<Object>}
 */
function _discoverDecoratorClusters(subprojectPath, allFiles, stackId) {
  const re = _decoratorRegexFor(stackId);
  if (!re) return [];

  // For C# the regex matches all [Name] tokens; we post-filter to those near a `class`.
  // For other stacks the regex already ensures proximity to class/def/function.
  const isDotnet = stackId === 'dotnet';

  // decoratorName → Set<relFile>
  const usage = new Map();

  for (const f of allFiles) {
    const content = readFileSafe(f);
    if (!content) continue;

    const rel = relativePath(subprojectPath, f);
    // Must rebuild a fresh regex per file (lastIndex state on shared global regex is unsafe)
    const fileRe = new RegExp(re.source, re.flags);

    if (isDotnet) {
      // Split into lines for proximity check: attribute must be within 5 lines of `class`
      const lines = content.split(/\r?\n/);
      // Build a set of line indices that are within 5 lines before a class declaration
      const classLineRe = /^\s*(?:public\s+|internal\s+)?(?:partial\s+|abstract\s+|sealed\s+)?class\b/;
      const nearClass = new Set();
      for (let i = 0; i < lines.length; i++) {
        if (classLineRe.test(lines[i])) {
          for (let j = Math.max(0, i - 5); j < i; j++) nearClass.add(j);
        }
      }

      let m;
      while ((m = fileRe.exec(content)) !== null) {
        const name = m[1] || '';
        if (!name) continue;
        // Find which line index this match falls on
        const lineIdx = content.slice(0, m.index).split(/\r?\n/).length - 1;
        if (!nearClass.has(lineIdx)) continue;
        if (!usage.has(name)) usage.set(name, new Set());
        usage.get(name).add(rel);
      }
    } else {
      let m;
      while ((m = fileRe.exec(content)) !== null) {
        const name = (m[1] || '').split('.').pop(); // strip Python module path
        if (!name) continue;
        if (!usage.has(name)) usage.set(name, new Set());
        usage.get(name).add(rel);
      }
    }
  }

  const ext = _primaryExtForStack(stackId) || '';
  const clusters = [];

  for (const [decorator, fileSet] of usage) {
    if (fileSet.size < MIN_DECORATOR_USAGE) continue;

    const files = [...fileSet];
    const folders = [...new Set(files.map(f => path.dirname(f).replace(/\\/g, '/')))];
    const samples = files.slice(0, 3).map(f => path.basename(f));
    const folderPattern = folders.length === 1
      ? folders[0] + '/'
      : (_commonFolderSegment(folders) ? `**/${_commonFolderSegment(folders)}/` : '(multiple)');

    clusters.push({
      kind: 'decorator-cluster',
      decorator,
      suffix: decorator, // reused as label/slug for downstream skill-generator
      ext,
      fileCount: fileSet.size,
      folders,
      folderPattern,
      samples,
      label: decorator,
    });
  }

  return clusters;
}

// ---------------------------------------------------------------------------
// Step 6: function-prefix cluster discovery (camelCase/snake_case prefixes)
// ---------------------------------------------------------------------------

/**
 * Extract the leading "prefix" of a camelCase or snake_case identifier.
 *   useFooBar         → "use"
 *   makeWidgetFactory → "make"
 *   _internalHelper   → "_internal"
 *   user_repository   → "user"
 *   foobar            → null  (no boundary → indistinguishable from a single name)
 *
 * Boundary = first uppercase letter (camelCase) or first underscore after the start (snake_case).
 *
 * @param {string} name
 * @returns {string|null}
 */
function _extractFunctionPrefix(name) {
  if (!name || typeof name !== 'string') return null;

  // snake_case boundary (skip leading underscores, then look for next underscore)
  const stripped = name.replace(/^_+/, '');
  const leadingUnderscores = name.length - stripped.length;
  const snakeIdx = stripped.indexOf('_');
  if (snakeIdx > 0) {
    return name.slice(0, leadingUnderscores + snakeIdx);
  }

  // camelCase boundary
  const camelMatch = stripped.match(/^([a-z]+)(?=[A-Z])/);
  if (camelMatch) {
    return name.slice(0, leadingUnderscores + camelMatch[1].length);
  }

  return null;
}

/**
 * Top-level function regex per language.
 *  - TypeScript: function declarations + arrow consts at the start of a line.
 *  - Python:     `def name(...)` at column 0 (top-level only — indented defs ignored).
 *
 * @returns {RegExp[]|null} - one or more regexes (multiline-aware)
 */
function _functionRegexesFor(stackId) {
  switch (stackId) {
    case 'typescript':
      return [
        /^(?:export\s+)?(?:async\s+)?function\s+([a-zA-Z_]\w+)\s*\(/gm,
        /^(?:export\s+)?const\s+([a-zA-Z_]\w+)\s*(?::\s*[^=]+)?=\s*(?:async\s*)?\(/gm,
      ];
    case 'python':
      return [
        /^def\s+([a-zA-Z_]\w+)\s*\(/gm,
        /^async\s+def\s+([a-zA-Z_]\w+)\s*\(/gm,
      ];
    default:
      return null;
  }
}

/**
 * Find shared function-name prefixes across the subproject.
 * Threshold: MIN_FUNCTION_PREFIX_USAGE distinct files per prefix; prefix length ≥ MIN_FUNCTION_PREFIX_LEN.
 *
 * @param {string} subprojectPath
 * @param {string[]} allFiles
 * @param {string} stackId
 * @returns {Array<Object>}
 */
function _discoverFunctionPrefixClusters(subprojectPath, allFiles, stackId) {
  const regexes = _functionRegexesFor(stackId);
  if (!regexes) return [];

  // prefix → Set<relFile>
  const usage = new Map();

  for (const f of allFiles) {
    const content = readFileSafe(f);
    if (!content) continue;
    const rel = relativePath(subprojectPath, f);

    for (const re of regexes) {
      const fileRe = new RegExp(re.source, re.flags);
      let m;
      while ((m = fileRe.exec(content)) !== null) {
        const fnName = m[1];
        const prefix = _extractFunctionPrefix(fnName);
        if (!prefix || prefix.length < MIN_FUNCTION_PREFIX_LEN) continue;

        if (!usage.has(prefix)) usage.set(prefix, new Set());
        usage.get(prefix).add(rel);
      }
    }
  }

  const ext = _primaryExtForStack(stackId) || '';
  const clusters = [];

  for (const [prefix, fileSet] of usage) {
    if (fileSet.size < MIN_FUNCTION_PREFIX_USAGE) continue;

    const files = [...fileSet];
    const folders = [...new Set(files.map(f => path.dirname(f).replace(/\\/g, '/')))];
    const samples = files.slice(0, 3).map(f => path.basename(f));
    const folderPattern = folders.length === 1
      ? folders[0] + '/'
      : (_commonFolderSegment(folders) ? `**/${_commonFolderSegment(folders)}/` : '(multiple)');

    clusters.push({
      kind: 'function-prefix-cluster',
      prefix,
      suffix: prefix, // reused as label/slug
      ext,
      fileCount: fileSet.size,
      folders,
      folderPattern,
      samples,
      label: prefix,
    });
  }

  return clusters;
}

// ---------------------------------------------------------------------------
// Step 7: filename cluster discovery
// ---------------------------------------------------------------------------

/**
 * Detect the same basename repeating across multiple folders. Catches the
 * Next.js feature-folder pattern (e.g. `detail.tsx` in `companies/_components/`,
 * `users/_components/`, `tenants/_components/`, ...) that PascalCase suffix
 * matchers miss because the basename is a single lowercase word.
 *
 * Threshold: MIN_FILENAME_FOLDERS distinct folders sharing the same basename.
 * Structural basenames (`page`, `layout`, `index`, etc.) are skipped — those
 * are framework conventions, not codebase-specific patterns.
 *
 * @param {string} subprojectPath
 * @param {string[]} allFiles - absolute file paths for the stack's primary extension
 * @param {string} ext - file extension including dot
 * @returns {Array<Object>}
 */
function _discoverFilenameClusters(subprojectPath, allFiles, ext, extraFiles = []) {
  const byBasename = new Map(); // basename (no ext) → [{folder, file, ext}]

  // Combine primary-ext files with any extra-ext files (e.g. .tsx for typescript).
  // We strip whatever extension the file actually has, not a fixed one — basenames
  // for `detail.tsx` and `detail.ts` both normalize to `detail` and group together.
  const combined = [...allFiles, ...extraFiles];

  for (const f of combined) {
    const rel = relativePath(subprojectPath, f);
    const folder = path.dirname(rel).replace(/\\/g, '/');
    const fileExt = path.extname(rel);
    const baseNoExt = path.basename(rel, fileExt);
    if (STRUCTURAL_BASENAMES.has(baseNoExt.toLowerCase())) continue;
    if (baseNoExt.length < 3) continue;

    if (!byBasename.has(baseNoExt)) byBasename.set(baseNoExt, []);
    byBasename.get(baseNoExt).push({ folder, file: path.basename(rel), ext: fileExt });
  }

  const clusters = [];

  for (const [basename, occurrences] of byBasename) {
    const folders = [...new Set(occurrences.map(o => o.folder))];
    if (folders.length < MIN_FILENAME_FOLDERS) continue;

    // Pick the most common extension among occurrences (e.g. .tsx wins for React components).
    const extCounts = new Map();
    for (const o of occurrences) extCounts.set(o.ext, (extCounts.get(o.ext) || 0) + 1);
    const dominantExt = [...extCounts.entries()].sort((a, b) => b[1] - a[1])[0][0];

    const sharedParent = _commonFolderSegment(folders);
    const folderPattern = sharedParent
      ? `**/${sharedParent}/${basename}${dominantExt}`
      : `**/${basename}${dominantExt}`;
    const samples = occurrences.slice(0, 3).map(o => `${o.folder}/${o.file}`);

    clusters.push({
      kind: 'filename-cluster',
      suffix: basename,
      ext: dominantExt,
      fileCount: folders.length, // count distinct folders, not files
      folders,
      folderPattern,
      samples,
      label: basename,
    });
  }

  return clusters;
}

// ---------------------------------------------------------------------------
// Suffix extraction helpers
// ---------------------------------------------------------------------------

/**
 * Given an array of PascalCase basenames (without extension), find all
 * "trailing word groups" shared by ≥2 entries.
 *
 * Algorithm:
 *   1. Split each name into PascalCase words.
 *   2. For each possible trailing word count (1 to max-1), build the suffix.
 *   3. Count how many names end with that suffix.
 *   4. Return suffixes meeting count ≥ 2 (consolidated handles ≥5 threshold).
 *
 * @param {string[]} basenames
 * @returns {Map<string, string[]>} suffix → matching basenames
 */
function _groupBySuffix(basenames) {
  const result = new Map();

  for (const name of basenames) {
    const words = _splitPascalCase(name);
    if (words.length < 2) continue;

    // Try each possible trailing word group (from 1 word up to len-1)
    for (let wordCount = 1; wordCount < words.length; wordCount++) {
      const suffix = words.slice(words.length - wordCount).join('');
      if (suffix.length < MIN_SUFFIX_LENGTH) continue;

      if (!result.has(suffix)) result.set(suffix, []);
      result.get(suffix).push(name);
    }
  }

  // Only keep suffixes where ≥2 names match (pruning noise)
  for (const [suffix, names] of result) {
    if (names.length < 2) result.delete(suffix);
  }

  // Prefer the LONGEST suffix that still has the most matches
  // (avoids redundant shorter suffixes that are subsets of longer ones)
  return _pruneSuffixSubsets(result);
}

/**
 * Remove shorter suffixes that are proper suffixes of a longer, equally-matched suffix.
 * e.g., if "QueryResolver" has 24 matches and "Resolver" has 24 matches, keep "QueryResolver".
 *
 * @param {Map<string, string[]>} suffixMap
 * @returns {Map<string, string[]>}
 */
function _pruneSuffixSubsets(suffixMap) {
  const pruned = new Map(suffixMap);

  for (const [shorter] of suffixMap) {
    for (const [longer] of suffixMap) {
      if (longer === shorter) continue;
      if (!longer.endsWith(shorter)) continue;
      // longer is a superset-suffix of shorter
      const shorterSet = new Set(suffixMap.get(shorter));
      const longerSet = new Set(suffixMap.get(longer));
      // If every file matching shorter also matches longer, shorter is redundant
      const allInLonger = [...longerSet].every(n => shorterSet.has(n));
      if (allInLonger && longerSet.size === shorterSet.size) {
        pruned.delete(shorter);
      }
    }
  }

  return pruned;
}

/**
 * Split a PascalCase identifier into its component words.
 * e.g., "QueryResolver" → ["Query", "Resolver"]
 * e.g., "ApikeyQueryResolver" → ["Apikey", "Query", "Resolver"]
 *
 * @param {string} s
 * @returns {string[]}
 */
function _splitPascalCase(s) {
  // Insert boundary before each uppercase letter that follows a lowercase letter,
  // or before an uppercase letter followed by lowercase (handles "XML" style)
  return s.split(/(?=[A-Z][a-z])|(?<=[a-z])(?=[A-Z])/).filter(Boolean);
}

// ---------------------------------------------------------------------------
// Folder pattern helpers
// ---------------------------------------------------------------------------

/**
 * Find a shared folder name segment among a list of folder paths.
 * e.g., ["Contracts/Resolvers", "Banks/Resolvers", "Users/Resolvers"] → "Resolvers"
 *
 * @param {string[]} folders
 * @returns {string|null}
 */
function _commonFolderSegment(folders) {
  if (!folders.length) return null;

  // Extract all path segments from each folder
  const segmentSets = folders.map(f => new Set(f.split('/').filter(Boolean)));

  // Find segments present in ALL folders
  const first = folders[0].split('/').filter(Boolean);
  const common = first.filter(seg =>
    folders.every(f => f.split('/').includes(seg))
  );

  if (!common.length) return null;

  // Return the last common segment (deepest common folder name)
  return common[common.length - 1];
}

/**
 * Return the primary file extension for a given stack.
 * Returns null for stacks where cluster discovery is not yet implemented.
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

// ---------------------------------------------------------------------------
// Folder frequency index (for agnostic stopword derivation downstream)
// ---------------------------------------------------------------------------

/**
 * Compute how often each folder path segment appears across all folders of a
 * subproject. Downstream consumers (e.g. skill-generator) use this frequency
 * map to derive stopwords agnostically — segments present in the vast majority
 * of folders are structural, not contextual.
 *
 * @param {string} subprojectPath - absolute path to subproject root
 * @param {string} stackId
 * @returns {{ totalFolders: number, segments: Object<string, number> }}
 */
function computeFolderFrequency(subprojectPath, stackId) {
  try {
    const ext = _primaryExtForStack(stackId);
    if (!ext) return { totalFolders: 0, segments: {} };

    const allFiles = collectFiles(subprojectPath, ext);
    const folderSet = new Set();
    for (const f of allFiles) {
      const rel = relativePath(subprojectPath, f);
      const dir = path.dirname(rel).replace(/\\/g, '/');
      folderSet.add(dir);
    }

    const segments = {};
    for (const folder of folderSet) {
      const parts = folder.split('/').filter(Boolean);
      const seen = new Set();
      for (const p of parts) {
        // Count each segment at most once per folder to avoid bias from
        // nested repetition (e.g. "/Backend/Modules/Backend/..." would double-count).
        if (seen.has(p)) continue;
        seen.add(p);
        segments[p] = (segments[p] || 0) + 1;
      }
    }

    return { totalFolders: folderSet.size, segments };
  } catch {
    return { totalFolders: 0, segments: {} };
  }
}

// ---------------------------------------------------------------------------
// Cluster enrichment — universal heuristics (no stack-specific keywords)
// ---------------------------------------------------------------------------

/**
 * Enrich a cluster with up to 5 universal metadata fields derived from reading
 * the cluster's samples. Heuristics are stack-agnostic: based on basename
 * matching, indentation, and top-of-file lines. No language keywords are
 * hardcoded. Fields that cannot be inferred are set to null (not omitted).
 *
 * Mutates the cluster object in place; also returns it.
 *
 * @param {Object} cluster - cluster descriptor with `samples` and `folders`
 * @param {string} subprojectPath - absolute path to subproject root
 * @returns {Object} - same cluster, mutated
 */
function _enrichCluster(cluster, subprojectPath) {
  cluster.namingPattern       = null;
  cluster.declarationKeywords = null;
  cluster.declarationSuffix   = null;
  cluster.topOfFileLines      = null;
  cluster.memberSuffixes      = null;

  try {
    const samplePaths = _resolveSamplePaths(cluster, subprojectPath);
    if (samplePaths.length === 0) return cluster;

    const sampleContents = samplePaths
      .map(p => ({ p, c: readFileSafe(p) }))
      .filter(x => x.c);
    if (sampleContents.length === 0) return cluster;

    cluster.namingPattern       = _extractNamingPattern(cluster, sampleContents.map(x => x.p));
    cluster.declarationKeywords = _extractDeclarationKeywords(sampleContents);
    cluster.declarationSuffix   = _extractDeclarationSuffix(sampleContents);
    cluster.topOfFileLines      = _extractTopOfFileLines(sampleContents.map(x => x.c));
    cluster.memberSuffixes      = _extractMemberSuffixes(sampleContents.map(x => x.c));
  } catch { /* fail-open: keep nulls */ }

  return cluster;
}

/**
 * Resolve cluster.samples (basenames) to absolute file paths by trying each
 * cluster.folders entry. Stops at MAX_ENRICHMENT_SAMPLES.
 *
 * @param {Object} cluster
 * @param {string} subprojectPath
 * @returns {string[]}
 */
function _resolveSamplePaths(cluster, subprojectPath) {
  const out = [];
  const samples = Array.isArray(cluster.samples) ? cluster.samples : [];
  const folders = Array.isArray(cluster.folders) ? cluster.folders
                : (cluster.folder ? [cluster.folder] : []);

  for (const sample of samples) {
    if (out.length >= MAX_ENRICHMENT_SAMPLES) break;
    // sample may already include a folder path (filename-cluster does this)
    const directCandidate = path.join(subprojectPath, sample);
    try {
      if (fs.existsSync(directCandidate) && fs.statSync(directCandidate).isFile()) {
        out.push(directCandidate);
        continue;
      }
    } catch { /* try folders */ }

    for (const folder of folders) {
      const candidate = path.join(subprojectPath, folder, sample);
      try {
        if (fs.existsSync(candidate) && fs.statSync(candidate).isFile()) {
          out.push(candidate);
          break;
        }
      } catch { /* skip */ }
    }
  }
  return out;
}

/**
 * Detect whether the cluster suffix appears at the start or end of the
 * basename across the samples. Returns 'suffix-after', 'suffix-before',
 * or null when ambiguous / no signal.
 *
 * @param {Object} cluster
 * @param {string[]} samplePaths
 * @returns {string|null}
 */
function _extractNamingPattern(cluster, samplePaths) {
  const target = cluster.suffix || cluster.label;
  if (!target || target.length < 2) return null;

  let after = 0;
  let before = 0;

  for (const p of samplePaths) {
    const ext = path.extname(p);
    const base = path.basename(p, ext);
    if (base === target) continue; // single-word match — no positional signal
    if (base.endsWith(target)) after++;
    else if (base.startsWith(target)) before++;
  }

  if (after === 0 && before === 0) return null;
  if (after === before) return null;
  return after > before ? 'suffix-after' : 'suffix-before';
}

/**
 * Find tokens (whitespace-separated identifiers) appearing BEFORE the basename
 * on the line where it first appears as a whole-word token. Returns top-3
 * combinations across samples. Skips comment lines.
 *
 * @param {Array<{p: string, c: string}>} sampleContents
 * @returns {string[]|null}
 */
function _extractDeclarationKeywords(sampleContents) {
  const combos = [];
  for (const { p, c } of sampleContents) {
    const ext = path.extname(p);
    const base = path.basename(p, ext);
    const declLine = _findDeclarationLine(c, base);
    if (!declLine) continue;

    const re = _wholeWordRegex(base);
    const idx = declLine.search(re);
    if (idx <= 0) continue;

    const before = declLine.slice(0, idx).trim();
    if (!before) continue;

    const tokens = before.split(/\s+/).filter(t => /^[a-zA-Z_][\w]*$/.test(t));
    if (tokens.length === 0) continue;
    combos.push(tokens.join(' '));
  }
  if (combos.length === 0) return null;
  return _topN(combos, 3);
}

/**
 * Find the segment of the declaration line AFTER the basename (typed bases,
 * implements lists, struct opener, etc.). Returns top-3 across samples.
 *
 * @param {Array<{p: string, c: string}>} sampleContents
 * @returns {string[]|null}
 */
function _extractDeclarationSuffix(sampleContents) {
  const tails = [];
  for (const { p, c } of sampleContents) {
    const ext = path.extname(p);
    const base = path.basename(p, ext);
    const declLine = _findDeclarationLine(c, base);
    if (!declLine) continue;

    const re = _wholeWordRegex(base);
    const m = declLine.match(re);
    if (!m) continue;
    const afterStart = (declLine.indexOf(m[0]) + m[0].length);
    let after = declLine.slice(afterStart).trim();
    after = after.replace(/[{(]\s*$/, '').trim();
    if (!after) continue;
    tails.push(after.slice(0, 80));
  }
  if (tails.length === 0) return null;
  return _topN(tails, 3);
}

/**
 * Top-of-file lines shared across ALL samples (intersection), excluding
 * comments and blank lines. Looks at the first 20 lines of each sample.
 * Returns up to 5 lines.
 *
 * @param {string[]} contents
 * @returns {string[]|null}
 */
function _extractTopOfFileLines(contents) {
  if (contents.length === 0) return null;
  const sets = contents.map(c => {
    const lines = c.split(/\r?\n/).slice(0, 20);
    return new Set(
      lines
        .map(l => l.trim())
        .filter(l => l.length > 0 && !_isCommentLine(l))
    );
  });

  const base = sets[0];
  const shared = [];
  for (const line of base) {
    if (sets.every(s => s.has(line))) shared.push(line);
  }
  if (shared.length === 0) return null;
  return shared.slice(0, 5);
}

/**
 * Member-style identifiers: tokens preceded by whitespace and followed by `(`.
 * Extracts the trailing word (PascalCase split) of each — captures async-style
 * suffixes (`Async`), private prefixes, etc. Returns top-3 suffixes.
 *
 * @param {string[]} contents
 * @returns {string[]|null}
 */
function _extractMemberSuffixes(contents) {
  const suffixes = [];
  for (const c of contents) {
    const lines = c.split(/\r?\n/);
    for (const line of lines) {
      if (!/^\s+/.test(line)) continue;
      const m = line.match(/\b([a-zA-Z_]\w*)\s*\(/);
      if (!m) continue;
      const words = _splitPascalCase(m[1]);
      if (words.length < 2) continue;
      suffixes.push(words[words.length - 1]);
    }
  }
  if (suffixes.length === 0) return null;
  return _topN(suffixes, 3);
}

/**
 * Find the first non-comment, non-empty line containing `token` as a whole word.
 *
 * @param {string} content
 * @param {string} token
 * @returns {string|null}
 */
function _findDeclarationLine(content, token) {
  const re = _wholeWordRegex(token);
  const lines = content.split(/\r?\n/);
  for (const line of lines) {
    const trimmed = line.trim();
    if (!trimmed || _isCommentLine(trimmed)) continue;
    if (re.test(line)) return line;
  }
  return null;
}

/**
 * Build a whole-word regex for a token, escaping regex metacharacters.
 *
 * @param {string} token
 * @returns {RegExp}
 */
function _wholeWordRegex(token) {
  const escaped = token.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  return new RegExp(`\\b${escaped}\\b`);
}

/**
 * Detect if a line starts with a universal comment prefix.
 *
 * @param {string} line
 * @returns {boolean}
 */
function _isCommentLine(line) {
  const t = line.trim();
  if (!t) return false;
  return COMMENT_PREFIXES.some(p => t.startsWith(p));
}

/**
 * Top-N most frequent items, ordered by count desc.
 *
 * @param {string[]} items
 * @param {number} n
 * @returns {string[]}
 */
function _topN(items, n) {
  const counts = new Map();
  for (const item of items) counts.set(item, (counts.get(item) || 0) + 1);
  return [...counts.entries()]
    .sort((a, b) => b[1] - a[1])
    .slice(0, n)
    .map(([item]) => item);
}

// ---------------------------------------------------------------------------
// Exports
// ---------------------------------------------------------------------------

module.exports = { discoverClusters, computeFolderFrequency };
