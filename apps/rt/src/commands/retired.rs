//! As recusas dos comandos antigos que saíram do fluxo: cada um recusa na
//! entrada, sem gravar nada, e manda para o comando novo. A mensagem sai do
//! catálogo, no idioma do projeto, e a saída tem a forma de toda recusa:
//! `ok: false`, a razão curta em `reason` e a mensagem em `hint`, com exit 1.

use std::path::Path;

use mustard_core::platform::i18n::translate;
use mustard_core::ProjectConfig;
use serde_json::{json, Value};

/// A recusa como o comando imprime: a razão `reason` e a mensagem da chave
/// `key`, com as vagas `slots` preenchidas, no idioma do projeto em `root`.
#[must_use]
pub(crate) fn report(root: &Path, reason: &str, key: &str, slots: &[(&str, &str)]) -> Value {
    let lang = ProjectConfig::load(root).language().text_or_default();
    let hint = slots.iter().fold(translate(key, lang).to_string(), |text, (slot, value)| text.replace(slot, value));
    json!({ "ok": false, "reason": reason, "hint": hint })
}

/// Imprime a recusa de [`report`] e sai com o código 1.
pub(crate) fn refuse(root: &Path, reason: &str, key: &str, slots: &[(&str, &str)]) -> ! {
    let report = report(root, reason, key, slots);
    println!("{}", serde_json::to_string_pretty(&report).unwrap_or_else(|_| "{}".into()));
    std::process::exit(1);
}
