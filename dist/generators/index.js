import { existsSync } from 'fs';
import { mkdir, writeFile, copyFile, readFile } from 'fs/promises';
import { join, dirname } from 'path';
import { fileURLToPath } from 'url';
import { generateClaudeMd as generateClaudeMdLLM } from './claude-md-llm.js';
import { generateClaudeMd as generateClaudeMdTemplate } from './claude-md-template.js';
import { generatePrompts } from './prompts.js';
import { generateCommands, MUSTARD_COMMANDS_FOLDER } from './commands.js';
import { generateHooks } from './hooks.js';
import { generateRegistry } from './registry.js';
import { generateContext } from './context.js';
/**
 * Deep merge two objects, with source taking priority
 */
function deepMerge(target, source) {
    const result = { ...target };
    for (const key of Object.keys(source)) {
        const sourceValue = source[key];
        const targetValue = target[key];
        if (sourceValue !== null &&
            typeof sourceValue === 'object' &&
            !Array.isArray(sourceValue) &&
            targetValue !== null &&
            typeof targetValue === 'object' &&
            !Array.isArray(targetValue)) {
            result[key] = deepMerge(targetValue, sourceValue);
        }
        else if (sourceValue !== undefined) {
            result[key] = sourceValue;
        }
    }
    return result;
}
/**
 * Main generator orchestrator
 */
export async function generateAll(projectPath, projectInfo, analysis, options = {}) {
    const { useOllama = true, model, verbose = false, overwriteClaudeMd = true, codeSamples = {} } = options;
    const log = (msg) => { if (verbose)
        console.log(`  ${msg}`); };
    const claudePath = join(projectPath, '.claude');
    const generatedFiles = [];
    // Create .claude directory structure
    await mkdir(join(claudePath, 'prompts'), { recursive: true });
    await mkdir(join(claudePath, 'commands', MUSTARD_COMMANDS_FOLDER), { recursive: true });
    await mkdir(join(claudePath, 'hooks'), { recursive: true });
    await mkdir(join(claudePath, 'core'), { recursive: true });
    await mkdir(join(claudePath, 'docs'), { recursive: true });
    await mkdir(join(claudePath, 'context'), { recursive: true });
    // Generate CLAUDE.md (only if allowed or doesn't exist)
    const claudeMdPath = join(claudePath, 'CLAUDE.md');
    const claudeMdExists = existsSync(claudeMdPath);
    log('Generating CLAUDE.md...');
    if (!claudeMdExists || overwriteClaudeMd) {
        let claudeMdContent = null;
        if (useOllama) {
            claudeMdContent = await generateClaudeMdLLM(projectInfo, analysis, { model, verbose });
        }
        if (!claudeMdContent) {
            claudeMdContent = generateClaudeMdTemplate(projectInfo, analysis);
        }
        await writeFile(claudeMdPath, claudeMdContent);
        generatedFiles.push('CLAUDE.md');
    }
    // Generate prompts
    log('Generating prompts (may use Ollama)...');
    const prompts = await generatePrompts(projectInfo, analysis, { useOllama, model });
    for (const [name, content] of Object.entries(prompts)) {
        await writeFile(join(claudePath, 'prompts', `${name}.md`), content);
        generatedFiles.push(`prompts/${name}.md`);
    }
    // Generate prompts index
    const promptsIndex = generatePromptsIndex(Object.keys(prompts));
    await writeFile(join(claudePath, 'prompts', '_index.md'), promptsIndex);
    generatedFiles.push('prompts/_index.md');
    // Generate commands (in mustard/ subfolder)
    log('Generating commands...');
    const commands = generateCommands(projectInfo);
    for (const [name, content] of Object.entries(commands)) {
        await writeFile(join(claudePath, 'commands', MUSTARD_COMMANDS_FOLDER, `${name}.md`), content);
        generatedFiles.push(`commands/${MUSTARD_COMMANDS_FOLDER}/${name}.md`);
    }
    // Generate hooks
    log('Generating hooks...');
    const hooks = generateHooks(projectInfo, options);
    for (const [name, content] of Object.entries(hooks)) {
        await writeFile(join(claudePath, 'hooks', name), content);
        generatedFiles.push(`hooks/${name}`);
    }
    // Generate entity registry
    log('Generating entity registry...');
    const registry = generateRegistry(projectInfo, analysis);
    await writeFile(join(claudePath, 'entity-registry.json'), JSON.stringify(registry, null, 2));
    generatedFiles.push('entity-registry.json');
    // Generate core files (enforcement, pipeline - NOT naming-conventions)
    log('Generating core files...');
    await generateCoreFiles(claudePath, projectInfo);
    generatedFiles.push('core/enforcement.md', 'core/pipeline.md');
    // Generate context folder with README
    await generateContextFolder(claudePath);
    generatedFiles.push('context/README.md');
    // Copy settings.json from template
    log('Copying settings.json...');
    await copySettingsJson(claudePath);
    generatedFiles.push('settings.json');
    // Copy scripts folder
    log('Copying scripts...');
    await copyScripts(claudePath);
    generatedFiles.push('scripts/statusline.js');
    // Generate auto-populated context files (architecture.md, patterns.md, naming.md)
    log('Generating context files (may use Ollama)...');
    const contextFiles = await generateContext(claudePath, projectInfo, analysis, codeSamples, {
        useOllama,
        model,
        verbose
    });
    generatedFiles.push(...contextFiles);
    return generatedFiles;
}
/**
 * Generate prompts index file
 */
