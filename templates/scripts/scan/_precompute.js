'use strict';

/**
 * scan/_precompute.js
 *
 * Pure helper functions for pre-dispatch deterministic work.
 * Called by orchestrate.js before dispatching Task agents.
 *
 * Design:
 *   - No orchestrate.js state references (no `result`, no global ROOT).
 *   - All functions receive absolute paths.
 *   - Every function is idempotent.
 *   - Every function wraps I/O in try/catch; returns empty result on error.
 *   - CommonJS, Node built-ins only.
 */

const fs = require('fs');
const path = require('path');

const DEFAULT_IGNORE = new Set([
  'node_modules', '.git', '.next', 'bin', 'obj', 'dist', 'build',
  'migrations', '_backup', '.claude',
]);

/**
 * Whether a file contains the `<!-- mustard:generated` marker anywhere.
 * Reads the full file so the marker is detected even when it sits after
 * a long YAML frontmatter (SKILL.md case — frontmatter is mandated to come
 * before the marker, and description fields routinely push the marker past
 * any small head-only buffer). Returns false on any I/O error.
 */
function hasGeneratedMarker(filePath) {
  try {
    const content = fs.readFileSync(filePath, 'utf-8');
    return /<!--\s*mustard:generated/.test(content);
  } catch {
    return false;
  }
}

/**
 * backupGeneratedMds(absCommandsDir)
 *
 * Walks *.md files (depth 1) in absCommandsDir.
 * Files whose first 200 bytes contain '<!-- mustard:generated' are moved
 * to absCommandsDir/_backup/. Creates _backup/ if needed.
 * Idempotent — skips files already in _backup/ (they are in a subdir).
 *
 * @param {string} absCommandsDir
 * @returns {{ moved: string[], created_backup_dir: boolean }}
 */
function backupGeneratedMds(absCommandsDir) {
  const result = { moved: [], created_backup_dir: false };
  try {
    if (!fs.existsSync(absCommandsDir)) return result;
    const entries = fs.readdirSync(absCommandsDir, { withFileTypes: true });
    const mds = entries.filter(e => e.isFile() && e.name.endsWith('.md'));
    if (mds.length === 0) return result;

    const backupDir = path.join(absCommandsDir, '_backup');
    let backupCreated = false;

    for (const entry of mds) {
      const src = path.join(absCommandsDir, entry.name);
      if (!hasGeneratedMarker(src)) continue;

      if (!backupCreated) {
        if (!fs.existsSync(backupDir)) {
          fs.mkdirSync(backupDir, { recursive: true });
          result.created_backup_dir = true;
        }
        backupCreated = true;
      }

      const dst = path.join(backupDir, entry.name);
      fs.renameSync(src, dst);
      result.moved.push(entry.name);
    }
  } catch {
    // fail-open
  }
  return result;
}

/**
 * purgeGeneratedSkills(absSkillsDir)
 *
 * Walks subdirectories of absSkillsDir.
 * Subdirs whose SKILL.md first 200 bytes include '<!-- mustard:generated'
 * are removed recursively. Idempotent. Missing dir returns { removed: [] }.
 *
 * @param {string} absSkillsDir
 * @returns {{ removed: string[] }}
 */
function purgeGeneratedSkills(absSkillsDir) {
  const result = { removed: [] };
  try {
    if (!fs.existsSync(absSkillsDir)) return result;
    const entries = fs.readdirSync(absSkillsDir, { withFileTypes: true });
    for (const entry of entries) {
      if (!entry.isDirectory()) continue;
      const skillMd = path.join(absSkillsDir, entry.name, 'SKILL.md');
      if (!fs.existsSync(skillMd)) continue;
      if (!hasGeneratedMarker(skillMd)) continue;
      const subdir = path.join(absSkillsDir, entry.name);
      fs.rmSync(subdir, { recursive: true, force: true });
      result.removed.push(entry.name);
    }
  } catch {
    // fail-open
  }
  return result;
}

/**
 * ensureNotesMd(absCommandsDir, name, role)
 *
 * Creates notes.md in absCommandsDir if it does not exist.
 * Never overwrites an existing file (user-authored).
 *
 * @param {string} absCommandsDir
 * @param {string} name  - subproject name
 * @param {string} role  - subproject role
 * @returns {boolean} true if created, false if already existed
 */
function ensureNotesMd(absCommandsDir, name, role) {
  try {
    const notesPath = path.join(absCommandsDir, 'notes.md');
    if (fs.existsSync(notesPath)) return false;
    fs.mkdirSync(absCommandsDir, { recursive: true });
    const content = [
      `# Notes: ${name} (${role})`,
      '',
      `> Project-specific notes for ${name}. Edit freely — this file is never overwritten by /scan.`,
      '',
      '## Mandatory Patterns',
      '',
      '## Known Pitfalls',
      '',
      '## Observations',
      '',
    ].join('\n');
    fs.writeFileSync(notesPath, content, 'utf-8');
    return true;
  } catch {
    return false;
  }
}

