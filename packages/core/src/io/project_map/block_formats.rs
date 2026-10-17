//! O formato de cada bloco do banco do mapa preso à versão dele. O banco
//! guarda a versão com que cada bloco foi gravado, e só pela versão diferente
//! o programa sabe que deve refazer ou converter o bloco ao abrir. A tabela ou
//! a coluna que muda com a versão parada chega a um banco antigo que não a
//! tem, e a leitura e a gravação seguintes falham nela.

use super::DB_BLOCKS;
use crate::io::{code_vectors, map_db::Block, map_meaning, sha256::Sha256};

/// Cada formato que um bloco já gravou: o nome, a versão e o resumo das
/// tabelas e do esquema. A lista só cresce. O formato novo entra numa linha
/// nova, com a versão seguinte, e a linha da versão anterior fica: é ela que
/// impede outro formato com o mesmo número.
const FORMATS: &[(&str, u32, &str)] = &[
    ("census", 6, "82de009dfc6a7086"),
    ("files", 3, "625128f59d9ed383"),
    ("decls", 16, "7e435577684a4215"),
    ("decls", 17, "b1f36056af61050d"),
    ("routes", 3, "9efa0b314dfde841"),
    ("graph", 7, "5d77db9a54d49f99"),
    ("history", 3, "0287dcc5eee97688"),
    ("resources", 2, "cae9d1c6a6631d44"),
    ("lineage", 4, "a060daf3e00f2bec"),
    ("pulls", 1, "2485c0c5eaa197a0"),
    ("specs", 2, "186054aebda60aea"),
    ("glossary", 1, "b1b6441fa111fe64"),
    ("notes", 2, "a7d9d32bf5ca50e1"),
    ("notes", 3, "49795b2dab494b2a"),
    ("knowledge_notes", 1, "75ccf73cc654be5b"),
    ("knowledge_index", 3, "26a72894d27e271c"),
    ("search_facts", 1, "4efc3fabb45e666a"),
    ("precise_symbols", 1, "a754e6ecf02fe219"),
    ("expertise", 1, "c223d4677d1e2d26"),
    ("project_profile", 1, "8724dbc573fe03ca"),
    ("meaning", 1, "33dc0fb8ac2360b4"),
    ("code-meaning", 2, "b5f7a2ad8c64fea8"),
];

/// O resumo das tabelas e do esquema de `block`.
fn format_of(block: &Block) -> String {
    let mut digest = Sha256::new();
    digest.update(block.tables.join(" ").as_bytes());
    digest.update(b"\n");
    digest.update(block.schema.as_bytes());
    digest.hex_digest()[..16].to_string()
}

#[test]
fn every_block_changes_its_format_only_with_a_new_version() {
    let blocks = DB_BLOCKS.iter().chain([&map_meaning::BLOCK, &code_vectors::BLOCK]);
    let unpinned: Vec<String> = blocks
        .map(|block| (block.name, block.version, format_of(block)))
        .filter(|&(name, version, ref format)| !FORMATS.contains(&(name, version, format.as_str())))
        .map(|(name, version, format)| format!("(\"{name}\", {version}, \"{format}\")"))
        .collect();
    assert!(
        unpinned.is_empty(),
        "the format of these blocks is not pinned to their version; a new format takes the next version and a new line: {unpinned:#?}"
    );
    let mut versions: Vec<(&str, u32)> = FORMATS.iter().map(|&(name, version, _)| (name, version)).collect();
    versions.sort_unstable();
    let pinned = versions.len();
    versions.dedup();
    assert_eq!(versions.len(), pinned, "one version of a block names one format: {FORMATS:#?}");
}
