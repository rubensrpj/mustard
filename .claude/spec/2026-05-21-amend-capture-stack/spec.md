# amend_capture stack overflow — aumentar /STACK do binário rt no Windows

### Status: completed
### Phase: CLOSE
### Scope: light
### Checkpoint: 2026-05-21T00:55:00Z
### Lang: pt
### Parent: 2026-05-20-tactical-fix-via-sub-spec

## Contexto

Tactical fix derivado de [[2026-05-20-tactical-fix-via-sub-spec]]. O teste de integração `apps/rt/tests/amend_capture.rs::amend_capture_dispatcher_exits_zero` falha no debug build do Windows com exit code `-1073741571` (`STATUS_STACK_OVERFLOW` / `0xC00000FD`). Confirmado pelo review backend como regressão pré-existente — o dispatcher em debug mode (frames grandes por monomorfização) ultrapassa a stack default de 1 MiB do binário Windows. Reproduzido localmente rodando o `target\debug\mustard-rt.exe` com payload trivial: `thread 'main' has overflowed its stack`.

Solução: adicionar `build.rs` em `apps/rt/` que injeta `cargo::rustc-link-arg=/STACK:8388608` (8 MiB) APENAS no target Windows. Endereça a root cause (stack default Windows pequena para builds debug) sem mexer no código de produção do dispatcher e sem ignorar o teste.

## Critérios de Aceitação

- [x] AC-1: Crate compila — Command: `cargo build -p mustard-rt`
- [x] AC-2: Test `amend_capture_dispatcher_exits_zero` passa — Command: `cargo test -p mustard-rt --test amend_capture amend_capture_dispatcher_exits_zero`
- [x] AC-3: `apps/rt/build.rs` contém `/STACK:` para Windows — Command: `node -e "const c=require('fs').readFileSync('apps/rt/build.rs','utf8');process.exit((c.includes('/STACK:')&&c.includes('windows'))?0:1)"`

## Arquivos

```
apps/rt/build.rs    — novo: injeta /STACK:8388608 no link do binário em Windows
apps/rt/Cargo.toml  — confirmar/adicionar `build = "build.rs"` se necessário
```

## Tarefas

- [x] Criar `apps/rt/build.rs`:
  ```rust
  fn main() {
      if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
          // Debug builds in Windows default to a 1 MiB main-thread stack, which
          // is too small for the rt dispatcher's monomorphized frames. 8 MiB
          // matches the POSIX default on most distros.
          println!("cargo:rustc-link-arg-bins=/STACK:8388608");
      }
  }
  ```
- [x] Se `apps/rt/Cargo.toml` não tem `build = "build.rs"`, adicionar (Cargo descobre `build.rs` automaticamente, mas explicitar é defensivo).
- [x] Validar: `cargo clean -p mustard-rt && cargo build -p mustard-rt && cargo test -p mustard-rt --test amend_capture`.

## Limites

- `apps/rt/build.rs` (novo) + `apps/rt/Cargo.toml` (eventual edit).

**Fora dos limites:**
- Refatorar o dispatcher para reduzir frame size.
- Spawnar dispatcher numa thread com stack custom (mais invasivo, muda código de produção).
- Ignorar o teste com `#[cfg(not(windows))]` (pinta verde sem resolver).

## Checklist

- [x] AC-1 a AC-3 passam
- [x] Comportamento em Linux/macOS inalterado (build.rs no-op nesses targets)
