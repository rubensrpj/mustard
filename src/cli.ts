import { Command } from 'commander';
import { createRequire } from 'node:module';
import { initCommand } from './commands/init.js';
import { updateCommand } from './commands/update.js';
import { configCommand } from './commands/config.js';
import { autoUpdateCommand } from './commands/auto-update.js';
import { addCommand } from './commands/add.js';
import { reviewCommand } from './commands/review.js';

const require = createRequire(import.meta.url);
const { version } = require('../package.json');

export function run(): void {
  const program = new Command();

  program
    .name('mustard')
    .description('Framework-agnostic CLI for Claude Code project setup')
    .version(version);

  program
    .command('init')
    .description('Copy .claude/ structure into the current project')
    .option('-f, --force', 'Overwrite existing .claude/ directory without backup')
    .option('-y, --yes', 'Skip confirmation prompts')
    .option('--cursor', 'Install Cursor IDE adapter at .cursor/hooks/adapter.js (experimental)')
    .action(initCommand);

  program
    .command('update')
    .description('Update Mustard core files (preserves user customizations)')
    .option('-f, --force', 'Skip backup and confirmation')
    .action(updateCommand);

  program
    .command('config')
    .description('Configure or reconfigure mustard.json (git flow)')
    .option('-y, --yes', 'Accept defaults without prompting')
    .action(configCommand);

  program
    .command('auto-update')
    .description('Check for updates and install latest version from npm')
    .option('--check-only', 'Only check for updates, do not install')
    .option('-y, --yes', 'Skip confirmation prompts')
    .action(autoUpdateCommand);

  program
    .command('add <template>')
    .description('Install a community template (e.g., mustard add template:dotnet-clean-arch)')
    .option('-f, --force', 'Overwrite existing files')
    .action(addCommand);

  program
    .command('review')
    .description('Review a pull request (local or CI mode)')
    .option('--ci', 'CI mode: post review as PR comment, exit 1 on critical issues')
    .option('--pr <number>', 'PR number to review')
    .action(reviewCommand);

  program.parse();
}