function generatePromptsIndex(promptNames) {
    return `# Prompts Index

This directory contains specialized prompts for agents.

## Available Prompts

${promptNames.map(name => `- [${name}.md](./${name}.md)`).join('\n')}

## How to Use

Prompts are automatically loaded by the pipeline when needed.
To delegate tasks, use:

\`\`\`javascript
Task({
  subagent_type: "general-purpose",
  model: "opus",
  prompt: \`
    [appropriate prompt content]

    # TASK
    [task description]
  \`
})
\`\`\`
`;
}
/**
 * Generate context folder with README
 */
async function generateContextFolder(claudePath) {
    const contextReadme = `# Project Context

Place markdown files here to provide context to Claude during implementations.

## Purpose

Files in this folder are loaded into memory MCP at the start of \`/feature\` or \`/bugfix\` pipelines.
This gives Claude instant access to project specifications, architecture decisions, and patterns.

## Supported Files

Any \`.md\` file placed in this folder will be automatically loaded.

**Suggested files:**
- \`project-spec.md\` - Project overview and specifications
- \`architecture.md\` - Architecture decisions and patterns
- \`business-rules.md\` - Domain-specific rules and logic
- \`api-guidelines.md\` - API design guidelines
- \`tips.md\` - Project-specific tips for Claude
- \`service-example.md\` - Code example for services
- \`component-example.md\` - Code example for components

## Rules

1. **Markdown only** - Only \`.md\` files are loaded
2. **Keep files focused** - One topic per file
3. **Use headers** - Claude uses headers to understand structure
4. **Max 500 lines** - Longer files are truncated
5. **Max 20 files** - Total limit for loaded files

## How It Works

Files are automatically loaded at the start of \`/feature\` or \`/bugfix\` pipelines.
Each file is stored as a \`UserContext:{filename}\` entity in memory MCP.

## Example: architecture.md

\`\`\`markdown
# Architecture

## Layers
- Database: Drizzle ORM with PostgreSQL
- Backend: .NET 9 with FastEndpoints
- Frontend: React 19 with TanStack Query

## Patterns
- Repository pattern for data access
- Services for business logic
- DTOs for API contracts
\`\`\`

## Manual Refresh

To force a context refresh, use:

\`\`\`
/sync-context --refresh
\`\`\`

## See Also

- [/sync-context](../commands/mustard/sync-context.md) - Manual context loading
- [/feature](../commands/mustard/feature.md) - Feature pipeline
- [pipeline.md](../core/pipeline.md) - Pipeline documentation
`;
    await writeFile(join(claudePath, 'context', 'README.md'), contextReadme);
    // Create .gitkeep for empty folder preservation
    await writeFile(join(claudePath, 'context', '.gitkeep'), '');
}
/**
 * Generate core documentation files
 * Note: naming-conventions moved to prompts/naming.md
 */
