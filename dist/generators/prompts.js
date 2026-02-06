import { existsSync } from 'fs';
import { readFile } from 'fs/promises';
import { join, dirname } from 'path';
import { fileURLToPath } from 'url';
/**
 * Get the templates directory path
 */
function getTemplatesDir() {
    const __filename = fileURLToPath(import.meta.url);
    const __dirname = dirname(__filename);
    return join(__dirname, '..', '..', 'templates', 'prompts');
}
/**
 * Load a template file and return its content
 */
async function loadTemplate(name) {
    const templatePath = join(getTemplatesDir(), `${name}.md`);
    if (existsSync(templatePath)) {
        return await readFile(templatePath, 'utf-8');
    }
    return null;
}
/**
 * Generate prompt files from templates
 */
export async function generatePrompts(projectInfo, _analysis) {
    // Determine which prompts to generate
    const hasBackend = projectInfo.stacks.some(s => ['dotnet', 'node', 'python', 'java', 'go', 'rust'].includes(s.name));
    const hasFrontend = projectInfo.stacks.some(s => ['react', 'nextjs', 'vue', 'angular', 'svelte'].includes(s.name));
    const hasDatabase = projectInfo.stacks.some(s => ['drizzle', 'prisma', 'typeorm', 'sequelize'].includes(s.name));
    // Load templates from files
    const orchestratorTemplate = await loadTemplate('orchestrator');
    const bugfixTemplate = await loadTemplate('bugfix');
    const reviewTemplate = await loadTemplate('review');
    const namingTemplate = await loadTemplate('naming');
    const prompts = {
        orchestrator: orchestratorTemplate ?? generateOrchestratorFallback(),
        bugfix: bugfixTemplate ?? generateBugfixFallback(),
        review: reviewTemplate ?? generateReviewFallback(),
        naming: namingTemplate ?? generateNamingFallback(projectInfo)
    };
    // Load backend if detected
    if (hasBackend) {
        const backendTemplate = await loadTemplate('backend');
        prompts.backend = backendTemplate ?? generateBackendFallback(projectInfo);
    }
    // Load frontend if detected
    if (hasFrontend) {
        const frontendTemplate = await loadTemplate('frontend');
        prompts.frontend = frontendTemplate ?? generateFrontendFallback(projectInfo);
    }
    // Load database if detected
    if (hasDatabase) {
        const databaseTemplate = await loadTemplate('database');
        prompts.database = databaseTemplate ?? generateDatabaseFallback(projectInfo);
    }
    // Load report (always)
    const reportTemplate = await loadTemplate('report');
    if (reportTemplate) {
        prompts.report = reportTemplate;
    }
    return prompts;
}
// ============== Fallback Templates (used only if file not found) ==============
function generateOrchestratorFallback() {
    return `# Orchestrator

## Identity

You are the **Orchestrator**. You coordinate the development pipeline but **DO NOT implement code directly**.

## Required Pipeline

\`\`\`
1. EXPLORE   → Task(Explore) to analyze requirements
2. SPEC      → Create spec at spec/active/{name}/spec.md
3. APPROVE   → Present spec for user approval
4. IMPLEMENT → Task(general-purpose) with specialized prompts
5. REVIEW    → Task(general-purpose) with review prompt
6. COMPLETE  → Update registry, move spec to completed/
\`\`\`

## Rules

- **NEVER** write code directly
- **ALWAYS** delegate via Task tool
- **FOLLOW** the pipeline strictly
- **PRESENT** spec before implementing
`;
}
function generateBugfixFallback() {
    return `# Bugfix Specialist

## Identity

You are the **Bugfix Specialist**. You diagnose and fix bugs in the code.

## Process

1. **REPRODUCE** - Understand how the bug manifests
2. **DIAGNOSE** - Find the root cause
3. **FIX** - Apply the minimal necessary fix
4. **VALIDATE** - Verify the fix works

## Rules

- **NEVER** make changes unrelated to the bug
- **DOCUMENT** the root cause before fixing
- **TEST** the fix before finalizing
`;
}
function generateReviewFallback() {
    return `# Review Specialist

## Identity

You are the **Review Specialist**. You validate implementations and ensure quality.

## Review Checklist

- [ ] Follows project naming conventions
- [ ] Uses dependency injection correctly
- [ ] Has no duplicate code
- [ ] Handles errors appropriately
- [ ] Is testable

## Result

After review, respond with:
- **APPROVED** - If everything is correct
- **ADJUSTMENTS** - List of issues found
`;
}
function generateNamingFallback(projectInfo) {
    const classPattern = projectInfo.patterns?.classes ?? 'PascalCase';
    const filePattern = projectInfo.patterns?.files ?? 'kebab-case';
    const folderPattern = projectInfo.patterns?.folders ?? 'plural';
    return `# Naming Conventions

## Detected Conventions

| Type | Pattern |
|------|---------|
| Classes | ${classPattern} |
| Files | ${typeof filePattern === 'object' ? JSON.stringify(filePattern) : filePattern} |
| Folders | ${folderPattern} |

## Quick Reference

| Type | Pattern | Example |
|------|---------|---------|
| Entity/Class | PascalCase singular | \`Contract\`, \`Person\` |
| DB Table | snake_case plural | \`contracts\`, \`people\` |
| Endpoint/Route | kebab-case | \`/api/contracts\` |
| Component | PascalCase | \`ContractForm\` |
| Hook | use + camelCase | \`useContracts\` |
`;
}
function generateBackendFallback(projectInfo) {
    const backendStack = projectInfo.stacks.find(s => ['dotnet', 'node', 'python', 'java', 'go', 'rust'].includes(s.name));
    const stackName = backendStack?.name ?? 'unknown';
    const stackVersion = backendStack?.version ?? '';
    return `# Backend Specialist

## Identity

You are the **Backend Specialist**. You implement server-side code.

## Stack

- **Language/Framework:** ${stackName} ${stackVersion}
- **Architecture:** ${projectInfo.structure?.architecture?.type ?? 'layered'}

## Responsibilities

- Implement endpoints/APIs
- Create services and business logic
- Configure repositories/data access
- Manage authentication/authorization
- Handle errors and validations
`;
}
function generateFrontendFallback(projectInfo) {
    const frontendStack = projectInfo.stacks.find(s => ['react', 'nextjs', 'vue', 'angular', 'svelte'].includes(s.name));
    const stackName = frontendStack?.name ?? 'react';
    const stackVersion = frontendStack?.version ?? '';
    return `# Frontend Specialist

## Identity

You are the **Frontend Specialist**. You implement client-side code.

## Stack

- **Framework:** ${stackName} ${stackVersion}

## Responsibilities

- Create React/Vue/etc components
- Implement custom hooks
- Manage state
- Integrate with APIs
- Ensure accessibility
`;
}
function generateDatabaseFallback(projectInfo) {
    const dbStack = projectInfo.stacks.find(s => ['drizzle', 'prisma', 'typeorm', 'sequelize'].includes(s.name));
    const ormName = dbStack?.name ?? 'drizzle';
    return `# Database Specialist

## Identity

You are the **Database Specialist**. You manage schemas and migrations.

## ORM

- **ORM:** ${ormName}

## Responsibilities

- Create/modify table schemas
- Manage migrations
- Define relationships
- Configure indexes
`;
}
// ============== Sync/Merge Functions ==============
const AUTO_START = '<!-- MUSTARD:AUTO-START -->';
const AUTO_END = '<!-- MUSTARD:AUTO-END -->';
/**
 * Generate auto-populated context section for a prompt
 */
