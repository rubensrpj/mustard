import { glob } from 'glob';
import { readFile, readdir } from 'fs/promises';
import { join, basename } from 'path';
/**
 * Architecture patterns to detect
 */
const ARCHITECTURE_PATTERNS = {
    mvc: {
        folders: ['Controllers', 'Models', 'Views'],
        confidence: 'high'
    },
    clean: {
        folders: ['Domain', 'Application', 'Infrastructure', 'Presentation'],
        confidence: 'high'
    },
    featureBased: {
        patterns: [/Modules\/\w+\/Endpoints/, /features\/\w+\/components/],
        confidence: 'high'
    },
    reactStandard: {
        folders: ['components', 'hooks', 'pages'],
        confidence: 'medium'
    },
    layered: {
        folders: ['Services', 'Repositories', 'Entities'],
        confidence: 'medium'
    }
};
/**
 * Scan project structure
 */
export async function scanStructure(projectPath, options = {}) {
    const { verbose = false } = options;
    // Get project name from directory or package.json
    const name = await getProjectName(projectPath);
    // Check if monorepo
    const isMonorepo = await detectMonorepo(projectPath);
    // Get top-level directories
    const directories = await getDirectories(projectPath);
    // Detect architecture pattern
    const architecture = await detectArchitecture(projectPath, directories);
    // Detect folder naming style
    const folderStyle = detectFolderStyle(directories);
    // Get subprojects if monorepo
    const subprojects = isMonorepo ? await getSubprojects(projectPath) : [];
    return {
        name,
        type: isMonorepo ? 'monorepo' : 'single',
        architecture,
        directories,
        folderStyle,
        subprojects
    };
}
/**
 * Get project name from package.json or directory name
 */
async function getProjectName(projectPath) {
    try {
        const pkgPath = join(projectPath, 'package.json');
        const pkg = JSON.parse(await readFile(pkgPath, 'utf-8'));
        return pkg.name ?? basename(projectPath);
    }
    catch {
        return basename(projectPath);
    }
}
/**
 * Detect if project is a monorepo
 */
async function detectMonorepo(projectPath) {
    // Check for common monorepo indicators
    const indicators = [
        'pnpm-workspace.yaml',
        'lerna.json',
        'nx.json',
        'turbo.json',
        'rush.json'
    ];
    for (const indicator of indicators) {
        const matches = await glob(indicator, { cwd: projectPath });
        if (matches.length > 0)
            return true;
    }
    // Check for workspaces in package.json
    try {
        const pkgPath = join(projectPath, 'package.json');
        const pkg = JSON.parse(await readFile(pkgPath, 'utf-8'));
        if (pkg.workspaces)
            return true;
    }
    catch { /* ignored */ }
    // Check for multiple package.json files in subdirectories
    const packageJsons = await glob('*/package.json', { cwd: projectPath });
    if (packageJsons.length >= 2)
        return true;
    // Check for .NET solution with multiple projects
    const slnFiles = await glob('*.sln', { cwd: projectPath });
    if (slnFiles.length > 0) {
        const csprojFiles = await glob('**/*.csproj', {
            cwd: projectPath,
            ignore: ['**/bin/**', '**/obj/**']
        });
        if (csprojFiles.length >= 2)
            return true;
    }
    return false;
}
/**
 * Get top-level directories
 */
async function getDirectories(projectPath) {
    const entries = await readdir(projectPath, { withFileTypes: true });
    return entries
        .filter(entry => entry.isDirectory())
        .filter(entry => !entry.name.startsWith('.'))
        .filter(entry => !['node_modules', 'bin', 'obj', 'dist', '.next', '__pycache__', 'venv', '.venv'].includes(entry.name))
        .map(entry => entry.name);
}
/**
 * Detect architecture pattern
 */
async function detectArchitecture(projectPath, directories) {
    const allPaths = await glob('**/', {
        cwd: projectPath,
        ignore: ['**/node_modules/**', '**/bin/**', '**/obj/**', '**/.next/**', '**/dist/**']
    });
    const detectedPatterns = [];
    for (const [patternName, config] of Object.entries(ARCHITECTURE_PATTERNS)) {
        if (config.folders) {
            // Check if all required folders exist
            const hasAll = config.folders.every(folder => directories.some(d => d.toLowerCase() === folder.toLowerCase()) ||
                allPaths.some(p => p.toLowerCase().includes(folder.toLowerCase())));
            if (hasAll) {
                detectedPatterns.push({
                    type: patternName,
                    confidence: config.confidence
                });
            }
        }
        if (config.patterns) {
            // Check regex patterns
            const matchesAny = config.patterns.some(pattern => allPaths.some(p => pattern.test(p)));
            if (matchesAny) {
                detectedPatterns.push({
                    type: patternName,
                    confidence: config.confidence
                });
            }
        }
    }
    // Return the highest confidence match
    if (detectedPatterns.length > 0) {
        const highConfidence = detectedPatterns.find(p => p.confidence === 'high');
        return highConfidence ?? detectedPatterns[0];
    }
    return { type: 'unknown', confidence: 'low' };
}
/**
 * Detect folder naming style (singular vs plural)
 */
