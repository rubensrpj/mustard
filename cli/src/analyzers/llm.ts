import * as ollama from '../services/ollama.js';
import type { ProjectInfo, Analysis, CodeSample, OllamaOptions, GeneratedPrompts, Stack } from '../types.js';

/**
 * Analyze code snippets using Ollama LLM
 */
export async function analyzeCode(codeSnippets: CodeSample[], options: OllamaOptions = {}): Promise<Analysis> {
  const { model = 'llama3.2' } = options;

  const snippetsText = codeSnippets
    .filter(s => s && s.content)
    .map(s => `### ${s.file}\n\`\`\`\n${s.content}\n\`\`\``)
    .join('\n\n');

  if (!snippetsText) {
    return getDefaultAnalysis();
  }

  const prompt = `You are a code analyzer. Analyze the following code snippets and extract structured information about the project.

## Code snippets:
${snippetsText}

## Extract in JSON format:
{
  "architecture": {
    "type": "mvc|clean|feature-based|layered|other",
    "description": "brief description of the architecture"
  },
  "patterns": ["repository", "service-layer", "dto", "mapper", ...],
  "naming": {
    "classes": "PascalCase|camelCase|snake_case",
    "files": "PascalCase|kebab-case|snake_case|camelCase",
    "folders": "singular|plural"
  },
  "rules": [
    "rule 1 that Claude should follow when working on this codebase",
    "rule 2...",
    "rule 3..."
  ],
  "frameworks": ["framework1", "framework2", ...]
}

Return ONLY the JSON, no explanations.`;

  try {
    const response = await ollama.generateJSON<Partial<Analysis>>(prompt, { model });
    return {
      ...getDefaultAnalysis(),
      ...response
    };
  } catch (error: unknown) {
    const message = error instanceof Error ? error.message : 'Unknown error';
    console.error('Ollama analysis failed:', message);
    return getDefaultAnalysis();
  }
}

/**
 * Generate CLAUDE.md content using Ollama
 */
export async function generateClaudeMd(projectInfo: ProjectInfo, analysis: Analysis, options: OllamaOptions = {}): Promise<string | null> {
  const { model = 'llama3.2' } = options;

  const prompt = `You are a documentation generator for Claude Code (an AI coding assistant).
Generate a CLAUDE.md file for this project.

## Project Information:
- Name: ${projectInfo.name}
- Type: ${projectInfo.type}
- Stacks: ${projectInfo.stacks.map(s => `${s.name} ${s.version ?? ''}`).join(', ')}
- Architecture: ${analysis.architecture?.type ?? 'unknown'}
- Package Manager: ${projectInfo.packageManager ?? 'npm'}

## Detected Patterns:
${JSON.stringify(analysis.patterns ?? [], null, 2)}

## Naming Conventions:
${JSON.stringify(projectInfo.patterns ?? {}, null, 2)}

## Rules:
${(analysis.rules ?? []).map(r => `- ${r}`).join('\n')}

## Generate a CLAUDE.md that includes:
1. Quick Reference section with naming conventions
2. Project state section (ports, technologies)
3. Available commands adapted for the detected stack
4. Structure explanation
5. Specific rules Claude should follow
6. Entity Registry usage instructions

Use markdown formatting. Be specific to this project.
Include a section about the pipeline (/mtd-pipeline-feature, /mtd-pipeline-bugfix commands).
Include grepai as the preferred search tool.

Return ONLY the markdown content, no code blocks wrapping it.`;

  try {
    const response = await ollama.generate(prompt, { model });
    return response;
  } catch (error: unknown) {
    const message = error instanceof Error ? error.message : 'Unknown error';
    console.error('Ollama CLAUDE.md generation failed:', message);
    return null;
  }
}

/**
 * Generate specialized prompts using Ollama
 */
export async function generatePrompts(projectInfo: ProjectInfo, analysis: Analysis, options: OllamaOptions = {}): Promise<Partial<GeneratedPrompts>> {
  const { model = 'llama3.2' } = options;
  const prompts: Partial<GeneratedPrompts> = {};

  // Determine which prompts to generate based on stacks
  const hasBackend = projectInfo.stacks.some(s =>
    ['dotnet', 'node', 'python', 'java', 'go', 'rust'].includes(s.name)
  );
  const hasFrontend = projectInfo.stacks.some(s =>
    ['react', 'nextjs', 'vue', 'angular', 'svelte'].includes(s.name)
  );
  const hasDatabase = projectInfo.stacks.some(s =>
    ['drizzle', 'prisma', 'typeorm', 'sequelize'].includes(s.name)
  );

  // Generate orchestrator prompt (always)
  prompts.orchestrator = await generateOrchestratorPrompt(projectInfo, analysis, { model });

  // Generate backend prompt
  if (hasBackend) {
    prompts.backend = await generateBackendPrompt(projectInfo, analysis, { model });
  }

  // Generate frontend prompt
  if (hasFrontend) {
    prompts.frontend = await generateFrontendPrompt(projectInfo, analysis, { model });
  }

  // Generate database prompt
  if (hasDatabase) {
    prompts.database = await generateDatabasePrompt(projectInfo, analysis, { model });
  }

  return prompts;
}

/**
 * Generate orchestrator prompt
 */
