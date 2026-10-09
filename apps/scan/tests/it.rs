//! Os testes de integração deste pacote, num programa só: cada arquivo de
//! `tests/` é um módulo daqui. Um programa por arquivo custava uma ligação, um
//! início de processo e uma pasta de compilação por arquivo.
//!
//! Arquivo novo em `tests/` entra nesta lista com uma linha `mod <nome>;`: a
//! descoberta automática está desligada (`autotests = false` no
//! `Cargo.toml`), e o arquivo fora da lista não roda.

// Cada arquivo traz os ajudantes dele pelo caminho (`#[path = "support/..."]`):
// o mesmo arquivo entra como módulo de vários arquivos. Assim cada um conserva
// o `cfg` e só o que usa, em todos os sistemas, como quando era um programa à
// parte.
#![allow(clippy::duplicate_mod)]

mod body_text;
mod constant_objects;
mod dart_mining_e2e;
mod declaration_detail;
mod declared_test_module;
mod engine_is_language_blind;
mod fixed_texts;
mod full_read_is_byte_stable;
mod function_as_value;
mod generated_class;
mod graph_resolution;
mod header_parameter;
mod history;
mod incremental_read;
mod inline_test_block;
mod kinds_content;
mod kinds_parity;
mod knowledge;
mod search_gateway;
mod map_database;
mod markup_page;
mod member_read;
mod members_and_links;
mod php_extraction;
mod php_laravel_fixture;
mod quality;
mod retired_search_commands;
mod resources;
mod catalog;
mod evidence_retrieval;
mod routes;
mod skip_dirs_excludes_claude;
mod stack_detection_e2e;
mod stack_evidence_excludes;
mod use_in_every_language;