export function generateAutoSection(_promptType, projectInfo, analysis, patterns) {
    const stacks = projectInfo.stacks.map(s => `${s.name} ${s.version || ''}`).join(', ');
    const entities = patterns.entities?.map(e => e.name).slice(0, 10).join(', ') || 'None detected';
    const architecture = analysis.architecture?.type || projectInfo.structure?.architecture?.type || 'unknown';
    const detectedPatterns = analysis.patterns?.length > 0 ? analysis.patterns.join(', ') : 'Standard';
    return `${AUTO_START}
## Project Context (Auto-Generated)

- **Stacks:** ${stacks}
- **Architecture:** ${architecture}
- **Entities:** ${entities}${patterns.entities && patterns.entities.length > 10 ? ` (+${patterns.entities.length - 10} more)` : ''}
- **Patterns:** ${detectedPatterns}

> This section is auto-updated by \`mustard sync\`. Edit content below the marker.
${AUTO_END}`;
}
/**
 * Merge auto-generated content with existing prompt, preserving user content
 */
export function mergePromptContext(existingContent, newAutoSection) {
    const hasAutoSection = existingContent.includes(AUTO_START) && existingContent.includes(AUTO_END);
    if (hasAutoSection) {
        // Replace existing auto section
        const regex = new RegExp(`${escapeRegex(AUTO_START)}[\\s\\S]*?${escapeRegex(AUTO_END)}`, 'g');
        return existingContent.replace(regex, newAutoSection);
    }
    else {
        // Insert auto section after the title (first # line)
        const lines = existingContent.split('\n');
        const titleIndex = lines.findIndex(l => l.startsWith('# '));
        if (titleIndex >= 0) {
            lines.splice(titleIndex + 1, 0, '', newAutoSection, '');
            return lines.join('\n');
        }
        else {
            // No title found, prepend
            return newAutoSection + '\n\n' + existingContent;
        }
    }
}
function escapeRegex(str) {
    return str.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}
//# sourceMappingURL=prompts.js.map