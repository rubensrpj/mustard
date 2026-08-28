pub mod cli;

// `doctor::doctor` repete o nome do pai de proposito: este modulo E a porta,
// e cada irmao ao lado dele e UMA checagem especifica (`doctor_i1`,
// `docs_stale_check`, ...). Renomear a porta para se distinguir dos proprios
// checks tocaria todo chamador em troca de uma opiniao de nomenclatura.
#[allow(clippy::module_inception)]
pub mod doctor;
pub mod doctor_claude_paths;
pub mod doctor_i1;
pub mod doctor_workspace_leaks;
pub mod language_audit;
pub mod docs_stale_check;
pub mod superseded_check;
pub mod capability_drift_check;
pub mod guards_scaffold_check;
pub mod inject_delivery_check;
