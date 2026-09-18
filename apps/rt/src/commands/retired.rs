//! As recusas dos comandos antigos que saíram do fluxo: cada um recusa na
//! entrada, sem gravar nada, e manda para o comando novo. A mensagem sai do
//! catálogo, no idioma do projeto, e a saída tem a forma de toda recusa:
//! `ok: false`, a razão curta em `reason` e a mensagem em `hint`, com exit 1.

use std::path::Path;

use mustard_core::platform::i18n::translate;
use mustard_core::ProjectConfig;
use serde_json::{json, Value};

/// A mensagem da chave `key`, com as vagas `slots` preenchidas, no idioma do
/// projeto em `root`: a que o comando aposentado imprime ao recusar, e a mesma
/// que qualquer portão manda dizer no lugar de mandar rodá-lo.
#[must_use]
pub(crate) fn hint(root: &Path, key: &str, slots: &[(&str, &str)]) -> String {
    let lang = ProjectConfig::load(root).language().text_or_default();
    slots.iter().fold(translate(key, lang).to_string(), |text, (slot, value)| text.replace(slot, value))
}

/// A recusa como o comando imprime: a razão `reason` e a mensagem de [`hint`].
#[must_use]
pub(crate) fn report(root: &Path, reason: &str, key: &str, slots: &[(&str, &str)]) -> Value {
    json!({ "ok": false, "reason": reason, "hint": hint(root, key, slots) })
}

/// Imprime a recusa de [`report`] e sai com o código 1.
pub(crate) fn refuse(root: &Path, reason: &str, key: &str, slots: &[(&str, &str)]) -> ! {
    let report = report(root, reason, key, slots);
    println!("{}", serde_json::to_string_pretty(&report).unwrap_or_else(|_| "{}".into()));
    std::process::exit(1);
}
