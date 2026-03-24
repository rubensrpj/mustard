import { Command } from 'commander';
import { initCommand } from './commands/init.js';
import { updateCommand } from './commands/update.js';
import { autoUpdateCommand } from './commands/auto-update.js';

export function run(): void {
  const program = new Command();

  program
    .name('mustard')
    .description('Framework-agnostic CLI for Claude Code project setup')
    .version('3.0.0');

  program
    .command('init')
    .description('Copy .claude/ structure into the current project')
    .option('-f, --force', 'Overwrite existing .claude/ directory without backup')
    .option('-y, --yes', 'Skip confirmation prompts')
    .action(initCommand);

  program
    .command('update')
    .description('Update Mustard core files (preserves user customizations)')
    .option('-f, --force', 'Skip backup and confirmation')
    .action(updateCommand);

  program
    .command('auto-update')
    .description('Check for updates and install latest version from npm')
    .option('--check-only', 'Only check for updates, do not install')
    .option('-y, --yes', 'Skip confirmation prompts')
    .action(autoUpdateCommand);

  program.parse();
}
