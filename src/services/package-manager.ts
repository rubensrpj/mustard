/**
 * Package Manager Service
 *
 * Provides utilities for working with different JavaScript package managers.
 * Supports npm, yarn, pnpm, and bun with their respective command differences.
 */

export type PackageManager = 'npm' | 'yarn' | 'pnpm' | 'bun';

export interface PackageManagerConfig {
  run: (script: string) => string;
  install: string;
  installDev: string;
  exec: string;
  add: (pkg: string) => string;
  addDev: (pkg: string) => string;
}

/**
 * Configuration for each package manager
 */
const configs: Record<PackageManager, PackageManagerConfig> = {
  npm: {
    run: (script: string) => `npm run ${script}`,
    install: 'npm install',
    installDev: 'npm install --save-dev',
    exec: 'npx',
    add: (pkg: string) => `npm install ${pkg}`,
    addDev: (pkg: string) => `npm install --save-dev ${pkg}`,
  },
  yarn: {
    run: (script: string) => `yarn ${script}`,
    install: 'yarn',
    installDev: 'yarn add --dev',
    exec: 'yarn exec',
    add: (pkg: string) => `yarn add ${pkg}`,
    addDev: (pkg: string) => `yarn add --dev ${pkg}`,
  },
  pnpm: {
    run: (script: string) => `pnpm ${script}`,
    install: 'pnpm install',
    installDev: 'pnpm add --save-dev',
    exec: 'pnpm exec',
    add: (pkg: string) => `pnpm add ${pkg}`,
    addDev: (pkg: string) => `pnpm add --save-dev ${pkg}`,
  },
  bun: {
    run: (script: string) => `bun run ${script}`,
    install: 'bun install',
    installDev: 'bun add --dev',
    exec: 'bunx',
    add: (pkg: string) => `bun add ${pkg}`,
    addDev: (pkg: string) => `bun add --dev ${pkg}`,
  },
};

/**
 * Display names for each package manager
 */
const displayNames: Record<PackageManager, string> = {
  npm: 'npm',
  yarn: 'Yarn',
  pnpm: 'pnpm',
  bun: 'Bun',
};

/**
 * Validate and normalize package manager string
 */
function normalizePackageManager(pm: string): PackageManager {
  const normalized = pm.toLowerCase() as PackageManager;
  if (normalized in configs) {
    return normalized;
  }
  // Default to npm for unknown package managers
  return 'npm';
}

/**
 * Get the full configuration for a package manager
 */
export function getConfig(pm: string): PackageManagerConfig {
  return configs[normalizePackageManager(pm)];
}

/**
 * Get the command to run a script
 *
 * @example
 * getRunCommand('npm', 'build')    // 'npm run build'
 * getRunCommand('yarn', 'build')   // 'yarn build'
 * getRunCommand('pnpm', 'build')   // 'pnpm build'
 * getRunCommand('bun', 'build')    // 'bun run build'
 */
export function getRunCommand(pm: string, script: string): string {
  return configs[normalizePackageManager(pm)].run(script);
}

/**
 * Get the command to install all dependencies
 *
 * @example
 * getInstallCommand('npm')   // 'npm install'
 * getInstallCommand('yarn')  // 'yarn'
 * getInstallCommand('pnpm')  // 'pnpm install'
 * getInstallCommand('bun')   // 'bun install'
 */
export function getInstallCommand(pm: string): string {
  return configs[normalizePackageManager(pm)].install;
}

/**
 * Get the command to execute a binary (like npx)
 *
 * @example
 * getExecCommand('npm')   // 'npx'
 * getExecCommand('yarn')  // 'yarn exec'
 * getExecCommand('pnpm')  // 'pnpm exec'
 * getExecCommand('bun')   // 'bunx'
 */
export function getExecCommand(pm: string): string {
  return configs[normalizePackageManager(pm)].exec;
}

/**
 * Get the command to add a package
 *
 * @example
 * getAddCommand('npm', 'lodash')              // 'npm install lodash'
 * getAddCommand('npm', 'typescript', true)   // 'npm install --save-dev typescript'
 * getAddCommand('yarn', 'lodash')             // 'yarn add lodash'
 * getAddCommand('yarn', 'typescript', true)  // 'yarn add --dev typescript'
 */
export function getAddCommand(pm: string, pkg: string, dev: boolean = false): string {
  const config = configs[normalizePackageManager(pm)];
  return dev ? config.addDev(pkg) : config.add(pkg);
}

/**
 * Check if the package manager can omit "run" for scripts
 *
 * yarn and pnpm allow running scripts without "run":
 *   yarn build (instead of yarn run build)
 *   pnpm build (instead of pnpm run build)
 *
 * npm and bun require "run":
 *   npm run build
 *   bun run build
 *
 * @example
 * canOmitRun('yarn')  // true
 * canOmitRun('pnpm')  // true
 * canOmitRun('npm')   // false
 * canOmitRun('bun')   // false
 */
export function canOmitRun(pm: string): boolean {
  const normalized = normalizePackageManager(pm);
  return normalized === 'yarn' || normalized === 'pnpm';
}

/**
 * Get the display name for a package manager
 *
 * @example
 * getDisplayName('npm')   // 'npm'
 * getDisplayName('yarn')  // 'Yarn'
 * getDisplayName('pnpm')  // 'pnpm'
 * getDisplayName('bun')   // 'Bun'
 */
export function getDisplayName(pm: string): string {
  return displayNames[normalizePackageManager(pm)];
}

/**
 * Check if a string is a valid package manager
 */
export function isValidPackageManager(pm: string): pm is PackageManager {
  return pm.toLowerCase() in configs;
}

/**
 * Get all supported package managers
 */
export function getSupportedPackageManagers(): PackageManager[] {
  return Object.keys(configs) as PackageManager[];
}
