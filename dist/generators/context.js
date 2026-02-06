import { writeFile, mkdir } from 'fs/promises';
import { join } from 'path';
import * as ollama from '../services/ollama.js';
/**
 * Generate context files for the .claude/context/ folder
 * These files provide instant context to agents during implementations
 */
export async function generateContext(claudePath, projectInfo, analysis, codeSamples, options = {}) {
    const generatedFiles = [];
    const contextPath = join(claudePath, 'context');
    await mkdir(contextPath, { recursive: true });
    // Try Ollama for richer context generation
    if (options.useOllama) {
        const ollamaContent = await generateContextWithOllama(projectInfo, analysis, codeSamples, options);
        if (ollamaContent) {
            for (const [filename, content] of Object.entries(ollamaContent)) {
                if (content) {
                    const filePath = join(contextPath, filename);
                    await writeFile(filePath, content);
                    generatedFiles.push(`context/${filename}`);
                }
            }
            return generatedFiles;
        }
    }
    return generateContextFromTemplate(contextPath, projectInfo, analysis);
}
// ============== Ollama Generation ==============
async function generateContextWithOllama(projectInfo, analysis, codeSamples, options) {
    try {
        const isAvailable = await ollama.checkOllamaAvailable();
        if (!isAvailable) {
            if (options.verbose)
                console.log('  Ollama not available, using templates');
            return null;
        }
        const models = await ollama.getAvailableModels();
        const model = options.model || await ollama.selectBestModel(models);
        if (!model)
            return null;
        if (options.verbose)
            console.log(`  Using Ollama (${model}) for context generation`);
        const prompt = buildOllamaPrompt(projectInfo, analysis, codeSamples);
        return await ollama.generateJSON(prompt, { model, timeout: 90000 });
    }
    catch {
        if (options.verbose)
            console.log('  Ollama generation failed, falling back to templates');
        return null;
    }
}
/**
 * Build the Ollama prompt using ONLY detected project data.
 * No hardcoded patterns - Ollama must infer from the data provided.
 */
