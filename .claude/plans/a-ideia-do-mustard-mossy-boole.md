# Plano — Suíte ERP Mustard: core MyApp + plugin ERPBrain, Supabase + PowerSync

## Context

Comercializar o Mustard como suíte de desenvolvimento para ERP legado. O motor do Mustard
(`scan`, `/feature`, skills) orquestra uma arquitetura strangler-fig: o ERP legado fica
intocado e a modernização acontece numa camada moderna ao lado dele, sobre Supabase.

Decisões travadas com o usuário (substituem versões anteriores deste plano):

1. **MyApp é um core pronto, não um scaffold vazio.** O usuário entrega uma base de API já
   pré-configurada — login pronto, agentes, recursos já mapeados. **O cliente só cria coisas
   novas; nunca mexe no core.**
2. **ERPBrain** é o **mapeamento de sincronização ERP → Supabase**. A sincronização é um
   recurso configurável que **chama procedures ou views do banco**. O usuário entrega o
   mapeamento do **WinThor pronto** (ele já conhece o WinThor); outros ERPs o cliente configura.
3. As **tabelas do Mustard ficam no Supabase** — projeto Supabase configurado pelo cliente.
4. **PowerSync** entra como engine de sincronização **Supabase ⇄ MyApp** (offline-first):
   lê o WAL do Postgres do Supabase, mantém um SQLite local no app em sync, fila de upload
   para escrita. É a camada app-sync — distinta da ingestão ERP→Supabase.
5. O Mustard **mapeia tudo via `scan`** (estrutura, tabelas ERP e Supabase, plugins) e atua
   como **grande gerador de specs** para o Claude Code implementar as coisas novas.

## Arquitetura — camadas

```
ERP legado (WinThor, Oracle)          fonte, intocado
        │
        │  ERPBrain: mapeamento de sync — chama procedures/views do ERP
        │  (WinThor entregue pronto; outros ERPs o cliente configura)
        ▼
Supabase (Postgres do cliente)        plano de dados — "tabelas do Mustard"
        │
        │  PowerSync: sync Supabase ⇄ app (offline-first, WAL → SQLite local)
        ▼
MyApp (core pronto: API + frontend Vite/React/Shadcn + login + agentes)
        │
        └── extensões do cliente      criadas via /feature; core protegido
```

- **ERP** — fonte operacional; permanece como está.
- **ERPBrain** — define e executa a ingestão ERP→Supabase via procedures/views.
- **Supabase** — Postgres gerenciado; hospeda as tabelas canônicas.
- **PowerSync** — sync offline-first entre Supabase e o app MyApp.
- **MyApp** — core entregue pronto; o cliente só adiciona features novas.
- **Mustard** — scaneia tudo, gera specs, orquestra a criação das coisas novas.

## Os dois plugins

Distribuídos pelo mecanismo já existente: `mustard add <name>`
(`apps/cli/src/commands/add.ts`) — instala de `github.com/mustard-templates/<name>` ou npm
`mustard-template-<name>` via manifesto `mustard-template.json` (`files[]` + `hooks_additions[]`).

### Plugin MyApp — o core pronto
- Entrega um projeto fullstack **Vite + React + Shadcn** funcionando: API base, login,
  agentes, recursos já mapeados.
- O **core é protegido**: o `path_guard` (hook `mustard-rt`) bloqueia edição dos diretórios
  do core; features novas vão para diretórios de extensão. O cliente estende, não modifica.

### Plugin ERPBrain — mapeamento + sincronização ERP→Supabase
- Recurso de mapping declarativo: para cada tabela Supabase, qual **procedure/view do ERP**
  alimenta os dados, com colunas, transformações, chave e agendamento.
- Mapeamento **WinThor entregue pronto**; demais ERPs configuráveis pelo cliente.
- Carrega skills de domínio fiscal-br/tributário como referência.

## O motor Mustard

- **Scan** (`sync-detect.js` + `sync-registry.js`): mapeia o monorepo (subprojetos, papéis),
  o **schema do Supabase** (introspecção Postgres — novo scanner) e os plugins instalados.
- **Gerador de specs**: com estrutura + tabelas conhecidas, `/feature` gera specs precisas
  para o Claude Code implementar as features novas sobre o core MyApp.

## Plano faseado

### Fase 0 — Esqueleto dos plugins
- Pacotes `mustard-template-myapp` e `mustard-template-erpbrain` com manifesto.
- Validar `mustard add myapp` e `mustard add erpbrain` ponta-a-ponta.

### Fase 1 — MyApp core + proteção de core
- Empacotar a base pronta (API + Vite/React/Shadcn + login + agentes + recursos mapeados).
- Estender o `path_guard` para bloquear edição dos diretórios do core; definir convenção de
  diretórios de extensão onde features novas são permitidas.
