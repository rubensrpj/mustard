//! Os testes sem dono: os que a obra criou e que nenhum critério cita na
//! prova dele. A rodada os aponta ao juntar as ondas, com o mapa de antes e o
//! de depois delas; o fechamento, com o retrato da base da obra e o mapa de
//! agora. É a mesma leitura nos dois, e só aviso, nunca recusa. O teste que
//! já existia na base fica de fora, mesmo mudado: a obra não o criou.

use std::collections::BTreeSet;

use mustard_core::domain::project_map::ProjectMap;
use mustard_core::domain::spec_events::{Block, BlockQuery, SpecLog};
use mustard_core::platform::i18n::{Locale, translate};

/// O motivo do aviso de teste sem dono, o mesmo na rodada e no fechamento.
pub(crate) const REASON: &str = "unowned-test";

/// O aviso de cada teste que `after` declara num arquivo de `files` e que
/// `before` não tinha no mesmo arquivo, quando nenhuma prova de critério do
/// `log` cita o nome dele. Sai na ordem dos arquivos e, em cada um, na das
/// declarações. Teste é a declaração que o mapa marca como teste
/// ([`mustard_core::domain::project_map::MapDecl::is_test`]).
pub(crate) fn hints(log: &SpecLog, before: &ProjectMap, after: &ProjectMap, files: &[String], lang: Locale) -> Vec<String> {
    let proofs: Vec<&str> =
        log.block(BlockQuery::Block(Block::Criteria)).into_iter().filter(|e| e.event_type == "criterion").filter_map(|e| e.str_field("proof")).collect();
    let files: BTreeSet<&str> = files.iter().map(String::as_str).collect();
    let mut out = Vec::new();
    for file in files {
        let Some(module) = after.module(file) else {
            continue;
        };
        let old = before.module(file);
        for decl in &module.declarations {
            let existed = old.is_some_and(|m| m.declarations.iter().any(|d| d.name == decl.name && d.kind == decl.kind));
            if existed || !decl.is_test() || proofs.iter().any(|proof| proof.contains(&decl.name)) {
                continue;
            }
            let hint = translate("close.unowned_test", lang).replace("{name}", &decl.name).replace("{file}", file).replace("{line}", &decl.line.to_string());
            out.push(hint);
        }
    }
    out
}
