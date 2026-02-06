import { Command } from 'commander';
import { initCommand } from './commands/init.js';
import { updateCommand } from './commands/update.js';
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
        .description('Update Mustard core files (preserves client customizations)')
        .option('-f, --force', 'Skip backup and confirmation')
        .option('--ollama', 'Use Ollama for personalized generation (slower)')
        .option('--no-grepai', 'Skip grepai semantic search')
        .option('-v, --verbose', 'Show detailed output')
        .option('--include-claude-md', 'Also update CLAUDE.md (normally preserved)')
        .action(updateCommand);
    program.parse();
}
//# sourceMappingURL=cli.js.map