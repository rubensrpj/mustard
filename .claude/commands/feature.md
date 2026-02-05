# /feature - Feature Pipeline

## Trigger

`/feature <feature-name>`

## Description

Starts the full pipeline to implement a new feature.

## Flow

1. **EXPLORE**
   - Use Task(Explore) to analyze requirements
   - Map related existing files
   - Identify patterns to follow

2. **SPEC**
   - Create spec at `spec/active/{name}/spec.md`
   - List files to create/modify
   - Define implementation checklist

3. **APPROVE**
   - Present spec to user
   - Wait for /approve or feedback

4. **IMPLEMENT**
   - Delegate to specialized agents
   - Backend → Frontend → Database (order as needed)

5. **REVIEW**
   - Validate implementation
   - Verify checklist

6. **COMPLETE**
   - Update entity-registry if needed
   - Move spec to completed/

## Example

```
User: /feature add-partner-email-field

Claude:
1. Explores codebase to understand Partner
2. Creates spec with implementation plan
3. Presents spec for approval
4. (after /approve) Implements Database → Backend → Frontend
5. Validates and completes
```