/**
 * buildToolingBlock(subprojectPath, stack)
 *
 * Reads package.json / *.csproj / pyproject.toml and extracts build/test/lint/typecheck commands.
 * Returns a markdown block string, or '' if nothing detected.
 *
 * @param {string} subprojectPath  - absolute path to the subproject root
 * @param {string} stack           - stackSummary string (e.g. 'TypeScript', '.NET 9', 'Python')
 * @returns {string}
 */
function buildToolingBlock(subprojectPath, stack) {
  try {
    const lines = [];
    const stackLower = (stack || '').toLowerCase();
    const isNet = stackLower.includes('.net') || stackLower.includes('csharp') || stackLower.includes('c#');
    const isPython = stackLower.includes('python') || stackLower.includes('fastapi') || stackLower.includes('django');

    if (!isNet && !isPython) {
      // TypeScript/JavaScript — read package.json scripts
      const pkgPath = path.join(subprojectPath, 'package.json');
      if (fs.existsSync(pkgPath)) {
        let pkg;
        try {
          pkg = JSON.parse(fs.readFileSync(pkgPath, 'utf-8'));
        } catch {
          return '';
        }
        const scripts = (pkg && pkg.scripts) || {};
        const keys = ['build', 'test', 'lint', 'typecheck', 'type-check', 'check'];
        for (const key of keys) {
          if (scripts[key]) {
            const label = key === 'type-check' ? 'typecheck' : key;
            lines.push(`- ${label}: ${scripts[key]} (source: package.json scripts.${key})`);
          }
        }
      }
    } else if (isNet) {
      // .NET — try to find *.csproj
      let csprojFiles = [];
      try {
        csprojFiles = fs.readdirSync(subprojectPath).filter(f => f.endsWith('.csproj'));
      } catch {
        // ignore
      }
      if (csprojFiles.length > 0) {
        lines.push(`- build: dotnet build (source: ${csprojFiles[0]})`);
        lines.push(`- test: dotnet test (source: ${csprojFiles[0]})`);
      }
    } else if (isPython) {
      // Python — read pyproject.toml scripts sections
      const pyprojectPath = path.join(subprojectPath, 'pyproject.toml');
      if (fs.existsSync(pyprojectPath)) {
        let content;
        try {
          content = fs.readFileSync(pyprojectPath, 'utf-8');
        } catch {
          return '';
        }
        // Simple TOML parsing for [tool.poetry.scripts] or [project.scripts]
        const scriptSectionRe = /\[(?:tool\.poetry\.scripts|project\.scripts)\]([\s\S]*?)(?=\n\[|$)/;
        const m = content.match(scriptSectionRe);
        if (m) {
          const sectionText = m[1];
          const entryRe = /^(\w+)\s*=\s*"([^"]+)"/gm;
          let em;
          while ((em = entryRe.exec(sectionText)) !== null) {
            lines.push(`- ${em[1]}: ${em[2]} (source: pyproject.toml scripts.${em[1]})`);
          }
        }
        // Fallback: detect common Python tools
        if (lines.length === 0) {
          if (content.includes('pytest')) lines.push('- test: pytest (source: pyproject.toml)');
          if (content.includes('ruff')) lines.push('- lint: ruff check . (source: pyproject.toml)');
        }
      }
    }

    if (lines.length === 0) return '';
    return ['## Tooling detected', ...lines, ''].join('\n');
  } catch {
    return '';
  }
}

/**
 * buildStructureBlock(subprojectPath)
 *
 * Globs top-level dirs (depth 1), filters DEFAULT_IGNORE.
 * Counts files (non-recursive) in each dir.
 * Returns '' if ≤1 dir survives filter.
 *
 * @param {string} subprojectPath  - absolute path to the subproject root
 * @returns {string}
 */
function buildStructureBlock(subprojectPath) {
  try {
    if (!fs.existsSync(subprojectPath)) return '';

    let entries;
    try {
      entries = fs.readdirSync(subprojectPath, { withFileTypes: true });
    } catch {
      return '';
    }

    const dirs = entries
      .filter(e => e.isDirectory() && !DEFAULT_IGNORE.has(e.name))
      .slice(0, 12);

    if (dirs.length <= 1) return '';

    const lines = ['## Project structure'];
    for (const dir of dirs) {
      const dirPath = path.join(subprojectPath, dir.name);
      let fileCount = 0;
      try {
        const dirEntries = fs.readdirSync(dirPath, { withFileTypes: true });
        fileCount = dirEntries.filter(e => e.isFile()).length;
      } catch {
        fileCount = 0;
      }
      lines.push(`- ${dir.name}/ — ${fileCount} files`);
    }
    lines.push('');
    return lines.join('\n');
  } catch {
    return '';
  }
}

module.exports = {
  backupGeneratedMds,
  purgeGeneratedSkills,
  ensureNotesMd,
  buildToolingBlock,
  buildStructureBlock,
};
