---
name: test-tsconfig-exclude
description: Test files (.test.tsx/__tests__) must be excluded from tsconfig.json to avoid tsc build failures when vitest/RTL devDeps are not installed
metadata:
  type: feedback
---

When writing test files for the dashboard, vitest and @testing-library/react are not in devDependencies yet. Without tsconfig `exclude`, the build (`tsc -b`) will fail with "Cannot find module 'vitest'" errors.

**Why:** The dashboard's `package.json` has `"test": "echo \"no tests yet\""` — Vitest is not wired. Test files must be excluded from the production tsconfig while still being written for future use.

**How to apply:** Add to `tsconfig.json`:
```json
"exclude": ["src/**/__tests__/**", "src/**/*.test.tsx", "src/**/*.test.ts"]
```
Do this whenever test files are created before the test runner is installed.