function buildOllamaPrompt(projectInfo, analysis, codeSamples) {
    const stacksStr = projectInfo.stacks.map(s => `- ${s.name} ${s.version} (${s.path})`).join('\n');
    const subprojectsStr = projectInfo.structure.subprojects.map(s => `- ${s.name} (${s.path})`).join('\n');
    const dirsStr = projectInfo.structure.directories.slice(0, 30).join('\n');
    let samplesSection = '';
    for (const [key, sample] of Object.entries(codeSamples)) {
        if (sample) {
            samplesSection += `\n### ${key} (${sample.file}):\n\`\`\`\n${sample.content.slice(0, 2000)}\n\`\`\`\n`;
        }
    }
    return `You are generating project documentation for an AI coding assistant.
Analyze the project data below and generate documentation that accurately describes THIS project's architecture, patterns, and naming conventions.

IMPORTANT:
- Only document patterns you can SEE in the code samples and directory structure
- Do NOT invent patterns or conventions not present in the data
- Do NOT generate code examples - only describe the patterns found
- Use the actual file/folder names from the directory structure

## Project: ${projectInfo.name}
- Type: ${projectInfo.type}
- Package Manager: ${projectInfo.packageManager}
- Architecture: ${analysis.architecture.type} (${analysis.architecture.confidence} confidence)

## Stacks:
${stacksStr}

## Subprojects:
${subprojectsStr || 'Single project'}

## Detected Patterns: ${analysis.patterns.length > 0 ? analysis.patterns.join(', ') : 'none detected'}

## Naming Conventions (detected):
- Classes: ${projectInfo.patterns.classes}
- Files: ${JSON.stringify(projectInfo.patterns.files)}
- Folders: ${projectInfo.patterns.folders}

## Directory Structure:
\`\`\`
${dirsStr}
\`\`\`

## Dependencies (from package.json / .csproj):
${formatDepsForPrompt(projectInfo.dependencies)}

## Code Samples:
${samplesSection || 'No code samples available'}

## TASK

Generate 3 markdown files. Base ALL content on the data above. Never invent patterns not visible in the samples/structure.

1. **architecture.md**: Project type, stacks with versions, subproject layout, layer organization (inferred from directories), how layers connect
2. **patterns.md**: For each stack, describe the actual patterns visible in the code samples and directory structure. Include folder conventions found.
3. **naming.md**: Document the actual naming conventions found in file names, folder names, and code samples.

Response format (JSON only):
{
  "architecture.md": "# Architecture\\n\\n...",
  "patterns.md": "# Patterns\\n\\n...",
  "naming.md": "# Naming Conventions\\n\\n..."
}`;
}
// ============== Template Generation ==============
async function generateContextFromTemplate(contextPath, projectInfo, analysis) {
    const generatedFiles = [];
    const architecture = generateArchitectureMd(projectInfo, analysis);
    await writeFile(join(contextPath, 'architecture.md'), architecture);
    generatedFiles.push('context/architecture.md');
    const patterns = generatePatternsMd(projectInfo, analysis);
    await writeFile(join(contextPath, 'patterns.md'), patterns);
    generatedFiles.push('context/patterns.md');
    const naming = generateNamingMd(projectInfo);
    await writeFile(join(contextPath, 'naming.md'), naming);
    generatedFiles.push('context/naming.md');
    return generatedFiles;
}
// ============== Helpers ==============
function getBackendStacks(stacks) {
    return stacks.filter(s => ['dotnet', 'node', 'python', 'java', 'go', 'rust', 'fastapi', 'django', 'spring'].includes(s.name));
}
function getFrontendStacks(stacks) {
    return stacks.filter(s => ['react', 'nextjs', 'vue', 'angular', 'svelte'].includes(s.name));
}
function getDatabaseStacks(stacks) {
    return stacks.filter(s => ['drizzle', 'prisma', 'typeorm', 'sequelize'].includes(s.name));
}
function guessSubprojectRole(name) {
    const lower = name.toLowerCase();
    if (lower.includes('backend') || lower.includes('api') || lower.includes('server'))
        return 'Backend API';
    if (lower.includes('frontend') || lower.includes('web') || lower.includes('client') || lower.includes('ui'))
        return 'Frontend App';
    if (lower.includes('database') || lower.includes('db') || lower.includes('data'))
        return 'Database/Migrations';
    if (lower.includes('lib') || lower.includes('shared') || lower.includes('common'))
        return 'Shared Libraries';
    if (lower.includes('test') || lower.includes('spec'))
        return 'Tests';
    return 'Module';
}
// ============== Architecture Template ==============
function generateArchitectureMd(projectInfo, analysis) {
    const backend = getBackendStacks(projectInfo.stacks);
    const frontend = getFrontendStacks(projectInfo.stacks);
    const database = getDatabaseStacks(projectInfo.stacks);
    let subprojectsSection = '';
    if (projectInfo.structure.subprojects.length > 0) {
        subprojectsSection = `## Subprojects

| Project | Path | Role |
|---------|------|------|
${projectInfo.structure.subprojects.map(s => `| **${s.name}** | \`${s.path}\` | ${guessSubprojectRole(s.name)} |`).join('\n')}
`;
    }
    const stackRows = projectInfo.stacks.map(s => `| ${s.name} | ${s.version} | \`${s.path}\` |`).join('\n');
    const dirsDisplay = projectInfo.structure.directories.slice(0, 25).join('\n');
    // Layer diagram based only on detected stacks
    const layerParts = [];
    if (frontend.length > 0) {
        layerParts.push(`  [Frontend: ${frontend.map(s => `${s.name} ${s.version}`).join(', ')}]`);
        layerParts.push('          |');
        layerParts.push('       HTTP/API');
        layerParts.push('          |');
    }
    if (backend.length > 0) {
        layerParts.push(`  [Backend: ${backend.map(s => `${s.name} ${s.version}`).join(', ')}]`);
        layerParts.push('          |');
    }
    if (database.length > 0) {
        layerParts.push(`  [Database: ${database.map(s => `${s.name} ${s.version}`).join(', ')}]`);
    }
    return `# Architecture

> Auto-generated by Mustard CLI. Edit to add project-specific details.

## Overview

| Property | Value |
|----------|-------|
| **Project** | ${projectInfo.name} |
| **Type** | ${projectInfo.type} |
| **Architecture** | ${analysis.architecture.type} |
| **Package Manager** | ${projectInfo.packageManager} |

${subprojectsSection}
## Technology Stack

| Stack | Version | Path |
|-------|---------|------|
${stackRows}

${layerParts.length > 0 ? `## Layer Interactions

