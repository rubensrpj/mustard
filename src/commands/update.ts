import { existsSync } from 'fs';
import { rename, rm, cp, readFile, writeFile } from 'fs/promises';
import { join, resolve } from 'path';
import chalk from 'chalk';
import ora from 'ora';
import inquirer from 'inquirer';

import { scanProject } from '../scanners/index.js';
import * as ollamaService from '../services/ollama.js';
import * as grepaiService from '../services/grepai.js';
import * as semanticAnalyzer from '../analyzers/semantic.js';
import { generateCoreOnly } from '../generators/index.js';
import { MUSTARD_COMMANDS_FOLDER } from '../generators/commands.js';
import type { InitOptions, DependenciesCheck, ProjectInfo, Analysis, DiscoveredPatterns } from '../types.js';

export interface UpdateOptions extends InitOptions {
  includeClaudeMd?: boolean;
}

/**
 * Update command - updates Mustard core files while preserving client customizations
 */
export async function updateCommand(options: UpdateOptions): Promise<void> {
  const projectPath = resolve(process.cwd());
  const claudePath = join(projectPath, '.claude');

  console.log(chalk.bold('\n🌿 Mustard CLI v2.0 - Update\n'));

  // Check if .claude/ exists
  if (!existsSync(claudePath)) {
    console.log(chalk.red('❌ No .claude/ directory found.'));
    console.log(chalk.gray('   Run "mustard init" first to initialize the project.\n'));
    return;
  }

  // Show what will be updated vs preserved
  console.log(chalk.white('📋 Update plan:\n'));
  console.log(chalk.green('  ✓ Will update (core files):'));
  console.log(chalk.gray('    • commands/mustard/*.md (Mustard commands)'));
  console.log(chalk.gray('    • hooks/*.js'));
  console.log(chalk.gray('    • core/*.md'));
  console.log(chalk.gray('    • scripts/*.js'));
  console.log(chalk.gray('    • context/README.md'));
  console.log(chalk.gray('    • entity-registry.json'));
  console.log(chalk.gray('    • settings.json (merged)'));

  console.log(chalk.yellow('\n  ⚡ Will preserve (client files):'));
  console.log(chalk.gray('    • CLAUDE.md'));
  console.log(chalk.gray('    • commands/*.md (user commands)'));
  console.log(chalk.gray('    • prompts/*.md'));
  console.log(chalk.gray('    • context/*.md (user files)'));
  console.log(chalk.gray('    • docs/*'));

  if (options.includeClaudeMd) {
    console.log(chalk.cyan('\n  ℹ️  --include-claude-md: CLAUDE.md will also be updated'));
  }

  // Create backup unless --force
  if (!options.force) {
    const { proceed } = await inquirer.prompt<{ proceed: boolean }>([
      {
        type: 'confirm',
        name: 'proceed',
        message: 'Create backup and proceed with update?',
        default: true
      }
    ]);

    if (!proceed) {
      console.log(chalk.yellow('\n⚠️  Cancelled.\n'));
      return;
    }

    await backupExistingClaude(claudePath);
  }

  // Check dependencies
  const deps = await checkDependencies(options);

  // Scan project
  const scanSpinner = ora('Scanning project...').start();
  let projectInfo: ProjectInfo;
  try {
    projectInfo = await scanProject(projectPath, { verbose: options.verbose });
    scanSpinner.succeed('Project scanned');
  } catch (error: unknown) {
    scanSpinner.fail('Scan failed');
    const message = error instanceof Error ? error.message : 'Unknown error';
    console.error(chalk.red(message));
    return;
  }

  // Semantic analysis (if grepai available)
  let patterns: DiscoveredPatterns = {
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
    } catch (error: unknown) {
      semanticSpinner.warn('Semantic analysis limited');
      if (options.verbose) {
        const message = error instanceof Error ? error.message : 'Unknown error';
        console.log(chalk.gray(`  ${message}`));
      }
    }
  }

  // Build analysis object
  const analysis: Analysis = {
    architecture: projectInfo.structure?.architecture ?? { type: 'unknown', confidence: 'low' },
    patterns: [],
    rules: [],
    entities: patterns.entities
  };

  // Clean mustard commands folder before regenerating
  // This ensures old Mustard commands are removed while preserving user commands in commands/
  const mustardCommandsPath = join(claudePath, 'commands', MUSTARD_COMMANDS_FOLDER);
  if (existsSync(mustardCommandsPath)) {
    await rm(mustardCommandsPath, { recursive: true, force: true });
  }

  // Generate core files only
  const genSpinner = ora('Updating core files...').start();
  try {
    const files = await generateCoreOnly(projectPath, projectInfo, analysis, {
      useOllama: deps.ollama && options.ollama === true,
      model: deps.ollamaModel ?? undefined,
      hasGrepai: deps.grepai,
      verbose: options.verbose,
      overwriteClaudeMd: options.includeClaudeMd ?? false
    });

    genSpinner.succeed(`Updated ${files.length} files`);

    // Display updated files
    console.log(chalk.gray('\n  Updated files:'));
    const grouped = groupFiles(files);
    for (const [dir, dirFiles] of Object.entries(grouped)) {
      if (dir === '') {
        for (const file of dirFiles) {
          console.log(chalk.gray(`    .claude/${file}`));
        }
      } else {
        console.log(chalk.gray(`    .claude/${dir}/`));
        for (const file of dirFiles) {
          console.log(chalk.gray(`      ${file}`));
        }
      }
    }
  } catch (error: unknown) {
    genSpinner.fail('Update failed');
    const message = error instanceof Error ? error.message : 'Unknown error';
    console.error(chalk.red(message));
    if (options.verbose && error instanceof Error) {
      console.error(error.stack);
    }
    return;
  }

  // Done!
  console.log(chalk.green.bold('\n✅ Update complete!\n'));
  console.log(chalk.gray('  Your customizations in CLAUDE.md, context/, and commands/ were preserved.'));
  console.log(chalk.gray('  Mustard commands are in commands/mustard/ (safe to overwrite on updates).'));
  console.log(chalk.gray('  A backup was created in case you need to restore.\n'));
}

