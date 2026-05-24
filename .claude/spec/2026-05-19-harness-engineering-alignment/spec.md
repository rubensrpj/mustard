# Alinhamento com Harness Engineering

> **Referência externa:** `~/Downloads/book1-claude-code-en.pdf` (agentway.dev, 2026-04-01) — capítulos 5-7, 9 e Appendix A. Esta spec endereça 4 desalinhamentos concretos identificados em auditoria contra o livro.

## PRD

## Contexto

Mustard implementa ~85% dos 10 princípios de Harness Engineering, mas a auditoria contra o texto-fonte expôs 4 desalinhamentos que o usuário já vive na prática mesmo sem nomeá-los assim. O REVIEW phase despacha um agente diferente do impl (separação de papéis ok), mas o prompt do reviewer só carrega um checklist de 7 categorias — falta o framing cético explícito que o `coordinatorMode.ts` do Claude Code carrega literalmente. O `pre_compact.rs` salva snapshot de continuação (bom), mas nenhum template diz ao agente "retome onde parou, sem reapresentar contexto" — agentes pós-compact tendem a recapitular. Entries de `knowledge.json` são injetadas em `SessionStart` sem nenhum sinal de que podem estar stale, contradizendo a memória `feedback_check_canonical_store` que o próprio usuário já gravou. E a substituição deliberada do `MEMORY.md` (índice do livro) por `memory/decisions.json` ranqueado por confidence é boa decisão de design, mas invisível para quem chega ao projeto vindo do livro.

## Usuários/Stakeholders

Engenheiros usando Mustard em projetos reais; equipe que opera pipelines `/feature`/`/bugfix` e precisa confiar no veredito do REVIEW; futuros mantenedores que vão comparar Mustard ao livro de referência. Pedido implícito do usuário ao solicitar avaliação contra o livro.

## Métrica de sucesso

Auditoria repetida contra o livro vira ~90% aderência (de ~85%): 3 dos 4 furos fechados via mudança de prompt/schema, o 4º via 1 parágrafo de doc. Sinal observável: próximo REVIEW dispatch carrega as 4 diretivas céticas e próxima injeção de `knowledge.json` em SessionStart marca entries não-verificadas com hint visível.

## Não-Objetivos

- **Não enforçar** verificação de memória via hook bloqueante. O `verifiedAt: null` é sinal soft que o agente deve atender — enforcement com gate vira spec futura quando houver evidência de não-atendimento.
- **Não reescrever** o REVIEW phase como agente totalmente novo. Mudança é cirúrgica no prompt do reviewer dentro do `agent-prompt.md`.
- **Não tocar** no formato wire de `MEMORY.md` plain (o usuário tem o seu em `~/.claude/...`). Spec só documenta que Mustard projeto-gerado substitui isso por JSON ranqueado.
- **Não criar página "Audit" na dashboard** — diferido (respeita `feedback_dashboard_value_over_features`: pausar antes de adicionar feature de dashboard).
- **Não adicionar precedência declarada de prompt layers** (princípio 2 do livro) nem modo `ask` para path_guard (princípio 4) — sem sintoma observado; respeita `feedback_no_permission_loops` e `feedback_analysis_pattern` (subtrair > adicionar).

## Critérios de Aceitação

Testable, binary (pass/fail) criteria. Each MUST be executable and independent.

- [x] AC-1: Template de prompt do reviewer contém as 4 diretivas céticas (`stay skeptical`, `do not rubber-stamp`, `run tests with the feature enabled`, `investigate errors instead of dismissing`) — Command: `node -e "const s=require('fs').readFileSync('apps/cli/templates/refs/agent-prompt/agent-prompt.md','utf8');const p=['stay skeptical','do not rubber-stamp','run tests with the feature enabled','investigate errors'];process.exit(p.every(x=>s.includes(x))?0:1)"`

- [x] AC-2: Agent Return Format Compact instrui continuação sem reapresentação — Command: `node -e "const s=require('fs').readFileSync('apps/cli/templates/pipeline-config.md','utf8');process.exit(/resume.{0,40}where the prior turn|no apologies|no preamble|sem reapresent/i.test(s)?0:1)"`

