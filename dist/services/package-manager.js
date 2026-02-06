/**
 * Package Manager Service
 *
 * Provides utilities for working with different JavaScript package managers.
 * Supports npm, yarn, pnpm, and bun with their respective command differences.
 */
/**
 * Configuration for each package manager
 */
const configs = {
    npm: {
        run: (script) => `npm run ${script}`,
        install: 'npm install',
        installDev: 'npm install --save-dev',
        exec: 'npx',
        add: (pkg) => `npm install ${pkg}`,
        addDev: (pkg) => `npm install --save-dev ${pkg}`,
    },
    yarn: {
        run: (script) => `yarn ${script}`,
        install: 'yarn',
        installDev: 'yarn add --dev',
        exec: 'yarn exec',
        add: (pkg) => `yarn add ${pkg}`,
        addDev: (pkg) => `yarn add --dev ${pkg}`,
    },
    pnpm: {
        run: (script) => `pnpm ${script}`,
        install: 'pnpm install',
        installDev: 'pnpm add --save-dev',
        exec: 'pnpm exec',
        add: (pkg) => `pnpm add ${pkg}`,
        addDev: (pkg) => `pnpm add --save-dev ${pkg}`,
    },
    bun: {
        run: (script) => `bun run ${script}`,
        install: 'bun install',
        installDev: 'bun add --dev',
        exec: 'bunx',
        add: (pkg) => `bun add ${pkg}`,
        addDev: (pkg) => `bun add --dev ${pkg}`,
    },
};
/**
 * Display names for each package manager
 */
const displayNames = {
    npm: 'npm',
    yarn: 'Yarn',
    pnpm: 'pnpm',
    bun: 'Bun',
};
/**
 * Validate and normalize package manager string
 */
function normalizePackageManager(pm) {
    const normalized = pm.toLowerCase();
    if (normalized in configs) {
        return normalized;
    }
    // Default to npm for unknown package managers
    return 'npm';
}
/**
 * Get the full configuration for a package manager
 */
export function getConfig(pm) {
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
export function getRunCommand(pm, script) {
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
export function getInstallCommand(pm) {
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
export function getExecCommand(pm) {
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
export function getAddCommand(pm, pkg, dev = false) {
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
export function canOmitRun(pm) {
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
export function getDisplayName(pm) {
    return displayNames[normalizePackageManager(pm)];
}
/**
 * Check if a string is a valid package manager
 */
export function isValidPackageManager(pm) {
    return pm.toLowerCase() in configs;
}
/**
 * Get all supported package managers
 */
export function getSupportedPackageManagers() {
    return Object.keys(configs);
}
//# sourceMappingURL=package-manager.js.map