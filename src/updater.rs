use console::style;
use std::thread::JoinHandle;
use std::time::Duration;
use update_informer::{registry, Check};

const PKG_NAME: &str = "M-Igashi/headroom";
const CHECK_INTERVAL: Duration = Duration::from_secs(60 * 60 * 24);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(3);

/// Start the version check on a background thread so a slow or offline
/// network never stalls startup. The result is printed later via `notify`.
pub fn spawn_check() -> JoinHandle<Option<String>> {
    std::thread::spawn(check)
}

/// Print the update notification (if any) from a previously spawned check.
pub fn notify(handle: JoinHandle<Option<String>>) {
    let Ok(Some(new_version)) = handle.join() else {
        return;
    };

    let current = env!("CARGO_PKG_VERSION");
    let stripped = new_version.strip_prefix('v').unwrap_or(&new_version);

    println!(
        "\n{} Update available: {} → {}",
        style("↑").yellow().bold(),
        style(format!("v{}", current)).dim(),
        style(format!("v{}", stripped)).green().bold()
    );

    for line in update_commands() {
        println!("   {}", style(line).dim());
    }
    println!();
}

fn check() -> Option<String> {
    if std::env::var_os("HEADROOM_NO_UPDATE_CHECK").is_some() {
        return None;
    }

    let informer = update_informer::new(registry::GitHub, PKG_NAME, env!("CARGO_PKG_VERSION"))
        .interval(CHECK_INTERVAL)
        .timeout(REQUEST_TIMEOUT);

    informer.check_version().ok().flatten().map(|v| v.to_string())
}

fn update_commands() -> Vec<String> {
    if let Some(method) = detect_install_method() {
        return vec![method.command().to_string()];
    }

    let mut lines = Vec::new();
    if cfg!(target_os = "macos") {
        lines.push(InstallMethod::Homebrew.command().to_string());
        lines.push(InstallMethod::Cargo.command().to_string());
    } else if cfg!(target_os = "windows") {
        lines.push(InstallMethod::Winget.command().to_string());
        lines.push(InstallMethod::Cargo.command().to_string());
    } else {
        lines.push(InstallMethod::Cargo.command().to_string());
    }
    lines
}

#[derive(Clone, Copy)]
enum InstallMethod {
    Homebrew,
    Winget,
    Cargo,
}

impl InstallMethod {
    fn command(self) -> &'static str {
        match self {
            InstallMethod::Homebrew => "brew upgrade headroom",
            InstallMethod::Winget => "winget upgrade M-Igashi.headroom",
            InstallMethod::Cargo => "cargo install headroom",
        }
    }
}

fn detect_install_method() -> Option<InstallMethod> {
    let exe = std::env::current_exe().ok()?;
    let path = exe.to_string_lossy().to_lowercase();

    if path.contains("/homebrew/") || path.contains("/cellar/") || path.contains("/linuxbrew/") {
        Some(InstallMethod::Homebrew)
    } else if path.contains("\\winget\\") || path.contains("/winget/") {
        Some(InstallMethod::Winget)
    } else if path.contains("/.cargo/") || path.contains("\\.cargo\\") {
        Some(InstallMethod::Cargo)
    } else {
        None
    }
}