- [x] AC-3: KnowledgeEntry escrita pelo hook carrega `verifiedAt` (null por default) e `sourceFiles` (array, possivelmente vazio) — Command: `cargo test -p mustard-rt knowledge_entry_carries_verification_metadata`

- [x] AC-4: SessionStart injeta entries não-verificadas (`verifiedAt: null`) com hint `(unverified — verify before recommending)` no texto de contexto — Command: `cargo test -p mustard-rt session_start_injection_marks_unverified_knowledge`

- [x] AC-5: `apps/cli/templates/CLAUDE.md` documenta substituição `MEMORY.md` → `memory/decisions.json + memory/lessons.json` com ranking por confidence — Command: `node -e "const s=require('fs').readFileSync('apps/cli/templates/CLAUDE.md','utf8');process.exit(/MEMORY\.md[\s\S]{0,400}memory\/(decisions|lessons)\.json|memory\/(decisions|lessons)\.json[\s\S]{0,400}MEMORY\.md/.test(s)?0:1)"`

- [x] AC-6: Workspace build inteiro passa sem regressão — Command: `cargo build -p mustard-core -p mustard-rt -p mustard-cli`

## Plano

## Informações da Entidade

**Entidade alterada:** `KnowledgeEntry` (existe no pipeline mas não no entity-registry de domínio — é entidade interna do harness). Hoje serializada como `serde_json::Value` solta no `apps/rt/src/hooks/knowledge.rs:259-260`:

```json
{"version": 1, "entries": []}
```

Cada entry hoje (extraído de `.claude/knowledge.json` real):
```
confidence, createdAt, description, id, lastSeen, name, occurrences, source, tags, type, updatedAt
```

**Mudança:** adicionar 2 campos opcionais (default-able) ao writer:
- `verifiedAt: null | ISO-8601` — quando o agente confirmou contra a realidade atual. Default `null` (não-verificado).
- `sourceFiles: string[]` — paths de arquivos citados pela entry, para o agente saber **o que** grep antes de recomendar. Default `[]` (sem âncora).

**Compat retroativa:** consumers existentes (`apps/dashboard/src/pages/Knowledge.tsx`, `apps/dashboard/src-tauri/src/watcher.rs`, `apps/rt/src/run/memory.rs`, `apps/rt/src/run/epic_fold.rs`) leem campos por chave — campos desconhecidos são ignorados em TS/Rust serde com `#[serde(default)]`. Sem migração de dados — entries antigas seguem válidas com defaults.

## Arquivos

**Templates payload** (`apps/cli/templates/`, agente `templates-impl`):
- `refs/agent-prompt/agent-prompt.md` — adicionar bloco `## REVIEW SKEPTICISM` quando `{role_block}` resolve para `review` (AC-1)
- `pipeline-config.md` — adicionar bullet de continuação ao bloco `## Agent Return Format (Compact)` (AC-2)
- `CLAUDE.md` — adicionar 1 parágrafo curto sob `## Stack` documentando substituição `MEMORY.md` → `memory/*.json` ranqueado (AC-5)

**Runtime** (`apps/rt/`, agente `rt-impl`):
- `src/hooks/knowledge.rs` — popular `verifiedAt: null` e `sourceFiles: []` no record gravado por `save_friction`; teste `knowledge_entry_carries_verification_metadata` (AC-3)
- `src/hooks/session_start.rs` — na função que monta o texto de injeção de knowledge, prefixar entries com `verifiedAt == null` com `(unverified — verify before recommending)`; teste `session_start_injection_marks_unverified_knowledge` (AC-4)
- `src/run/memory.rs` — **expansão de boundary durante EXECUTE** (rationale em `## Concerns`): inserir `verifiedAt: null` e `sourceFiles: []` no `json!({...})` da entry pushada em `knowledge` subcommand (linhas 334-346). É o write path canônico do `knowledge.json` que o reviewer cético identificou como faltando na lista original

**Total: 6 arquivos, 2 subprojetos.**

## Component Contract

Não aplicável — esta spec não toca UI/componentes.

## Tarefas

### templates-impl Agent (Wave 1, parallel-safe)

