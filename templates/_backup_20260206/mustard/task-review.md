# /task-review - Code Review

> Performs code review in a **separate Task context** (L0 Universal Delegation).
> Use for QA, SOLID validation, security checks, or general code quality reviews.

## Usage

```
/task-review <scope>
/task-review src/services/auth
/task-review "last commit"
```

## What It Does

1. **Delegates** to Task(general-purpose) - NEVER reviews in parent context
2. **Analyzes** code quality, patterns, security
3. **Reports** findings with severity levels

## Pipeline

```
/task-review <scope>
     │
     ▼
┌────────────────────────────────┐
│  Task(general-purpose)         │
│  model: opus                   │
│  description: 🔎 Review...     │
└──────────────┬─────────────────┘
               │
               ▼
         Report findings
```

## Implementation

```javascript
// CRITICAL: Always delegate - never review in parent context
Task({
  subagent_type: "general-purpose",
  model: "opus",
  description: `🔎 Review: ${scope}`,
  prompt: `
# 🔎 CODE REVIEW TASK

## Scope
${scope}

## Review Checklist
- [ ] SOLID principles compliance
- [ ] Error handling coverage
- [ ] Security vulnerabilities
- [ ] Performance concerns
- [ ] Code duplication
- [ ] Naming conventions
- [ ] Documentation gaps
- [ ] Test coverage

## Search Strategy
1. Use grepai_search for semantic search
2. Read files in scope
3. Trace dependencies with grepai_trace_*
4. Check related tests

## Output Format
For each issue found:

### [Severity] Issue Title
- **File**: path/to/file.ts
- **Line**: 42
- **Issue**: Description of the problem
- **Impact**: What could go wrong
- **Suggestion**: How to fix

Severity levels:
- 🔴 Critical - Must fix before merge
- 🟠 Major - Should fix soon
- 🟡 Minor - Nice to have
- 🔵 Info - Observation
  `
})
```

## Arguments

| Argument | Description | Example |
|----------|-------------|---------|
| `<scope>` | What to review | `src/services`, `"Contract entity"` |

## Examples

```bash
# Review a directory
/task-review src/services/payment

# Review an entity
/task-review "Contract entity implementation"

# Review recent changes
/task-review "changes in last commit"

# Security focused
/task-review "security in auth module"
```

## Output

```
🔎 Reviewing: src/services/payment

Task(general-purpose): Analyzing code quality...

Code Review Complete:

### 🔴 Critical: SQL Injection Vulnerability
- **File**: src/services/payment/queries.ts
- **Line**: 45
- **Issue**: Raw SQL with string interpolation
- **Impact**: Allows SQL injection attacks
- **Suggestion**: Use parameterized queries

### 🟠 Major: Missing Error Handling
- **File**: src/services/payment/stripe.ts
- **Line**: 78
- **Issue**: API call without try/catch
- **Impact**: Unhandled promise rejection
- **Suggestion**: Wrap in try/catch, handle StripeError

### 🟡 Minor: Magic Number
- **File**: src/services/payment/calculator.ts
- **Line**: 23
- **Issue**: Hardcoded value 0.029
- **Impact**: Hard to maintain
- **Suggestion**: Extract to constant TAX_RATE

## Summary
- 🔴 Critical: 1
- 🟠 Major: 3
- 🟡 Minor: 5
- 🔵 Info: 2
```

## L0 Enforcement

**CRITICAL**: This command enforces L0 Universal Delegation:
- Parent context does NOT read code
- Parent context does NOT analyze quality
- Parent context ONLY coordinates and presents results
- ALL review happens in the Task(general-purpose) context

## Related Commands

| Command | Description |
|---------|-------------|
| `/task-analyze` | Exploratory code analysis |
| `/task-refactor` | Refactoring with plan |
| `/bugfix` | Fix bugs found in review |

## See Also

- [review.md](../../prompts/review.md) - Review specialist prompt
- [enforcement.md](../../core/enforcement.md) - L0 Universal Delegation rule
