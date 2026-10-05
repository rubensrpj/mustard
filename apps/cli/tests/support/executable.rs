//! O programa falso ou o gancho que o teste grava e depois roda.
//!
//! Os testes de um pacote rodam em paralelo, no mesmo processo. Enquanto este
//! processo grava um arquivo, ele o tem aberto para escrita; um teste vizinho
//! que abre um programa nesse instante leva uma cópia dessa abertura para o
//! processo que nasce, e, até esse processo virar o programa dele, o Linux
//! recusa rodar o arquivo ("Text file busy"): o falso não roda, e o teste cai
//! sem motivo. Gravado por um shell à parte, o arquivo nunca fica aberto neste
//! processo, e já está fechado em todo lugar quando o shell sai.
//!
//! Um arquivo por pacote, como `manifest_dir.rs`: os testes da pasta `tests/`
//! o trazem pelo caminho, e os de dentro de `src/`, pelo módulo que a raiz do
//! pacote declara só para teste.

use std::path::Path;

/// Grava `text` em `path` e deixa o arquivo pronto para rodar. Fora do unix
/// não há shell nem permissão de execução: o arquivo é gravado direto.
pub fn write_executable(path: &Path, text: &str) {
    #[cfg(unix)]
    {
        let written = std::process::Command::new("/bin/sh")
            .args(["-c", "printf '%s' \"$2\" > \"$1\" && chmod 755 \"$1\"", "sh"])
            .arg(path)
            .arg(text)
            .status()
            .expect("the shell that writes the program starts");
        assert!(written.success(), "{} was not written", path.display());
    }
    #[cfg(not(unix))]
    std::fs::write(path, text).unwrap_or_else(|e| panic!("write {}: {e}", path.display()));
}

#[cfg(target_os = "linux")]
#[test]
fn the_program_is_written_by_another_process_and_runs_as_written() {
    use std::os::unix::fs::PermissionsExt as _;

    // O quanto esta linha de execução já escreveu, pela conta do próprio
    // Linux: a escrita de outro processo não entra nela.
    fn written_by_this_thread() -> u64 {
        let io = std::fs::read_to_string("/proc/thread-self/io").expect("the thread write count");
        io.lines()
            .find_map(|line| line.strip_prefix("wchar:"))
            .and_then(|n| n.trim().parse().ok())
            .expect("the wchar line")
    }

    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("fake");
    let text = format!("#!/bin/sh\n# {}\nprintf '%s' '100% \\n \"$HOME\"'\n", "x".repeat(4096));

    let before = written_by_this_thread();
    write_executable(&path, &text);
    let written_here = written_by_this_thread() - before;

    assert!(
        written_here < text.len() as u64,
        "this process wrote {written_here} bytes of a {}-byte program: it held the file open",
        text.len()
    );
    assert_eq!(std::fs::read_to_string(&path).expect("read back"), text, "every byte as given");
    let mode = std::fs::metadata(&path).expect("stat").permissions().mode() & 0o777;
    assert_eq!(mode, 0o755, "{mode:o}");
    let ran = std::process::Command::new(&path).output().expect("the program runs");
    assert!(ran.status.success(), "{ran:?}");
    assert_eq!(String::from_utf8_lossy(&ran.stdout), "100% \\n \"$HOME\"");
}
