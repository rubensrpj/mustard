import { existsSync, readdirSync } from 'fs';
import { rm, cp, mkdir, copyFile } from 'fs/promises';
import { join, resolve, dirname } from 'path';
import { fileURLToPath } from 'url';
import chalk from 'chalk';
import ora from 'ora';
import inquirer from 'inquirer';

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

  console.log(chalk.green.bold('\n✅ Update complete!\n'));
}
