# Plano — Fortalecer Captura de Dispatch Failures no Mustard

## Contexto

Durante uma pipeline Mustard executada no projeto Zelya, a Wave 1 Backend falhou silenciosamente com `Tool result missing due to internal error`. O CLAUDE.md do Mustard declara que falhas de dispatch são capturadas em `pipeline-state.lastDispatchFailure` e `/mustard:resume` Step 0 auto-recupera em até 10 min — mas isso não aconteceu. Investigação revelou a causa raiz:

**`templates/hooks/subagent-tracker.js:202-204` tem uma regex narrow demais** — captura apenas overload/rate-limit keywords (`overload|rate.?limit|\b429\b|\b529\b|throttl|too many requests`). Erros de infraestrutura do Claude Code (ex: "Tool result missing", HTTP 5xx) não casam e ficam silenciosos. A flag `lastDispatchFailure` nunca é escrita → `/resume` Step 0 não tem o que recuperar.

Parallel-wise, `feature/SKILL.md:241` e `resume/SKILL.md:137` já documentam o tratamento de "Internal error" no **in-session flow** (orchestrator re-dispatcha sequencial), mas isso só funciona quando o orchestrator está ativo e lê o retorno — não cobre o caso cross-session (sessão fechada antes do retry).

A correção foca no **cross-session safety net** (hook), preservando o contrato in-session já existente. Escopo: apenas Mustard upstream — Zelya será cuidado pelo usuário depois (via `mustard update` + `/mustard:resume`).

## Decisões alinhadas com o usuário

- **Detector shape:** Regex expandida + reason único renomeado de `api_overload` → `dispatch_failure` (cobre tanto overload quanto internal errors sob um único caminho de recovery)
- **Escopo:** Apenas `C:\Atiz\Mustard\templates\` — não tocar em `src/` do Mustard, não tocar em `.claude/hooks/` de projetos target (seriam sobrescritos por `mustard update`)
- **Zelya:** Fora do escopo do plano; usuário cuidará depois

## Causa Raiz

**Arquivo:** `templates/hooks/subagent-tracker.js:202-204`

```js
const isOverload =
  toolResponse.is_error === true &&
  /overload|rate.?limit|\b429\b|\b529\b|throttl|too many requests/.test(responseText);
```

- "Tool result missing due to internal error" tem `is_error === true` mas **nenhum keyword da regex casa**
- Resultado: `isOverload = false` → early return na linha 206 → `lastDispatchFailure` nunca escrito
- `/resume/SKILL.md:15-32` (Step 0) depende deste campo — sem ele, Step 0 é no-op

## Mudanças

### 1. `templates/hooks/subagent-tracker.js` (coração da correção)

**Linhas 198-204** — expandir regex + renomear variável:

```js
// Detect dispatch failures conservatively: require is_error=true (Claude Code
// sets this on Task tool failures) AND at least one failure keyword. Covers:
//   - API overload / rate limiting (429, 529, throttle, too many requests)
//   - Infrastructure errors (tool result missing, HTTP 5xx, service unavailable)
// The regex avoids false positives on agents that merely *document* error
// handling in their returned content (see "unrelated error" test below).
const isDispatchFailure =
  toolResponse.is_error === true &&
  /overload|rate.?limit|\b429\b|\b529\b|throttl|too many requests|tool result missing|\b50[0-4]\b|service unavailable/.test(responseText);

if (!isDispatchFailure) return;
```

**Linha 234** — renomear reason:

```js
reason: 'dispatch_failure',
```

**Rationale do conjunto de keywords adicionados:**
- `tool result missing` — literal exato do erro observado em Zelya (string única e estável do Claude Code infra)
- `\b50[0-4]\b` — HTTP 500/501/502/503/504 (server-side infra failures)
- `service unavailable` — complemento ao 503 quando aparece em texto
- **NÃO adicionado:** `internal error` puro — ambíguo demais, pode aparecer em output legítimo de agents descrevendo erros de código (ex: "Internal error: JSON parse failed on line 42"). O termo `tool result missing` é específico de infra.
- **NÃO adicionado:** `timeout` puro — comum demais em output legítimo.

### 2. `templates/hooks/__tests__/hooks.test.js` (tests paralelos)

**Linha 557** — atualizar assert de reason:

```js
assert.equal(state.lastDispatchFailure.reason, "dispatch_failure");
```

**Após linha 563** — adicionar 2 novos test cases (espelham os existentes):

```js
it("should flag lastDispatchFailure on tool result missing infrastructure error", async () => {
  const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "infra-missing-"));
  const pipelinePath = setupPipelineState(tmpDir);
  try {
    const r = await dispatchTaskResult(tmpDir, {
      is_error: true,
      content: "Tool result missing due to internal error",
    });
    assert.equal(r.code, 0);
    const state = JSON.parse(fs.readFileSync(pipelinePath, "utf8"));
    assert.ok(state.lastDispatchFailure, "flag must be set on infra failure");
    assert.equal(state.lastDispatchFailure.reason, "dispatch_failure");
  } finally {
    fs.rmSync(tmpDir, { recursive: true, force: true });
  }
});

