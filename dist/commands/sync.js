import { existsSync } from 'fs';
import { readFile, writeFile, readdir, mkdir } from 'fs/promises';
import { join, resolve } from 'path';
import chalk from 'chalk';
import ora from 'ora';
import inquirer from 'inquirer';
import { scanProject } from '../scanners/index.js';
import * as ollamaService from '../services/ollama.js';
import * as grepaiService from '../services/grepai.js';
import * as semanticAnalyzer from '../analyzers/semantic.js';
import { generateContext } from '../generators/context.js';
import { generateRegistry } from '../generators/registry.js';
import { generateAutoSection, mergePromptContext } from '../generators/prompts.js';
/**
 * Sync command - syncs prompts and context with current codebase state
 */
export async function syncCommand(options) {
    const projectPath = resolve(process.cwd());
    const claudePath = join(projectPath, '.claude');
    console.log(chalk.bold('\n🌿 Mustard CLI v2.0 - Sync\n'));
    // 1. Check .claude/ exists
    if (!existsSync(claudePath)) {
        console.log(chalk.red('❌ No .claude/ directory found.'));
        console.log(chalk.gray('   Run "mustard init" first.\n'));
        return;
    }
    // Determine scope
    const syncAll = !options.prompts && !options.context && !options.registry;
    const syncPrompts = syncAll || options.prompts;
    const syncContext = syncAll || options.context;
    const syncRegistry = syncAll || options.registry;
    // Show plan
    console.log(chalk.white('📋 Sync plan:\n'));
    if (syncPrompts)
        console.log(chalk.green('  ✓ Prompts (merge with existing)'));
    if (syncContext)
        console.log(chalk.green('  ✓ Context files (regenerate)'));
    if (syncRegistry)
        console.log(chalk.green('  ✓ Entity registry (update)'));
    console.log(chalk.yellow('\n  ⚡ Preserved: CLAUDE.md, commands/, hooks/\n'));
    // Confirm
    if (!options.force) {
        const { proceed } = await inquirer.prompt([{
                type: 'confirm',
                name: 'proceed',
                message: 'Proceed with sync?',
                default: true
            }]);
        if (!proceed) {
            console.log(chalk.yellow('\n⚠️  Cancelled.\n'));
            return;
        }
    }
    // 2. Check dependencies
    const deps = await checkDependencies(options);
    // 3. Scan project
    const scanSpinner = ora('Scanning project...').start();
    let projectInfo;
    try {
        projectInfo = await scanProject(projectPath, { verbose: options.verbose });
        scanSpinner.succeed('Project scanned');
    }
    catch (error) {
        scanSpinner.fail('Scan failed');
        const message = error instanceof Error ? error.message : 'Unknown error';
        console.error(chalk.red(message));
        return;
    }
    // 4. Semantic analysis (if grepai available)
    let patterns = {
        services: [],
        repositories: [],
        endpoints: [],
        components: [],
        hooks: [],
        entities: [],
        callGraph: null
    };
    if (deps.grepai && options.grepai !== false) {
        const semanticSpinner = ora('Analyzing codebase (grepai)...').start();
        try {
            patterns = await semanticAnalyzer.discoverPatterns({
                stacks: projectInfo.stacks,
                verbose: options.verbose
            });
            const entities = await semanticAnalyzer.discoverEntities(projectInfo.stacks);
            patterns.entities = entities;
            semanticSpinner.succeed(`Found ${patterns.entities.length} entities`);
        }
        catch (error) {
            semanticSpinner.warn('Semantic analysis limited');
            if (options.verbose) {
                const message = error instanceof Error ? error.message : 'Unknown error';
                console.log(chalk.gray(`  ${message}`));
            }
        }
    }
    // 5. Build analysis object
    const analysis = {
        architecture: projectInfo.structure?.architecture ?? { type: 'unknown', confidence: 'low' },
        patterns: [],
        rules: [],
        entities: patterns.entities
    };
    // 6. Sync prompts
    let promptsUpdated = 0;
    if (syncPrompts) {
        const promptsSpinner = ora('Syncing prompts...').start();
        try {
            promptsUpdated = await syncPromptsWithContext(claudePath, projectInfo, analysis, patterns);
            promptsSpinner.succeed(`Updated ${promptsUpdated} prompts`);
        }
        catch (error) {
            promptsSpinner.fail('Prompt sync failed');
            const message = error instanceof Error ? error.message : 'Unknown error';
            console.error(chalk.red(message));
        }
    }
    // 7. Sync context
    if (syncContext) {
        const contextSpinner = ora('Syncing context files...').start();
        try {
            await generateContext(claudePath, projectInfo, analysis, {}, {
                useOllama: deps.ollama && options.ollama !== false,
                model: deps.ollamaModel ?? undefined,
                verbose: options.verbose
            });
            contextSpinner.succeed('Context files updated');
        }
        catch (error) {
            contextSpinner.fail('Context sync failed');
            const message = error instanceof Error ? error.message : 'Unknown error';
            console.error(chalk.red(message));
        }
    }
    // 8. Sync registry
    if (syncRegistry) {
        const registrySpinner = ora('Updating entity registry...').start();
        try {
            const registry = generateRegistry(projectInfo, analysis);
            const registryPath = join(claudePath, 'entity-registry.json');
            await writeFile(registryPath, JSON.stringify(registry, null, 2));
            registrySpinner.succeed(`Entity registry updated (${Object.keys(registry.e).length} entities)`);
        }
        catch (error) {
            registrySpinner.fail('Registry sync failed');
            const message = error instanceof Error ? error.message : 'Unknown error';
            console.error(chalk.red(message));
        }
    }
    // Done!
    console.log(chalk.green.bold('\n✅ Sync complete!\n'));
    // Show summary
    const changes = [];
    if (syncPrompts && promptsUpdated > 0)
        changes.push(`${promptsUpdated} prompts`);
    if (syncContext)
        changes.push('context files');
    if (syncRegistry)
        changes.push('entity registry');
    if (changes.length > 0) {
        console.log(chalk.gray(`  Updated: ${changes.join(', ')}`));
    }
    console.log(chalk.gray('  Run "mustard sync --help" for more options.\n'));
}
/**
 * Check available dependencies
 */
