# Change Log — scan-upsert-terminam-pela-metade

_Solicitações registradas automaticamente durante o pipeline (mid-spec). O `spec.md` (narrativa congelada) NÃO é alterado; dobre o que muda comportamento em `## Acceptance Criteria` e rode o QA de novo._

- **2026-08-21T18:31:39.039Z** _(Execute)_ — segue
- **2026-08-21T19:22:31.892Z** _(Execute)_ — **Instruction:** O commit automatico do selo de versao acontece em QUALQUER branch, inclusive numa protegida como a main: o selo e configuracao do projeto, nao trabalho do operador, e um clone novo esta sempre na branch padrao, que e onde se instala. Recusar ali devolveria a arvore suja no caso mais comum. Em troca, a decisao deixa de ser acidental: a linha de log nomeia a branch em que o commit caiu, e um teste tranca o comportamento numa branch protegida para que ninguem o mude sem perceber.
