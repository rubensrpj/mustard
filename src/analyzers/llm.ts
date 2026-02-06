import * as ollama from '../services/ollama.js';
import type { ProjectInfo, Analysis, CodeSample, OllamaOptions } from '../types.js';

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
Include a section about the pipeline (/feature, /bugfix commands).
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
