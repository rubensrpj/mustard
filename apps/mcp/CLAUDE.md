# Mcp

> Parent: [../../CLAUDE.md](../../CLAUDE.md) | Orchestrator: [../../.claude/CLAUDE.md](../../.claude/CLAUDE.md)



## Guards

<!-- mustard:guards -->
<!-- facts: kind=cargo; frameworks=mustard-core, serde, serde_json, rmcp, tokio, tempfile -->
- Esta face é somente-leitura por contrato: nunca grave estado aqui — escritas só acontecem nos hooks, onde a atribuição de sessão/onda/spec é autêntica; as ferramentas MCP só consultam.
- Falhe-vazio, nunca em pânico: toda leitura que dá errado degrada para resultado vazio, pois unwrap/expect são `deny` no caminho de protocolo; pânico aqui derruba o servidor que o Claude Code mantém vivo.
- Diagnóstico vai só para stderr (`eprintln!`): o stdout é exclusivo do canal JSON-RPC do MCP — qualquer escrita lá corrompe o protocolo.
- Mantenha o `tokio` confinado: construa o runtime `current_thread` localmente dentro de `run`; não anote `main` com `#[tokio::main]` nem espalhe async — as faces síncronas continuam síncronas.
- Tokens e métricas saem dos readers de `mustard_core::domain::economy` (ex.: `metric_token_summary`); não refaça a agregação à mão sobre `pipeline.telemetry.run` (sempre reportaria zero).
- Preserve a paridade byte-a-byte das shapes serde com o original TypeScript: nomes de campo via `#[serde(rename)]` (camelCase) e os clamp de `limit` por ferramenta são contrato.
<!-- /mustard:guards -->

<!-- mustard:scan-map -->
Tipo: cargo · 2 arquivos
O terreno já está na sua janela (o census de orientação injetado no início da sessão). Para localizar: `grep` para termo exato conhecido; `mustard-rt run feature` (digest) para conceito; depois leia os arquivos apontados — o digest acha onde olhar, não substitui ler.
<!-- /mustard:scan-map -->
