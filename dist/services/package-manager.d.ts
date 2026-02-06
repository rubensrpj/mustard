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
 * Get the full configuration for a package manager
 */
export declare function getConfig(pm: string): PackageManagerConfig;
/**
 * Get the command to run a script
 *
 * @example
 * getRunCommand('npm', 'build')    // 'npm run build'
 * getRunCommand('yarn', 'build')   // 'yarn build'
 * getRunCommand('pnpm', 'build')   // 'pnpm build'
 * getRunCommand('bun', 'build')    // 'bun run build'
 */
export declare function getRunCommand(pm: string, script: string): string;
/**
 * Get the command to install all dependencies
 *
 * @example
 * getInstallCommand('npm')   // 'npm install'
 * getInstallCommand('yarn')  // 'yarn'
 * getInstallCommand('pnpm')  // 'pnpm install'
 * getInstallCommand('bun')   // 'bun install'
 */
export declare function getInstallCommand(pm: string): string;
/**
 * Get the command to execute a binary (like npx)
 *
 * @example
 * getExecCommand('npm')   // 'npx'
 * getExecCommand('yarn')  // 'yarn exec'
 * getExecCommand('pnpm')  // 'pnpm exec'
 * getExecCommand('bun')   // 'bunx'
 */
export declare function getExecCommand(pm: string): string;
/**
 * Get the command to add a package
 *
 * @example
 * getAddCommand('npm', 'lodash')              // 'npm install lodash'
 * getAddCommand('npm', 'typescript', true)   // 'npm install --save-dev typescript'
 * getAddCommand('yarn', 'lodash')             // 'yarn add lodash'
 * getAddCommand('yarn', 'typescript', true)  // 'yarn add --dev typescript'
 */
export declare function getAddCommand(pm: string, pkg: string, dev?: boolean): string;
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
export declare function canOmitRun(pm: string): boolean;
/**
 * Get the display name for a package manager
 *
 * @example
 * getDisplayName('npm')   // 'npm'
 * getDisplayName('yarn')  // 'Yarn'
 * getDisplayName('pnpm')  // 'pnpm'
 * getDisplayName('bun')   // 'Bun'
 */
export declare function getDisplayName(pm: string): string;
/**
 * Check if a string is a valid package manager
 */
export declare function isValidPackageManager(pm: string): pm is PackageManager;
/**
 * Get all supported package managers
 */
export declare function getSupportedPackageManagers(): PackageManager[];
