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

mod agents_find_code_through_the_map;
mod approval_refusal_explains;
mod clean_exit;
mod command_frontmatter;
mod command_guard;
mod development_build_handover;
mod doctor_follows_config_dir;
mod doctor_known_events;
mod end_to_end_flow;
mod final_review_crosses_agreed;
mod hooks_never_break_the_session;
mod installer_switches;
mod installs_nothing;
mod map_created_when_missing;
mod map_of_another_scan;
mod one_page_engine;
mod open_cli;
mod pending_ledger_cli;
mod plan_approval_marker;
mod plan_write_prepares_copy;
mod plugin_agents;
mod plugin_pointer_parity;
mod plugin_prose_matches_shipped_behaviour;
mod pr_prose_door;
mod pr_review_verdict_refuses;
mod private_scan;
mod private_surface;
mod project_page_install;
mod round_dispatch;
mod round_report_usage_documented;
mod run_command_surface;
mod search_answer_layers;
mod session_map_arrives;
mod session_start_merged;
mod spec_events_cli;
mod spec_flow_prose;
mod spend_cli;
mod stale_task_wave_leaves_without_a_check;
mod survey_cli;
mod template_parity;
mod version_full;
mod vocabulary;
mod wave_request_token_cap;
mod worktree_redirect;
