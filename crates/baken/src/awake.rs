//! Keeps the computer from going to sleep on its own while a long run works
//! (issue #376). A sleep the user asks for, or a closed lid, still happens;
//! the display may still sleep. Linux asks for nothing: `systemd-inhibit`
//! has to wrap the process from outside.

/// Held for as long as the work runs; dropping it lets the computer idle
/// into sleep again. Failing to get it changes nothing else.
pub struct KeepAwake {
    #[cfg(target_os = "macos")]
    caffeinate: Option<std::process::Child>,
}

impl KeepAwake {
    /// macOS: `caffeinate -i` against idle system sleep, tied to this process
    /// with `-w` so it never outlives baken.
    #[cfg(target_os = "macos")]
    pub fn new() -> Self {
        use std::process::{Command, Stdio};
        let caffeinate = Command::new("/usr/bin/caffeinate")
            .args(["-i", "-w", &std::process::id().to_string()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .ok();
        KeepAwake { caffeinate }
    }

    /// Windows: `ES_SYSTEM_REQUIRED` for this thread until it is cleared.
    #[cfg(windows)]
    pub fn new() -> Self {
        // SAFETY: a plain Win32 call that takes and returns integers.
        unsafe {
            windows::SetThreadExecutionState(windows::ES_CONTINUOUS | windows::ES_SYSTEM_REQUIRED)
        };
        KeepAwake {}
    }

    #[cfg(not(any(target_os = "macos", windows)))]
    pub fn new() -> Self {
        KeepAwake {}
    }
}

impl Drop for KeepAwake {
    fn drop(&mut self) {
        #[cfg(target_os = "macos")]
        if let Some(mut child) = self.caffeinate.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        #[cfg(windows)]
        // SAFETY: as in `new`.
        unsafe {
            windows::SetThreadExecutionState(windows::ES_CONTINUOUS);
        }
    }
}

#[cfg(windows)]
mod windows {
    pub const ES_CONTINUOUS: u32 = 0x8000_0000;
    pub const ES_SYSTEM_REQUIRED: u32 = 0x0000_0001;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        pub fn SetThreadExecutionState(flags: u32) -> u32;
    }
}
