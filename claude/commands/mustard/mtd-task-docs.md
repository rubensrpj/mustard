# /mtd-task-docs - Documentation Generation

> Generates documentation in a **separate Task context** (L0 Universal Delegation).
> Use for API docs, README updates, code comments, or technical documentation.

## Usage

```
/mtd-task-docs <scope>
/mtd-task-docs "API endpoints"
/mtd-task-docs "Contract entity"
```

## What It Does

1. **Delegates** to Task(general-purpose) - NEVER generates docs in parent context
2. **Analyzes** code to understand what to document
3. **Generates** appropriate documentation
4. **Presents** for review before saving

## Pipeline

```
/mtd-task-docs <scope>
     │
     ▼
┌────────────────────────────────┐
│  Task(general-purpose)         │
│  model: sonnet                 │
│  description: 📊 Docs...       │
└──────────────┬─────────────────┘
               │
               ▼
         Present for review
               │
               ▼
         Save documentation
```

## Implementation

```javascript
// CRITICAL: Always delegate - never generate docs in parent context
Task({
  subagent_type: "general-purpose",
  model: "sonnet",
  description: `📊 Docs: ${scope}`,
  prompt: `
# 📊 DOCUMENTATION TASK

## Scope
${scope}

## Instructions
1. Use grepai_search to find relevant code
2. Read source files to understand functionality
3. Generate appropriate documentation

## Documentation Types
Based on scope, generate:

### For API Endpoints
- Endpoint URL and method
- Request parameters/body
- Response format
- Error codes
- Example requests

### For Entities/Models
- Entity description
- Properties with types
- Relationships
- Example usage

### For Services/Functions
- Purpose
- Parameters
- Return value
- Side effects
- Example usage

### For README
- Project overview
- Installation
- Configuration
- Usage examples
- API reference

## Output Format
Provide the documentation in markdown format.
Indicate where it should be saved (file path).
  `
})
```

## Arguments

| Argument | Description | Example |
|----------|-------------|---------|
| `<scope>` | What to document | `"API endpoints"`, `"auth flow"` |

## Examples

```bash
# Document API endpoints
/mtd-task-docs "all API endpoints"

# Document an entity
/mtd-task-docs "Contract entity"

# Update README
/mtd-task-docs "update README with new features"

# Document a module
/mtd-task-docs "payment module architecture"
```

## Output

```
📊 Generating docs: API endpoints

Task(general-purpose): Analyzing codebase...

## Documentation Generated

### File: docs/api/contracts.md

# Contracts API

## Endpoints

### GET /api/contracts
List all contracts for current tenant.

**Query Parameters:**
| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| page | number | No | Page number (default: 1) |
| limit | number | No | Items per page (default: 20) |
| status | string | No | Filter by status |

**Response:**
\`\`\`json
{
  "data": [...],
  "pagination": {
    "page": 1,
    "limit": 20,
    "total": 100
  }
}
\`\`\`

### POST /api/contracts
Create a new contract.

**Request Body:**
\`\`\`json
{
  "personId": "uuid",
  "companyId": "uuid",
  "startDate": "2026-01-01",
  "value": 1000.00
}
\`\`\`

...

---

Save to docs/api/contracts.md? [Y/n]
```

## L0 Enforcement

**CRITICAL**: This command enforces L0 Universal Delegation:
- Parent context does NOT read code
- Parent context does NOT generate documentation
- Parent context ONLY coordinates and presents results
- ALL documentation work happens in the Task(general-purpose) context

## Related Commands

| Command | Description |
|---------|-------------|
| `/mtd-task-analyze` | Analyze code before documenting |
| `/mtd-task-review` | Review documentation quality |
| `/mtd-sync-context` | Update context files |

## See Also

- [enforcement.md](../../core/enforcement.md) - L0 Universal Delegation rule
- [context/README.md](../../context/README.md) - Context file format