async function generateCoreFiles(claudePath, projectInfo) {
    // Enforcement rules (stack-agnostic)
    const enforcement = `# Enforcement Rules

> Mustard v3.0 (stack-agnostic)

## Enforcement Levels

| Level | Rule | Description | Details |
|-------|------|-------------|---------|
| L0 | Delegation | Main Claude does NOT implement code | This file |
| L1 | grepai | Prefer grepai for semantic search | This file |
| L2 | Pipeline | Pipeline required for features/bugs | This file |
| L3 | Naming | Follow naming conventions | \`prompts/naming.md\` |
| L4 | Validation | Code must pass static validation | \`prompts/review.md\` |
| L5 | Build | Project must compile | \`prompts/review.md\` |
| L6 | Registry | Sync registry after creating entities | This file |

## Details

### L0 - Delegation
Main Claude coordinates but does not implement. Always delegates via Task tool.

### L1 - grepai
Use grepai for semantic search instead of Grep/Glob when possible.

### L2 - Pipeline
Features and bugfixes must follow the pipeline: Explore -> Spec -> Implement -> Review.

### L3 - Naming
Follow naming conventions defined in [prompts/naming.md](../prompts/naming.md).

### L4/L5 - Validation & Build
Validation and build commands depend on the project stack. See [prompts/review.md](../prompts/review.md).

### L6 - Registry
After creating/modifying entities, run \`/sync-registry\`.
`;
    await writeFile(join(claudePath, 'core', 'enforcement.md'), enforcement);
    // Pipeline documentation
    const pipeline = `# Development Pipeline

## Flow

\`\`\`
/feature or /bugfix
         │
         ▼
    EXPLORE (analysis)
         │
         ▼
      SPEC (approval)
         │
         ▼
    IMPLEMENT
    (delegation)
         │
         ▼
    REVIEW
         │
         ▼
    COMPLETED
\`\`\`

## Commands

| Command | Description |
|---------|-------------|
| /feature <name> | Starts feature pipeline |
| /bugfix <error> | Starts bugfix pipeline |
| /approve | Approves spec for implementation |
| /complete | Finalizes pipeline |
| /resume | Resumes active pipeline |
`;
    await writeFile(join(claudePath, 'core', 'pipeline.md'), pipeline);
}
/**
 * Copy settings.json from template to target .claude directory
 */
async function copySettingsJson(claudePath) {
    const __filename = fileURLToPath(import.meta.url);
    const __dirname = dirname(__filename);
    const templatePath = join(__dirname, '..', '..', 'templates', 'settings.json');
    const targetPath = join(claudePath, 'settings.json');
    await copyFile(templatePath, targetPath);
}
/**
 * Copy scripts folder from template to target .claude directory
 */
async function copyScripts(claudePath) {
    const __filename = fileURLToPath(import.meta.url);
    const __dirname = dirname(__filename);
    const templateDir = join(__dirname, '..', '..', 'templates', 'scripts');
    const targetDir = join(claudePath, 'scripts');
    await mkdir(targetDir, { recursive: true });
    // Copy statusline.js
    const statuslinePath = join(templateDir, 'statusline.js');
    if (existsSync(statuslinePath)) {
        await copyFile(statuslinePath, join(targetDir, 'statusline.js'));
    }
}
/**
 * Merge settings.json - preserves client customizations while updating core structure
 */
async function mergeSettingsJson(claudePath) {
    const __filename = fileURLToPath(import.meta.url);
    const __dirname = dirname(__filename);
    const templatePath = join(__dirname, '..', '..', 'templates', 'settings.json');
    const targetPath = join(claudePath, 'settings.json');
    // Get template settings
    const content = await readFile(templatePath, 'utf-8');
    const templateSettings = JSON.parse(content);
    // Get existing settings if any
    let existingSettings = {};
    if (existsSync(targetPath)) {
        try {
            const content = await readFile(targetPath, 'utf-8');
            existingSettings = JSON.parse(content);
        }
        catch {
            // Invalid JSON, use template
        }
    }
    // Merge: template as base, existing takes priority
    const merged = deepMerge(templateSettings, existingSettings);
    await writeFile(targetPath, JSON.stringify(merged, null, 2));
}
/**
 * Generate only core files (for update command)
 * Preserves: CLAUDE.md, context/*.md (except README), docs/*
 * Updates: commands/, prompts/, hooks/, core/, scripts/, settings.json, entity-registry.json
 */