async function checkDependencies(options) {
    const deps = {
        ollama: false,
        ollamaModel: null,
        grepai: false
    };
    if (options.ollama !== false) {
        const ollamaSpinner = ora('Checking Ollama...').start();
        const ollamaAvailable = await ollamaService.checkOllamaAvailable();
        if (ollamaAvailable) {
            const models = await ollamaService.getAvailableModels();
            if (models.length > 0) {
                deps.ollama = true;
                deps.ollamaModel = await ollamaService.selectBestModel(models);
                ollamaSpinner.succeed(`Ollama: ${deps.ollamaModel}`);
            }
            else {
                ollamaSpinner.warn('Ollama running but no models');
            }
        }
        else {
            ollamaSpinner.warn('Ollama not available');
        }
    }
    if (options.grepai !== false) {
        const grepaiSpinner = ora('Checking grepai...').start();
        const grepaiAvailable = await grepaiService.checkGrepaiAvailable();
        if (grepaiAvailable) {
            deps.grepai = true;
            grepaiSpinner.succeed('grepai: available');
        }
        else {
            grepaiSpinner.warn('grepai not available');
        }
    }
    return deps;
}
/**
 * Sync prompts with current project context
 */
async function syncPromptsWithContext(claudePath, projectInfo, analysis, patterns) {
    const promptsPath = join(claudePath, 'prompts');
    // Create prompts directory if it doesn't exist
    if (!existsSync(promptsPath)) {
        await mkdir(promptsPath, { recursive: true });
        return 0;
    }
    const files = await readdir(promptsPath);
    let updated = 0;
    for (const file of files) {
        if (!file.endsWith('.md') || file === '_index.md')
            continue;
        const filePath = join(promptsPath, file);
        const content = await readFile(filePath, 'utf-8');
        // Generate new auto section based on prompt type
        const promptType = file.replace('.md', '');
        const autoSection = generateAutoSection(promptType, projectInfo, analysis, patterns);
        // Merge: replace auto section, preserve rest
        const merged = mergePromptContext(content, autoSection);
        if (merged !== content) {
            await writeFile(filePath, merged, 'utf-8');
            updated++;
        }
    }
    return updated;
}
//# sourceMappingURL=sync.js.map