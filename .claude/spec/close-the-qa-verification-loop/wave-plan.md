---
id: wave.close-the-qa-verification-loop.plan
---

# Wave Plan

## Wave Table

| Wave | Spec | Role | Depends on | Summary |
|------|------|------|------------|---------|
| 1 | [[wave.close-the-qa-verification-loop.1-gate]] | gate | — | O Check de Stop: auto-restrição, execução via reuso do qa-run, contador próprio, texto via i18n |
| 2 | [[wave.close-the-qa-verification-loop.2-wiring]] | wiring | [[wave.close-the-qa-verification-loop.1-gate]] | A fiacao: registrar o Check no trigger Stop e emitir decision:block no evento Stop |
