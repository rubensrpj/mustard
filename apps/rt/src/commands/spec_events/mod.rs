//! `mustard-rt run read`, `mustard-rt run write` e `mustard-rt run index` — o
//! arquivo de eventos de uma spec (`spec.ndjson`), escrito só pelo binário e
//! lido por bloco, e o índice das specs.
//!
//! O `write` grava um evento do tipo certo e recusa tipo desconhecido, campo
//! obrigatório vazio e, no ponto, fato sem fonte ou arquivo citado que não
//! existe; a cada gravação, refaz a linha da spec no índice. O `read` devolve
//! só o bloco pedido, nunca o arquivo inteiro. O `index` refaz o índice
//! inteiro a partir dos arquivos de eventos.
//!
//! As regras moram em `mustard_core::domain::spec_events` e
//! `mustard_core::domain::spec_index`, e a gravação em
//! `mustard_core::io::spec_events` e `mustard_core::io::spec_index`; aqui
//! ficam os argumentos, a resolução do
//! projeto e a saída. Recusa sai com exit 1 e o JSON `ok: false`, com a razão
//! curta em `reason` e a mensagem no idioma do projeto em `hint`, como o
//! `pending`.

pub mod cli;
pub mod index;
pub(crate) mod pages;
pub mod read;
pub mod write;

use std::path::{Path, PathBuf};

use mustard_core::domain::spec_events::Refusal;
use mustard_core::SupportedLocale;
use serde_json::{json, Value};

/// O projeto em que as specs moram, visto de onde o comando roda, e o idioma
/// das mensagens dele.
pub(crate) struct Project {
    pub root: PathBuf,
    pub lang: SupportedLocale,
}

pub(crate) fn project(start: &Path) -> Project {
    let root = mustard_core::io::spec_events::spec_root(start);
    let lang = mustard_core::ProjectConfig::load(&root).language().text_or_default();
    Project { root, lang }
}

/// A recusa como o comando imprime.
pub(crate) fn refused(refusal: &Refusal, lang: SupportedLocale) -> Value {
    json!({ "ok": false, "reason": refusal.reason(), "hint": refusal.message(lang) })
}
