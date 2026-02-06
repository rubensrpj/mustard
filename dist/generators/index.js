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
    const { useOllama = true, model, verbose = false, overwriteClaudeMd = true } = options;
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
    // Generate context folder with subfolders for each agent
    const contextFiles = await generateContextFolder(claudePath);
    generatedFiles.push(...contextFiles);
    // Copy settings.json from template
    log('Copying settings.json...');
    await copySettingsJson(claudePath);
    generatedFiles.push('settings.json');
    // Copy scripts folder
    log('Copying scripts...');
    await copyScripts(claudePath);
    generatedFiles.push('scripts/statusline.js');
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
 * Context folders organized by agent
 */
const CONTEXT_FOLDERS = {
    shared: 'Contexto comum carregado por TODOS os agentes',
    backend: 'Padrões de API, serviços, repositórios - carregado pelo Backend Specialist',
    frontend: 'Componentes, hooks, estilos - carregado pelo Frontend Specialist',
    database: 'Schemas, migrações, queries - carregado pelo Database Specialist',
    bugfix: 'Issues comuns, dicas de debug - carregado pelo Bugfix Specialist',
    review: 'Checklists, regras de qualidade - carregado pelo Review Specialist',
    orchestrator: 'Visão geral, fluxos de pipeline - carregado pelo Orchestrator'
};
/**
 * Generate context folder with subfolders for each agent
 */
async function generateContextFolder(claudePath) {
    const createdFiles = [];
    // Create each subfolder with README
    for (const [folder, description] of Object.entries(CONTEXT_FOLDERS)) {
        const folderPath = join(claudePath, 'context', folder);
        await mkdir(folderPath, { recursive: true });
        const readme = `# ${folder.charAt(0).toUpperCase() + folder.slice(1)} Context

${description}

## Como usar

Crie arquivos \`.md\` aqui com informações específicas para o agente **${folder}**.

## Carregamento

Quando o agente ${folder} é chamado:
1. Arquivos de \`shared/\` são carregados primeiro
2. Arquivos desta pasta são carregados depois
3. Entidades criadas: \`AgentContext:${folder}:{filename}\`

## Regras

- Apenas arquivos \`.md\`
- Máximo 500 linhas por arquivo
- Evite duplicar conteúdo que já está em \`shared/\`
`;
        await writeFile(join(folderPath, 'README.md'), readme);
        createdFiles.push(`context/${folder}/README.md`);
    }
    // Create main README in context/
    const mainReadme = `# Context Files

Esta pasta contém **arquivos de contexto** organizados por agente.

## Estrutura

\`\`\`
context/
├── shared/       # Contexto comum (TODOS os agentes)
├── backend/      # Só o Backend Specialist vê
├── frontend/     # Só o Frontend Specialist vê
├── database/     # Só o Database Specialist vê
├── bugfix/       # Só o Bugfix Specialist vê
├── review/       # Só o Review Specialist vê
└── orchestrator/ # Só o Orchestrator vê
\`\`\`

## Como Funciona

1. Quando um agente é chamado (ex: backend.md)
2. Ele carrega \`shared/*.md\` + \`backend/*.md\`
3. Cria entidades no Memory MCP: \`AgentContext:backend:{filename}\`
4. Depois faz \`mcp__memory__search_nodes\` normalmente

## Regras

- Apenas arquivos \`.md\`
- Máximo 500 linhas por arquivo
- Máximo 20 arquivos por pasta
- Use \`shared/\` para contexto comum
- Use pastas específicas para contexto do agente
`;
    await writeFile(join(claudePath, 'context', 'README.md'), mainReadme);
    createdFiles.push('context/README.md');
    return createdFiles;
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
    // Update context subfolders and READMEs (preserve other context files)
    log('Updating context folders...');
    await mkdir(join(claudePath, 'context'), { recursive: true });
    const contextFiles = await generateContextReadme(claudePath);
    generatedFiles.push(...contextFiles);
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
 * Generate context subfolders and READMEs (for update command)
 * Creates subfolders if they don't exist, always updates READMEs
 */
async function generateContextReadme(claudePath) {
    const createdFiles = [];
    // Create each subfolder with README (preserves existing content files)
    for (const [folder, description] of Object.entries(CONTEXT_FOLDERS)) {
        const folderPath = join(claudePath, 'context', folder);
        await mkdir(folderPath, { recursive: true });
        const readme = `# ${folder.charAt(0).toUpperCase() + folder.slice(1)} Context

${description}

## Como usar

Crie arquivos \`.md\` aqui com informações específicas para o agente **${folder}**.

## Carregamento

Quando o agente ${folder} é chamado:
1. Arquivos de \`shared/\` são carregados primeiro
2. Arquivos desta pasta são carregados depois
3. Entidades criadas: \`AgentContext:${folder}:{filename}\`

## Regras

- Apenas arquivos \`.md\`
- Máximo 500 linhas por arquivo
- Evite duplicar conteúdo que já está em \`shared/\`
`;
        await writeFile(join(folderPath, 'README.md'), readme);
        createdFiles.push(`context/${folder}/README.md`);
    }
    // Create main README in context/
    const mainReadme = `# Context Files

Esta pasta contém **arquivos de contexto** organizados por agente.

## Estrutura

\`\`\`
context/
├── shared/       # Contexto comum (TODOS os agentes)
├── backend/      # Só o Backend Specialist vê
├── frontend/     # Só o Frontend Specialist vê
├── database/     # Só o Database Specialist vê
├── bugfix/       # Só o Bugfix Specialist vê
├── review/       # Só o Review Specialist vê
└── orchestrator/ # Só o Orchestrator vê
\`\`\`

## Como Funciona

1. Quando um agente é chamado (ex: backend.md)
2. Ele carrega \`shared/*.md\` + \`backend/*.md\`
3. Cria entidades no Memory MCP: \`AgentContext:backend:{filename}\`
4. Depois faz \`mcp__memory__search_nodes\` normalmente

## Regras

- Apenas arquivos \`.md\`
- Máximo 500 linhas por arquivo
- Máximo 20 arquivos por pasta
- Use \`shared/\` para contexto comum
- Use pastas específicas para contexto do agente
`;
    await writeFile(join(claudePath, 'context', 'README.md'), mainReadme);
    createdFiles.push('context/README.md');
    return createdFiles;
}
//# sourceMappingURL=index.js.map