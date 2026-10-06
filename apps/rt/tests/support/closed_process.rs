//! O processo que já fechou: o Claude Code que mandou uma onda e terminou.
//!
//! Um arquivo só para o pacote inteiro: os testes da pasta `tests/` o trazem
//! pelo caminho, e os de dentro de `src/`, pelo módulo que a rodada declara só
//! para teste, entre os ajudantes que os testes dela dividem.

/// O par — o número e a hora de início — de um processo que já terminou,
/// nascido numa hora que o número dele nunca teve: quem confere o par o lê
/// como fechado, não importa quem lançou a suíte. O processo é o próprio
/// executável do teste listando os testes, que toda máquina tem, no lugar de
/// um `true` que o Windows não traz, e o par só sai depois de ele acabar.
pub fn closed_process() -> (u32, u64) {
    let mut gone = std::process::Command::new(std::env::current_exe().expect("the test executable"))
        .arg("--list")
        .stdout(std::process::Stdio::null())
        .spawn()
        .expect("the process runs");
    let pid = gone.id();
    gone.wait().expect("the process ends");
    (pid, 1)
}
