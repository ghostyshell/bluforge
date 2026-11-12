//! Keep the machine awake while a long rip runs.
//!
//! Each platform has its own mechanism; the guard releases it on drop so an
//! interrupted or failed rip does not pin the machine awake.

/// An acquired sleep inhibitor. Dropping it releases the inhibitor.
pub struct SleepGuard {
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    child: Option<std::process::Child>,
}

impl Drop for SleepGuard {
    fn drop(&mut self) {
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        #[cfg(target_os = "windows")]
        unsafe {
            windows_sys::Win32::System::Power::SetThreadExecutionState(
                windows_sys::Win32::System::Power::ES_CONTINUOUS,
            );
        }
    }
}

/// Inhibit sleep until the returned guard is dropped.
pub fn prevent() -> SleepGuard {
    #[cfg(target_os = "macos")]
    {
        let child = std::process::Command::new("caffeinate")
            .args(["-dimsu"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .ok();
        return SleepGuard { child };
    }
    #[cfg(target_os = "linux")]
    {
        let child = if crate::binaries::helper("systemd-inhibit").is_some() {
            std::process::Command::new("systemd-inhibit")
                .args([
                    "--what=sleep:idle",
                    "--who=Keepsake Atelier",
                    "--why=Ripping a disc",
                    "sleep",
                    "infinity",
                ])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .ok()
        } else {
            None
        };
        return SleepGuard { child };
    }
    #[cfg(target_os = "windows")]
    {
        unsafe {
            windows_sys::Win32::System::Power::SetThreadExecutionState(
                windows_sys::Win32::System::Power::ES_CONTINUOUS
                    | windows_sys::Win32::System::Power::ES_SYSTEM_REQUIRED
                    | windows_sys::Win32::System::Power::ES_DISPLAY_REQUIRED,
            );
        }
        return SleepGuard {};
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        SleepGuard {}
    }
}
