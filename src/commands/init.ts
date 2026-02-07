import { existsSync } from 'fs';
import { rename, rm, cp } from 'fs/promises';
import { join, resolve } from 'path';
import chalk from 'chalk';
import ora from 'ora';
import inquirer from 'inquirer';

import { scanProject } from '../scanners/index.js';
import { scanCodeSamples } from '../scanners/samples.js';
import * as ollamaService from '../services/ollama.js';
import * as grepaiService from '../services/grepai.js';
import * as semanticAnalyzer from '../analyzers/semantic.js';
import * as llmAnalyzer from '../analyzers/llm.js';
import { generateAll } from '../generators/index.js';
import type { InitOptions, DependenciesCheck, ProjectInfo, Analysis, DiscoveredPatterns, Entity, CodeSamples } from '../types.js';

/**
 * Main init command
 */
export async function initCommand(options: InitOptions): Promise<void> {
  const projectPath = resolve(process.cwd());

  console.log(chalk.bold('\n🌿 Mustard CLI v2.0\n'));

  // Check for existing .claude directory
  const claudePath = join(projectPath, '.claude');
  const claudeMdPath = join(claudePath, 'CLAUDE.md');
  let overwriteClaudeMd = true;

  if (existsSync(claudePath) && !options.force) {
    if (options.yes) {
      // With --yes, preserve existing files (no backup, just update)
      console.log(chalk.gray('  .claude/ exists - updating files...'));
    } else {
      const handleExisting = await promptExistingClaude();
      if (handleExisting === 'cancel') {
        console.log(chalk.yellow('\n⚠️  Cancelled.\n'));
        return;
      }
      if (handleExisting === 'backup') {
        await backupExistingClaude(claudePath);
      }
      // 'overwrite' continues without backup
    }
  }

  // Check if CLAUDE.md already exists and prompt user
  if (existsSync(claudeMdPath) && !options.force) {
    if (options.yes) {
      // With --yes, default to NOT overwriting existing CLAUDE.md
      overwriteClaudeMd = false;
      console.log(chalk.yellow('⚠️  CLAUDE.md already exists - preserving existing file'));
    } else {
      overwriteClaudeMd = await promptOverwriteClaudeMd();
      if (!overwriteClaudeMd) {
        console.log(chalk.gray('  CLAUDE.md will be preserved'));
      }
    }
  }

  // Check dependencies
  const deps = await checkDependencies(options);

  // Phase 1: Basic scan
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

  // Display detected info
  displayProjectInfo(projectInfo);

  // Confirm with user
  const confirmed = await promptConfirmation(projectInfo, options.yes);
  if (!confirmed) {
    console.log(chalk.yellow('\n⚠️  Cancelled.\n'));
    return;
  }

  // Phase 2: Semantic analysis (if grepai available)
  let patterns: DiscoveredPatterns = { services: [], repositories: [], endpoints: [], components: [], hooks: [], entities: [], callGraph: null };
  if (deps.grepai && options.grepai !== false) {
    const semanticSpinner = ora('Analyzing codebase semantically (grepai)...').start();
    try {
      patterns = await semanticAnalyzer.discoverPatterns({
        stacks: projectInfo.stacks,
        verbose: options.verbose
      });

      // Discover entities
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

  // Phase 3: LLM analysis (if Ollama available)
  let analysis: Analysis = {
    architecture: projectInfo.structure?.architecture ?? { type: 'unknown', confidence: 'low' },
    patterns: [],
    rules: [],
    entities: patterns.entities
  };

  // Get code samples for context generation
  let codeSamples: CodeSamples = {};
  if (deps.grepai && options.grepai !== false) {
    try {
      codeSamples = await semanticAnalyzer.getCodeSamples(patterns);
    } catch {
      // Ignore - samples are optional
    }
  }

  // Fallback: scan filesystem for code samples if grepai didn't find any
  if (Object.keys(codeSamples).length === 0) {
    const sampleSpinner = ora('Scanning for code samples...').start();
    try {
      codeSamples = await scanCodeSamples(projectPath, projectInfo.stacks, { verbose: options.verbose });
      const found = Object.keys(codeSamples).length;
      if (found > 0) {
        sampleSpinner.succeed(`Found ${found} code sample${found > 1 ? 's' : ''}`);
      } else {
        sampleSpinner.warn('No code samples found');
      }
    } catch {
      sampleSpinner.warn('Code sample scan failed');
    }
  }

  if (deps.ollama && options.ollama === true) {
    const llmSpinner = ora('Analyzing code patterns (Ollama)...').start();
    try {
      const samplesList = Object.values(codeSamples).filter((s): s is NonNullable<typeof s> => Boolean(s));

      if (samplesList.length > 0) {
        const llmResult = await llmAnalyzer.analyzeCode(samplesList, {
          model: deps.ollamaModel ?? undefined
        });
        analysis = { ...analysis, ...llmResult };
        llmSpinner.succeed('Code patterns analyzed');
      } else {
        llmSpinner.warn('No code samples for analysis');
      }
    } catch (error: unknown) {
      llmSpinner.warn('LLM analysis limited');
      if (options.verbose) {
        const message = error instanceof Error ? error.message : 'Unknown error';
        console.log(chalk.gray(`  ${message}`));
      }
    }
  }

  // Phase 4: Generate files
  const genSpinner = ora('Generating .claude/ structure...').start();
  try {
    const files = await generateAll(projectPath, projectInfo, analysis, {
      useOllama: deps.ollama && options.ollama === true, // Only for CLAUDE.md
      model: deps.ollamaModel ?? undefined,
      hasGrepai: deps.grepai,
      verbose: options.verbose,
      overwriteClaudeMd,
      codeSamples
    });

    genSpinner.succeed(`Generated ${files.length} files`);

    // Display generated files
    console.log(chalk.gray('\n  Generated files:'));
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
    genSpinner.fail('Generation failed');
    const message = error instanceof Error ? error.message : 'Unknown error';
    console.error(chalk.red(message));
    if (options.verbose && error instanceof Error) {
      console.error(error.stack);
    }
    return;
  }

  // Done!
  console.log(chalk.green.bold('\n✅ Done!\n'));
  console.log(chalk.white('Use these commands to get started:'));
  console.log(chalk.cyan('  /mustard:feature <name>') + chalk.gray(' - Start a new feature'));
  console.log(chalk.cyan('  /mustard:bugfix <error>') + chalk.gray(' - Fix a bug'));
  console.log(chalk.cyan('  /mustard:status') + chalk.gray('         - Check project status'));
  console.log();
}

/**
 * Check available dependencies (Ollama, grepai)
 */
async function checkDependencies(options: InitOptions): Promise<DependenciesCheck> {
  const deps: DependenciesCheck = {
    ollama: false,
    ollamaModel: null,
    grepai: false
  };

  // Check Ollama (only if --ollama flag is passed)
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
      ollamaSpinner.warn('Ollama not available (using templates)');
    }
  }

  // Check grepai
  if (options.grepai !== false) {
    const grepaiSpinner = ora('Checking grepai...').start();
    const grepaiAvailable = await grepaiService.checkGrepaiAvailable();

    if (grepaiAvailable) {
      deps.grepai = true;
      grepaiSpinner.succeed('grepai: available');
    } else {
      grepaiSpinner.warn('grepai not available (limited semantic search)');
    }
  }

  return deps;
}

