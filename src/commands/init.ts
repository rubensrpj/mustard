import { existsSync, readdirSync, statSync, readFileSync } from 'fs';
import { mkdir, copyFile, rename, cp, writeFile } from 'fs/promises';
import { join, resolve, dirname, sep } from 'path';
import { fileURLToPath } from 'url';
import { execSync } from 'child_process';
import { homedir } from 'os';
import chalk from 'chalk';
import ora from 'ora';
import inquirer from 'inquirer';

export interface InitOptions {
  force?: boolean;
  yes?: boolean;
}

function getTemplatesDir(): string {
  const __filename = fileURLToPath(import.meta.url);
  const __dirname = dirname(__filename);
  return join(__dirname, '..', '..', 'templates');
}

/**
 * Recursively copy a directory. If overwrite=false, skip existing files.
 */
async function copyDir(src: string, dest: string, overwrite = true): Promise<number> {
  let count = 0;
  await mkdir(dest, { recursive: true });

  const entries = readdirSync(src, { withFileTypes: true });
  for (const entry of entries) {
    const srcPath = join(src, entry.name);
    const destPath = join(dest, entry.name);

    if (entry.isDirectory()) {
      count += await copyDir(srcPath, destPath, overwrite);
    } else {
      if (overwrite || !existsSync(destPath)) {
        await copyFile(srcPath, destPath);
        count++;
      }
    }
  }
  return count;
}

/**
 * mustard init — copies templates/ → .claude/
 */
export async function initCommand(options: InitOptions): Promise<void> {
  const projectPath = resolve(process.cwd());
  const claudePath = join(projectPath, '.claude');
  const templatesDir = getTemplatesDir();

  console.log(chalk.bold('\n🌿 Mustard\n'));

  // Handle existing .claude/
  if (existsSync(claudePath)) {
    if (options.force) {
      // Force: overwrite without backup
    } else if (options.yes) {
      console.log(chalk.gray('  .claude/ exists — updating without overwriting user files'));
      const spinner = ora('Copying templates...').start();
      const count = await copyDir(templatesDir, claudePath, false);
      spinner.succeed(`Copied ${count} new files (existing files preserved)`);
      await ensureGlobalPermissions();
      printNextSteps();
      return;
    } else {
      const { action } = await inquirer.prompt<{ action: string }>([{
        type: 'list',
        name: 'action',
        message: '.claude/ already exists.',
        choices: [
          { name: 'Backup and overwrite', value: 'backup' },
          { name: 'Merge (skip existing files)', value: 'merge' },
          { name: 'Cancel', value: 'cancel' }
        ]
      }]);

      if (action === 'cancel') {
        console.log(chalk.yellow('\n⚠️  Cancelled.\n'));
        return;
      }

      if (action === 'backup') {
        const ts = new Date().toISOString().replace(/[:.]/g, '-').slice(0, 19);
        const backupPath = `${claudePath}.backup.${ts}`;
        const backupSpinner = ora('Creating backup...').start();
        try {
          await cp(claudePath, backupPath, { recursive: true });
          backupSpinner.succeed(`Backup: ${backupPath}`);
        } catch {
          backupSpinner.fail('Backup failed');
          return;
        }
      }

      if (action === 'merge') {
        const spinner = ora('Merging templates...').start();
        const count = await copyDir(templatesDir, claudePath, false);
        spinner.succeed(`Copied ${count} new files (existing files preserved)`);
        await ensureGlobalPermissions();
        printNextSteps();
        return;
      }
    }
  }

  // Fresh copy
  const spinner = ora('Copying .claude/ structure...').start();
  const count = await copyDir(templatesDir, claudePath, true);

  // Create empty entity-registry.json
  const registryPath = join(claudePath, 'entity-registry.json');
  if (!existsSync(registryPath)) {
    const { writeFile } = await import('fs/promises');
    await writeFile(registryPath, JSON.stringify({ _patterns: {}, _enums: {}, e: {} }, null, 2));
    count; // already counted
  }

  spinner.succeed(`Copied ${count} files to .claude/`);

  // Ensure global permissions for Claude Code (Read, Write, Edit)
  await ensureGlobalPermissions();

  // Generate mustard.json (git flow config)
  await generateMustardJson(projectPath, options);

  printNextSteps();
}

function detectDefaultBranch(): string {
  try {
    const ref = execSync('git symbolic-ref refs/remotes/origin/HEAD 2>/dev/null', { encoding: 'utf-8' }).trim();
    return ref.replace('refs/remotes/origin/', '');
  } catch {
    try {
      // Fallback: check if main or master exists
      const branches = execSync('git branch -r', { encoding: 'utf-8' });
      if (branches.includes('origin/main')) return 'main';
      if (branches.includes('origin/master')) return 'master';
    } catch { /* not a git repo */ }
    return 'main';
  }
}

function detectHasSubmodules(): boolean {
  try {
    return existsSync(join(process.cwd(), '.gitmodules'));
  } catch {
    return false;
  }
}

function detectCurrentBranch(): string | null {
  try {
    return execSync('git rev-parse --abbrev-ref HEAD', { encoding: 'utf-8' }).trim();
  } catch {
    return null;
  }
}

function detectRemoteBranches(): string[] {
  try {
    const output = execSync('git branch -r --format="%(refname:short)"', { encoding: 'utf-8' });
    return output.split('\n').filter(Boolean).map(b => b.replace('origin/', ''));
  } catch {
    return [];
  }
}

