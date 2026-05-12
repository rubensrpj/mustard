# Mustard 2.0 — Runtime Migration (Node ↔ Bun)

> **Em português simples (1 parágrafo):** Hoje cada hook do Mustard sobe um processo Node novo a cada ferramenta que o Claude usa — isso custa entre 100ms e 300ms por chamada, e em uma sessão real isso vira ~50min só de "esquentar motor". Bun (o runtime que a Anthropic adquiriu em dez/2025 e que já vem dentro do Claude Code v2.1.113+) faz a mesma coisa em 10-30ms, tem SQLite nativo via `bun:sqlite` (não precisa instalar pacote nenhum) e roda TypeScript sem build step. Esta Phase 0 NÃO refatora nada — só ensina o Mustard a detectar Bun quando ele existe e a continuar usando Node quando não existe. Zero quebra em projetos que já rodam. As Phases 1+ (event store, telemetria) é que vão de fato consumir os ganhos.

## Status

- **Phase**: 0 (Compatibility Layer)
- **Spec**: [`.claude/spec/active/2026-05-12-mustard-2-0-phase-0-compat-runtime/spec.md`](../.claude/spec/active/2026-05-12-mustard-2-0-phase-0-compat-runtime/spec.md)
- **Runtime alvo**: Bun 1.x+ (testado a partir de 1.2 no Windows; 1.3 disponível no Linux/macOS)
- **Fallback**: Node.js >= 18

## Detecção automática

`mustard init` e `mustard update` rodam a detecção de runtime em três passos:

1. Verifica se o processo atual é Bun (`typeof Bun !== 'undefined'` ou `process.versions.bun`)
2. Se não, procura `bun` no `PATH` via `which bun` (POSIX) / `where bun` (Windows)
3. Se nenhum dos dois, cai pra Node.js (mínimo 18)

A detecção é exposta em `src/runtime/detect-runtime.ts` e retorna:

```ts
{
  kind: 'bun' | 'node',
  version: string,           // ex: '1.2.18' ou '20.11.1'
  bunSqliteAvailable: boolean,
  claudeCodeVersion?: string // se rodando dentro do Claude Code
}
```

### Matriz de decisão

| Bun instalado | Bun no PATH | Processo atual é Bun | `MUSTARD_RUNTIME` | Runtime escolhido |
|---------------|-------------|----------------------|-------------------|-------------------|
| sim           | sim         | sim                  | `auto` (default)  | **bun**           |
| sim           | sim         | não                  | `auto`            | **bun**           |
| sim           | não         | não                  | `auto`            | **node**          |
| não           | —           | não                  | `auto`            | **node**          |
| sim           | sim         | qualquer             | `node`            | **node** (forçado)|
| não           | —           | —                    | `bun`             | **erro** (falha rápida) |
| sim           | sim         | —                    | `bun`             | **bun** (forçado) |

O fallback é sempre transparente: se Bun some do PATH em um `mustard update`, o projeto continua rodando em Node sem nenhuma ação manual.

## Forçando runtime via env

Três variáveis controlam a escolha:

```bash
# Default: detecta e prefere Bun se disponível
MUSTARD_RUNTIME=auto mustard init

# Força Node mesmo se Bun estiver disponível
MUSTARD_RUNTIME=node mustard init

# Força Bun (falha com exit 1 se Bun não estiver instalado)
MUSTARD_RUNTIME=bun mustard init

# Loga a detecção em stderr (útil pra debug em CI)
MUSTARD_RUNTIME_VERBOSE=1 mustard init
```

Saída típica com `MUSTARD_RUNTIME_VERBOSE=1`:

```text
[mustard:runtime] detected kind=bun version=1.2.18 bunSqlite=true
[mustard:runtime] source=process.versions.bun
[mustard:runtime] claudeCodeVersion=2.1.113
```

## Onde fica registrado

A escolha é persistida em `.claude/mustard.json` no campo `runtime`:

```jsonc
// .claude/mustard.json   (NÃO confundir com ./mustard.json do root — ver seção abaixo)
{
  "runtime": {
    "kind": "bun",
    "version": "1.2.18",
    "chosenAt": "2026-05-12T18:10:00Z"
  }
}
```

Comportamento:

- `mustard init` grava `runtime` com base na detecção atual (ou no `--runtime=` passado)
- `mustard update` **preserva** o `runtime` existente — não sobrescreve
- O timestamp em `chosenAt` ajuda a auditar quando o projeto mudou de runtime

Para forçar a regravação do runtime (não existe flag dedicada em Phase 0):

```bash
# Opção 1: remover o arquivo e rodar init de novo
rm .claude/mustard.json
node bin/mustard.js init --runtime=node    # ou --runtime=bun

# Opção 2: editar manualmente .claude/mustard.json e ajustar runtime.kind
```

### Dois arquivos `mustard.json` (importante)

O Mustard mantém **dois arquivos com o mesmo basename**, em locais diferentes e com responsabilidades distintas:

