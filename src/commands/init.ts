import { existsSync, readdirSync, statSync } from 'fs';
import { mkdir, copyFile, rename, cp } from 'fs/promises';
import { join, resolve, dirname } from 'path';
import { fileURLToPath } from 'url';
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
  printNextSteps();
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
