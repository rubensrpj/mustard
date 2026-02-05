# Mustard Prompts Index

> **TEMPLATE FILES:** These prompt files are templates that can be customized for your project.
> You may modify the content, but **do not rename the files** - the filenames are required by Mustard.

## How to Use

Claude Code accepts only 4 native `subagent_type` values:
- `Explore` - Fast codebase exploration
- `Plan` - Implementation planning
- `general-purpose` - Implementation, bug fixes, reviews
- `Bash` - Terminal commands

Mustard "agents" are **prompts** that load specialized instructions into a `Task(general-purpose)`:

```javascript
// Example: Call "Backend Specialist"
Task({
  subagent_type: "general-purpose",  // Native type
  model: "opus",
  description: "Backend implementation",
  prompt: `
# You are the BACKEND SPECIALIST

[content from prompts/backend.md]

# TASK
${description}
  `
})
```

---

## Prompt Mapping

| Role | subagent_type | Model | File |
|------|---------------|-------|------|
| Orchestrator | `general-purpose` | opus | [orchestrator.md](./orchestrator.md) |
| Explorer | `Explore` | haiku | (native - no prompt) |
| Backend | `general-purpose` | opus | [backend.md](./backend.md) |
| Frontend | `general-purpose` | opus | [frontend.md](./frontend.md) |
| Database | `general-purpose` | opus | [database.md](./database.md) |
| Bugfix | `general-purpose` | opus | [bugfix.md](./bugfix.md) |
| Review | `general-purpose` | opus | [review.md](./review.md) |
| Report | `general-purpose` | sonnet | [report.md](./report.md) |
| **Naming** | (reference) | - | [naming.md](./naming.md) |

> **Note:** `naming.md` is a reference prompt for naming conventions (L3).
> All other prompts should consult this file.

---

## Usage Map

```
User Request
         |
         +-- Bug/Error -------------> general-purpose + bugfix.md
         |
         +-- New Feature -----------> general-purpose + orchestrator.md
         |                                |
         |                    +-----------+-----------+
         |                    v           v           v
         |                database.md  backend.md  frontend.md
         |
         +-- Explore/Understand ----> Explore (native)
         |
         +-- Code Review -----------> general-purpose + review.md
         |
         +-- Reports ---------------> general-purpose + report.md
```

---

## Invocation Rules

1. **Main Claude** never implements code directly
2. **All implementation** goes through `Task(general-purpose)` with specialized prompt
3. **Complex features** always use orchestrator.md for coordination
4. **Exploration** always uses `Task(Explore)` (native type, no custom prompt)

---

## Models by Prompt

| Model | Prompts | Usage |
|-------|---------|-------|
| **Opus** | orchestrator, backend, database, bugfix, review, frontend | Complex tasks, architectural decisions |
| **Sonnet** | report | Structured tasks, templates |
| **Haiku** | (none - Explore is native) | Fast search, light exploration |

---

## Call Examples

### Orchestrator (Feature Pipeline)

```javascript
Task({
  subagent_type: "general-purpose",
  model: "opus",
  description: "Feature Invoice",
  prompt: `
# You are the ORCHESTRATOR
[CONTENT FROM orchestrator.md]
# TASK: Implement Invoice feature
  `
})
```

### Backend Specialist

```javascript
Task({
  subagent_type: "general-purpose",
  model: "opus",
  description: "Backend Invoice",
  prompt: `
# You are the BACKEND SPECIALIST
[CONTENT FROM backend.md]
# TASK: Create endpoints for Invoice
  `
})
```

### Explorer (Native)

```javascript
Task({
  subagent_type: "Explore",  // NATIVE type
  model: "haiku",
  description: "Explore Invoice",
  prompt: "Analyze structure for Invoice entity. Map similar files."
})
```

---

## Customization

You can customize these prompts for your project:
- Add project-specific patterns and conventions
- Include examples from your codebase
- Adjust rules to match your architecture
- Add stack-specific instructions

**Remember:** Keep the filenames unchanged - Mustard relies on them.
