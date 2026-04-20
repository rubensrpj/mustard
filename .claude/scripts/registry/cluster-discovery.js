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
 *   - 'base-class-cluster' : ≥3 C# classes extend the same base class (cross-folder)
 *
 * Limits: at most 10 clusters per call (ranked by fileCount desc) to avoid skill-spam.
 */

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
const MIN_FILES_PER_SUFFIX = 5;

/** Minimum suffix length in characters to avoid false positives (e.g. "s", "ed"). */
const MIN_SUFFIX_LENGTH = 6;

/** Minimum number of classes extending the same base for a base-class-cluster. */
const MIN_BASE_CLASS_INHERITORS = 3;

/** Maximum clusters returned per discovery call (before skill-generator applies its own top-10 cap). */
const MAX_CLUSTERS = 15;

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/**
 * Discover structural clusters in a subproject.
 *
 * @param {string} subprojectPath - absolute path to the subproject root
 * @param {string} stackId - detected stack id (e.g. 'dotnet', 'typescript')
 * @returns {Array<Object>} - array of cluster descriptors
 */
function discoverClusters(subprojectPath, stackId) {
  try {
    const ext = _primaryExtForStack(stackId);
    if (!ext) return [];

    const allFiles = collectFiles(subprojectPath, ext);

    // Step 1: global suffix scan across ALL files (catches 1-per-folder patterns like
    // "one QueryResolver per module folder" that per-folder grouping would miss)
    const globalClusters = _discoverGlobalSuffixClusters(subprojectPath, allFiles, ext);

    // Step 2: per-folder suffix clusters (catches dense folders with ≥5 same-suffix files)
    const folderClusters = _discoverFolderClusters(subprojectPath, allFiles, ext);

    // Step 3: consolidate per-folder clusters into cross-folder suffix clusters
    const { consolidated, remaining } = _consolidateClusters(folderClusters);

    // Step 4: for C#, discover base-class clusters (cross-folder)
    const baseClassClusters = stackId === 'dotnet'
      ? _discoverBaseClassClusters(subprojectPath, allFiles)
      : [];
    // TODO: phase 2 — add base-class discovery for TypeScript (extends keyword) and Python

    // Merge all clusters; deduplicate by suffix (global wins over per-folder if same suffix)
    const all = _mergeClusters([...globalClusters, ...consolidated, ...remaining, ...baseClassClusters]);
    all.sort((a, b) => b.fileCount - a.fileCount);

    return all.slice(0, MAX_CLUSTERS);
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
 * Merge cluster arrays, deduplicating by suffix. When the same suffix appears
 * in multiple sources (global + per-folder), keep the one with the highest fileCount.
 *
 * @param {Array<Object>} clusters
 * @returns {Array<Object>}
 */
function _mergeClusters(clusters) {
  const bySuffix = new Map();
  for (const c of clusters) {
    const key = (c.suffix || c.commonBaseClass || '') + c.ext;
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
function _discoverBaseClassClusters(subprojectPath, allFiles) {
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
      suffix: baseClass, // used as label/slug
      ext: '.cs',
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
// Exports
// ---------------------------------------------------------------------------

module.exports = { discoverClusters, computeFolderFrequency };
