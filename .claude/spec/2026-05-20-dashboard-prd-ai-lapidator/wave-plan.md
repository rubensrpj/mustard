# Plano de Waves: dashboard-prd-ai-lapidator (v2)

### Status: draft
### Scope: full (wave plan)
### Total waves: 3
### Lang: pt
### Checkpoint: 2026-05-20T00:00:00Z

## Contexto

O dashboard Mustard já tem uma página `PRD Builder` (`apps/dashboard/src/pages/Prd.tsx`) onde o usuário preenche ~10 campos manualmente (tipo, slug, título, escopo, resumo, layers, boundaries, checklist, AC, decisões, não-objetivos) e copia o markdown final pro CLI. O atrito real é triplo: o usuário tem uma intenção curta na cabeça e precisa estruturar tudo à mão; o sistema já conhece dados que ele está sendo forçado a redigitar (entidades do registry, paths do projeto, slug derivável); e a UI não distingue visualmente onde começar nem o que é mais importante.

Esta feature ataca os três atritos juntos: adiciona um **lapidador IA** (`/mustard:prd` + Tauri command + UI hero) que recebe a intenção livre e preenche os campos automaticamente, **reduz digitação manual** com slug auto, escopo auto-inferido, entity picker multi-select carregado do `entity-registry.json`, pre-populate de paths sugeridos, listas editáveis (em vez de textareas) para boundaries/checklist, e **redesign incremental** da página com hero section pra intenção e grupos colapsáveis pros campos secundários. O confronto contra registry + Glob é **puramente mecânico** — IA estrutura, não opina sobre se a ideia faz sentido.

A IA é o `claude` CLI já instalado/logado (modo `--print`), forçando `--model claude-sonnet-4-6` pra rapidez e economia de quota. Consome a quota Claude Code que o usuário já paga — zero custo Anthropic novo. **Execução 100% background no Windows** (CREATE_NO_WINDOW flag em Rust) — nenhuma janela de console aparece. OpenRouter fica como provider opcional pra spec futura (código já prepara `trait PrdProvider` pra plugar sem refator).

## Métrica de sucesso

Usuário escreve 1 textarea de 1-3 linhas, clica "Lapidar com IA", **sem nenhuma janela visível abrir no Windows**, em ≤30s vê: escopo inferido, entidades pré-marcadas, boundaries sugeridos como linhas editáveis, AC sugeridas em cards. Tempo total de "ideia → PRD pronto pra copiar" cai de ~5 minutos (preenchimento manual completo) pra ≤1 minuto típico.

## Não-Objetivos

- **NÃO validar negócio.** A IA não opina sobre se a ideia faz sentido — só estrutura.
- **NÃO criar spec no disco** a partir do dashboard. A página PRD continua só copiando pro clipboard; quem cria `.claude/spec/active/{slug}/spec.md` é o `/mustard:feature` no CLI.
- **NÃO ler arquivos de código** durante o confronto. Apenas Grep no `entity-registry.json` + Glob de paths.
- **NÃO implementar OpenRouter agora.** Trait `PrdProvider` fica preparada; provider concreto fica pra spec futura.
- **NÃO substituir o form manual.** Form continua disponível pra quem quiser preencher direto sem IA.
- **NÃO usar Task(Explore)** dentro do `/mustard:prd`. Comando é leve, ≤5 segundos típicos de execução interna.
- **NÃO substituir `prd-template.ts`** (gerador de markdown). Wave 3 só altera a coleta de input; o gerador continua igual.
- **NÃO criar componentes novos no `components/page/`** — a primitiva `CollapsibleGroup` já existe e é reusada; novos componentes ficam isolados em `components/prd/` (sem poluir o barrel global de page primitives).

## Tabela de Waves

| Wave | Spec | Role | Depende de | Resumo |
|------|------|------|------------|--------|
| 1 | [[wave-1-general]] | general | — | Slash command `/mustard:prd` em `apps/cli/templates/commands/mustard/prd/SKILL.md` + cópia em `.claude/commands/mustard/prd/SKILL.md`. Recebe intenção livre, **infere escopo**, **pré-marca entidades**, **pre-popula paths**, devolve JSON estruturado no shape do `PrdForm`. |
| 2 | [[wave-2-general]] | general | [[1]] | Tauri command `lapidate_prd` + `check_claude_available` em `apps/dashboard/src-tauri/src/prd_lapidator.rs`. Executa `claude -p` **em background invisível no Windows** (CREATE_NO_WINDOW flag). Trait `PrdProvider` pra OpenRouter futuro. |
| 3 | [[wave-3-ui]] | ui | [[2]] | Lapidador + redesign incremental: `IntentHero` (textarea + botão Lapidar), `EntityPicker` (multi-select filtrável), `EditableList` (boundaries/checklist), `useEntityRegistry`, slug auto, escopo auto, pre-populate, layout em `CollapsibleGroup`. |