/**
 * Prompt user about existing .claude directory
 */
async function promptExistingClaude(): Promise<string> {
  const { action } = await inquirer.prompt<{ action: string }>([
    {
      type: 'list',
      name: 'action',
      message: '.claude/ directory already exists. What would you like to do?',
      choices: [
        { name: 'Backup and overwrite', value: 'backup' },
        { name: 'Overwrite without backup', value: 'overwrite' },
        { name: 'Cancel', value: 'cancel' }
      ]
    }
  ]);

  return action;
}

/**
 * Prompt user about overwriting existing CLAUDE.md
 */
async function promptOverwriteClaudeMd(): Promise<boolean> {
  const { overwrite } = await inquirer.prompt<{ overwrite: boolean }>([
    {
      type: 'confirm',
      name: 'overwrite',
      message: 'CLAUDE.md already exists. Do you want to overwrite it?',
      default: false
    }
  ]);

  return overwrite;
}

/**
 * Backup existing .claude directory
 */
async function backupExistingClaude(claudePath: string): Promise<void> {
  const timestamp = new Date().toISOString().replace(/[:.]/g, '-').slice(0, 19);
  const backupPath = `${claudePath}.backup.${timestamp}`;

  const spinner = ora('Creating backup...').start();
  try {
    // Try rename first (faster)
    await rename(claudePath, backupPath);
    spinner.succeed(`Backup created: ${backupPath}`);
  } catch (error: unknown) {
    // On Windows, rename can fail if files are locked
    // Fall back to copy + delete
    const err = error as NodeJS.ErrnoException;
    if (err.code === 'EPERM' || err.code === 'EBUSY') {
      try {
        await cp(claudePath, backupPath, { recursive: true });
        await rm(claudePath, { recursive: true, force: true });
        spinner.succeed(`Backup created: ${backupPath}`);
      } catch (copyError: unknown) {
        spinner.fail('Backup failed');
        throw copyError;
      }
    } else {
      spinner.fail('Backup failed');
      throw error;
    }
  }
}

/**
 * Display detected project info
 */
function displayProjectInfo(projectInfo: ProjectInfo): void {
  console.log(chalk.bold('\n📋 Detected:\n'));

  // Project type
  console.log(chalk.white(`  Type: ${chalk.cyan(projectInfo.type)}`));

  // Stacks
  console.log(chalk.white('  Stacks:'));
  for (const stack of projectInfo.stacks) {
    const version = stack.version ? ` ${stack.version}` : '';
    const path = stack.path !== '.' ? ` (${stack.path})` : '';
    console.log(chalk.gray(`    • ${stack.name}${version}${path}`));
  }

  // Architecture
  if (projectInfo.structure?.architecture?.type !== 'unknown') {
    console.log(chalk.white(`  Architecture: ${chalk.cyan(projectInfo.structure.architecture.type)}`));
  }

  // Package manager
  if (projectInfo.packageManager) {
    console.log(chalk.white(`  Package manager: ${chalk.cyan(projectInfo.packageManager)}`));
  }

  // Naming patterns
  if (projectInfo.patterns) {
    console.log(chalk.white('  Naming:'));
    console.log(chalk.gray(`    • Classes: ${projectInfo.patterns.classes}`));
    console.log(chalk.gray(`    • Folders: ${projectInfo.patterns.folders}`));
  }

  console.log();
}

/**
 * Prompt user for confirmation
 */
async function promptConfirmation(projectInfo: ProjectInfo, skipPrompt: boolean = false): Promise<boolean> {
  if (skipPrompt) {
    return true;
  }

  const { confirmed } = await inquirer.prompt<{ confirmed: boolean }>([
    {
      type: 'confirm',
      name: 'confirmed',
      message: 'Generate .claude/ structure with these settings?',
      default: true
    }
  ]);

  return confirmed;
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
