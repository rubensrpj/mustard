// Integration tests are separate binary targets and not exempt from
// `clippy::unwrap_used` etc. via `#[cfg(test)]`. Mirror the carve-out from
// `src/main.rs` so test panics on `.unwrap()` remain valid assertions.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::uninlined_format_args)]
// O script conferido aqui é de shell e a cópia de mentira precisa da
// permissão de execução do Unix: no Windows o arquivo nem compila.
#![cfg(unix)]

//! O fecho de `scripts/dev-install.sh`: quem responde pelo nome no caminho de
//! busca.
//!
//! O script troca duas cópias — a do plugin e a do sistema — e, até esta
//! obra, dizia trocado sem olhar quem o terminal escolhe. Em 21/09/2026 quem
//! rodou passou a testar uma cópia velha em `~/.cargo/bin`, que vem antes no
//! caminho, três commits atrás, com o script dizendo que estava tudo trocado.
//!
//! A prova roda o próprio fecho do script, tirado do arquivo entregue, sob um
//! caminho de busca montado aqui: uma vez com uma terceira cópia à frente
//! (tem de avisar) e uma vez com a cópia do plugin à frente (não avisa).

use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::Command;

/// A raiz do repositório, subindo de `<repo>/apps/rt` até achar o script.
fn repo_root() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut dir = manifest.as_path();
    loop {
        if dir.join("scripts").join("dev-install.sh").is_file() {
            return dir.to_path_buf();
        }
        dir = dir.parent().expect("scripts/dev-install.sh must be reachable");
    }
}

/// Um executável de mentira, só para o caminho de busca ter o que achar.
fn fake_binary(path: &Path) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, "#!/bin/sh\nexit 0\n").unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// O fecho do script — do comentário que o abre até o `done` do laço —, como
/// ele sai no arquivo entregue. Tirar esse trecho do script derruba a prova
/// aqui mesmo, antes de qualquer asserção.
fn closing_block() -> String {
    let script = std::fs::read_to_string(repo_root().join("scripts").join("dev-install.sh"))
        .expect("scripts/dev-install.sh must be readable");
    let marker = "# --- quem responde pelo nome no caminho de busca";
    let start = script
        .find(marker)
        .expect("dev-install.sh tem de resolver o nome pelo caminho de busca ao terminar");
    let rest = &script[start..];
    let end = rest.find("\ndone\n").expect("o laço da conferência tem de fechar") + "\ndone\n".len();
    rest[..end].to_string()
}

/// Roda o fecho do script com as duas cópias dadas e o caminho de busca dado.
fn run_closing_block(dir: &Path, plugin_copy: &Path, system_dir: &Path, path: &str) -> (String, String) {
    let script = dir.join("fecho.sh");
    std::fs::write(
        &script,
        format!(
            "set -eu\nPLUGIN_COPY=\"{}\"\nSYSTEM_DIR=\"{}\"\n{}",
            plugin_copy.display(),
            system_dir.display(),
            closing_block()
        ),
    )
    .unwrap();
    let out = Command::new("sh")
        .arg(&script)
        .env("PATH", path)
        .output()
        .expect("sh roda o fecho do script");
    (String::from_utf8_lossy(&out.stdout).into_owned(), String::from_utf8_lossy(&out.stderr).into_owned())
}

#[test]
fn o_script_de_desenvolvimento_avisa_quem_responde_no_caminho() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let plugin_copy = root.join("plugin-copy");
    let system_dir = root.join("system-copy");
    let intruso = root.join("outra-copia");
    fake_binary(&plugin_copy.join("bin").join("mustard-rt"));
    fake_binary(&system_dir.join("bin").join("mustard-rt"));
    fake_binary(&intruso.join("mustard-rt"));

    // Caso 1 — a terceira cópia vem antes no caminho, exatamente como a de
    // `~/.cargo/bin` vinha em 21/09/2026: o fecho tem de avisar, dizendo de
    // onde o nome responde e que aquilo não é cópia trocada nenhuma.
    let (_, erros) = run_closing_block(
        root,
        &plugin_copy,
        &system_dir,
        &format!("{}:/usr/bin:/bin", intruso.display()),
    );
    let intruso_rt = intruso.join("mustard-rt");
    assert!(
        erros.contains("aviso: mustard-rt responde de") && erros.contains(&intruso_rt.display().to_string()),
        "o fecho não avisou sobre a cópia de fora do script: {erros}"
    );
    assert!(
        erros.contains("NÃO é nenhuma das cópias trocadas"),
        "o aviso não diz que quem responde está fora das duas cópias: {erros}"
    );

    // Caso 2 — a cópia do plugin, que o script acabou de trocar, é quem
    // responde: nada de aviso, e o script diz de onde o nome vem.
    let (saida, erros) = run_closing_block(
        root,
        &plugin_copy,
        &system_dir,
        &format!("{}:/usr/bin:/bin", plugin_copy.join("bin").display()),
    );
    assert!(
        saida.contains("mustard-rt responde de") && saida.contains("uma das cópias trocadas"),
        "o fecho não disse que quem responde é a cópia trocada: {saida}"
    );
    assert!(
        !erros.contains("aviso: mustard-rt"),
        "o fecho avisou sobre a própria cópia que acabou de trocar: {erros}"
    );
}