- `sync-detect.js` `detectRole()` + `ROLE_AGENT_MAP`: reconhecer o subprojeto MyApp.

### Fase 2 — Supabase no scan
- Novo scanner Postgres em `templates/scripts/registry/scanners/`: introspecta tabelas/
  colunas/FKs do Supabase via connection string configurável (`mustard.json`/env, nunca
  hardcoded) e popula o `entity-registry.json`.

### Fase 3 — PowerSync (Supabase ⇄ MyApp)
- Integrar o PowerSync SDK no core MyApp: sync rules, SQLite local, fila de upload.
- Configuração da connection do PowerSync via env do cliente.

### Fase 4 — ERPBrain (mapping ERP→Supabase)
- Formato do recurso de mapping: `erpbrain/mappings/*.json` — tabela Supabase ← procedure/
  view do ERP, colunas, transformações, chave, agendamento.
- Entregar o mapping do **WinThor pronto**.
- Executor da sincronização (lê via procedure/view, upsert no Supabase, log e conflito).

### Fase 5 — Gerador de specs ciente do todo
- `/feature` usa o scan (estrutura + tabelas ERP/Supabase + core + plugins) para gerar specs
  completas das features novas — o Claude Code implementa a partir delas.

### Fase 6 — Empacotamento comercial
- Fluxo de instalação, licenciamento/gating, documentação dos dois plugins.

## MVP recomendado (fatia vertical — fazer primeiro)

Provar a arquitetura inteira com **uma entidade só**:
- 1 tabela Supabase (ex.: `nota_fiscal`) introspectada pelo scan.
- 1 mapping ERPBrain WinThor: view/procedure `PCNFSAID` → `nota_fiscal` (read-only).
- Sync ERP→Supabase rodando para essa tabela.
- PowerSync mantendo `nota_fiscal` no MyApp offline-first.
- MyApp listando `nota_fiscal` (core); 1 feature de extensão criada via `/feature`.

Se a fatia rodar ERP → Supabase → PowerSync → MyApp ponta-a-ponta, replicar para as demais.

## Arquivos críticos

| Arquivo | Mudança |
|---|---|
| `apps/cli/src/commands/add.ts` | Validar instalação dos plugins MyApp e ERPBrain |
| `apps/cli/templates/scripts/sync-detect.js` | Papel MyApp em `detectRole()` + `ROLE_AGENT_MAP` |
| `apps/rt/src/hooks/` (`path_guard`) | Proteção dos diretórios do core MyApp |
| `apps/cli/templates/scripts/registry/scanners/` | Novo scanner de introspecção Postgres/Supabase |
| `apps/cli/templates/scripts/sync-registry.js` | Acoplar o scanner Supabase |
| `.claude/entity-registry.json` | Hospedar tabelas do Supabase e do ERP |
| `mustard-template-myapp` (novo pacote) | Core pronto Vite/React/Shadcn + API + login + agentes |
| `mustard-template-erpbrain` (novo pacote) | Recurso de mapping + WinThor pronto + skills fiscais |

## Verificação

1. **Plugins instalam:** `mustard add myapp` e `mustard add erpbrain` → arquivos copiados,
   manifesto válido.
2. **Core protegido:** tentar editar arquivo do core MyApp → `path_guard` bloqueia; editar em
   diretório de extensão → permitido.
3. **Scan vê o Supabase:** connection configurada, `bun scripts/sync-registry.js` →
   `entity-registry.json` lista as tabelas do Supabase.
4. **Sync ERP→Supabase:** disparar o executor → linhas via `PCNFSAID` chegam em `nota_fiscal`.
5. **PowerSync E2E:** alterar `nota_fiscal` no Supabase → reflete no SQLite local do MyApp.
6. **App E2E:** MyApp lista `nota_fiscal`; feature de extensão criada via `/feature` funciona.
7. **QA:** AC com comandos runnable; usar `templates/scripts/` (fonte viva).

## Riscos honestos

- **Escopo grande** — core + dois plugins + scanner Supabase + dois níveis de sync. Por isso
  o MVP é fatia vertical de uma entidade.
- **PowerSync cobre só Supabase ⇄ app.** A ingestão ERP→Supabase é ETL separado (procedures/
  views do ERPBrain) — não confundir as duas camadas de sync.
- **Proteção de core** depende de convenção de diretórios clara; sem isso o `path_guard` não
  tem como distinguir core de extensão.
- **Acesso ao ERP** via procedure/view exige que o cliente exponha esses objetos no banco;
  o WinThor o usuário entrega pronto, os demais dependem do cliente.
- **Conhecimento fiscal muda** (Reforma Tributária até 2033) — manter como skill/referência
  sincronizável, nunca alíquota hardcoded.
