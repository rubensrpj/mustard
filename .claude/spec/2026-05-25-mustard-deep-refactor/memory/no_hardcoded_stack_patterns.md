---
name: no-hardcoded-stack-patterns
description: Mustard nunca declara catálogo de padrões esperados por stack; tudo emerge do filesystem do projeto-alvo via heurísticas agnósticas
metadata:
  type: principle
  origin_spec: 2026-05-25-mustard-deep-refactor
  origin_wave: wave-3-mixed
---

# No Hardcoded Stack Patterns

Mustard é **ferramenta**, não framework. Quando alguém roda `mustard init` num projeto Django, Spring, Express, Flutter, .NET, Go ou qualquer outro, o `/scan` precisa descobrir os padrões reais daquele projeto **sem nenhum catálogo prévio**.

## Distinção crítica

Existem dois Mustards que NÃO podem se confundir:

- **Mustard-projeto** (repo `C:\Atiz\mustard\`) — implementação em Rust+React+Tauri. Padrões deste repo (`cli-command-pattern`, `rt-run-subcommand-pattern`, `dashboard-page-primitives`, etc.) são DESTE projeto.
- **Mustard-ferramenta** (instalado via `mustard init` em qualquer projeto) — agnóstico. Não tem conhecimento prévio de NENHUMA stack.

Usar padrões do Mustard-projeto como métrica universal (ex: "AC valida que cli-command-pattern emerge no cluster_discovery") quebra agnosticismo. Aquele padrão só existe num projeto Rust+CLI específico.

## Regra

**Zero catálogo hardcoded** de:
- "Padrões canônicos esperados" do projeto-alvo
- "Recipes pré-definidos" por tipo de stack
- Nome de tecnologia (`Rust`/`React`/`Django`/`Spring`/etc.) em código ou prompt do agent

Tudo emerge de:
- Manifests do projeto (Cargo.toml, package.json, requirements.txt, etc.)
- Heurísticas agnósticas do `cluster_discovery` (suffix/base-class/decorator/function-prefix/basename)
- Amostras de código do filesystem real

## Como receitas continuam funcionando sem catálogo

Receitas são **DERIVADAS** dos clusters que emergem:

1. `cluster_discovery` detecta cluster com label `xyz` (label emerge do sufixo/decorador/etc. encontrado)
2. Gerador lê 2-3 amostras do cluster
3. Extrai imports comuns + skeleton mínimo + barrel pattern (se existe)
4. Emite `recipes/{sub}/add-{xyz}.json` com paths reais do projeto-alvo

Num projeto Django emerge `add-view`, `add-serializer`, `add-model`.
Num projeto Express emerge `add-route-handler`, `add-middleware`.
Num projeto Mustard-este-repo emerge `add-options-execute`, `add-run-subcommand`, `add-hook-module`.

Mustard não sabe o que é nenhum deles a priori. Descobre.

## Origem

Esta política foi formalizada em [[2026-05-25-mustard-deep-refactor]] durante [[wave-3-mixed]] depois de reanálise crítica em 2026-05-25 onde o user corrigiu drift: "vc está errado mustard é agnóstico, vc está focando em rust ele precisa reconhecer o projeto em que o mustard foi adicionado". Reescrita resultou em scan-structural sem nenhuma menção a stack específica no prompt do agent.

## Aplica-se a

- Antes de adicionar AC que cita nome de padrão específico: pergunte se esse nome emergiria de um projeto Django. Se não, AC é vazado.
- Antes de adicionar template/catálogo no Rust com nome de stack: refatorar para receber via input do manifest parser.
- Antes de adicionar exemplo "Rust" no prompt do agent: substituir por categoria genérica ("compiled-strongly-typed", "dynamic-scripting", etc.).
- Antes de hardcodear path como `apps/cli/src/commands/X`: substituir por path derivado de cluster que o scan descobriu.

## Status

Active — política inviolável.

## Relacionado

- [[scan_rust_first]] — mecanismo de execução agnóstico
- [[recipes_from_scan]] — recipes saem do scan, não de templates
