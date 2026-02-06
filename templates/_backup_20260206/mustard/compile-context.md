# /compile-context - Compile Agent Contexts

> Compiles context files for all agents into optimized `.context.md` files.
> **v2.3** - Auto-invoked by /feature and /bugfix.

## Usage

```
/compile-context
/compile-context --force
```

## What It Does

1. Gets current git commit hash
2. For each agent (backend, frontend, database, bugfix, review, orchestrator):
   - Checks if `prompts/{agent}.context.md` exists
   - Checks if the hash in the file matches current commit
   - If missing or outdated: compiles context from source files
3. Saves compiled contexts to `prompts/{agent}.context.md`

## When It Runs

- **Automatically** at the start of `/feature` or `/bugfix`
- **Manually** when you run `/compile-context`
- **Force recompile** with `/compile-context --force`

## Execution Steps

### Step 1: Get Current Commit Hash

```bash
git rev-parse --short HEAD
```

Save as `currentHash`.

### Step 2: For Each Agent, Check and Compile

For each agent in: `backend`, `frontend`, `database`, `bugfix`, `review`, `orchestrator`

#### 2a. Check if compilation needed

```javascript
// Check if compiled file exists
const compiledPath = `.claude/prompts/${agent}.context.md`;
const exists = Glob(compiledPath).length > 0;

if (exists) {
  // Read first line to get hash
  const content = Read(compiledPath);
  const hashMatch = content.match(/compiled-from-commit: (\w+)/);
  const fileHash = hashMatch ? hashMatch[1] : null;

  if (fileHash === currentHash) {
    // Skip - already up to date
    continue;
  }
}
```

#### 2b. Compile context

```javascript
// Find source files
const sharedFiles = Glob(".claude/context/shared/*.md")
  .filter(f => !f.includes("README"));
const agentFiles = Glob(`.claude/context/${agent}/*.md`)
  .filter(f => !f.includes("README"));

// Read all source files
const sources = [];
for (const file of [...sharedFiles, ...agentFiles]) {
  const content = Read(file);
  sources.push({ path: file, content });
}

// Synthesize: remove duplicates, consolidate, optimize
// Claude does this intelligently based on the content
```

#### 2c. Write compiled context

```javascript
Write(`.claude/prompts/${agent}.context.md`, `<!-- compiled-from-commit: ${currentHash} -->
<!-- sources: ${sources.map(s => s.path).join(', ')} -->
<!-- compiled-at: ${new Date().toISOString()} -->

# ${agent.charAt(0).toUpperCase() + agent.slice(1)} Context

${synthesizedContent}
`);
```

### Step 3: Report

```
✅ Context compiled for all agents

| Agent | Status | Sources |
|-------|--------|---------|
| backend | compiled | conventions.md, patterns.md |
| frontend | compiled | conventions.md, patterns.md |
| database | compiled | conventions.md, patterns.md |
| bugfix | up-to-date | - |
| review | compiled | conventions.md |
| orchestrator | compiled | conventions.md |

Commit: abc1234
```

## Synthesis Rules

When compiling multiple source files into one context:

1. **Remove duplicates** - If the same section appears in shared and agent-specific, keep agent-specific
2. **Consolidate similar sections** - Merge sections with similar headings
3. **Keep code examples concise** - Prefer short, clear examples
4. **Optimize for tokens** - Remove verbose explanations, keep actionable content
5. **Preserve structure** - Keep clear headings and organization

## Arguments

| Argument | Description |
|----------|-------------|
| `--force` | Recompile all contexts even if up-to-date |

## Output Format

Each compiled file has this structure:

```markdown
<!-- compiled-from-commit: abc1234 -->
<!-- sources: shared/conventions.md, backend/patterns.md -->
<!-- compiled-at: 2026-02-06T07:30:00.000Z -->

# Backend Context

## Naming Conventions
{from shared/conventions.md}

## Patterns
{from backend/patterns.md}

## Architecture
{consolidated from multiple sources}
```

## Notes

- Run automatically by `/feature` and `/bugfix`
- Compiled files are cached until git commit changes
- Source files in `context/` are preserved, compiled files in `prompts/`
- Force recompile with `--force` if manual edits to context files

## See Also

- [/feature](./feature.md) - Feature pipeline (auto-runs compile)
- [/bugfix](./bugfix.md) - Bugfix pipeline (auto-runs compile)
- [context/README.md](../../context/README.md) - How to create context files
