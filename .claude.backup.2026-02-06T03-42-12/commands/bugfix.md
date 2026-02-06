# /bugfix - Bug Fix Pipeline

## Trigger

`/bugfix <error-description>`

## Description

Starts the pipeline to diagnose and fix a bug.

## Flow

1. **DIAGNOSE**
   - Use grepai to search related code
   - Identify root cause
   - Document findings

2. **SPEC**
   - Create spec at `spec/active/{name}/spec.md`
   - Describe root cause
   - Propose fix

3. **APPROVE**
   - Present diagnosis to user
   - Wait for /approve

4. **FIX**
   - Apply minimal fix
   - Do not make unrelated changes

5. **VALIDATE**
   - Verify bug is fixed
   - Verify nothing broke

6. **COMPLETE**
   - Document solution
   - Move spec to completed/

## Example

```
User: /bugfix error saving contract

Claude:
1. Uses grepai to find save code
2. Identifies root cause
3. Presents diagnosis
4. (after /approve) Fixes
5. Validates and completes
```