1. Em `apps/cli/templates/refs/agent-prompt/agent-prompt.md`, localizar onde o `{role_block}` é descrito e adicionar bloco condicional:
   ```markdown
   ## REVIEW SKEPTICISM (when role=review)
   - Stay skeptical. The implementer is not authoritative.
   - Do not rubber-stamp. If you cannot independently confirm a claim, reject it.
   - Run tests with the feature enabled — code presence is not effectiveness.
   - Investigate errors instead of dismissing them as unrelated.
   ```
   Inserir antes da seção `## ROLE`. Manter EN (código/template).

2. Em `apps/cli/templates/pipeline-config.md`, dentro do bloco `## Agent Return Format (Compact)` (linhas 257-273), adicionar como novo bullet sob "DO NOT include in the return:" um item positivo:
   ```markdown
   **DO include**: resume from where the prior turn stopped — no apologies, no preamble, no restatement of context. The parent already has it.
   ```
   Inserir logo após o último "DO NOT" bullet, antes do parágrafo "The parent orchestrator already has context".

3. Em `apps/cli/templates/CLAUDE.md`, dentro da seção `## Stack` (depois do parágrafo sobre Rust/hooks/skills), adicionar parágrafo:
   ```markdown
   ## Memory Layout — Substitution vs Harness Engineering Book

   The Harness Engineering book (§5.3) treats `MEMORY.md` as an entry index with a hard cap (200 lines / 25 KB). Mustard substitutes this with structured `memory/decisions.json` and `memory/lessons.json` ranked by confidence × recency. Same goal (index, not body), different mechanism — structured ranking lets `SessionStart` inject only the top-N relevant entries within a capped budget, rather than a fixed line-limit on a plain-text file. The `MEMORY.md` you see at `~/.claude/projects/<project>/memory/MEMORY.md` is your user-global memory (managed by Claude Code), not the project memory layer.
   ```

4. Validar: `node` AC-1, AC-2, AC-5 da seção `## Critérios de Aceitação`.

### rt-impl Agent (Wave 1, parallel-safe)

1. Em `apps/rt/src/hooks/knowledge.rs`, localizar onde `record` (`serde_json::Map<String, Value>`) é montado dentro de `save_friction` (perto da linha 281-291). Adicionar:
   ```rust
   record.insert("verifiedAt".to_string(), Value::Null);
   record.insert("sourceFiles".to_string(), Value::Array(Vec::new()));
   ```
   Adicionar antes da inserção em `store_entries`. Se houver outras funções que materializam entries (procurar por `"createdAt"` no arquivo), aplicar o mesmo. Comentários EN.

2. Adicionar teste `#[test] fn knowledge_entry_carries_verification_metadata()` em `apps/rt/src/hooks/knowledge.rs` (módulo `tests` ao fim do arquivo):
   - Cria uma `FrictionEntry` sintética.
   - Roda `save_friction` em tempdir.
   - Lê o JSON gravado, assert que primeiro entry tem `verifiedAt == null` e `sourceFiles == []`.

3. Em `apps/rt/src/hooks/session_start.rs`, localizar a função que monta o texto de `additionalContext` para knowledge entries (perto das constantes `MEMORY_MAX_CHARS=2000`, `KB_MAX_ENTRIES=5`). Para cada entry com `verifiedAt: null`, prefixar a linha de descrição com `(unverified — verify before recommending) `.

4. Adicionar teste `#[test] fn session_start_injection_marks_unverified_knowledge()`:
   - Cria tempdir com `.claude/knowledge.json` contendo 1 entry com `verifiedAt: null` e 1 com `verifiedAt: "2026-05-19T00:00:00Z"`.
   - Invoca o pedaço de session_start que monta a injeção.
   - Assert que o texto contém `(unverified — verify before recommending)` antes da entry não-verificada, e NÃO contém antes da verificada.

5. Validar: `cargo test -p mustard-rt` (AC-3, AC-4); `cargo build -p mustard-core -p mustard-rt -p mustard-cli` (AC-6).

### review-cli + review-rt Agents (Wave 2)

