//! System desktop notification dispatcher for Hawk Terminal (Windows Toast / WinRT API)

#[cfg(target_os = "windows")]
pub const HAWK_AUMID: &str = "Hawk.Terminal";

#[cfg(target_os = "windows")]
fn ensure_windows_aumid_registered() {
    use std::sync::atomic::{AtomicBool, Ordering};
    static REGISTERED: AtomicBool = AtomicBool::new(false);
    if REGISTERED.swap(true, Ordering::Relaxed) {
        return;
    }

    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x08000000;

    let icon_path = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|parent| parent.join("icon.ico")))
        .filter(|p| p.exists())
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();

    let mut cmd = std::process::Command::new("reg");
    cmd.args([
        "add",
        r"HKCU\Software\Classes\AppUserModelId\Hawk.Terminal",
        "/v",
        "DisplayName",
        "/d",
        "Hawk Terminal",
        "/f",
    ])
    .creation_flags(CREATE_NO_WINDOW);
    let _ = cmd.output();

    if !icon_path.is_empty() {
        let mut icon_cmd = std::process::Command::new("reg");
        icon_cmd
            .args([
                "add",
                r"HKCU\Software\Classes\AppUserModelId\Hawk.Terminal",
                "/v",
                "IconUri",
                "/d",
                &icon_path,
                "/f",
            ])
            .creation_flags(CREATE_NO_WINDOW);
        let _ = icon_cmd.output();
    }

    let mut settings_cmd = std::process::Command::new("reg");
    settings_cmd
        .args([
            "add",
            r"HKCU\Software\Classes\AppUserModelId\Hawk.Terminal",
            "/v",
            "ShowInSettings",
            "/t",
            "REG_DWORD",
            "/d",
            "1",
            "/f",
        ])
        .creation_flags(CREATE_NO_WINDOW);
    let _ = settings_cmd.output();
}

pub fn send_desktop_notification(title: &str, body: &str) {
    let title = title.to_string();
    let body = body.to_string();

    std::thread::spawn(move || {
        let mut notif = notify_rust::Notification::new();
        notif.appname("Hawk Terminal").summary(&title).body(&body);

        #[cfg(target_os = "windows")]
        {
            ensure_windows_aumid_registered();
            notif.app_id(HAWK_AUMID);
            notif.sound_name("Default");
        }

        if let Err(e) = notif.show() {
            log::warn!("Failed to display desktop notification: {e}");
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_notification_dispatch_does_not_panic() {
        // Dispatch test notification in headless/test environment
        send_desktop_notification("Test Alert", "BTC crossed 70000");
    }
}