## Cobertura de Críticas (audit consolidado dos concerns da conversa)

| Item levantado | Onde tratado |
|---|---|
| "Quero usar OpenRouter free" | Não-objetivo desta spec — trait `PrdProvider` deixa o plug pronto; provider OpenRouter vira spec futura sob demanda. |
| "Não quero gastar com API key Anthropic" | Wave 2 usa `claude -p` que consome quota do plano Claude Code existente. Zero key/custo novo. |
| "PRD ≠ spec, é estágio anterior" | Wave 1 — `/mustard:prd` NÃO cria `.claude/spec/active/...`. Devolve JSON. Spec só nasce quando user roda `/mustard:feature` depois. |
| "Hoje no dashboard não consigo usar Claude CLI" | Wave 2 — Tauri command Rust executa `Command::new("claude")` no backend. |
| "Quero preencher os CAMPOS, não texto livre" | Wave 3 — handler distribui JSON via `setForm({...prev, ...parsed})` campo a campo. |
| "Atalho gerar PRD primeiro, depois feature" | Workflow: dashboard lapida → user copia → user roda `/mustard:feature`. Wave 3 mantém botão "Copiar com /mustard:feature". |
| "Diretórios mapeados e entidades apenas" | Wave 1 — confronto = só Grep no `entity-registry.json` + Glob de paths. Sem leitura de código. |
| "Não é para validar negócio" | Não-objetivo explícito. Wave 1 system prompt instrui "não opinar". |
| Logging de free models (privacidade) | Resolvido por design — `claude -p` não usa OpenRouter, sem logging em terceiros. |
| "Melhorar visualmente a página de PRD" | Wave 3 expandida — hero section, `CollapsibleGroup`, hierarquia visual nas ações, layout reorganizado. |
| "Dados como tabelas / pre-carregados" | Wave 3 — boundaries e checklist viram `EditableList` (listas crescentes com inputs), entity picker carrega registry. |
| "Slug pode ser automático" | Wave 3 — campo slug removido; derivado de `slugify(title)`. AC-6 verifica. |
| "Escopo eu não sei se será full ou light" | Wave 3 — escopo `auto` por default; Wave 1 infere; user pode override com toggle exposto pós-lapidate. |
| "Mapeamento de entidades pra selecionar" | Wave 3 — novo `EntityPicker` multi-select filtrável carrega `entity-registry.json` via `useEntityRegistry`; entidades do `_confront.entitiesFound` vêm pré-marcadas. |
| "Sem combo (quero adicionar mais de um)" | Wave 3 — `EntityPicker` usa lista filtrável com checkboxes (não Combobox single-select), permitindo seleção múltipla; `EditableList` cresce linha-a-linha. |
| "Ao chamar claude, não abrir janela" | Wave 2 — `CREATE_NO_WINDOW` flag (`0x08000000`) condicional via `#[cfg(windows)]`. AC-5 verifica. |

## Preocupações (riscos vivos)

- **Dependência de `claude` no PATH do dashboard.** Em máquinas sem Claude Code, o botão quebra. Wave 2 expõe `check_claude_available` e Wave 3 desabilita o botão com tooltip "Claude CLI não encontrado — instale via claude.ai/cli". Gracioso, não silencioso.
- **JSON estruturado pode vir malformado** (~5% das chamadas Sonnet). Wave 2 valida com schema; em falha, retorna `InvalidJson(String)` com o stdout cru no payload pra debug. Wave 3 mostra toast com "Resposta da IA mal-formada — tente reformular a intenção".
- **Dupla fonte da verdade do shape do PRD** (slash command + struct Rust + interface TS). Mitigação: `PrdData` Rust e `LapidatedPrd` TS são estrutura ÚNICA com camelCase serde; Wave 1 referencia explicitamente `LapidatedPrd` no SKILL.md; AC-3 cada wave verifica presença dos campos.
- **`pnpm tauri:dev` muda cwd para `src-tauri/`** (memória `tauri_current_dir_gotcha`). Wave 2 recebe `projectPath` absoluto do frontend; nunca usa cwd relativo.
- **Spinner bloqueia UI por 10-20s.** Wave 3 desabilita form durante `isLapidating`, mostra spinner + texto "Lapidando…"; opcional `AbortController` se Tauri command suportar cancel (não obrigatório pra v1).

## Rationale

Decomposição em 3 waves justificada por: `file_count = 11` (slash command + duas cópias + Tauri Rust + lib.rs + 4 componentes React + 1 hook + tipos + wrapper API + edit Prd.tsx), `layer_count = 3` (CLI templates + Rust backend + React frontend), `independent_subbehaviors = 3` (slash command output JSON, Tauri provider abstraction, integração + redesign UI). Wave 3 cresceu significativamente após o feedback do user (de 3 arquivos pra 7) — ainda é uma wave única porque todo o trabalho é frontend coeso e compartilha state da página.
