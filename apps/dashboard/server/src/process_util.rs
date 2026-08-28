/// Build a `std::process::Command` that will not open a visible console window
/// on Windows. On non-Windows platforms this is identical to
/// `std::process::Command::new(program)`.
///
/// Every external-process spawn in the dashboard backend must go through this
/// helper so packaged users never see a flickering cmd.exe window.
pub fn no_window_command(program: &str) -> std::process::Command {
    // `mut` is only exercised by the Windows branch below.
    #[cfg_attr(not(windows), allow(unused_mut))]
    let mut cmd = std::process::Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // `CREATE_NO_WINDOW`, copiado da documentação da Microsoft. Sem
        // separadores de dígito de propósito: é uma constante PUBLICADA, e quem
        // a confere procura por `0x08000000` na página da API — agrupar os
        // dígitos faz o valor deixar de bater com a fonte.
        #[allow(clippy::unreadable_literal)]
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}
