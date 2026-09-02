//! Cross-platform child-process launch policy for the OmicsOps desktop app.
//!
//! Windows GUI applications otherwise create a visible console window whenever
//! they start a console-subsystem child such as Python, PowerShell, Docker, or
//! an MCP server. All background children must be created through this module.

use std::ffi::OsStr;

use tokio::process::Command;

/// Windows `CREATE_NO_WINDOW` from `winbase.h`.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Creates a Tokio command suitable for a background desktop operation.
///
/// On Windows this prevents console-subsystem children from allocating a
/// visible console window. Other platforms retain Tokio's default behavior.
pub fn background_command(program: impl AsRef<OsStr>) -> Command {
    let mut command = Command::new(program);
    configure_background_command(&mut command);
    command
}

/// Applies the desktop background-process policy to an existing command.
pub fn configure_background_command(command: &mut Command) -> &mut Command {
    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW);
    command
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[tokio::test]
    async fn windows_background_child_has_no_console_window() {
        let script = concat!(
            "Add-Type -Name NativeMethods -Namespace OmicsOps -MemberDefinition '",
            "[System.Runtime.InteropServices.DllImport(\"kernel32.dll\")] ",
            "public static extern System.IntPtr GetConsoleWindow();'; ",
            "if ([OmicsOps.NativeMethods]::GetConsoleWindow() -ne [System.IntPtr]::Zero) ",
            "{ exit 17 }"
        );
        let status = background_command("powershell.exe")
            .args([
                "-NoLogo",
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                script,
            ])
            .status()
            .await
            .expect("Windows PowerShell should be available");
        assert!(status.success(), "background child allocated a console");
    }

    #[cfg(not(windows))]
    #[test]
    fn non_windows_configuration_is_a_no_op() {
        let mut command = Command::new("true");
        let original = std::ptr::addr_of!(command);
        let configured =
            configure_background_command(&mut command) as *mut Command as *const Command;
        assert_eq!(configured, original);
    }
}