export async function generateCoreOnly(projectPath, projectInfo, analysis, options = {}) {
    const { useOllama = false, model, verbose = false, overwriteClaudeMd = false } = options;
    const log = (msg) => { if (verbose)
        console.log(`  ${msg}`); };
    const claudePath = join(projectPath, '.claude');
    const generatedFiles = [];
    // Ensure directory structure exists
    await mkdir(join(claudePath, 'commands', MUSTARD_COMMANDS_FOLDER), { recursive: true });
    await mkdir(join(claudePath, 'hooks'), { recursive: true });
    await mkdir(join(claudePath, 'core'), { recursive: true });
    await mkdir(join(claudePath, 'scripts'), { recursive: true });
    // Generate CLAUDE.md only if explicitly requested
    if (overwriteClaudeMd) {
        log('Generating CLAUDE.md...');
        const claudeMdPath = join(claudePath, 'CLAUDE.md');
        let claudeMdContent = null;
        if (useOllama) {
            claudeMdContent = await generateClaudeMdLLM(projectInfo, analysis, { model, verbose });
        }
        if (!claudeMdContent) {
            claudeMdContent = generateClaudeMdTemplate(projectInfo, analysis);
        }
        await writeFile(claudeMdPath, claudeMdContent);
        generatedFiles.push('CLAUDE.md');
    }
    // NOTE: Prompts are NOT regenerated during update - only during init
    // This preserves user customizations to prompt files
    // Generate commands (in mustard/ subfolder - user commands in commands/ are preserved)
    log('Generating commands...');
    const commands = generateCommands(projectInfo);
    for (const [name, content] of Object.entries(commands)) {
        await writeFile(join(claudePath, 'commands', MUSTARD_COMMANDS_FOLDER, `${name}.md`), content);
        generatedFiles.push(`commands/${MUSTARD_COMMANDS_FOLDER}/${name}.md`);
    }
    // Generate hooks
    log('Generating hooks...');
    const hooks = generateHooks(projectInfo, options);
    for (const [name, content] of Object.entries(hooks)) {
        await writeFile(join(claudePath, 'hooks', name), content);
        generatedFiles.push(`hooks/${name}`);
    }
    // Generate entity registry
    log('Generating entity registry...');
    const registry = generateRegistry(projectInfo, analysis);
    await writeFile(join(claudePath, 'entity-registry.json'), JSON.stringify(registry, null, 2));
    generatedFiles.push('entity-registry.json');
    // Generate core files (enforcement, pipeline - NOT naming-conventions)
    log('Generating core files...');
    await generateCoreFiles(claudePath, projectInfo);
    generatedFiles.push('core/enforcement.md', 'core/pipeline.md');
    // Update context/README.md only (preserve other context files)
    log('Updating context/README.md...');
    await mkdir(join(claudePath, 'context'), { recursive: true });
    await generateContextReadme(claudePath);
    generatedFiles.push('context/README.md');
    // Merge settings.json (preserve client hooks)
    log('Merging settings.json...');
    await mergeSettingsJson(claudePath);
    generatedFiles.push('settings.json');
    // Copy scripts
    log('Copying scripts...');
    await copyScripts(claudePath);
    generatedFiles.push('scripts/statusline.js');
    return generatedFiles;
}
/**
 * Generate only context/README.md (for update command)
 */
async function generateContextReadme(claudePath) {
    const contextReadme = `# Project Context

Place markdown files here to provide context to Claude during implementations.

## Purpose

Files in this folder are loaded into memory MCP at the start of \`/feature\` or \`/bugfix\` pipelines.
This gives Claude instant access to project specifications, architecture decisions, and patterns.

## Supported Files

Any \`.md\` file placed in this folder will be automatically loaded.

**Suggested files:**
- \`project-spec.md\` - Project overview and specifications
- \`architecture.md\` - Architecture decisions and patterns
- \`business-rules.md\` - Domain-specific rules and logic
- \`api-guidelines.md\` - API design guidelines
- \`tips.md\` - Project-specific tips for Claude
- \`service-example.md\` - Code example for services
- \`component-example.md\` - Code example for components

## Rules

1. **Markdown only** - Only \`.md\` files are loaded
2. **Keep files focused** - One topic per file
3. **Use headers** - Claude uses headers to understand structure
4. **Max 500 lines** - Longer files are truncated
5. **Max 20 files** - Total limit for loaded files

## How It Works

Files are automatically loaded at the start of \`/feature\` or \`/bugfix\` pipelines.
Each file is stored as a \`UserContext:{filename}\` entity in memory MCP.

## Example: architecture.md

\`\`\`markdown
# Architecture

## Layers
- Database: Drizzle ORM with PostgreSQL
- Backend: .NET 9 with FastEndpoints
- Frontend: React 19 with TanStack Query

## Patterns
- Repository pattern for data access
- Services for business logic
- DTOs for API contracts
\`\`\`

## Manual Refresh

To force a context refresh, use:

\`\`\`
/sync-context --refresh
\`\`\`

## See Also

- [/sync-context](../commands/mustard/sync-context.md) - Manual context loading
- [/feature](../commands/mustard/feature.md) - Feature pipeline
- [pipeline.md](../core/pipeline.md) - Pipeline documentation
`;
    await writeFile(join(claudePath, 'context', 'README.md'), contextReadme);
}
//# sourceMappingURL=index.js.map