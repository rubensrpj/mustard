# TF — cluster_discovery devolve [] no próprio repo Mustard

## Status: superseded

Hipótese errada. A causa real do `entities:[]` não era `MUSTARD_CLUSTER_MIN_FILES` mas sim um bug de campo no `build_profile` em `apps/rt/src/run/scan/interpret.rs:191` — a função buscava `cluster.get("files")`, mas o `cluster_discovery` emite `samples`. Fix aplicado na própria Wave 2 do parent `2026-05-24-mustard-unification`. Registry agora gera 25 entities + 2 enums. Esta TF não é mais necessária; mantida no histórico como rastro do diagnóstico.

## PRD

## Contexto

Surgiu na Wave 2 do parent `2026-05-24-mustard-unification`: `mustard-rt run sync-registry` no monorepo Mustard produz `entities: []` para todos os 4 subprojetos (cli, rt, dashboard, core). A Wave 2 reescreveu corretamente o canal de invocação do cold-path (`Command::new("claude")` substituindo SDK Anthropic), mas isso não popula entidades — o profile que chega ao modelo já vem com `clusters: []`, então o modelo não tem o que classificar.

Causa raiz suspeita: `MUSTARD_CLUSTER_MIN_FILES` default = 5 em `apps/rt/src/run/scan/cluster_discovery.rs:34` (`min_files_per_suffix()`). O Mustard é metaframework Rust sem CRUD — provavelmente nenhum sufixo de nome de arquivo repete 5 vezes nos 4 subprojetos. Heurística que é adequada para projetos de aplicação (50+ entities `*Entity.cs` / `*.entity.ts`) é inadequada para o próprio Mustard.

## Usuários/Stakeholders

- **Rubens** — operador do Mustard; quer dogfooding real (Mustard usa Mustard).
- **`/economia` do dashboard** — depende de `entity-registry.json` populado para algumas métricas.

## Métrica de sucesso

`mustard-rt run sync-registry` no Mustard monorepo produz `entities[]` com pelo menos 5 entradas relevantes (módulos públicos dos crates `mustard-rt`, `mustard-cli`, `mustard-core`, ou componentes React/Tauri do dashboard).

## Não-Objetivos

- Não reescrever `cluster_discovery.rs` (Wave 1 do project-profiler está fechada).
- Não adicionar dependência stack-specific (Rust hard-coded).
- Não baixar default global de `MUSTARD_CLUSTER_MIN_FILES` (quebra projetos grandes).

## Critérios de Aceitação

- [ ] **AC-TF-1.** Diagnóstico documentado: rodar `MUSTARD_CLUSTER_MIN_FILES=2 mustard-rt run sync-registry` e confirmar se o registry sai populado. Comando: `rtk MUSTARD_CLUSTER_MIN_FILES=2 mustard-rt run sync-registry && node -e "const j=JSON.parse(require('fs').readFileSync('.claude/entity-registry.json','utf8'));console.log('entities:',j.entities?.length||0);process.exit((j.entities?.length||0)>0?0:1)"`
- [ ] **AC-TF-2.** Solução adotada: documentar em `apps/rt/CLAUDE.md` (ou na ADR `2026-05-24-mustard-unification.md` durante W13) qual env override aplicar quando rodar scan no próprio Mustard. Alternativa: detector heurístico que baixa o min para 2 quando `total_files < N`.

## Plano

## Arquivos

- `apps/rt/src/run/scan/cluster_discovery.rs` (opcional: heurística adaptativa)
- `apps/rt/CLAUDE.md` (documentação do override)
- `.claude/entity-registry.json` (regenerar)

## Tarefas

- [ ] **TF.1.** Rodar `MUSTARD_CLUSTER_MIN_FILES=2 mustard-rt run sync-registry` no Mustard repo e capturar saída.
- [ ] **TF.2.** Se entidades aparecerem, decidir entre: (a) env override permanente em `.claude/.env` do Mustard, (b) heurística adaptativa em `cluster_discovery.rs` (`min_files = max(2, total_files/100)` ou similar), (c) doc-only no `apps/rt/CLAUDE.md`.
- [ ] **TF.3.** Se entidades NÃO aparecerem com min=2, investigar segunda camada: o profile builder pode estar filtrando files no `build_profile` antes do modelo ver.
- [ ] **TF.4.** Aplicar a opção escolhida. Re-validar com `sync-registry` limpo.

## Limites

- ≤100 LOC totais (regra tactical-fix).
- Nenhum contrato público alterado (sem mudança em CLI flags, sem schema novo).
- Sem nova dependência.
