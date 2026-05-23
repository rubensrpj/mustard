# Project Profiler — motor de scan unificado + grafo de injeção

### Stage: Plan
### Outcome: Active
### Flags: 
### Checkpoint: 2026-05-22T00:00:00Z
### Lang: pt

## PRD

## Contexto

O scan do Mustard é a base de tudo: dele saem os arquivos de contexto (skills, recipes, convenções) que o orquestrador injeta nos agentes durante cada pipeline. Hoje esse motor tem três limitações que se reforçam. Primeiro, é lento — para cada subprojeto ele varre a árvore de arquivos várias vezes, uma por faceta (entidades, enums, rotas, DTOs, serviços) mais a descoberta de clusters, relendo o mesmo arquivo cerca de seis vezes, em uma única thread. Segundo, a detecção de entidades está presa a oito arquivos Rust escritos à mão, um por linguagem, que reconhecem apenas convenções fixas e falham quando o projeto foge delas. Terceiro, os artefatos gerados são monolíticos e isolados — quando o orquestrador precisa de um, injeta o arquivo inteiro, repetindo a mesma convenção em vários agentes e gastando tokens com contexto que não importa para a tarefa. O resultado é um motor caro de manter, caro em tokens e difícil de medir. Esta spec reconstrói o motor como um perfilador de projeto rápido, com interpretação assistida por modelo, e um grafo de referências navegável que deixa o orquestrador injetar apenas o estritamente necessário.

## Usuários/Stakeholders

Quem mantém o Mustard (precisa parar de criar um arquivo Rust por linguagem nova) e o próprio orquestrador em runtime (precisa de scan rápido e injeção enxuta). O usuário final ganha um vault Obsidian navegável de tudo que o Mustard sabe sobre o projeto dele.

## Métrica de sucesso

Tokens injetados por agente caem de forma mensurável (telemetria de prompt do `budget`) sem queda na taxa de QA passando; o scan de um subprojeto passa de ~6 varreduras para 1 (paralela); adicionar uma linguagem nova não exige novo arquivo Rust; o `.claude/` abre como vault no Obsidian e o grafo conecta convenções, skills e specs.

## Não-Objetivos

- Não colocar modelo no hot path do scan (re-escanear arquivos a cada run) — interpretação só no cold path, cacheada e congelada por SHA.
- Não substituir o `cluster_discovery` agnóstico — ele fica e é a base determinística do perfil.
- Não converter referências de plumbing (config, wiring de hooks, imports de código) em wirelinks — só conhecimento (skill/convenção/recipe/entidade/spec).
- Não preservar os 8 `*_scanner.rs` com banner de legado — eliminação direta (fase dev, sem usuário em produção).
- Não construir plugin/integração custom do Obsidian — usar `[[ ]]` nativo + uma pasta `.obsidian/` só de config.
- Não trocar Sonnet por Haiku na interpretação para economizar — a aresta é o ativo crítico; qualidade primeiro.

## Critérios de Aceitação

Critérios transversais (AC-P). Cada wave declara os seus próprios no respectivo `wave-N-*/spec.md`. Testáveis, binários (passa/falha), executáveis e independentes.

- [ ] AC-P-1: workspace compila — Command: `cargo build --workspace`
- [ ] AC-P-2: clippy sem warnings no rt — Command: `cargo clippy -p mustard-rt -- -D warnings`
- [ ] AC-P-3: testes do rt passam — Command: `cargo test -p mustard-rt`
- [ ] AC-P-4: paridade pós-W1 — o registry gerado pela passada única é igual ao baseline congelado — Command: `cargo test -p mustard-rt single_pass_parity`
- [ ] AC-P-5: nenhum scanner por linguagem resta após W2 — Command: `node -e "const fs=require('fs');const d='apps/rt/src/run/scan';const n=fs.readdirSync(d).filter(f=>/_scanner\.rs$/.test(f)).length;process.exit(n===0?0:1)"`
- [ ] AC-P-6: grafo válido (sem aresta órfã nem ciclo) — Command: `cargo test -p mustard-rt graph_validation`
- [ ] AC-P-7: vault Obsidian presente — `.claude/.obsidian/` + `.claude/graph/index.md` existem — Command: `node -e "const fs=require('fs');process.exit(fs.existsSync('.claude/.obsidian')&&fs.existsSync('.claude/graph/index.md')?0:1)"`
- [ ] AC-P-8: injeção mínima comprovada — o fecho resolvido para um escopo de teste é menor que o conjunto completo de nós — Command: `cargo test -p mustard-rt resolve_closure_is_minimal`

## Plano

O detalhamento por ondas, dependências, gates e riscos está em [[wave-plan]] (`wave-plan.md`). Cada onda tem seu próprio `spec.md` com Contexto, Arquivos, Tarefas, Critérios de Aceitação e Limites.
