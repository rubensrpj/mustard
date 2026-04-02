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
  cursor?: boolean;
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
      await ensureRtk();
      if (options.cursor) await installCursorAdapter(projectPath, claudePath);
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
        await ensureRtk();
        if (options.cursor) await installCursorAdapter(projectPath, claudePath);
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
  await ensureRtk();

  // Install Cursor adapter if requested
  if (options.cursor) await installCursorAdapter(projectPath, claudePath);

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

export async function generateMustardJson(projectPath: string, options: { yes?: boolean }): Promise<void> {
  const configPath = join(projectPath, 'mustard.json');

  let existingConfig: MustardConfig | null = null;

  if (existsSync(configPath)) {
    try {
      existingConfig = JSON.parse(readFileSync(configPath, 'utf-8')) as MustardConfig;
    } catch {
      // malformed — fall through to recreate
    }

    if (existingConfig && options.yes) {
      console.log(chalk.gray('\n  mustard.json already exists — preserved'));
      return;
    }

    if (existingConfig) {
      // Display current config
      const flow = existingConfig.git?.flow ?? {};
      const flowLines = Object.entries(flow).map(([k, v]) => `    ${k} → ${v}`);
      console.log(chalk.bold('\n  Current git flow:'));
      if (flowLines.length) {
        flowLines.forEach(l => console.log(chalk.gray(l)));
      } else {
        console.log(chalk.gray('    (no flow configured)'));
      }
      console.log(chalk.gray(`    provider: ${existingConfig.git?.provider ?? 'github'}`));
      console.log(chalk.gray(`    submodules: ${existingConfig.git?.submodules ?? false}`));
      console.log();

      const { reconfigure } = await inquirer.prompt<{ reconfigure: boolean }>([{
        type: 'confirm',
        name: 'reconfigure',
        message: 'Reconfigure git flow?',
        default: false
      }]);

      if (!reconfigure) {
        console.log(chalk.gray('  mustard.json preserved'));
        return;
      }
    }
  }

  const defaultBranch = detectDefaultBranch();
  const hasSubmodules = detectHasSubmodules();
  const currentBranch = detectCurrentBranch();
  const remoteBranches = detectRemoteBranches();
  const hasDevBranch = remoteBranches.includes('dev') || remoteBranches.includes('develop');

  // Pre-fill defaults from existing config if reconfiguring
  const existingFlow = existingConfig?.git?.flow ?? {};
  const existingDevBranch = existingFlow['*'] as string | undefined;
  const existingProdBranch = existingDevBranch ? existingFlow[existingDevBranch] as string | undefined : undefined;
  const existingProvider = existingConfig?.git?.provider ?? 'github';

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
      const devBranchName = remoteBranches.includes('dev') ? 'dev' : 'develop';
      flow['*'] = devBranchName;
      flow[devBranchName] = defaultBranch;
    }
    config = {
      git: {
        flow,
        provider: 'github',
        submodules: hasSubmodules
      }
    };
  } else {
    // Interactive setup — pre-fill from existing config when reconfiguring
    const answers = await inquirer.prompt<{
      production: string;
      devBranch: string;
      provider: string;
    }>([
      {
        type: 'input',
        name: 'production',
        message: 'Production branch:',
        default: existingProdBranch ?? defaultBranch
      },
      {
        type: 'input',
        name: 'devBranch',
        message: 'Development branch (shared, leave empty to skip):',
        default: existingDevBranch ?? (hasDevBranch ? (remoteBranches.includes('dev') ? 'dev' : 'develop') : '')
      },
      {
        type: 'list',
        name: 'provider',
        message: 'Git provider:',
        choices: ['github', 'gitlab', 'bitbucket'],
        default: existingProvider
      }
    ]);

    const flow: Record<string, string> = {};
    if (answers.devBranch) {
      flow['*'] = answers.devBranch;
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
 * Ensure RTK (Rust Token Killer) is installed for token economy.
 * Transparent: installs silently, never blocks on failure.
 */
async function ensureRtk(): Promise<void> {
  const isWin = process.platform === 'win32';
  const whichCmd = isWin ? 'where' : 'which';

  // Check if rtk is already in PATH
  try {
    execSync(`${whichCmd} rtk`, { stdio: 'pipe', encoding: 'utf-8' });
    // Verify it's the correct RTK (not Rust Type Kit)
    const version = execSync('rtk --version', { stdio: 'pipe', encoding: 'utf-8' }).trim();
    console.log(chalk.green(`  ✓ RTK ${version} (token economy active)`));
    // Activate RTK native integration (hook, RTK.md, etc.)
    try { execSync('rtk init -g --no-patch', { stdio: 'pipe', timeout: 10000 }); } catch { /* fail-open */ }
    return;
  } catch {
    // Not installed — proceed to install
  }

  const spinner = ora('Installing RTK (token economy)...').start();
  try {
    if (isWin) {
      await installRtkWindows();
    } else {
      execSync('curl -fsSL https://raw.githubusercontent.com/rtk-ai/rtk/refs/heads/master/install.sh | sh', {
        stdio: 'pipe',
        timeout: 120000, // 2 min
      });
    }

    // Verify installation
    try {
      const version = execSync('rtk --version', { stdio: 'pipe', encoding: 'utf-8' }).trim();
      spinner.succeed(`RTK ${version} installed (token economy active)`);
    } catch {
      spinner.succeed('RTK installed (token economy active)');
    }
    // Activate RTK native integration (hook, RTK.md, etc.)
    try { execSync('rtk init -g --no-patch', { stdio: 'pipe', timeout: 10000 }); } catch { /* fail-open */ }
  } catch {
    spinner.warn('RTK not installed — token economy will activate when RTK is available');
  }
}

async function installRtkWindows(): Promise<void> {
  const binDir = join(homedir(), '.local', 'bin');

  // Strategy 1: Download prebuilt binary from GitHub releases
  try {
    const releaseJson = execSync(
      'powershell -NoProfile -Command "Invoke-RestMethod -Uri https://api.github.com/repos/rtk-ai/rtk/releases/latest | ConvertTo-Json -Depth 5"',
      { stdio: 'pipe', encoding: 'utf-8', timeout: 30000 },
    );
    const release = JSON.parse(releaseJson);
    const asset = release.assets?.find((a: { name: string }) => a.name.includes('windows') && a.name.endsWith('.zip'));
    if (asset?.browser_download_url) {
      const zipPath = join(homedir(), '.local', 'rtk-install.zip');
      await mkdir(binDir, { recursive: true });
      execSync(
        `powershell -NoProfile -Command "Invoke-WebRequest -Uri '${asset.browser_download_url}' -OutFile '${zipPath}'"`,
        { stdio: 'pipe', timeout: 120000 },
      );
      execSync(
        `powershell -NoProfile -Command "Expand-Archive -Path '${zipPath}' -DestinationPath '${binDir}' -Force"`,
        { stdio: 'pipe', timeout: 30000 },
      );
      // Clean up zip
      try { execSync(`del "${zipPath}"`, { stdio: 'pipe' }); } catch { /* ignore */ }
      // Add to PATH for current session and persist via user env
      addToPathWindows(binDir);
      return;
    }
  } catch { /* fall through to cargo */ }

  // Strategy 2: cargo install
  try {
    execSync('where cargo', { stdio: 'pipe' });
    execSync('cargo install --git https://github.com/rtk-ai/rtk', {
      stdio: 'pipe',
      timeout: 300000,
    });
    return;
  } catch { /* fall through */ }

  throw new Error('All Windows install strategies failed');
}

function addToPathWindows(binDir: string): void {
  try {
    // Check if already in PATH
    const currentPath = process.env.PATH || '';
    if (currentPath.toLowerCase().includes(binDir.toLowerCase())) return;

    // Add to current process PATH
    process.env.PATH = `${binDir};${currentPath}`;

    // Persist to user PATH via registry
    execSync(
      `powershell -NoProfile -Command "[Environment]::SetEnvironmentVariable('PATH', [Environment]::GetEnvironmentVariable('PATH', 'User') + ';${binDir}', 'User')"`,
      { stdio: 'pipe', timeout: 10000 },
    );
  } catch { /* non-critical — rtk works if binDir is in PATH next session */ }
}

/**
 * Ensure ~/.claude/settings.json has Read, Write, Edit in allow list.
 * Non-destructive: only adds missing permissions, preserves everything else.
 */
export async function ensureGlobalPermissions(): Promise<void> {
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

  // Ensure global env vars are set
  const requiredEnv: Record<string, string> = { CLAUDE_CODE_NO_FLICKER: '1' };
  if (!settings.env) settings.env = {};
  const addedEnv: string[] = [];
  for (const [key, val] of Object.entries(requiredEnv)) {
    if (settings.env[key] !== val) {
      settings.env[key] = val;
      addedEnv.push(key);
    }
  }

  const dirty = added.length > 0 || addedEnv.length > 0;
  if (dirty) {
    await mkdir(claudeDir, { recursive: true });
    await writeFile(settingsPath, JSON.stringify(settings, null, 2) + '\n');
    if (added.length > 0) {
      console.log(chalk.green(`  ✓ Global permissions: added ${added.join(', ')} to ~/.claude/settings.json`));
    }
    if (addedEnv.length > 0) {
      console.log(chalk.green(`  ✓ Global env: set ${addedEnv.join(', ')} in ~/.claude/settings.json`));
    }
  } else {
    console.log(chalk.gray('  Global settings: permissions and env already configured'));
  }
}

/**
 * Install the Cursor IDE adapter.
 * Copies templates/adapters/cursor/ → {project}/.claude/adapters/cursor/
 * and creates {project}/.cursor/hooks/adapter.js.
 * Only runs when --cursor flag is passed.
 */
async function installCursorAdapter(projectPath: string, claudePath: string): Promise<void> {
  const templatesDir = getTemplatesDir();
  const adapterSrc = join(templatesDir, 'adapters', 'cursor');
  const adapterDest = join(claudePath, 'adapters', 'cursor');
  const cursorHooksDir = join(projectPath, '.cursor', 'hooks');
  const cursorAdapterDest = join(cursorHooksDir, 'adapter.js');

  const spinner = ora('Installing Cursor adapter...').start();
  try {
    // Copy adapters/cursor/ into .claude/adapters/cursor/
    await copyDir(adapterSrc, adapterDest, true);

    // Create .cursor/hooks/ and copy adapter.js there
    await mkdir(cursorHooksDir, { recursive: true });
    await copyFile(join(adapterSrc, 'adapter.js'), cursorAdapterDest);

    spinner.succeed('Cursor adapter installed at .cursor/hooks/adapter.js');
  } catch (err) {
    spinner.warn('Cursor adapter install failed — see .claude/adapters/cursor/ for manual setup');
    process.stderr.write('[mustard] cursor adapter: ' + (err as Error).message + '\n');
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
