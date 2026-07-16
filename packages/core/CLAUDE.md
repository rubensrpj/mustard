# Core

> Parent: [../../CLAUDE.md](../../CLAUDE.md) | Orchestrator: [../../.claude/CLAUDE.md](../../.claude/CLAUDE.md)



## Guards

<!-- mustard:guards -->
<!-- facts: kind=cargo; frameworks=serde, serde_json, thiserror, sha2, rayon, tiktoken-rs, aho-corasick, toml, tree-sitter, tree-sitter-loader, tree-sitter-rust, tree-sitter-typescript -->
- Tipos `serde` em `domain/model/` são contrato público: outras crates (rt, dashboard) renderizam em cima deles — mude campo/forma só com migração, não quebre o shape.
- Mantenha `domain/model/` puro: zero IO, log ou disco. Efeito colateral só nas camadas `io`/`platform` — não importe `fs` aqui.
- Escreva arquivos sempre via `io::fs::write_atomic` (tempfile + rename); nunca `std::fs` direto.
- Trate ausência de arquivo como `Error::NotFound` (distinto de `Error::Io`) e degrade sem panic — nada nesta crate entra em pânico por erro de IO/config.
- `ProjectConfig` (`domain/config.rs`) é o dono único do schema de `mustard.json`: chaves de topo em camelCase, lido só do raiz via `ClaudePaths` — não crie parser ad-hoc nem leia o JSON como `Value` solto.
- `unwrap()`/`expect()` são `deny` no workspace fora de teste; propague `Result`. O automaton Aho-Corasick (`vocabulary/`) é único — reúse `KeyedAutomaton`, não instancie outro.
<!-- /mustard:guards -->

<!-- mustard:scan-map -->
Tipo: cargo · 85 arquivos
O terreno já está na sua janela (o census de orientação injetado no início da sessão). Para localizar: `grep` para termo exato conhecido; `mustard-rt run feature` (digest) para conceito; depois leia os arquivos apontados — o digest acha onde olhar, não substitui ler.
<!-- /mustard:scan-map -->
