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
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    cmd
}