interface MustardConfig {
  git: {
    flow: Record<string, string>;
    provider: string;
    submodules: boolean;
  };
}

async function generateMustardJson(projectPath: string, options: InitOptions): Promise<void> {
  const configPath = join(projectPath, 'mustard.json');

  if (existsSync(configPath)) {
    console.log(chalk.gray('\n  mustard.json already exists — preserved'));
    return;
  }

  const defaultBranch = detectDefaultBranch();
  const hasSubmodules = detectHasSubmodules();
  const currentBranch = detectCurrentBranch();
  const remoteBranches = detectRemoteBranches();
  const hasDevBranch = remoteBranches.includes('dev') || remoteBranches.includes('develop');

  console.log(chalk.bold('\n📋 Git Flow Configuration\n'));

  if (currentBranch) {
    console.log(chalk.gray(`  Detected: branch=${currentBranch}, default=${defaultBranch}, submodules=${hasSubmodules}`));
    if (hasDevBranch) console.log(chalk.gray(`  Found dev branch: ${remoteBranches.includes('dev') ? 'dev' : 'develop'}`));
    console.log();
  }

  let config: MustardConfig;

  if (options.yes) {
    // Auto-config with sensible defaults
    const flow: Record<string, string> = {};
    if (hasDevBranch) {
      flow['dev_*'] = remoteBranches.includes('dev') ? 'dev' : 'develop';
      flow[remoteBranches.includes('dev') ? 'dev' : 'develop'] = defaultBranch;
    }
    config = {
      git: {
        flow,
        provider: 'github',
        submodules: hasSubmodules
      }
    };
  } else {
    // Interactive setup
    const answers = await inquirer.prompt<{
      production: string;
      devBranch: string;
      devPattern: string;
      provider: string;
    }>([
      {
        type: 'input',
        name: 'production',
        message: 'Production branch:',
        default: defaultBranch
      },
      {
        type: 'input',
        name: 'devBranch',
        message: 'Development branch (shared, leave empty to skip):',
        default: hasDevBranch ? (remoteBranches.includes('dev') ? 'dev' : 'develop') : ''
      },
      {
        type: 'input',
        name: 'devPattern',
        message: 'Personal branch pattern (glob → dev branch):',
        default: 'dev_*',
        when: (a) => !!a.devBranch
      },
      {
        type: 'list',
        name: 'provider',
        message: 'Git provider:',
        choices: ['github', 'gitlab', 'bitbucket'],
        default: 'github'
      }
    ]);

    const flow: Record<string, string> = {};
    if (answers.devBranch) {
      if (answers.devPattern) {
        flow[answers.devPattern] = answers.devBranch;
      }
      flow[answers.devBranch] = answers.production;
    }

    config = {
      git: {
        flow,
        provider: answers.provider || 'github',
        submodules: hasSubmodules
      }
    };
  }

  const spinner = ora('Writing mustard.json...').start();
  await writeFile(configPath, JSON.stringify(config, null, 2) + '\n');
  spinner.succeed('Created mustard.json');
}

/**
 * Ensure ~/.claude/settings.json has Read, Write, Edit in allow list.
 * Non-destructive: only adds missing permissions, preserves everything else.
 */
async function ensureGlobalPermissions(): Promise<void> {
  const claudeDir = join(homedir(), '.claude');
  const settingsPath = join(claudeDir, 'settings.json');
  const requiredPerms = ['Read', 'Write', 'Edit'];

  let settings: Record<string, any> = {};

  if (existsSync(settingsPath)) {
    try {
      settings = JSON.parse(readFileSync(settingsPath, 'utf-8'));
    } catch {
      // Malformed JSON — start fresh but warn
      console.log(chalk.yellow('  ⚠️  ~/.claude/settings.json is malformed — recreating permissions section'));
    }
  }

  if (!settings.permissions) settings.permissions = {};
  if (!Array.isArray(settings.permissions.allow)) settings.permissions.allow = [];

  const allow: string[] = settings.permissions.allow;
  const added: string[] = [];

  for (const perm of requiredPerms) {
    // Check if already has a generic allow (e.g. "Write") or path-specific (e.g. "Write(//c/**)")
    const hasGeneric = allow.includes(perm);
    if (!hasGeneric) {
      // Remove path-specific versions since we're adding the generic one
      const filtered = allow.filter(p => !p.startsWith(`${perm}(`));
      settings.permissions.allow = filtered;
      settings.permissions.allow.push(perm);
      added.push(perm);
    }
  }

  if (added.length > 0) {
    await mkdir(claudeDir, { recursive: true });
    await writeFile(settingsPath, JSON.stringify(settings, null, 2) + '\n');
    console.log(chalk.green(`  ✓ Global permissions: added ${added.join(', ')} to ~/.claude/settings.json`));
  } else {
    console.log(chalk.gray('  Global permissions: Read, Write, Edit already configured'));
  }
}

function printNextSteps(): void {
  console.log(chalk.green.bold('\n✅ Done!\n'));
  console.log(chalk.white('Next: open Claude Code and run /scan to analyze your codebase.\n'));
  console.log(chalk.white('Commands available after /scan:'));
  console.log(chalk.cyan('  /feature <name>') + chalk.gray(' — Start a feature pipeline'));
  console.log(chalk.cyan('  /bugfix <error>') + chalk.gray('  — Fix a bug'));
  console.log(chalk.cyan('  /status') + chalk.gray('          — Check project status'));
  console.log();
}