function detectFolderStyle(directories) {
    const pluralIndicators = ['Controllers', 'Models', 'Services', 'Repositories', 'Entities', 'Modules', 'components', 'hooks', 'pages', 'features'];
    const singularIndicators = ['Controller', 'Model', 'Service', 'Repository', 'Entity', 'Module', 'component', 'hook', 'page', 'feature'];
    let pluralCount = 0;
    let singularCount = 0;
    for (const dir of directories) {
        if (pluralIndicators.some(p => dir.toLowerCase() === p.toLowerCase())) {
            pluralCount++;
        }
        if (singularIndicators.some(s => dir.toLowerCase() === s.toLowerCase())) {
            singularCount++;
        }
    }
    return pluralCount >= singularCount ? 'plural' : 'singular';
}
/**
 * Parse workspaces field from package.json
 * Supports npm, yarn (both array and object formats), and bun workspaces
 */
async function parsePackageJsonWorkspaces(projectPath) {
    try {
        const pkgPath = join(projectPath, 'package.json');
        const pkg = JSON.parse(await readFile(pkgPath, 'utf-8'));
        if (!pkg.workspaces) {
            return [];
        }
        // Handle array format (npm, bun, yarn classic)
        if (Array.isArray(pkg.workspaces)) {
            return pkg.workspaces;
        }
        // Handle object format with packages key (yarn berry)
        if (typeof pkg.workspaces === 'object' && pkg.workspaces.packages) {
            return pkg.workspaces.packages;
        }
        return [];
    }
    catch {
        return [];
    }
}
/**
 * Resolve workspace patterns to actual subprojects
 */
async function resolveWorkspacePatterns(projectPath, patterns) {
    const subprojects = [];
    for (const pattern of patterns) {
        const matches = await glob(pattern, { cwd: projectPath });
        for (const match of matches) {
            const pkgJsonPath = join(projectPath, match, 'package.json');
            try {
                const pkgJson = JSON.parse(await readFile(pkgJsonPath, 'utf-8'));
                subprojects.push({
                    name: pkgJson.name ?? match,
                    path: match
                });
            }
            catch {
                subprojects.push({ name: match, path: match });
            }
        }
    }
    return subprojects;
}
/**
 * Get subprojects in monorepo
 */
async function getSubprojects(projectPath) {
    const subprojects = [];
    // Check for pnpm workspace first
    let pnpmWorkspaceFound = false;
    try {
        const workspaceFile = join(projectPath, 'pnpm-workspace.yaml');
        const content = await readFile(workspaceFile, 'utf-8');
        // Simple yaml parsing for packages field
        const packagesMatch = content.match(/packages:\s*\n((?:\s+-\s*.+\n?)+)/);
        if (packagesMatch) {
            pnpmWorkspaceFound = true;
            const packages = packagesMatch[1]
                .split('\n')
                .map(line => line.replace(/^\s*-\s*['"]?/, '').replace(/['"]?\s*$/, ''))
                .filter(Boolean);
            const resolved = await resolveWorkspacePatterns(projectPath, packages);
            subprojects.push(...resolved);
        }
    }
    catch { /* ignored */ }
    // If no pnpm workspace, try package.json workspaces (npm, yarn, bun)
    if (!pnpmWorkspaceFound) {
        const workspacePatterns = await parsePackageJsonWorkspaces(projectPath);
        if (workspacePatterns.length > 0) {
            const resolved = await resolveWorkspacePatterns(projectPath, workspacePatterns);
            subprojects.push(...resolved);
        }
    }
    // Check for .NET projects
    const csprojFiles = await glob('**/*.csproj', {
        cwd: projectPath,
        ignore: ['**/bin/**', '**/obj/**']
    });
    for (const csproj of csprojFiles) {
        const projectDir = csproj.replace(/\/[^/]+\.csproj$/, '').replace(/\\[^\\]+\.csproj$/, '');
        const projectName = basename(csproj, '.csproj');
        // Avoid duplicates
        if (!subprojects.find(s => s.path === projectDir)) {
            subprojects.push({
                name: projectName,
                path: projectDir || '.'
            });
        }
    }
    return subprojects;
}
//# sourceMappingURL=structure.js.map