/**
 * Check available dependencies
 */
async function checkDependencies(options: UpdateOptions): Promise<DependenciesCheck> {
  const deps: DependenciesCheck = {
    ollama: false,
    ollamaModel: null,
    grepai: false
  };

  if (options.ollama === true) {
    const ollamaSpinner = ora('Checking Ollama...').start();
    const ollamaAvailable = await ollamaService.checkOllamaAvailable();

    if (ollamaAvailable) {
      const models = await ollamaService.getAvailableModels();
      if (models.length > 0) {
        deps.ollama = true;
        deps.ollamaModel = await ollamaService.selectBestModel(models);
        ollamaSpinner.succeed(`Ollama: ${deps.ollamaModel}`);
      } else {
        ollamaSpinner.warn('Ollama running but no models');
      }
    } else {
      ollamaSpinner.warn('Ollama not available');
    }
  }

  if (options.grepai !== false) {
    const grepaiSpinner = ora('Checking grepai...').start();
    const grepaiAvailable = await grepaiService.checkGrepaiAvailable();

    if (grepaiAvailable) {
      deps.grepai = true;
      grepaiSpinner.succeed('grepai: available');
    } else {
      grepaiSpinner.warn('grepai not available');
    }
  }

  return deps;
}

/**
 * Backup existing .claude directory
 */
async function backupExistingClaude(claudePath: string): Promise<void> {
  const timestamp = new Date().toISOString().replace(/[:.]/g, '-').slice(0, 19);
  const backupPath = `${claudePath}.backup.${timestamp}`;

  const spinner = ora('Creating backup...').start();
  try {
    await cp(claudePath, backupPath, { recursive: true });
    spinner.succeed(`Backup: ${backupPath}`);
  } catch (error: unknown) {
    spinner.fail('Backup failed');
    throw error;
  }
}

/**
 * Group files by directory for display
 */
function groupFiles(files: string[]): Record<string, string[]> {
  const grouped: Record<string, string[]> = {};

  for (const file of files) {
    const parts = file.split('/');
    if (parts.length === 1) {
      grouped[''] = grouped[''] ?? [];
      grouped['']!.push(file);
    } else {
      const dir = parts[0]!;
      const name = parts.slice(1).join('/');
      grouped[dir] = grouped[dir] ?? [];
      grouped[dir]!.push(name);
    }
  }

  return grouped;
}
