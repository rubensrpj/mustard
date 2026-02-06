import { Command } from 'commander';
import { initCommand } from './commands/init.js';
import { updateCommand } from './commands/update.js';
import { autoUpdateCommand } from './commands/auto-update.js';
export function run() {
    const program = new Command();
    program
        .name('mustard')
        .description('Framework-agnostic CLI for Claude Code project setup')
        .version('2.0.0');
    program
        .command('init')
        .description('Initialize .claude/ structure for the current project')
        .option('-f, --force', 'Overwrite existing .claude/ directory')
        .option('-y, --yes', 'Skip confirmation prompts')
        .option('--ollama', 'Use Ollama for personalized generation (slower)')
        .option('--no-grepai', 'Skip grepai semantic search')
        .option('-v, --verbose', 'Show detailed output')
        .action(initCommand);
    program
        .command('update')
        .description('Update Mustard core files (recreates prompts, commands/mustard, hooks, skills, scripts, settings.json)')
        .option('-f, --force', 'Skip backup and confirmation')
        .option('--ollama', 'Use Ollama for personalized generation (slower)')
        .option('--no-grepai', 'Skip grepai semantic search')
        .option('-v, --verbose', 'Show detailed output')
        .option('--include-claude-md', 'Also update CLAUDE.md (normally preserved)')
        .action(updateCommand);
    program
        .command('auto-update')
        .description('Check for updates and install latest version from npm')
        .option('--check-only', 'Only check for updates, do not install')
        .option('-y, --yes', 'Skip confirmation prompts')
        .action(autoUpdateCommand);
    program.parse();
}
//# sourceMappingURL=cli.js.map