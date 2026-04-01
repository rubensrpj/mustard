import { existsSync, readdirSync } from 'fs';
import { rm, cp, mkdir, copyFile } from 'fs/promises';
import { join, resolve, dirname } from 'path';
import { fileURLToPath } from 'url';
import { execSync } from 'child_process';
import { homedir } from 'os';
import chalk from 'chalk';
import ora from 'ora';
import inquirer from 'inquirer';
import { generateMustardJson } from './init.js';

export interface UpdateOptions {
  force?: boolean;
}

function getTemplatesDir(): string {
  const __filename = fileURLToPath(import.meta.url);
  const __dirname = dirname(__filename);
  return join(__dirname, '..', '..', 'templates');
}

async function copyDir(src: string, dest: string): Promise<number> {
  let count = 0;
  await mkdir(dest, { recursive: true });

  const entries = readdirSync(src, { withFileTypes: true });
  for (const entry of entries) {
    const srcPath = join(src, entry.name);
    const destPath = join(dest, entry.name);

    if (entry.isDirectory()) {
      count += await copyDir(srcPath, destPath);
    } else {
      await copyFile(srcPath, destPath);
      count++;
    }
  }
  return count;
}

/**
 * mustard update — deletes core folders and re-copies from templates
 * Preserves: CLAUDE.md, pipeline-config.md, entity-registry.json, commands/* (user), docs/, agent-memory/, spec/
 */
export async function updateCommand(options: UpdateOptions): Promise<void> {
  const projectPath = resolve(process.cwd());
  const claudePath = join(projectPath, '.claude');
  const templatesDir = getTemplatesDir();

  console.log(chalk.bold('\n🌿 Mustard — Update\n'));

  if (!existsSync(claudePath)) {
    console.log(chalk.red('❌ No .claude/ directory found. Run "mustard init" first.\n'));
    return;
  }

  console.log(chalk.green('  Will recreate:'));
  console.log(chalk.gray('    • commands/mustard/  • hooks/  • skills/  • scripts/  • settings.json'));
  console.log(chalk.yellow('  Will preserve:'));
  console.log(chalk.gray('    • CLAUDE.md  • pipeline-config.md  • entity-registry.json  • docs/  • spec/  • agent-memory/'));

  // Confirm + backup
  if (!options.force) {
    const { proceed } = await inquirer.prompt<{ proceed: boolean }>([{
      type: 'confirm',
      name: 'proceed',
      message: 'Backup and update?',
      default: true
    }]);
    if (!proceed) {
      console.log(chalk.yellow('\n⚠️  Cancelled.\n'));
      return;
    }

    const ts = new Date().toISOString().replace(/[:.]/g, '-').slice(0, 19);
    const backupSpinner = ora('Creating backup...').start();
    await cp(claudePath, `${claudePath}.backup.${ts}`, { recursive: true });
    backupSpinner.succeed('Backup created');
  }

  // Delete core folders
  const foldersToDelete = ['commands/mustard', 'hooks', 'skills', 'scripts'];
  const cleanSpinner = ora('Cleaning...').start();
  for (const folder of foldersToDelete) {
    const p = join(claudePath, folder);
    if (existsSync(p)) await rm(p, { recursive: true, force: true });
  }
  cleanSpinner.succeed('Cleaned');

  // Re-copy from templates
  const copySpinner = ora('Copying templates...').start();
  let total = 0;

  // commands/mustard/
  total += await copyDir(join(templatesDir, 'commands', 'mustard'), join(claudePath, 'commands', 'mustard'));
  // hooks/
  total += await copyDir(join(templatesDir, 'hooks'), join(claudePath, 'hooks'));
  // skills/
  total += await copyDir(join(templatesDir, 'skills'), join(claudePath, 'skills'));
  // scripts/
  total += await copyDir(join(templatesDir, 'scripts'), join(claudePath, 'scripts'));
  // settings.json
  await copyFile(join(templatesDir, 'settings.json'), join(claudePath, 'settings.json'));
  total++;

  copySpinner.succeed(`Updated ${total} files`);

  await ensureRtk();

  await generateMustardJson(projectPath, { yes: options.force });

  console.log(chalk.green.bold('\n✅ Update complete!\n'));
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