| Arquivo | Conteúdo | Quem grava |
|---------|----------|------------|
| `./mustard.json` (root) | Config legacy de git-flow (`branches`, `parent`, etc.) — usado por hooks como `close-gate.js` e `review-gate.js` | `mustard init` (interativo) |
| `./.claude/mustard.json` (novo em Phase 0) | Runtime info (`{ kind, version, chosenAt }`) — usado pela CLI e por `runtime-shim.js` | `mustard init` / detecção automática |

Os dois arquivos são **independentes**: editar um não afeta o outro. O mesmo basename é coincidência histórica; uma consolidação só será considerada em phase futura se trouxer ganho real. Hooks de produção Phase 0 **não** leem `.claude/mustard.json` — só a CLI e o runtime shim.

## Compatibilidade

### Hooks rodam idênticos em Bun e Node

Os 31 hooks ficam em `templates/hooks/*.js` como CommonJS Node-compat. Bun executa esse mesmo `.js` sem build step e sem mudar shebang. A regra "no npm deps after init" continua valendo nos dois runtimes — todos os hooks usam só built-ins (`fs`, `path`, `child_process`).

Para hooks que precisam saber qual runtime está ativo, use o helper compartilhado:

```js
const { pickRuntime } = require('./_lib/runtime-shim');
const rt = pickRuntime();
if (rt.kind === 'bun' && rt.bunSqliteAvailable) {
  // pode usar bun:sqlite
}
```

### TypeScript no `src/` ainda precisa de build

A CLI (`src/`) continua sendo compilada via `tsc` antes de publicar — independente do runtime de destino. Bun rodaria os `.ts` direto, mas mantemos o build pra publicar `dist/` no npm e suportar usuários em Node puro.

### `bun:sqlite` só na Phase 1

Phase 0 **não** consome `bun:sqlite`. A flag `bunSqliteAvailable` é gravada agora pra que Phase 1 (event store local) possa simplesmente ler `mustard.json.runtime` e ligar SQLite quando o ambiente suportar.

### Claude Code v2.1.113+

A partir dessa versão, Bun já vem embutido no Claude Code — o usuário final não precisa instalar nada. O Mustard detecta isso via `claudeCodeVersion` e prioriza Bun automaticamente nesses ambientes.

## Instalando Bun

### Windows

```powershell
# Opção 1: Scoop
scoop install bun

# Opção 2: PowerShell (oficial)
powershell -c "irm bun.sh/install.ps1 | iex"

# Opção 3: Cargo (compila do source)
cargo install bun-cli
```

Bun 1.2+ tem suporte production-grade no Windows. Limitações conhecidas no Windows Server 2022: file system watcher e separadores de path — não afeta o uso do Mustard, que só roda hooks pontuais.

### Linux / macOS

```bash
curl -fsSL https://bun.sh/install | bash
```

### Verificar instalação

```bash
bun --version
# Esperado: 1.2.x ou superior
```

## Troubleshooting

### "Bun detectado, mas hooks falham silenciosamente"

Habilite o log verboso e re-execute a operação:

```bash
MUSTARD_RUNTIME_VERBOSE=1 mustard update
```

Procure por linhas `[mustard:runtime]` em stderr. Causas comuns:

- Bun antigo (< 1.2) em projetos que dependem de APIs novas
- Permissão de execução faltando no binário Bun
- `PATH` diferente entre shell interativo e o processo que sobe os hooks

### "Quero rollback para Node"

Duas opções, sem perder estado:

```bash
# Temporário (uma sessão) — env var apenas
MUSTARD_RUNTIME=node mustard update

# Permanente — regrave .claude/mustard.json manualmente
#   1) abra .claude/mustard.json e troque runtime.kind para "node"
#   2) ou remova o arquivo inteiro e rode `mustard init --runtime=node`
```

Alternativa drástica: remover Bun do `PATH` faz a detecção cair pra Node automaticamente; o `.claude/mustard.json` ainda precisa ser ajustado para refletir a mudança permanente.

### "CI roda Node mas dev local roda Bun (ou vice-versa)"

Isso é esperado e **não é bug**. Cada ambiente detecta o runtime disponível. Se você precisa garantir paridade:

```bash
# .github/workflows/*.yml ou script local
export MUSTARD_RUNTIME=node
```

## Roadmap

Esta Phase 0 só estabelece a base. As próximas fases consomem:

- **Phase 1 — Event Store**: substitui `events.jsonl` por `bun:sqlite` quando `runtime.kind === 'bun'`; mantém JSONL como fallback Node. Ganho esperado: queries de métricas saem de O(n) por scan pra O(log n) indexado.
- **Phase 2 — OpenTelemetry token counting**: instrumentação real de custo por agente, exportável via OTLP. Independe do runtime mas se beneficia do cold-start menor de Bun em hooks de telemetria.

Referência completa do que está dentro/fora desta phase: [`spec.md`](../.claude/spec/active/2026-05-12-mustard-2-0-phase-0-compat-runtime/spec.md).
