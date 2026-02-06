import * as ollama from '../services/ollama.js';
import type { ProjectInfo, Analysis, GeneratorOptions } from '../types.js';

/**
 * Generate CLAUDE.md using Ollama LLM
 */
export async function generateClaudeMd(projectInfo: ProjectInfo, analysis: Analysis, options: GeneratorOptions = {}): Promise<string | null> {
  const { model = 'llama3.2', verbose = false } = options;

  // Build context for the LLM
  const stackList = projectInfo.stacks
    .map(s => `- ${s.name}${s.version ? ` ${s.version}` : ''} (${s.path})`)
    .join('\n');

  const patternsDetected = (analysis.patterns ?? []).join(', ') || 'none detected';

  const namingInfo = projectInfo.patterns
    ? `Classes: ${projectInfo.patterns.classes ?? 'PascalCase'}
Files: ${typeof projectInfo.patterns.files === 'object' ? JSON.stringify(projectInfo.patterns.files) : projectInfo.patterns.files}
Folders: ${projectInfo.patterns.folders ?? 'plural'}`
    : 'Using defaults';

  const architectureInfo = analysis.architecture
    ? `Type: ${analysis.architecture.type}\nDescription: ${analysis.architecture.description ?? 'N/A'}`
    : 'Unknown';

  const rulesDetected = (analysis.rules ?? []).length > 0
    ? analysis.rules!.map(r => `- ${r}`).join('\n')
    : 'No specific rules detected';

  const prompt = `You are generating a CLAUDE.md file for a software project. This file will be used by Claude Code (an AI coding assistant) to understand the project. Generate ALL content in English.

## Project Information

**Name:** ${projectInfo.name}
**Type:** ${projectInfo.type}
**Package Manager:** ${projectInfo.packageManager ?? 'npm'}

**Stacks:**
${stackList}

**Architecture:**
${architectureInfo}

**Patterns Detected:**
${patternsDetected}

**Naming Conventions:**
${namingInfo}

**Rules:**
${rulesDetected}

## Requirements

Generate a comprehensive CLAUDE.md IN ENGLISH that includes ALL of the following sections:

1. **Header** - Project name and brief description
2. **Quick Reference** - Table with naming conventions for entities, tables, endpoints, components, hooks
3. **Project State** - Table with projects, technologies, ports, and status
4. **Delegation via Task Tool** - Explain that Claude should delegate to specialized agents
5. **Available Commands** - List commands like /feature, /bugfix, /commit, /validate, /status
6. **Pipeline** - Explain the development pipeline (Explore → Spec → Implement → Review → Complete)
7. **Entity Registry** - Explain to always check .claude/entity-registry.json before searching for files
8. **grepai** - Explain that grepai should be used for semantic code search
9. **Rules** - Project-specific rules and patterns to follow
10. **Links** - Links to prompts, commands, and core documentation

## Format

Use markdown with:
- Tables where appropriate
- Code blocks for examples
- Clear section headers with ##
- Bullet points for lists

## Important

- ALL content must be in English
- Be specific to this project's stack (${projectInfo.stacks.map(s => s.name).join(', ')})
- Include actual commands appropriate for the detected stack
- Reference the correct file patterns for this project
- Keep it concise but comprehensive

Return ONLY the markdown content. Do not wrap it in code blocks.`;

  try {
    const response = await ollama.generate(prompt, { model });

    // Clean up response if it's wrapped in code blocks
    let content = response;
    if (content.startsWith('```markdown')) {
      content = content.slice(11);
    }
    if (content.startsWith('```')) {
      content = content.slice(3);
    }
    if (content.endsWith('```')) {
      content = content.slice(0, -3);
    }

    return content.trim();
  } catch (error: unknown) {
    if (verbose) {
      const message = error instanceof Error ? error.message : 'Unknown error';
      console.error('Ollama CLAUDE.md generation failed:', message);
    }
    return null;
  }
}
