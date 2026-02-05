import { readFile, readdir, stat } from 'fs/promises';
import { join, relative, extname, basename } from 'path';
import type { Stack, CodeSamples, CodeSample } from '../types.js';

interface SamplePattern {
  type: keyof CodeSamples;
  /** Glob-like path patterns to match */
  pathPatterns: RegExp[];
  /** File extensions to include */
  extensions: string[];
  /** Max file size in bytes (skip huge files) */
  maxSize?: number;
  /** Max lines to read */
  maxLines?: number;
}

const SAMPLE_PATTERNS: Record<string, SamplePattern[]> = {
  dotnet: [
    {
      type: 'service',
      pathPatterns: [/Services?[/\\]/i, /Service\.cs$/i],
      extensions: ['.cs'],
      maxLines: 120,
    },
    {
      type: 'endpoint',
      pathPatterns: [/EndPoints?[/\\]/i, /Controller\.cs$/i, /EndPoint\.cs$/i, /EndPoints\.cs$/i],
      extensions: ['.cs'],
      maxLines: 120,
    },
  ],
  node: [
    {
      type: 'service',
      pathPatterns: [/\.service\.(ts|js)$/i, /services?[/\\]/i],
      extensions: ['.ts', '.js'],
      maxLines: 100,
    },
    {
      type: 'endpoint',
      pathPatterns: [/\.controller\.(ts|js)$/i, /\.route\.(ts|js)$/i, /controllers?[/\\]/i, /routes?[/\\]/i],
      extensions: ['.ts', '.js'],
      maxLines: 100,
    },
  ],
  react: [
    {
      type: 'component',
      pathPatterns: [/components?[/\\].*\.tsx$/i, /_components[/\\].*\.tsx$/i],
      extensions: ['.tsx'],
      maxLines: 100,
    },
    {
      type: 'hook',
      pathPatterns: [/hooks?[/\\]use.*\.(ts|tsx)$/i, /use[A-Z].*\.(ts|tsx)$/i],
      extensions: ['.ts', '.tsx'],
      maxLines: 100,
    },
  ],
  nextjs: [
    {
      type: 'component',
      pathPatterns: [/_components[/\\].*\.tsx$/i, /components?[/\\].*\.tsx$/i],
      extensions: ['.tsx'],
      maxLines: 100,
    },
    {
      type: 'hook',
      pathPatterns: [/hooks?[/\\]use.*\.(ts|tsx)$/i],
      extensions: ['.ts', '.tsx'],
      maxLines: 100,
    },
  ],
  drizzle: [
    {
      type: 'service' as keyof CodeSamples,
      pathPatterns: [/schema[/\\](?!index\.).*\.ts$/i],
      extensions: ['.ts'],
      maxLines: 120,
    },
  ],
  prisma: [
    {
      type: 'service' as keyof CodeSamples,
      pathPatterns: [/schema\.prisma$/i],
      extensions: ['.prisma'],
      maxLines: 120,
    },
  ],
};

/** Directories to always skip */
const SKIP_DIRS = new Set([
  'node_modules', '.git', '.next', 'dist', 'build', 'bin', 'obj',
  '.claude', '.vs', '.vscode', 'coverage', '__pycache__', '.nuxt',
  'migrations', 'backup', 'backups', '.angular',
]);

/**
 * Scan the project for real code samples without grepai
 * Returns up to 1 sample per type (service, endpoint, component, hook)
 */
export async function scanCodeSamples(
  projectPath: string,
  stacks: Stack[],
  options: { verbose?: boolean } = {}
): Promise<CodeSamples> {
  const samples: CodeSamples = {};
  const stackNames = stacks.map(s => s.name);

  // Collect patterns for detected stacks
  const patterns: SamplePattern[] = [];
  for (const name of stackNames) {
    const stackPatterns = SAMPLE_PATTERNS[name];
    if (stackPatterns) {
      patterns.push(...stackPatterns);
    }
  }

  if (patterns.length === 0) return samples;

  // Walk the project tree looking for matching files
  const candidates = new Map<keyof CodeSamples, { file: string; score: number }[]>();

  await walkDir(projectPath, projectPath, patterns, candidates, 0, 6);

  // Pick the best candidate for each type
  for (const [type, files] of candidates) {
    if (files.length === 0) continue;
    // Prefer files with more content (not too small) and specific names
    files.sort((a, b) => b.score - a.score);
    const best = files[0];
    if (!best) continue;

    const pattern = patterns.find(p => p.type === type);
    const maxLines = pattern?.maxLines ?? 100;

    try {
      const content = await readFileTruncated(best.file, maxLines);
      if (content.length > 50) { // Skip very small files
        samples[type] = {
          file: relative(projectPath, best.file).replace(/\\/g, '/'),
          content,
          type,
        };
        if (options.verbose) {
          console.log(`  Found ${type} sample: ${relative(projectPath, best.file)}`);
        }
      }
    } catch {
      // Skip unreadable files
    }
  }

  return samples;
}

/**
 * Recursively walk directory to find matching files
 */
async function walkDir(
  dir: string,
  rootPath: string,
  patterns: SamplePattern[],
  candidates: Map<keyof CodeSamples, { file: string; score: number }[]>,
  depth: number,
  maxDepth: number
): Promise<void> {
  if (depth > maxDepth) return;

  let entries;
  try {
    entries = await readdir(dir, { withFileTypes: true });
  } catch {
    return;
  }

  for (const entry of entries) {
    const fullPath = join(dir, entry.name);

    if (entry.isDirectory()) {
      if (SKIP_DIRS.has(entry.name) || entry.name.startsWith('.')) continue;
      await walkDir(fullPath, rootPath, patterns, candidates, depth + 1, maxDepth);
    } else if (entry.isFile()) {
      const relPath = relative(rootPath, fullPath);
      const ext = extname(entry.name);

      for (const pattern of patterns) {
        if (!pattern.extensions.includes(ext)) continue;

        const matched = pattern.pathPatterns.some(rx => rx.test(relPath));
        if (!matched) continue;

        // Check file size (skip huge files)
        try {
          const info = await stat(fullPath);
          if (info.size > (pattern.maxSize ?? 50000)) continue;
          if (info.size < 100) continue;

          // Score: prefer medium-sized files with descriptive names
          const nameLen = basename(entry.name, ext).length;
          const score = Math.min(info.size, 5000) + (nameLen > 5 ? 100 : 0);

          const list = candidates.get(pattern.type) ?? [];
          list.push({ file: fullPath, score });
          candidates.set(pattern.type, list);
        } catch {
          // Skip
        }
      }
    }
  }
}

/**
 * Read a file, returning only the first N lines
 */
async function readFileTruncated(filePath: string, maxLines: number): Promise<string> {
  const content = await readFile(filePath, 'utf-8');
  const lines = content.split('\n');
  if (lines.length <= maxLines) return content;
  return lines.slice(0, maxLines).join('\n') + '\n// ... (truncated)';
}
