# F4 — Migração de instalações existentes para o modelo de plugin

O Mustard 2.0 deixa de copiar seu conteúdo para dentro do `.claude/` de cada
projeto e passa a distribuí-lo como um **plugin nativo do Claude Code**. Este
documento descreve como uma instalação antiga (feita pelo `mustard init` legado,
que copiava `commands/`, `skills/`, `agents/`, `refs/` e embutia `hooks` no
`settings.json`) migra para o novo modelo.

## O que muda

| Antes (legado) | Depois (plugin) |
|---|---|
| `mustard init` copiava templates para `.claude/{commands,skills,agents,refs}` | O plugin fornece esse conteúdo; o `.claude/` do projeto NÃO os carrega |
| `settings.json` com bloco `hooks` inline + `mcpServers` | Hooks e MCP vêm do plugin (`hooks/hooks.json`, `.mcp.json`) |
| `mustard update` / `refresh-claude` sincronizavam templates | Atualização é do marketplace (`claude plugin` / versão do `plugin.json`) |
| `mustard add skill:<nome>` instalava skills embarcadas | Skills públicas vêm do marketplace; molds `{role}-pattern` seguem gerados por projeto |

**Não muda:** `mustard.json` na raiz, os specs em `.claude/spec/`, a memória, e
as skills `{role}-pattern` geradas pelo `/scan` — tudo isso é conteúdo do
projeto e permanece.

## Procedimento de migração (guiado)

1. **Instale os binários novos** (`mustard`, `mustard-rt`, `scan`) — o
   instalador de plataforma (`install.ps1` / `.deb` / `.sh`) já aponta para a
   versão com plugin. Isso também traz o conserto do `work_branch_gate` em
   worktree aninhado (F2), que só passa a valer no binário reinstalado.

2. **Registre o marketplace e habilite o plugin.** Rodar `mustard init` num
   projeto já existente é idempotente: ele mescla no `settings.json` as chaves
   `extraKnownMarketplaces.mustard` e `enabledPlugins."mustard@mustard": true`
   sem sobrescrever suas outras chaves. Preencha a URL real do repositório do
   plugin no lugar do placeholder `REPLACE_WITH_MUSTARD_PLUGIN_MARKETPLACE_GIT_URL`.

3. **Remova o conteúdo antigo copiado** (senão coexiste com o do plugin, gerando
   um `/feature` do projeto E um `/mustard:feature` do plugin):
   apague de `.claude/` apenas os diretórios que eram gerenciados pelo Mustard —
   `commands/mustard/` (ou os comandos copiados), as skills públicas embarcadas,
   `agents/mustard-*`, `refs/`, e o `pipeline-config.md`. **Preserve**
   `.claude/skills/*-pattern` (molds do seu projeto), `.claude/spec/`,
   `.claude/memory/`, `.claude/knowledge/` e o `.claude/CLAUDE.md`.

4. **Reduza o `settings.json`:** remova o bloco `hooks` inline (o plugin os
   fornece) e o `mcpServers` (idem). Mantenha `env`, `permissions`, `statusLine`,
   `plansDirectory`. Rodar `mustard init` de novo já escreve a semente reduzida;
   se você tinha customizações no `hooks`, elas devem sair (o plugin é a fonte).

5. **Reinicie o Claude Code** e confirme: o plugin `mustard` aparece habilitado,
   os comandos resolvem como `/mustard:feature`, e os hooks disparam via
   `${CLAUDE_PLUGIN_ROOT}/bin/mustard-rt`.

## Nota de automação (follow-up)

O passo 3 (remover conteúdo legado preservando o gerado pelo usuário) é hoje
manual. Um modo `mustard init --migrate` que faça essa poda seletiva
automaticamente é um follow-up natural se a migração em massa (ex.: sialia e
outros consumidores) justificar.

## Limite de verificação honesto

A estrutura do plugin foi validada ponta a ponta de forma headless: manifestos
parseiam, o formato do comando de hook está correto, e o `init` fino produz a
semente de projeto exata. O único passo que exige um Claude Code real (não
verificável headless) é o carregamento do plugin a partir do marketplace e o
disparo efetivo dos hooks via `${CLAUDE_PLUGIN_ROOT}` — isso deve ser confirmado
num install real com a URL do marketplace preenchida e os binários em `plugin/bin/`.