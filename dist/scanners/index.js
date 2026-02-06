import { scanStack } from './stack.js';
import { scanStructure } from './structure.js';
import { scanDependencies } from './dependencies.js';
/**
 * Main scanner orchestrator
 * Scans the project directory and returns comprehensive project info
 */
export async function scanProject(projectPath, options = {}) {
    const { verbose = false } = options;
    // Scan stack (languages, frameworks)
    const stackInfo = await scanStack(projectPath, { verbose });
    // Scan structure (monorepo, folders)
    const structureInfo = await scanStructure(projectPath, { verbose });
    // Scan dependencies from package.json and .csproj
    const dependencies = await scanDependencies(projectPath, { verbose });
    // Combine results
    return {
        name: structureInfo.name,
        path: projectPath,
        type: structureInfo.type, // 'monorepo' | 'single'
        stacks: stackInfo.stacks,
        patterns: {
            classes: stackInfo.naming.classes,
            files: stackInfo.naming.files,
            folders: structureInfo.folderStyle
        },
        structure: structureInfo,
        packageManager: stackInfo.packageManager,
        entities: [], // Will be populated by semantic analyzer
        dependencies,
        raw: {
            stack: stackInfo,
            structure: structureInfo
        }
    };
}
//# sourceMappingURL=index.js.map