\`\`\`
${layerParts.join('\n')}
\`\`\`` : ''}

## Directory Structure

\`\`\`
${dirsDisplay}
${projectInfo.structure.directories.length > 25 ? '...' : ''}
\`\`\`

## Dependencies

${generateDependenciesSection(projectInfo)}

## Architecture Decisions

> Document your architecture decisions below:

-
-
`;
}
function formatDepsForPrompt(deps) {
    if (!deps || Object.keys(deps).length === 0)
        return 'No dependencies scanned';
    const lines = [];
    for (const [subPath, categories] of Object.entries(deps)) {
        lines.push(`### ${subPath}`);
        for (const [category, libs] of Object.entries(categories)) {
            if (libs && libs.length > 0) {
                lines.push(`- ${category}: ${libs.join(', ')}`);
            }
        }
    }
    return lines.join('\n');
}
function generateDependenciesSection(projectInfo) {
    const deps = projectInfo.dependencies;
    if (!deps || Object.keys(deps).length === 0) {
        return '> Run `mustard init` with package.json present to auto-detect dependencies.';
    }
    const sections = [];
    for (const [subPath, categories] of Object.entries(deps)) {
        const label = subPath === '.' ? 'Root' : subPath;
        const rows = [];
        for (const [category, libs] of Object.entries(categories)) {
            if (libs && libs.length > 0) {
                rows.push(`| **${category}** | ${libs.join(', ')} |`);
            }
        }
        if (rows.length > 0) {
            sections.push(`### ${label}\n\n| Category | Libraries |\n|----------|----------|\n${rows.join('\n')}`);
        }
    }
    return sections.join('\n\n');
}
// ============== Patterns Template ==============
function generatePatternsMd(projectInfo, analysis) {
    const backend = getBackendStacks(projectInfo.stacks);
    const frontend = getFrontendStacks(projectInfo.stacks);
    const database = getDatabaseStacks(projectInfo.stacks);
    const detectedList = analysis.patterns.length > 0
        ? analysis.patterns.map(p => `- ${p}`).join('\n')
        : '';
    const rulesList = analysis.rules.length > 0
        ? analysis.rules.map(r => `- ${r}`).join('\n')
        : '';
    let backendSection = '';
    for (const stack of backend) {
        backendSection += generateBackendPatternSection(stack);
    }
    let frontendSection = '';
    for (const stack of frontend) {
        frontendSection += generateFrontendPatternSection(stack);
    }
    let databaseSection = '';
    for (const stack of database) {
        databaseSection += generateDatabasePatternSection(stack);
    }
    return `# Patterns & Conventions

> Auto-generated by Mustard CLI. Edit to match your actual project patterns.
> Add code samples as .md files in context/ folder for Claude to reference.

${detectedList ? `## Detected Patterns\n\n${detectedList}\n` : ''}
${backendSection}
${frontendSection}
${databaseSection}
## General Principles

- Separate business logic from data access
- Validate at system boundaries (endpoints, forms)
- Use consistent error response shapes
- Follow established naming conventions (see naming.md)

${rulesList ? `## Project Rules\n\n${rulesList}\n` : ''}
## Anti-Patterns to Avoid

- Business logic in controllers/endpoints
- Data access outside the data layer
- Duplicated validation logic
- Hardcoded values instead of constants/enums
`;
}
function generateBackendPatternSection(stack) {
    const v = stack.version ? ` ${stack.version}` : '';
    if (stack.name === 'dotnet') {
        return `## Backend Patterns (.NET${v})

### Layered Architecture
- **Endpoints/Controllers** - HTTP route handlers, parameter binding, response mapping
- **Services** - Business logic, orchestration, validation
- **Repositories** - Data access, query building

### Dependency Injection
- Services receive dependencies via constructor injection
- Endpoints receive services as method parameters (Minimal APIs) or constructor (Controllers)

### DTO / Mapping
- Separate DTOs for request input and response output
- Map between entities and DTOs at the boundary

> Add service-example.md and endpoint-example.md to context/ for reference patterns.

`;
    }
    if (stack.name === 'node') {
        return `## Backend Patterns (Node.js${v})

### Module Structure
- Controllers/routes handle HTTP concerns
- Services contain business logic
- Repositories handle data access (if applicable)

### Dependency Injection
- Constructor injection or module-level imports

> Add service-example.md and endpoint-example.md to context/ for reference patterns.

`;
    }
    if (stack.name === 'python' || stack.name === 'fastapi' || stack.name === 'django') {
        return `## Backend Patterns (${stack.name}${v})

### Module Structure
- Route handlers / views
- Service layer for business logic
- Models for data access

> Add code examples as .md files in context/ for reference patterns.

`;
    }
    return `## Backend Patterns (${stack.name}${v})

> Add code examples as .md files in context/ for reference patterns.

`;
}
function generateFrontendPatternSection(stack) {
    const v = stack.version ? ` ${stack.version}` : '';
    if (stack.name === 'react' || stack.name === 'nextjs') {
        const name = stack.name === 'nextjs' ? `Next.js${v}` : `React${v}`;
        return `## Frontend Patterns (${name})

### Component Organization
- Page-level components handle routing and data fetching
- Feature components handle business UI logic
- Shared UI components in a common directory

### Hooks
- Custom hooks encapsulate data fetching and state logic
- Entity-specific hooks for CRUD operations

### Forms
- Form library for state management and validation
- Schema-based validation (Zod, Yup, or similar)

### State Management
- Server state via data-fetching library (TanStack Query, SWR, etc.)
- Client state via context, Zustand, or similar

> Add component-example.md and hook-example.md to context/ for reference patterns.

`;
    }
    if (stack.name === 'vue') {
        return `## Frontend Patterns (Vue${v})

### Component Organization
- Page components for routing
- Composable functions for reusable logic
- Shared components directory

> Add code examples as .md files in context/ for reference patterns.

`;
    }
    return `## Frontend Patterns (${stack.name}${v})

> Add code examples as .md files in context/ for reference patterns.

`;
}
function generateDatabasePatternSection(stack) {
    const v = stack.version ? ` ${stack.version}` : '';
    if (stack.name === 'drizzle') {
        return `## Database Patterns (Drizzle${v})

### Schema Organization
- One file per table in the schema directory
- Shared enums in a separate file
- Index file re-exports all schemas

### Common Column Patterns
- Auto-increment primary key
- UUID for public-facing identifiers
- Timestamp columns for audit trail
- Soft delete pattern (boolean flag + timestamp)

### Migrations
- Generated from schema changes (\`drizzle-kit generate\`)
- Applied with migration runner

> Add schema-example.md to context/ for reference patterns.

`;
    }
    if (stack.name === 'prisma') {
        return `## Database Patterns (Prisma${v})

### Schema
- Single schema file defining all models
- Relations defined with \`@relation\`

### Migrations
- Generated with \`prisma migrate dev\`
- Applied with \`prisma migrate deploy\`

> Add code examples as .md files in context/ for reference patterns.

`;
    }
    if (stack.name === 'typeorm') {
        return `## Database Patterns (TypeORM${v})

### Entity Pattern
- Decorator-based entity definitions
- Repository pattern for data access

> Add code examples as .md files in context/ for reference patterns.

`;
    }
    return `## Database Patterns (${stack.name}${v})

> Add code examples as .md files in context/ for reference patterns.

`;
}
// ============== Naming Template ==============
function generateNamingMd(projectInfo) {
    const filesPattern = typeof projectInfo.patterns.files === 'object'
        ? Object.entries(projectInfo.patterns.files)
            .map(([ext, pattern]) => `| \`.${ext}\` | ${pattern} |`)
            .join('\n')
        : `| all | ${projectInfo.patterns.files} |`;
    // Build stack-specific naming from detected stacks (generic per stack type)
    const backend = getBackendStacks(projectInfo.stacks);
    const frontend = getFrontendStacks(projectInfo.stacks);
    const database = getDatabaseStacks(projectInfo.stacks);
    let backendNaming = '';
    for (const stack of backend) {
        backendNaming += generateBackendNamingSection(stack);
    }
    let frontendNaming = '';
    for (const stack of frontend) {
        frontendNaming += generateFrontendNamingSection(stack);
    }
    let databaseNaming = '';
    for (const stack of database) {
        databaseNaming += generateDatabaseNamingSection(stack);
    }
    return `# Naming Conventions

> Auto-generated by Mustard CLI.
> For actual naming patterns, refer to the code samples in context/ folder.

## Detected Conventions

| Type | Convention |
|------|-----------|
| Classes | ${projectInfo.patterns.classes} |
| Folders | ${projectInfo.patterns.folders} |

### File Extensions

| Extension | Convention |
|-----------|-----------|
${filesPattern}

${backendNaming}
${frontendNaming}
${databaseNaming}

## General Rules

| Type | Convention |
|------|-----------|
| Entities | PascalCase singular |
| DB Tables | snake_case plural |
| DB Columns | snake_case |
| API Endpoints | kebab-case |

> Review code samples in context/ folder for the exact naming patterns used.
`;
}
function generateBackendNamingSection(stack) {
    if (stack.name === 'dotnet') {
        return `## Backend Naming (.NET)

| Type | Convention |
|------|-----------|
| Classes | PascalCase |
| Interfaces | \`I\` prefix + PascalCase |
| Services | \`{Entity}Service\` |
| Repositories | \`{Entity}Repository\` |
| DTOs | \`{Entity}{Purpose}Dto\` |
| Endpoints | \`{Entity}EndPoints\` or \`{Entity}Controller\` |
| Namespaces | Match folder path |

> Check actual file/folder structure in the code samples.

`;
    }
    if (stack.name === 'node') {
        return `## Backend Naming (Node.js)

| Type | Convention |
|------|-----------|
| Files | kebab-case or camelCase |
| Classes | PascalCase |
| Functions | camelCase |
| Controllers | \`{entity}.controller.ts\` |
| Services | \`{entity}.service.ts\` |

`;
    }
    return '';
}
function generateFrontendNamingSection(stack) {
    if (stack.name === 'react' || stack.name === 'nextjs') {
        return `## Frontend Naming (${stack.name === 'nextjs' ? 'Next.js' : 'React'})

| Type | Convention |
|------|-----------|
| Components | PascalCase or kebab-case files |
| Hooks | \`use-{name}\` or \`use{Name}\` files |
| Pages | \`page.tsx\` (Next.js) or PascalCase |
| Shared components | Common directory |
| Feature components | Co-located with pages |

> Check actual file structure in component-example.md and hook-example.md in context/.

`;
    }
    if (stack.name === 'vue') {
        return `## Frontend Naming (Vue)

| Type | Convention |
|------|-----------|
| Components | PascalCase .vue files |
| Composables | \`use{Name}\` functions |
| Pages | lowercase or kebab-case |

`;
    }
    return '';
}
function generateDatabaseNamingSection(stack) {
    if (stack.name === 'drizzle') {
        return `## Database Naming (Drizzle)

| Type | Convention |
|------|-----------|
| Schema files | kebab-case: \`{entity}.ts\` |
| Table names | snake_case plural |
| Column names | snake_case |
| Foreign keys | \`{referenced_table}_id\` |
| Indexes | \`{table}_{column}_idx\` |
| Unique indexes | \`{table}_{description}_unique\` |

> Check schema-example.md in context/ for actual column patterns.

`;
    }
    if (stack.name === 'prisma') {
        return `## Database Naming (Prisma)

| Type | Convention |
|------|-----------|
| Models | PascalCase singular |
| Fields | camelCase |
| Relations | camelCase |
| Enums | PascalCase |

`;
    }
    return '';
}
//# sourceMappingURL=context.js.map