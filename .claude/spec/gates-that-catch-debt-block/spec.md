---
id: spec.gates-that-catch-debt-block
---

# Gates that catch real debt must block instead of warning, and the house style must explain without jargon

<!-- drafter:tone=didactic — Write this spec narrative in didactic tone — expand abbreviations on first use (AC = Acceptance Criteria, wave = onda) and prefer plain words over jargon. -->

<!-- PRD -->

## Contexto

Gates that catch real debt must block instead of warning, and the house style must explain without jargon.

Por que agora.

## Usuários/Stakeholders

Quem se beneficia.

## Métrica de sucesso

Métrica de sucesso.

## Não-Objetivos

O que fica de fora.

## Critérios de Aceitação

- **AC-1** — when <o novo comportamento é acionado>, then <o resultado observável esperado se mantém>
  Command: `<comando executável que verifica este critério>`
  Control: `<a command that must be GREEN against the tree as it is today>`
- **AC-2** — when <um caminho de erro ou de borda ocorre>, then <o sistema responde conforme especificado>
  Command: `<comando executável que verifica este critério>`
  Control: `<a command that must be GREEN against the tree as it is today>`
- **AC-3** — o build do projeto passa verde
  Command: `cargo build --workspace`

## Checklist

- [ ] T1 — primeira tarefa rastreável.
