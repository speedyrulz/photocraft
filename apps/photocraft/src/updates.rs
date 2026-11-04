//! Release checks through `craft-update`, the library every Craft App shares. Only public release
//! metadata is requested.

use photocraft_engine::updates_cmds::{CheckFn, CheckResult, Release};
use photocraft_ui_egui::InstallUpdateFn;

pub fn service() -> CheckFn {
    Box::new(|| {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::Builder::new()
            .name("update check".into())
            .spawn(move || {
                let result = check();
                if let Err(e) = &result {
                    log::warn!("Update check: {e}");
                }
                let _ = tx.send(result);
            })
            .map_err(|e| format!("Could not start update check: {e}"))?;
        Ok(rx)
    })
}

fn check() -> CheckResult {
    let http = craft_update::Http::new("PhotoCraft/Update")?;
    let found = craft_update::check_for_update(&photocraft_ui_egui::UPDATE_APP, &http, photocraft_engine::build_info::VERSION)?;
    Ok(found.map(|release| Release { version: release.tag.trim_start_matches(['v', 'V']).to_string(), url: release.html_url }))
}

/// Runs the `update.bat` that `install.bat` put beside this program, when there is one. It waits
/// for this process to exit, replaces the app with the fork's newest release and starts it again.
#[cfg(windows)]
pub fn installer() -> Option<InstallUpdateFn> {
    let script = std::env::current_exe().ok()?.parent()?.join("update.bat");
    script.is_file().then(|| {
        Box::new(move || {
            use std::os::windows::process::CommandExt;
            let line = command_line(&script, std::process::id()).ok_or("The folder of update.bat can't be passed to the command prompt")?;
            std::process::Command::new("cmd.exe").raw_arg(line).spawn().map(drop).map_err(|e| format!("Could not start update.bat: {e}"))
        }) as InstallUpdateFn
    })
}

#[cfg(not(windows))]
pub fn installer() -> Option<InstallUpdateFn> {
    None
}

/// `cmd.exe`'s command line for `script`. `cmd` has its own quoting: `/S /C "..."` strips the outer
/// quotes and keeps the rest. `None` for a path a quote would break out of (Windows forbids `"` in
/// file names, so only a hostile path has one) or that `cmd` would expand.
#[cfg_attr(not(windows), allow(dead_code))]
fn command_line(script: &std::path::Path, pid: u32) -> Option<String> {
    let script = script.to_str().filter(|s| !s.contains(['"', '%', '\n', '\r']))?;
    Some(format!("/S /C \"\"{script}\" --wait-pid {pid}\""))
}

#[cfg(test)]
mod tests {
    use super::command_line;
    use std::path::Path;

    #[test]
    fn command_line_quotes_spaces_and_refuses_what_cmd_would_reinterpret() {
        let line = command_line(Path::new(r"C:\Users\a b\RulZ Photocraft\update.bat"), 42);
        assert_eq!(line.as_deref(), Some(r#"/S /C ""C:\Users\a b\RulZ Photocraft\update.bat" --wait-pid 42""#));
        assert_eq!(command_line(Path::new(r#"C:\a"&calc\update.bat"#), 1), None);
        assert_eq!(command_line(Path::new(r"C:\%TEMP%\update.bat"), 1), None);
    }
}
