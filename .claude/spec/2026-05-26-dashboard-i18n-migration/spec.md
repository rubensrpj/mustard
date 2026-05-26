# Dashboard i18n Migration — Hardcoded pt-BR Labels Sweep

### Stage: Close
### Outcome: Active
### Flags: followup_open
### Scope: full
### Checkpoint: 2026-05-26T00:00:00Z
### Lang: pt-BR
### Parent: 2026-05-26-template-agnostic-audit

## PRD

## Contexto

Durante a Wave 7 da spec [[2026-05-26-template-agnostic-audit]], o subcomando `mustard-rt run language-audit` foi criado e executado pela primeira vez no próprio repo do Mustard. Ele encontrou **25 hits legítimos** — todos em arquivos `.tsx` do dashboard React. Os hits são labels de UI em pt-BR hardcoded direto no JSX em vez de passar pelas chaves do `apps/dashboard/src/i18n.ts` (catálogo i18next que já existe e tem suporte a switch pt-BR/en-US). Exemplo: um botão `<button>Cancelar</button>` em vez de `<button>{t("button.cancel")}</button>`. Isso quebra duas premissas: (1) a política consolidada de "código sempre EN — i18n é a única superfície bilíngue" (memória [[project_code_language_policy]]); (2) a promessa de o dashboard funcionar em en-US para usuários que escolherem essa opção em `mustard.json#specLang`.

Esta spec faz o sweep: cada label PT hardcoded vira chave em `i18n.ts` (com pares pt-BR e en-US) e o JSX consome via `t(...)`. Trabalho mecânico mas tedioso — 25 arquivos identificados, número exato de labels por arquivo varia.

## Usuários/Stakeholders

Indireto: qualquer dev que abrir o dashboard em modo en-US e ver pedaços em pt-BR. Direto: maintainer único (Rubens) que quer integridade do contrato i18n do dashboard.

## Métrica de sucesso

`mustard-rt run language-audit --strict` no repo do Mustard sai com exit 0 (zero hits). Switch de idioma do dashboard (pt-BR ↔ en-US) muda 100% da UI, sem fragmento residual em pt-BR.

## Não-Objetivos

- Adicionar novos idiomas além de pt-BR / en-US (Mustard só tem catálogo nesses 2 — [[2026-05-26-template-agnostic-audit]] Wave 5 deixou isso explícito via `SupportedLocale` enum).
- Renomear chaves já existentes em `i18n.ts` por estética.
- Migrar comentários `//` em pt-BR no código TypeScript (esses são metadados de dev, não UI).
- Bloquear commit por hits do language-audit (continua soft warning).

## Critérios de Aceitação

- [ ] AC-1: `mustard-rt run language-audit --strict` exit 0 — Command: `bash -c 'cargo run -q -p mustard-rt -- run language-audit --strict'`
- [ ] AC-2: `pnpm --filter mustard-dashboard build` passa — Command: `pnpm --filter mustard-dashboard build`
- [ ] AC-3: Switch pt-BR → en-US no dashboard altera ao menos os 25 strings antes pt-BR — Command: manual (não há AC automatizada simples para checar render). Documentar como smoke test.
- [ ] AC-4: Nenhum arquivo `.tsx` em `apps/dashboard/src/` tem label pt-BR fora de strings literais protegidas (i18n keys file, comments) — Command: `bash -c 'cargo run -q -p mustard-rt -- run language-audit --format json | node -e "let s=\"\";process.stdin.on(\"data\",c=>s+=c).on(\"end\",()=>{const r=JSON.parse(s);process.exit(r.hits.filter(h=>h.file.endsWith(\".tsx\")).length===0?0:1)})"'`

## Plano

## Arquivos

Lista exata depende do output do `language-audit` no momento do scan. A spec parent W7 detectou **25 hits** em `.tsx`. Lista provável (a confirmar antes de EXECUTE):

- `apps/dashboard/src/pages/*.tsx` (múltiplos)
- `apps/dashboard/src/features/**/*.tsx` (múltiplos)
- `apps/dashboard/src/components/**/*.tsx` (múltiplos)
- `apps/dashboard/src/i18n.ts` (CREATE keys novas)

## Tarefas

### Dashboard Agent
- [ ] Rodar `mustard-rt run language-audit --format json` para obter a lista exata atual de hits
- [ ] Para cada arquivo na lista: extrair labels PT, criar key descritiva em `apps/dashboard/src/i18n.ts`, adicionar tradução EN equivalente, substituir literal por `t("key")` no JSX
- [ ] Padrão de naming de keys: `{page-or-feature}.{element}.{action-or-property}`. Exemplo: `economia.savings-card.title`, `specs.detail.delete-button`
- [ ] Para textos curtos repetidos (Cancelar/Confirmar/Salvar): usar keys compartilhadas em `common.{verb}`
- [ ] `pnpm --filter mustard-dashboard build` verde
- [ ] Manual smoke: rodar dashboard, abrir Settings, trocar lang pt-BR → en-US, navegar pelas páginas tocadas, confirmar que tudo virou EN

## Dependências

- [[2026-05-26-template-agnostic-audit]] Wave 5+W7 (SupportedLocale + UserLocale split, i18n.ts type Lang BCP-47) — já entregue.
- Não bloqueia nada — é puramente cleanup.

## Limites

- MODIFY: 25+ arquivos `.tsx` em `apps/dashboard/src/` + `apps/dashboard/src/i18n.ts` (provavelmente)
- FORA: i18n keys já existentes (não renomear); comentários de código; pasta `apps/dashboard/src-tauri/` (Rust não tem labels UI); arquivos `.test.tsx`/`.test.ts`
