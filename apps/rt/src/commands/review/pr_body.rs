//! A marca do corpo que o Mustard gera para o pull request.
//!
//! O corpo gerado leva no fim um comentário HTML, que o provedor não mostra,
//! com a impressão do texto acima dele. Antes de refazer o corpo, quem o refaz
//! lê o corpo atual pelo provedor: só o que segue igual ao último gerado — a
//! marca confere com o texto — ou o vazio é refeito. O corpo que alguém editou
//! fica, e quem chamou avisa. Apagar a descrição no provedor devolve o corpo
//! ao Mustard.

use crate::shared::pr_provider::{PrProvider, PrTextRead};

/// O começo da marca; o fim é o do comentário HTML.
const MARK: &str = "<!-- mustard-body:";

/// Por que o corpo não é refeito quando o provedor leu um corpo diferente do
/// último gerado.
pub(crate) const BODY_EDITED: &str = "body-edited";

/// A impressão de um corpo: o texto com as quebras de linha do git e sem os
/// brancos do fim, que o provedor pode mudar sem ninguém editar nada.
fn print_of(text: &str) -> String {
    mustard_core::io::observations::digest(text.replace("\r\n", "\n").trim_end().as_bytes())
}

/// O corpo `body` com a marca da impressão dele no fim.
pub(crate) fn marked(body: &str) -> String {
    format!("{}\n\n{MARK} {} -->", body.trim_end(), print_of(body))
}

/// O corpo `current` segue igual ao último que o Mustard gerou: vazio, ou com
/// a marca que confere com o texto acima dela.
fn untouched(current: &str) -> bool {
    let current = current.replace("\r\n", "\n");
    if current.trim().is_empty() {
        return true;
    }
    let Some((text, mark)) = current.trim_end().rsplit_once(MARK) else {
        return false;
    };
    mark.strip_suffix("-->").map(str::trim) == Some(print_of(text).as_str())
}

/// Por que o corpo do pull request `number` não pode ser refeito: `None`
/// quando ele segue igual ao último gerado, [`BODY_EDITED`] quando alguém o
/// editou, e o motivo do provedor quando não deu para lê-lo — sem a leitura,
/// não há como saber se a edição de alguém se perderia.
pub(crate) fn kept_reason(provider: &dyn PrProvider, number: u64) -> Option<String> {
    match provider.text(number, "") {
        Ok(PrTextRead::Read(text)) if untouched(&text.body) => None,
        Ok(_) => Some(BODY_EDITED.to_string()),
        Err(reason) => Some(reason),
    }
}