async function generateOrchestratorPrompt(projectInfo: ProjectInfo, analysis: Analysis, options: OllamaOptions): Promise<string> {
  const prompt = `Generate a prompt file for an "Orchestrator" agent that coordinates development work.

Project: ${projectInfo.name}
Stacks: ${projectInfo.stacks.map(s => s.name).join(', ')}

The orchestrator should:
1. NOT implement code directly - always delegate
2. Follow a pipeline: Explore -> Spec -> Implement -> Review -> Complete
3. Coordinate between backend, frontend, and database specialists

Return markdown content for the prompt file. Include:
- Identity section explaining the role
- Pipeline steps
- Delegation rules
- Task template examples`;

  try {
    return await ollama.generate(prompt, options);
  } catch {
    return getDefaultOrchestratorPrompt();
  }
}

/**
 * Generate backend prompt
 */
async function generateBackendPrompt(projectInfo: ProjectInfo, analysis: Analysis, options: OllamaOptions): Promise<string> {
  const backendStack = projectInfo.stacks.find(s =>
    ['dotnet', 'node', 'python', 'java', 'go', 'rust'].includes(s.name)
  );

  const prompt = `Generate a prompt file for a "Backend Specialist" agent.

Project: ${projectInfo.name}
Backend Stack: ${backendStack?.name ?? 'unknown'} ${backendStack?.version ?? ''}
Architecture: ${analysis.architecture?.type ?? 'unknown'}
Patterns: ${(analysis.patterns ?? []).join(', ')}

The backend specialist should:
1. Implement API endpoints, services, business logic
2. Follow the project's architecture patterns
3. Use proper dependency injection
4. Handle errors appropriately

Return markdown content for the prompt file.`;

  try {
    return await ollama.generate(prompt, options);
  } catch {
    return getDefaultBackendPrompt(backendStack?.name);
  }
}

/**
 * Generate frontend prompt
 */
async function generateFrontendPrompt(projectInfo: ProjectInfo, analysis: Analysis, options: OllamaOptions): Promise<string> {
  const frontendStack = projectInfo.stacks.find(s =>
    ['react', 'nextjs', 'vue', 'angular', 'svelte'].includes(s.name)
  );

  const prompt = `Generate a prompt file for a "Frontend Specialist" agent.

Project: ${projectInfo.name}
Frontend Stack: ${frontendStack?.name ?? 'unknown'} ${frontendStack?.version ?? ''}

The frontend specialist should:
1. Implement React components, hooks, pages
2. Follow the project's component patterns
3. Handle state management properly
4. Ensure type safety with TypeScript

Return markdown content for the prompt file.`;

  try {
    return await ollama.generate(prompt, options);
  } catch {
    return getDefaultFrontendPrompt(frontendStack?.name);
  }
}

/**
 * Generate database prompt
 */
async function generateDatabasePrompt(projectInfo: ProjectInfo, analysis: Analysis, options: OllamaOptions): Promise<string> {
  const dbStack = projectInfo.stacks.find(s =>
    ['drizzle', 'prisma', 'typeorm', 'sequelize'].includes(s.name)
  );

  const prompt = `Generate a prompt file for a "Database Specialist" agent.

Project: ${projectInfo.name}
ORM/Schema: ${dbStack?.name ?? 'unknown'}

The database specialist should:
1. Create and modify database schemas
2. Handle migrations properly
3. Follow naming conventions for tables and columns
4. Ensure referential integrity

Return markdown content for the prompt file.`;

  try {
    return await ollama.generate(prompt, options);
  } catch {
    return getDefaultDatabasePrompt(dbStack?.name);
  }
}

/**
 * Default analysis when Ollama is unavailable
 */
function getDefaultAnalysis(): Analysis {
  return {
    architecture: { type: 'unknown', description: '', confidence: 'low' },
    patterns: [],
    naming: {
      classes: 'PascalCase',
      files: 'kebab-case',
    },
    rules: [],
    frameworks: [],
    entities: []
  };
}

/**
 * Default orchestrator prompt
 */
function getDefaultOrchestratorPrompt(): string {
  return `# Orchestrator

## Identity
You are the Orchestrator agent. You coordinate development work but do NOT implement code directly.

## Pipeline
1. **EXPLORE**: Analyze requirements using Task(Explore)
2. **SPEC**: Create specification for approval
3. **IMPLEMENT**: Delegate to specialized agents
4. **REVIEW**: Validate implementation
5. **COMPLETE**: Update registry and finalize

## Rules
- Never write code directly
- Always delegate to specialized agents
- Follow the pipeline strictly
`;
}

/**
 * Default backend prompt
 */
function getDefaultBackendPrompt(stack: string = 'unknown'): string {
  return `# Backend Specialist

## Identity
You are the Backend Specialist. You implement server-side code.

## Stack
${stack}

## Responsibilities
- Implement API endpoints
- Create services and business logic
- Handle data access
- Manage authentication/authorization

## Rules
- Follow project architecture patterns
- Use dependency injection
- Handle errors properly
- Write clean, testable code
`;
}

/**
 * Default frontend prompt
 */
function getDefaultFrontendPrompt(stack: string = 'react'): string {
  return `# Frontend Specialist

## Identity
You are the Frontend Specialist. You implement client-side code.

## Stack
${stack}

## Responsibilities
- Create React components
- Implement custom hooks
- Handle state management
- Build responsive UI

## Rules
- Follow component patterns
- Ensure type safety
- Handle loading/error states
- Write accessible code
`;
}

/**
 * Default database prompt
 */
function getDefaultDatabasePrompt(orm: string = 'unknown'): string {
  return `# Database Specialist

## Identity
You are the Database Specialist. You manage database schemas and migrations.

## ORM
${orm}

## Responsibilities
- Create database schemas
- Handle migrations
- Design relationships
- Optimize queries

## Rules
- Follow naming conventions
- Ensure referential integrity
- Use appropriate data types
- Document schema changes
`;
}
