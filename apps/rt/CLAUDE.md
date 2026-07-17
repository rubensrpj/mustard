@.claude/scan-map.md

# Rt

> Parent: [../../CLAUDE.md](../../CLAUDE.md) | Orchestrator: [../../.claude/CLAUDE.md](../../.claude/CLAUDE.md)



## Guards

<!-- mustard:guards -->
<!-- facts: kind=cargo; frameworks=mustard-core, serde, serde_json, clap, tiny_http, mustard-mcp, ureq, tempfile, notify, rayon, sha2 -->
- Hook nunca pode entrar em pânico nem barrar a sessão por erro próprio: a degradação mora UMA vez no dispatcher (um `Check` com `Err` vira `Allow`) e no `main` (todo caminho termina em `process::exit(0)`); bloqueio se expressa no JSON `permissionDecision`, jamais via exit não-zero.
- `clippy::unwrap_used`/`expect_used` são `deny` em todo o crate (fora de `#[cfg(test)]`); em hook, degrade com `unwrap_or` / `let-else` / `ok()?`.
- Subcomando novo de `run` exige DOIS registros, ambos no `cli.rs` da família (`commands/<família>/cli.rs`): a variante no enum `{Família}Cmd` E o braço no `dispatch()` dela; esquecer o segundo compila mas o comando some. `RunCmd` (commands/mod.rs) só roteia: faz `#[command(flatten)]` de cada família (os nomes seguem planos — `run wave-advance`, nunca `run wave advance`) e `tests/run_command_surface.rs` tranca a lista publicada.
- Observers (`hooks/observe`,`session`,`task`) são só efeito-colateral/telemetria: `observe()` retorna `()` e roda fire-and-forget — nunca devolva veredito por ali (decisão de bloqueio vive num `Check`).
- As faces `run` e `mcp` NÃO leem o stdin do harness (despachadas antes da leitura no `main.rs`); só `on`/`check` consomem o `HookInput`. Mantenha `main.rs` magro: roteamento de argv + match das faces, sem lógica de negócio.
- Saída de comando `run` deve ser determinística e byte-estável (JSON ordenado, sem timestamps/caminhos voláteis) — há snapshots `insta` e gates que comparam a saída.
<!-- /mustard:guards -->