it("should flag lastDispatchFailure on HTTP 503 service unavailable", async () => {
  const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "infra-503-"));
  const pipelinePath = setupPipelineState(tmpDir);
  try {
    const r = await dispatchTaskResult(tmpDir, {
      is_error: true,
      content: "Error 503: service unavailable",
    });
    assert.equal(r.code, 0);
    const state = JSON.parse(fs.readFileSync(pipelinePath, "utf8"));
    assert.ok(state.lastDispatchFailure, "flag must be set on 5xx");
    assert.equal(state.lastDispatchFailure.reason, "dispatch_failure");
  } finally {
    fs.rmSync(tmpDir, { recursive: true, force: true });
  }
});
```

**Preserva (sem mudança):** o test "should NOT flag on unrelated error" (linha 581) usando "SyntaxError in src/foo.ts line 42" continua passando — nenhum keyword novo casa isso, mantém o guard contra falsos positivos.

### 3. `templates/CLAUDE.md` (documentação dos guards)

Linha atual (seção Guards):
```
- Task dispatch failures (API overload) are logged to `pipeline-state.lastDispatchFailure`; `/resume` auto-recovers within 10 min
```

Atualizar para:
```
- Task dispatch failures (API overload, HTTP 5xx, tool result missing) are logged to `pipeline-state.lastDispatchFailure`; `/resume` auto-recovers within 10 min
```

### 4. `templates/commands/mustard/resume/SKILL.md` (mensagem user-facing)

Linha 24:
```
Inform the user: `Detected failed dispatch ({agentType}) due to {reason} at {at}. Re-dispatching with same prompt.`
```

Sem mudança estrutural — o campo `{reason}` agora imprimirá `dispatch_failure` em vez de `api_overload`, o que já é mais claro para o usuário.

## Arquivos Críticos Tocados

| Arquivo | Linhas | Tipo |
|---|---|---|
| `templates/hooks/subagent-tracker.js` | 198-206, 234 | Edit |
| `templates/hooks/__tests__/hooks.test.js` | 557, +new block após 563 | Edit |
| `templates/CLAUDE.md` | guards section | Edit |

Total: 3 arquivos, ~25 linhas alteradas/adicionadas.

## Arquivos Referenciados (sem modificação)

- `templates/commands/mustard/resume/SKILL.md:15-32` — Step 0 lê `lastDispatchFailure.reason` via placeholder; funciona com novo valor sem mudança
- `templates/commands/mustard/feature/SKILL.md:241` + `resume/SKILL.md:137` — in-session Escalation Status Handling "Internal error" permanece inalterado; é um segundo nível de defesa complementar ao hook
- `templates/pipeline-config.md` — nenhuma referência a `api_overload` encontrada

## Verificação End-to-End

```bash
# 1. Rodar suite de testes dos hooks (Node built-in test runner)
rtk node --test templates/hooks/__tests__/hooks.test.js

# Expected output:
#   ✔ should flag lastDispatchFailure on real overload (is_error=true + 529)
#   ✔ should flag lastDispatchFailure on tool result missing infrastructure error  [NEW]
#   ✔ should flag lastDispatchFailure on HTTP 503 service unavailable              [NEW]
#   ✔ should NOT flag on happy-path agent that merely documents rate limiting
#   ✔ should NOT flag on unrelated error (is_error=true without overload keywords)

# 2. Grep defensivo: garantir que nenhum arquivo ficou com api_overload dangling
rtk grep -r "api_overload" templates/
# Expected: 0 matches

# 3. Manual smoke test — simular a falha original (opcional)
# Criar um tmpdir com .claude/.pipeline-states/fake.json e rodar o hook com stdin:
echo '{"hook_event_name":"PostToolUse","tool_name":"Task","tool_input":{"subagent_type":"general-purpose","description":"x","prompt":"y"},"tool_response":{"is_error":true,"content":"Tool result missing due to internal error"}}' | node templates/hooks/subagent-tracker.js
# Then: cat <tmpdir>/.claude/.pipeline-states/fake.json | jq '.lastDispatchFailure'
# Expected: { at, reason: "dispatch_failure", agentType: "general-purpose", ... }
```

## Notas para Propagação Futura (fora do escopo deste plano)

Quando o usuário quiser aplicar a correção em Zelya ou outros projetos:

1. Rodar `mustard update` no working directory do projeto target — isso regenera `.claude/hooks/subagent-tracker.js` a partir do template corrigido
2. Rodar `/mustard:resume` — o Step 0 agora auto-recupera internal errors dentro da janela de 10 min; fora da janela, Step 1 reconstrói handoff do spec (flow normal)

**Importante:** enquanto a correção não for propagada, o in-session Escalation Status Handling (`feature/SKILL.md:241`) ainda protege pipelines ativas — é um fallback que não depende do hook.
