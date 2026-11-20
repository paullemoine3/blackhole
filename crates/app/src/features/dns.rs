//! Encrypted DNS (Settings › Privacy): what each system needs once it is switched on or off, or
//! its resolver changes. Windows: WebView2's own DNS over HTTPS, at the next launch
//! (`views::set_encrypted_dns`). macOS: WebKit uses the system's DNS, so a system profile is
//! offered for the user to install (and removed by them from System Settings). Linux: the
//! system's resolver (systemd-resolved) decides.

use blackhole_domain::dns;

use crate::i18n::{t, tf};
use crate::state::Running;

impl Running {
    /// The Encrypted DNS card of Settings.
    pub(crate) fn dns_json(&self) -> serde_json::Value {
        let settings = self.svc.settings();
        let wanted = settings.encrypted_dns.then(|| dns::doh_template(&settings.dns_resolver)).flatten();
        serde_json::json!({
            "on": settings.encrypted_dns,
            "resolver": settings.dns_resolver,
            "resolvers": dns::RESOLVERS.iter().map(|r| serde_json::json!({"id": r.id, "name": r.name})).collect::<Vec<_>>(),
            // Windows: WebView2 itself, at the next launch; macOS: a system profile; elsewhere
            // the system's own setting.
            "applies": if cfg!(windows) { "restart" } else if cfg!(target_os = "macos") { "profile" } else { "system" },
            // Windows: the engine still runs with what it was started with.
            "restart": cfg!(windows) && crate::views::encrypted_dns_at_launch().map(str::to_owned) != wanted,
        })
    }

    pub(crate) fn encrypted_dns_changed(&mut self, on: bool) {
        let choice = self.svc.settings().dns_resolver.clone();
        if cfg!(windows) {
            self.toast(&t("toast.dns_restart"));
        } else if cfg!(target_os = "macos") {
            if on {
                self.install_dns_profile(&choice);
            } else {
                open_profiles_settings();
                self.toast(&t("toast.dns_profile_remove"));
            }
        } else {
            self.toast(&t("toast.dns_system"));
        }
    }

    /// Writes the profile for `choice` and hands it to the system, which asks the user to
    /// review it in System Settings › Profiles.
    fn install_dns_profile(&mut self, choice: &str) {
        let Some(template) = dns::doh_template(choice) else {
            return;
        };
        let title = tf("settings.dns_profile_name", &[("resolver", &dns::display_name(choice))]);
        match blackhole_infrastructure::write_dns_profile(&self.data, &dns::mobileconfig(&template, &title)) {
            Ok(path) => {
                open_with_system(&path);
                open_profiles_settings();
                self.toast(&t("toast.dns_profile_review"));
            }
            Err(why) => tracing::warn!("cannot write the DNS profile: {why}"),
        }
    }
}

/// macOS: opens a file with its app (a profile goes to System Settings).
fn open_with_system(path: &std::path::Path) {
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(path).spawn();
    #[cfg(not(target_os = "macos"))]
    let _ = path;
}

/// macOS: System Settings › General › Device Management (installed and downloaded profiles).
fn open_profiles_settings() {
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open")
        .arg("x-apple.systempreferences:com.apple.Profiles-Settings.extension")
        .spawn();
}
