---
id: spec.frente1b-despacho-subprojeto
---

# Frente 1b — Despacho por subprojeto (uma ordem por subprojeto)

<!-- drafter:tone=didactic -->

## Contexto

Doc-mãe: `docs/INTEGRIDADE-EXECUCAO-E-MEMORIA-UTIL.md` (Frente 1). Hoje uma onda que cruza 2+ subprojetos (backend + core) colapsava para **um agente só** (`detect_subproject` retorna `None` → `"."` → um `DispatchItem`), e esse agente recebia o pacote misturado que explorou para se safar (Wave 2 do sialia). Descoberta ao implementar: `--subproject` **não recortava** os arquivos no texto da tarefa (`build_reference_files` listava todos), então split sem filtro seria inócuo — precisa das DUAS partes.

## Métrica de sucesso

Uma onda cujos `## Files` cruzam 2+ subprojetos reconhecidos (`apps/*`/`packages/*`) é despachada com **um agente por subprojeto**, e cada agente vê **só os seus arquivos** — nenhum recebe pilha cruzada. Compõe com a 1a: a onda só completa quando TODOS os arquivos (de todos os agentes) foram tocados.

## Não-Objetivos

- Não é a guarda de cobertura (Frente 1a, já entregue).
- Não é re-despacho automático do gap (follow-up).
- Não cria mais ondas (a divisão por dependência é intocada); só reparte o despacho DENTRO da onda. Arquivos de raiz (sem subprojeto) nunca forçam split e acompanham cada agente (o filtro os mantém) — nada é descartado.

## Critérios de Aceitação

- **AC-1** — `build_plan` emite um item por subprojeto quando a onda cruza 2+
  Command: `cargo test -p mustard-rt dispatch`
- **AC-2** — o render recorta os `## Files` para o subprojeto do agente (mantém raiz, descarta outro subprojeto)
  Command: `cargo test -p mustard-rt agent_prompt`
- **AC-3** — suíte do rt verde
  Command: `cargo test -p mustard-rt`
- **AC-4** — lint limpo
  Command: `cargo clippy -p mustard-rt`

## Checklist

- [x] T1 — `build_reference_files` (agent-prompt-render): filtro `file_belongs_to_subproject` — mantém arquivos do próprio subprojeto + de raiz, descarta de outro subprojeto reconhecido; `.` mantém tudo (no-op byte-estável para o existente).
- [x] T2 — `build_plan` (dispatch-plan): `derive_subprojects` + `flat_map` — um `DispatchItem` por subprojeto quando a onda cruza 2+; `derive_subproject` (singular, órfão) removido.
- [x] T3 — testes (AC-1 `build_plan_splits_wave_by_subproject`; AC-2 `file_belongs...`) + suíte verde (3099) + clippy limpo + contrato byte-estável intacto.