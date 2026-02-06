import { exec } from 'child_process';
import { promisify } from 'util';
import { readFile } from 'fs/promises';
import { join, dirname } from 'path';
import { fileURLToPath } from 'url';

const execAsync = promisify(exec);

const PACKAGE_NAME = 'mustard-claude';

/**
 * Get the latest version of mustard-claude from npm registry
 */
export async function getLatestVersion(): Promise<string> {
  try {
    const { stdout } = await execAsync(`npm view ${PACKAGE_NAME} version`);
    return stdout.trim();
  } catch (error) {
    throw new Error('Failed to check npm registry. Are you online?');
  }
}

/**
 * Get the current installed version from package.json
 */
export async function getCurrentVersion(): Promise<string> {
  try {
    const __filename = fileURLToPath(import.meta.url);
    const __dirname = dirname(__filename);
    const packagePath = join(__dirname, '..', '..', 'package.json');
    const content = await readFile(packagePath, 'utf-8');
    const pkg = JSON.parse(content);
    return pkg.version;
  } catch (error) {
    throw new Error('Failed to read current version');
  }
}

/**
 * Compare two semver versions
 * @returns -1 if a < b, 0 if equal, 1 if a > b
 */
export function compareVersions(a: string, b: string): number {
  const partsA = a.split('.').map(Number);
  const partsB = b.split('.').map(Number);

  for (let i = 0; i < 3; i++) {
    const partA = partsA[i] ?? 0;
    const partB = partsB[i] ?? 0;

    if (partA < partB) return -1;
    if (partA > partB) return 1;
  }

  return 0;
}

/**
 * Check if an update is available
 */
export async function checkForUpdate(): Promise<{ hasUpdate: boolean; current: string; latest: string }> {
  const current = await getCurrentVersion();
  const latest = await getLatestVersion();
  const hasUpdate = compareVersions(current, latest) < 0;

  return { hasUpdate, current, latest };
}

/**
 * Update mustard-claude globally via npm
 */
export async function updateGlobal(): Promise<void> {
  try {
    await execAsync(`npm install -g ${PACKAGE_NAME}@latest`);
  } catch (error) {
    throw new Error('Failed to update. Try running with sudo or as administrator.');
  }
}
