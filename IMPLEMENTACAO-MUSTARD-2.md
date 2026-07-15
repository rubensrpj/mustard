# IMPLEMENTACAO — Mustard 2.0 plugado no Claude Code

> Guia operacional da execução. O plano aprovado (contexto, evidência, decisões) vive em
> `.claude/plans/o-mustard-um-magical-island.md`. Este documento diz QUEM faz O QUÊ:
> cada ponto tem um agente designado. Branch: `dev_mustard-2-plugado` (off dev pós-PR #34).
> 1 PR por fase na dev, aberto como draft; merge só com ordem do dono.

## Agentes e papéis

| Agente | Papel nesta implementação |
|---|---|
| **Explore** (read-only) | Rodar o ORÁCULO de 4 passos antes de todo corte; mapear consumidores antes de migrações (ex.: leitores do store de memória); verificação de ausência pós-corte |
| **general-purpose** | Toda implementação (cortes, splits, migrações, reescritas) — recebe worklist fechada + regras, edita via SHELL (bug do work_branch_gate em worktree aninhado; não tocar config) |
| **mustard-review** (read-only) | Revisão adversarial do diff AO FIM DE CADA FASE, antes do PR (Guards + moldes + testes) |
| **code-simplifier** | Passe de simplificação sobre o código recém-modificado, após o review de cada fase |
| **claude-code-guide** | Consultas pontuais de API nativa (sintaxe permissions.deny, manifesto de plugin, contrato ExitPlanMode) — F2 e F4 |
| **Plan** | Só se um ponto abrir fork de desenho não previsto |
| **Orquestrador** (esta sessão) | Gates de verificação, medições antes/depois, commits, PRs, AskUserQuestion, pesquisa web da F5 |

## Protocolo transversal (toda fase)

1. Corte só passa com ORÁCULO verde (argv-string, refs de função, artefatos, hooks/remediation) — Explore executa e devolve a lista final.
2. Toda mudança de superfície atualiza em LOCKSTEP: cli.rs da família · run_command_surface.rs · doctor.rs KNOWN_RUN_SUBCOMMANDS · MUSTARD-COMMANDS.md · whitelist do template_parity.
3. Gate de fase (orquestrador): cargo check 0 warnings · cargo test verde · golden CLI com diff intencional · dashboard compila à parte · template_parity verde · smoke real (run status, run feature, scan em fixture).
4. Fecho de fase: mustard-review adversarial → code-simplifier → commit → PR draft na dev.
5. Princípios: reuso primeiro (duplicações do plano) · SOLID sem fachadas · código/comentários em inglês.

## F1 — Faxina provada

| Ponto | Agente | Observação |
|---|---|---|
| F1.0 Oráculo nos ~23 comandos escuros + hooks mortos | Explore | devolve lista final com file:line; nada é cortado sem isso |
| F1.1 Cortes Rust rt (comandos escuros, hooks mortos, cadeia purpose no rt) | general-purpose (lote A) | sequencial com F1.2 (mesmo workspace) |
| F1.2 Cortes core+scan (SpecStatus, wasm_acquire, insta, pending-snap, braço Verify, chaves do modelo, fixture regression-w6, write-doubling + kinds órfãos) | general-purpose (lote A) | mesmo agente do F1.1 |
| F1.3 Dashboard (30 comandos, bindings, hooks, features/telemetry, 3 .rs mortos, 5 testes, rotas, deps, 1 i18n) + extinção .db (discovery/watcher) | general-purpose (lote B) | PARALELO ao lote A (árvore separada) |
| F1.4 Templates factuais (lexicon-suggest, skill-fetch bug, commit-workflow/wave-summary/obsidian fora, merge-protocol, contradições flat, paths mortos) | general-purpose (lote C) | APÓS lote A (parity depende da superfície final) |
| F1.5 template_parity.rs (catraca forward+reverse+whitelist) | general-purpose (lote C) | mesmo agente |
| F1.6 Prova rg de refs .db = 0 no repo | Orquestrador | anexa ao PR |

## F2 — Nativização das guardas

| Ponto | Agente |
|---|---|
| F2.0 Mapear TODOS os leitores do store de memória + do caminho de aprovação | Explore |
| F2.1 permissions.deny assume tabela de segurança (validar sintaxe antes) | claude-code-guide → general-purpose |
| F2.2 Split SOLID do bash_command_gate (rtk_rewrite / review_gate / pr_detect / estruturais) + path_gate vira boundary | general-purpose |
| F2.3 Aprovação via observer PostToolUse(ExitPlanMode) + plansDirectory | general-purpose |
| F2.4 Memória híbrida (store + 5 hooks + comando saem; decision/lesson via eventos; dashboard/MCP leem eventos) | general-purpose |
| F2.5 review.rs e security_scan saem; transcript/watcher/estimator saem; reader enxuto | general-purpose |
| F2.6 work_branch_gate: fix da tensão de design (estado=principal, git=local) + teste de worktree aninhado | general-purpose |

## F3 — Kernel otimizado (medição antes/depois obrigatória)

| Ponto | Agente |
|---|---|
| F3.0 Baseline: tamanho binário, latência run feature, tempo de hook | Orquestrador |
| F3.1 profile.release (strip/lto/cu1/panic) + profile.dev | Orquestrador (trivial) |
| F3.2 scan feature-bundle (4 spawns → 1) | general-purpose |
| F3.3 Memos no Ctx (ProjectConfig, spec/session) + count_active + canonicalize | general-purpose |
| F3.4 read_workspace_events memo por mtime | general-purpose |
| F3.5 Gramáticas tree-sitter atrás de feature (rt no piso textual) | general-purpose |
| F3.6 mine.rs rayon (byte-idêntico) + regex OnceLock | general-purpose |
| F3.7 Splits SOLID (emit_pipeline/work_branch, event_projections, qa_run, feature_retrieval) + testes inline pesados p/ tests/ | general-purpose |
| F3.8 Medição final + comparação | Orquestrador |

## F4 — Plugin

| Ponto | Agente |
|---|---|
| F4.0 Validar manifesto/limites do plugin na doc | claude-code-guide |
| F4.1 .claude-plugin/plugin.json + layout + hooks via CLAUDE_PLUGIN_ROOT | general-purpose |
| F4.2 init bootstrap fino; update/refresh saem; add p/ marketplace; unhook/rehook alias | general-purpose |
| F4.3 Desembarcar 5 skills públicas; install_nerd_font opt-in; install_grammars vira face rt | general-purpose |
| F4.4 Migração documentada (instalações existentes, sialia) + install e2e em fixture | Orquestrador |

## F5 — Reescrita dos .md com pesquisa

| Ponto | Agente |
|---|---|
| F5.0 Pesquisa (obra/superpowers, anthropics/skills, spec-kit) → guia curto de padrões | Orquestrador (WebSearch/WebFetch) |
| F5.1 Lote SKILLs de comando (18) — reescrita + frontmatters | general-purpose |
| F5.2 Lote refs git (4→2) + worktree-isolation | general-purpose |
| F5.3 Lote refs feature/spec/resume (fusões full-plan+wave-decomp, fix-loop→resume, task-prompts→task, prefix-order→agent-prompt, qa.core→mustard-review) | general-purpose |
| F5.4 agent_prompt_render −40% + context_inject + hooks 40→~24 + dependency_precheck + close_pipeline→close_orchestrate + doctor split + i18n dado + statusline + pagerank | general-purpose |
| F5.5 Validação: template_budget PASSA + parity + dry-run do fluxo feature em fixture | Orquestrador |

## F6 — SDD Perfeito + item 9 + MCP

| Ponto | Agente |
|---|---|
| F6.1 ACs EARS no spec_draft (forma de capability/mod.rs:70) | general-purpose |
| F6.2 Linter de AC-tautologia em analyze_validation | general-purpose |
| F6.3 satisfies em WavePlanEntry + cobertura AC↔onda + cross-artifact analyze | general-purpose |
| F6.4 Clarificação forçada no Full (.clarified marker) | general-purpose |
| F6.5 Capability por padrão no close (resíduo EARS vivo) | general-purpose |
| F6.6 doctor drift unificado | general-purpose |
| F6.7 post_edit data-driven dos Guards (prova em 2 stacks) | general-purpose |
| F6.8 MCP find_anchors + rank_files | general-purpose |
| F6.9 QA da fase: spec gerado exibe EARS+traceabilidade; linter pega tautologia plantada | Orquestrador + mustard-review |

## Regras de sessão (gotchas desta base)

- work_branch_gate bloqueia Write/Edit em worktree aninhado → agentes editam via shell (perl/node/pwsh); NUNCA tocar settings/hook para contornar (fix real é F2.6).
- bash_command_gate falso-positiva prosa com "/nome" perto de verbo de remoção → em docs, usar notação mustard:x.
- Heredoc bash longo quebra no Windows → here-string PowerShell.
- rtk engole stdin de git commit -F- → gravar mensagem em arquivo e usar -F arquivo.
- Binário precisa rebuild após mudanças no rt para os smoke tests (target/debug/mustard-rt.exe).