Dispatch standard de REVIEW por subprojeto, em paralelo. Cada um lê os arquivos alterados do seu subprojeto e aplica o checklist de 7 categorias. **Auto-validação meta**: estes reviewers serão os primeiros a operar com o novo prompt cético — se algum aprovar sem cobrir as 4 diretivas, isso é AC-1 falhando na prática.

## Dependências

- Nenhuma dependência entre Wave 1 templates-impl e rt-impl — paths não se cruzam.
- Wave 2 depende da Wave 1 completa.
- Nenhuma dependência das outras 2 specs ativas (`artifact-update-followups`, `dashboard-phase-from-sqlite`) — nenhum arquivo em comum.
- Bloqueador externo: nenhum.

## Limites

**Boundaries** — só estes paths podem ser tocados nesta spec:
- `apps/cli/templates/refs/agent-prompt/agent-prompt.md`
- `apps/cli/templates/pipeline-config.md`
- `apps/cli/templates/CLAUDE.md`
- `apps/rt/src/hooks/knowledge.rs`
- `apps/rt/src/hooks/session_start.rs`
- `apps/rt/src/run/memory.rs` — **adicionado durante EXECUTE** após achado do reviewer cético (ver `## Concerns`)

Tudo fora desta lista deve disparar `[BOUNDARY WARNING]`. Em particular:
- **Não tocar** em `apps/dashboard/` — schema de knowledge é forward-compat por design serde.
- **Não tocar** em `packages/core/src/knowledge.rs` nesta spec — o struct lá é só para extraction/selection, não é o writer do JSON. Se em runtime descobrir que o writer real usa um struct typed que precisa de campo novo, escalar como `BLOCKED` em vez de expandir scope.
- **Não tocar** em `.claude/knowledge.json` real do projeto Mustard (dado, não código).

## Concerns

- O AC-2 verifica o texto da continuação no `pipeline-config.md`, mas o agente real pode continuar apologizando — é diretiva soft. Mitigação aceita: spec futura adiciona telemetria de "agent prefix patterns" se virar problema real.
- O `verifiedAt` injetado como hint é decorativo sem hook que enforce; isso é deliberado (Não-Objetivo declarado). Se em 2-3 sessões os agentes seguirem recomendando sem verificar, spec futura adiciona gate.
- Reviewers céticos podem rejeitar PRs que antes eram aprovados — é o objetivo, mas pode aumentar fix-loops temporariamente. Tolerado.
- **REVIEW unconditional block (CONCERN-A do templates-explorer):** o bloco `## REVIEW SKEPTICISM` no `agent-prompt.md` ficou em PREFIX-STABLE (não-condicional). A conditionality vive apenas em prose-note. Significa que dispatches de impl/explore/plan também recebem as 4 diretivas céticas. Impacto: 2 dos 4 itens ("the implementer is not authoritative" e "do not rubber-stamp") são confusos para um impl agent — mas tecnicamente inofensivos. O fix correto (mover para `{review_skepticism}` placeholder preenchido por role) toca os orchestrators (`feature/SKILL.md`, `resume/SKILL.md`) e fica para spec follow-up. Aceito por ora.
- **Boundary expansion durante EXECUTE:** `apps/rt/src/run/memory.rs` foi adicionado depois que o reviewer rt-explorer identificou que o spec falava em "KnowledgeEntry escrita pelo hook" mas o writer real de `knowledge.json` (não `friction.json`) é `mustard-rt run memory`. O `save_friction` em `hooks/knowledge.rs` escreve em `friction.json` (telemetria), não em `knowledge.json`. AC-4 funciona em produção porque `is_verified` default-é-false quando o campo falta — então entries antigas recebem o prefix corretamente. Mas para futuras entries serem **explicitamente** marcadas como não-verificadas, `memory.rs` também precisa do campo. Patch de 2 linhas, boundary-gate avisou (WARN, não bloqueou). Decisão senior: incluir no escopo deste pipeline em vez de spec follow-up.
- **Lowercase-start bullets (CONCERN-C):** os bullets do `## REVIEW SKEPTICISM` ficaram em lowercase ("- stay skeptical." vs sentence-case do resto do arquivo) porque o validador AC-1 é case-sensitive. Diferido — fix de capitalização vira polish posterior, não afeta funcionalidade.
