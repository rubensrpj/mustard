import { glob } from 'glob';
import { readFile } from 'fs/promises';
import { join, basename } from 'path';
import type { Stack, StackScanResult, NamingConventions, ScanOptions, StackPatternConfig } from '../types.js';

/**
 * Stack detection patterns
 */
const STACK_PATTERNS: Record<string, StackPatternConfig> = {
  dotnet: {
    indicators: ['**/*.csproj', '**/*.sln', '**/*.cs'],
    configFiles: ['*.csproj', 'Directory.Build.props'],
    versionExtractor: async (path: string): Promise<string> => {
      const csprojFiles = await glob('**/*.csproj', { cwd: path, nodir: true });
      if (csprojFiles.length > 0) {
        try {
          const firstCsproj = csprojFiles[0];
          if (firstCsproj) {
            const content = await readFile(join(path, firstCsproj), 'utf-8');
            const match = content.match(/<TargetFramework>net(\d+\.\d+|\d+)<\/TargetFramework>/);
            return match?.[1] ?? 'unknown';
          }
        } catch { /* ignored */ }
      }
      return 'unknown';
    }
  },
  react: {
    indicators: ['**/package.json'],
    configFiles: ['package.json', 'vite.config.*', 'next.config.*'],
    // Custom scanner for react to check all package.json files
    customScanner: async (projectPath: string): Promise<Stack[]> => {
      const results: Stack[] = [];
      const pkgFiles = await glob('**/package.json', {
        cwd: projectPath,
        nodir: true,
        ignore: ['**/node_modules/**']
      });

      for (const pkgFile of pkgFiles) {
        try {
          const fullPath = join(projectPath, pkgFile);
          const pkg = JSON.parse(await readFile(fullPath, 'utf-8')) as {
            dependencies?: Record<string, string>;
            devDependencies?: Record<string, string>;
          };
          const deps = { ...pkg.dependencies, ...pkg.devDependencies };

          if ('react' in deps || 'react-dom' in deps) {
            const subPath = pkgFile.replace(/\/package\.json$/, '').replace(/\\package\.json$/, '') || '.';
            const version = deps['react']?.replace('^', '').replace('~', '') ?? 'unknown';
            results.push({
              name: 'react',
              version,
              path: subPath,
              files: [pkgFile]
            });
          }
        } catch { /* ignored */ }
      }

      return results;
    }
  },
  nextjs: {
    indicators: ['**/next.config.*', '**/package.json'],
    detector: async (path: string): Promise<boolean> => {
      try {
        const pkgPath = join(path, 'package.json');
        const pkg = JSON.parse(await readFile(pkgPath, 'utf-8')) as {
          dependencies?: Record<string, string>;
          devDependencies?: Record<string, string>;
        };
        const deps = { ...pkg.dependencies, ...pkg.devDependencies };
        return 'next' in deps;
      } catch { return false; }
    },
    versionExtractor: async (path: string): Promise<string> => {
      try {
        const pkgPath = join(path, 'package.json');
        const pkg = JSON.parse(await readFile(pkgPath, 'utf-8')) as {
          dependencies?: Record<string, string>;
          devDependencies?: Record<string, string>;
        };
        const deps = { ...pkg.dependencies, ...pkg.devDependencies };
        return deps['next']?.replace('^', '').replace('~', '') ?? 'unknown';
      } catch { return 'unknown'; }
    }
  },
  node: {
    indicators: ['**/package.json'],
    configFiles: ['package.json', 'tsconfig.json'],
    detector: async (path: string): Promise<boolean> => {
      const hasPackageJson = await glob('package.json', { cwd: path }).then(f => f.length > 0);
      return hasPackageJson;
    }
  },
  python: {
    indicators: ['**/requirements.txt', '**/pyproject.toml', '**/*.py'],
    configFiles: ['requirements.txt', 'pyproject.toml', 'setup.py'],
    versionExtractor: async (path: string): Promise<string> => {
      try {
        const pyprojectPath = join(path, 'pyproject.toml');
        const content = await readFile(pyprojectPath, 'utf-8');
        const match = content.match(/python\s*=\s*"[><=^~]*(\d+\.\d+)/);
        return match?.[1] ?? 'unknown';
      } catch { return 'unknown'; }
    }
  },
  java: {
    indicators: ['**/pom.xml', '**/build.gradle', '**/*.java'],
    configFiles: ['pom.xml', 'build.gradle', 'build.gradle.kts']
  },
  rust: {
    indicators: ['**/Cargo.toml', '**/*.rs'],
    configFiles: ['Cargo.toml']
  },
  go: {
    indicators: ['**/go.mod', '**/*.go'],
    configFiles: ['go.mod']
  },
  drizzle: {
    indicators: ['**/drizzle.config.*', '**/schema/*.ts'],
    // Custom scanner for drizzle to check all package.json files
    customScanner: async (projectPath: string): Promise<Stack[]> => {
      const results: Stack[] = [];
      const pkgFiles = await glob('**/package.json', {
        cwd: projectPath,
        nodir: true,
        ignore: ['**/node_modules/**']
      });

      for (const pkgFile of pkgFiles) {
        try {
          const fullPath = join(projectPath, pkgFile);
          const pkg = JSON.parse(await readFile(fullPath, 'utf-8')) as {
            dependencies?: Record<string, string>;
            devDependencies?: Record<string, string>;
          };
          const deps = { ...pkg.dependencies, ...pkg.devDependencies };

          if ('drizzle-orm' in deps) {
            const subPath = pkgFile.replace(/\/package\.json$/, '').replace(/\\package\.json$/, '') || '.';
            results.push({
              name: 'drizzle',
              version: deps['drizzle-orm']?.replace('^', '').replace('~', '') ?? 'unknown',
              path: subPath,
              files: [pkgFile]
            });
          }
        } catch { /* ignored */ }
      }

      return results;
    }
  }
};

/**
 * Package manager detection
 */
const PACKAGE_MANAGERS: Record<string, string[]> = {
  pnpm: ['pnpm-lock.yaml', 'pnpm-workspace.yaml'],
  yarn: ['yarn.lock', '.yarnrc.yml'],
  npm: ['package-lock.json'],
  bun: ['bun.lockb']
};

/**
 * Scan project for stacks (languages and frameworks)
 */
export async function scanStack(projectPath: string, options: ScanOptions = {}): Promise<StackScanResult> {
  const { verbose = false } = options;

  const stacks: Stack[] = [];
  const detectedPaths = new Map<string, boolean>();

  // Check each stack
  for (const [stackName, config] of Object.entries(STACK_PATTERNS)) {
    // Use custom scanner if available (for stacks that need to check multiple package.json)
    if (config.customScanner) {
      const customResults = await config.customScanner(projectPath);
      for (const result of customResults) {
        const key = `${result.name}-${result.path}`;
        if (!detectedPaths.has(key)) {
          detectedPaths.set(key, true);
          stacks.push(result);
        }
      }
      continue;
    }

    // Standard detection via indicators
    for (const pattern of config.indicators) {
      const matches = await glob(pattern, {
        cwd: projectPath,
        nodir: true,
        ignore: ['**/node_modules/**', '**/bin/**', '**/obj/**', '**/.next/**', '**/dist/**']
      });

      if (matches.length > 0) {
        // If there's a custom detector, use it
        if (config.detector) {
          const detected = await config.detector(projectPath);
          if (!detected) continue;
        }

        // Get version if extractor exists
        let version = 'unknown';
        if (config.versionExtractor) {
          version = await config.versionExtractor(projectPath);
        }

        // Determine subpath
        const firstMatch = matches[0];
        const subPath = firstMatch?.includes('/') ? firstMatch.split('/')[0] ?? '.' : '.';

        // Avoid duplicates
        if (!detectedPaths.has(`${stackName}-${subPath}`)) {
          detectedPaths.set(`${stackName}-${subPath}`, true);
          stacks.push({
            name: stackName,
            version,
            path: subPath,
            files: matches.slice(0, 5) // Sample files
          });
        }
      }
    }
  }

  // Detect package manager
  const packageManager = await detectPackageManager(projectPath);

  // Infer naming conventions from files
  const naming = await inferNamingConventions(projectPath, stacks);

  return {
    stacks,
    packageManager,
    naming
  };
}

/**
 * Detect package manager from lock files
 */
async function detectPackageManager(projectPath: string): Promise<string> {
  for (const [manager, lockFiles] of Object.entries(PACKAGE_MANAGERS)) {
    for (const lockFile of lockFiles) {
      const matches = await glob(lockFile, { cwd: projectPath });
      if (matches.length > 0) {
        return manager;
      }
    }
  }
  return 'npm'; // Default
}

/**
 * Infer naming conventions from existing files
 */
async function inferNamingConventions(projectPath: string, stacks: Stack[]): Promise<NamingConventions> {
  const naming: NamingConventions = {
    classes: 'PascalCase',
    files: {},
    variables: 'camelCase'
  };

  // Check .cs files for class naming
  const csFiles = await glob('**/*.cs', {
    cwd: projectPath,
    nodir: true,
    ignore: ['**/bin/**', '**/obj/**']
  });

  if (csFiles.length > 0) {
    (naming.files as Record<string, string>)['cs'] = 'PascalCase'; // .NET convention
  }

  // Check .ts/.tsx files
  const tsFiles = await glob('**/*.{ts,tsx}', {
    cwd: projectPath,
    nodir: true,
    ignore: ['**/node_modules/**', '**/.next/**', '**/dist/**']
  });

  if (tsFiles.length > 0) {
    // Analyze file names
    const sample = tsFiles.slice(0, 20);
    const kebabCount = sample.filter(f => /[a-z]+-[a-z]+/.test(basename(f))).length;
    const pascalCount = sample.filter(f => /^[A-Z][a-zA-Z]+\.tsx?$/.test(basename(f))).length;
    const camelCount = sample.filter(f => /^[a-z][a-zA-Z]+\.tsx?$/.test(basename(f))).length;

    const filesObj = naming.files as Record<string, string>;
    if (kebabCount > pascalCount && kebabCount > camelCount) {
      filesObj['ts'] = 'kebab-case';
      filesObj['tsx'] = 'kebab-case';
    } else if (pascalCount > camelCount) {
      filesObj['ts'] = 'PascalCase';
      filesObj['tsx'] = 'PascalCase';
    } else {
      filesObj['ts'] = 'camelCase';
      filesObj['tsx'] = 'camelCase';
    }
  }

  // Check Python files
  const pyFiles = await glob('**/*.py', {
    cwd: projectPath,
    nodir: true,
    ignore: ['**/venv/**', '**/.venv/**', '**/__pycache__/**']
  });

  if (pyFiles.length > 0) {
    (naming.files as Record<string, string>)['py'] = 'snake_case'; // Python convention
  }

  return naming;
}
