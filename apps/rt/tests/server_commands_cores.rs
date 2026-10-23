// Integration tests are separate binary targets and not exempt from
// `clippy::unwrap_used` etc. via `#[cfg(test)]`. Mirror the carve-out from
// `src/main.rs` so test panics on `.unwrap()` remain valid assertions.
#![allow(clippy::unwrap_used, clippy::expect_used)]

//! Os comandos do projeto rodam em núcleos que a máquina aguenta: a suíte que
//! o fechamento roda enxerga no máximo o menor entre os núcleos da máquina e
//! a memória total em GB dividida por 2. Um executor de testes que abre um
//! processo por núcleo visível — o jest, que lê a afinidade — deixa de abrir
//! mais processos do que a memória comporta.
//!
//! Prova de ponta a ponta, pelo binário de verdade, num repositório
//! temporário: a spec anda pelos comandos e ganchos do uso real até o
//! fechamento, e a suíte declarada no `mustard.json` grava o que o `nproc`
//! dela viu.
//!
//! Só no Linux: noutros sistemas os comandos rodam sem limite, como antes.

#![cfg(target_os = "linux")]

use crate::one_wave_spec;

use serde_json::json;

/// Onde a suíte do projeto anota quantos núcleos enxergou.
const CORES_FILE: &str = "cores.txt";

/// O limite que a máquina aguenta: o menor entre os núcleos que este
/// processo enxerga e a memória total em GB dividida por 2, no mínimo 1.
fn affordable_cores() -> usize {
    let cores = std::thread::available_parallelism().map_or(1, std::num::NonZero::get);
    let meminfo = std::fs::read_to_string("/proc/meminfo").expect("the memory of the machine");
    let kib: u64 = meminfo
        .lines()
        .find_map(|line| line.strip_prefix("MemTotal:"))
        .and_then(|rest| rest.trim().trim_end_matches("kB").trim().parse().ok())
        .expect("the total memory");
    let by_memory = usize::try_from(kib / (2 * 1024 * 1024)).unwrap_or(usize::MAX);
    cores.min(by_memory).max(1)
}

/// A suíte que o fechamento roda enxerga no máximo os núcleos que a memória
/// da máquina aguenta. Antes, o comando enxergava todos os núcleos, e numa
/// máquina de 12 núcleos e 15 GB o `nproc` da suíte dizia 12.
#[test]
fn the_closing_suite_sees_no_more_cores_than_the_memory_affords() {
    let project = one_wave_spec::Project::new("nucleos", &json!({"testCommand": format!("nproc > {CORES_FILE}")}), &[CORES_FILE]);
    project.approved();
    project.run(&["round", "--dispatch", "--spec", "nucleos"]);
    project.deliver_the_first_wave();
    let cores_file = project.root.join(CORES_FILE);
    let _ = std::fs::remove_file(&cores_file);

    let asked = project.run(&["close", "--spec", "nucleos"]);
    assert_eq!(asked["review"]["final"], json!(true), "{asked}");

    let seen = std::fs::read_to_string(&cores_file).unwrap_or_else(|e| panic!("the suite did not run in the closing ({e}): {asked}"));
    let seen: usize = seen.trim().parse().unwrap_or_else(|e| panic!("`nproc` wrote no number ({e}): {seen}"));
    let limit = affordable_cores();
    assert!(seen <= limit, "the suite saw {seen} cores, past the {limit} the memory affords");
}
