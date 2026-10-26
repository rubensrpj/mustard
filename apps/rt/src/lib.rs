//! `mustard-rt` library face — re-exports for integration tests.
//!
//! This crate is primarily a binary (`[[bin]]`). The `[lib]` target exists
//! solely so integration tests under `tests/` can import internal modules
//! without subprocess overhead. The lib crate must declare all modules that
//! `run/` submodules reference via `crate::` paths.
//!
//! Only `dead_code` is suppressed at crate level, and only because the
//! `hooks`/`report`/`dispatch` modules are reached **only** from the binary
//! face (`main.rs`), so the lib build sees them as unused —
//! false positives inherent to this test-only re-export face. The real
//! dead-code signal is the bin build (`cargo build --bin mustard-rt`), which
//! declares the same modules without this allow. `unused_imports` /
//! `unused_variables` / `unused_mut` are NOT suppressed: the bin build already
//! keeps the module tree clean of them, so enforcing them in the lib build too
//! (covering `#[cfg(test)]` code the bin build skips) is free and catches
//! regressions.

#![forbid(unsafe_code)]
#![allow(dead_code)]
#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::float_cmp,
        clippy::len_zero,
        clippy::format_push_string,
        clippy::needless_range_loop,
    )
)]

// A linha de comando que o binário lê, declarada aqui também para o teste da
// ajuda percorrer a árvore a partir da raiz.
pub mod cli;
pub mod commands;
pub mod shared;
pub mod util;
mod dispatch;
mod hooks;
// Público para o teste que confere o registro dos ganchos contra o manifesto
// que o Claude Code lê.
pub mod registry;
mod report;
// Declared here so the harness-response tests run on the LIB target. The binary
// declares it too but no longer runs any test (`test = false`), which is what
// stops every `#[cfg(test)]` block under `src/` from executing twice.
mod hook_output;
mod host;
mod mcp;

// A pasta do pacote lida na hora de rodar, para os testes de dentro de `src/`.
#[cfg(test)]
#[path = "../tests/support/manifest_dir.rs"]
pub(crate) mod manifest_dir;
// O relógio falso dos testes de dentro de `src/`.
#[cfg(test)]
#[path = "../../../packages/core/tests/support/fake_clock.rs"]
pub(crate) mod fake_clock;
// A espera dos testes de dentro de `src/` pelo evento de outro processo.
#[cfg(test)]
#[path = "../../../packages/core/tests/support/wait_for.rs"]
pub(crate) mod wait_for;
// O comando que os testes de dentro de `src/` seguram até soltá-lo.
#[cfg(test)]
#[path = "../../../packages/core/tests/support/held_command.rs"]
pub(crate) mod held_command;
// A pausa dos testes de dentro de `src/` antes de afirmar que nada aconteceu.
#[cfg(all(test, unix))]
#[path = "../../../packages/core/tests/support/pause.rs"]
pub(crate) mod pause;
// O cronômetro das medições de dentro de `src/`, que só vai ao relato delas.
#[cfg(test)]
#[path = "../../../packages/core/tests/support/stopwatch.rs"]
pub(crate) mod stopwatch;

// O programa falso que os testes de dentro de `src/` gravam e depois rodam.
#[cfg(test)]
#[path = "../tests/support/executable.rs"]
pub(crate) mod executable;
