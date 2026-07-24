---
id: wave.scope-scan-generated-role-pattern.plan
---

# Wave Plan

## Wave Table

| Wave | Spec | Role | Depends on | Summary |
|------|------|------|------------|---------|
| 1 | [[wave.scope-scan-generated-role-pattern.1-backend]] | backend | — | O glob do cluster sai do censo e chega ao molde como paths: |
| 2 | [[wave.scope-scan-generated-role-pattern.2-config]] | config | [[wave.scope-scan-generated-role-pattern.1-backend]] | O frontmatter dos comandos passa a usar as chaves que a plataforma honra |
