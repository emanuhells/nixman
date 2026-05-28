use std::collections::HashMap;

/// Resolves a NixOS `services.<name>` attribute name to the corresponding
/// systemd unit name.
///
/// Most NixOS services map trivially (`{name}.service`), but a handful use a
/// different underlying unit name.  The explicit map covers the most common
/// divergences; everything else falls back to `{name}.service`.
///
/// # Examples
///
/// ```
/// use nixman_core::services::config_map;
///
/// assert_eq!(config_map::resolve("nginx"),    "nginx.service");
/// assert_eq!(config_map::resolve("openssh"),  "sshd.service");
/// assert_eq!(config_map::resolve("printing"), "cups.service");
/// assert_eq!(config_map::resolve("unknown"),  "unknown.service");
/// ```
pub fn resolve(nix_service_name: &str) -> String {
    // The map is small; building it on each call is negligible and avoids the
    // need for global/lazy state.
    let explicit: HashMap<&str, &str> = [
        // SSH daemon: NixOS attr is "openssh", systemd unit is "sshd.service".
        ("openssh", "sshd.service"),
        // CUPS print spooler.
        ("printing", "cups.service"),
        // Avahi mDNS/DNS-SD daemon.
        ("avahi", "avahi-daemon.service"),
        // X.Org / Wayland display manager.
        ("xserver", "display-manager.service"),
        // systemd-resolved DNS stub resolver.
        ("resolved", "systemd-resolved.service"),
        // systemd-networkd.
        ("networkd", "systemd-networkd.service"),
        // NetworkManager (note: capitalised unit name).
        ("networkmanager", "NetworkManager.service"),
        // Postfix MTA.
        ("postfix", "postfix.service"),
        // OpenSMTPd mail server.
        ("opensmtpd", "opensmtpd.service"),
        // Fail2ban intrusion prevention.
        ("fail2ban", "fail2ban.service"),
        // Bluetooth daemon.
        ("bluetooth", "bluetooth.service"),
        // PipeWire audio/video router.
        ("pipewire", "pipewire.service"),
        // PulseAudio (for setups not using PipeWire).
        ("pulseaudio", "pulseaudio.service"),
        // Firewall (nftables-based on modern NixOS).
        ("firewall", "nftables.service"),
        // Cron / fcron job scheduler.
        ("cron", "cron.service"),
        // at / atd job scheduler.
        ("atd", "atd.service"),
        // earlyoom out-of-memory handler.
        ("earlyoom", "earlyoom.service"),
        // thermald CPU temperature management.
        ("thermald", "thermald.service"),
        // udisks2 storage daemon.
        ("udisks2", "udisks2.service"),
        // polkit authentication agent.
        ("polkit", "polkit.service"),
        // MySQL / MariaDB (NixOS uses "mysql" for both).
        ("mysql", "mysql.service"),
        ("mariadb", "mysql.service"),
        // logrotate (runs as a service triggered by a timer).
        ("logrotate", "logrotate.service"),
        // fstrim periodic SSD trim.
        ("fstrim", "fstrim.service"),
    ]
    .into_iter()
    .collect();

    explicit
        .get(nix_service_name)
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("{nix_service_name}.service"))
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_direct_mappings() {
        assert_eq!(resolve("openssh"), "sshd.service");
        assert_eq!(resolve("printing"), "cups.service");
        assert_eq!(resolve("avahi"), "avahi-daemon.service");
        assert_eq!(resolve("xserver"), "display-manager.service");
        assert_eq!(resolve("networkmanager"), "NetworkManager.service");
        assert_eq!(resolve("firewall"), "nftables.service");
        assert_eq!(resolve("mariadb"), "mysql.service");
    }

    #[test]
    fn test_fallback_appends_dot_service() {
        assert_eq!(resolve("nginx"), "nginx.service");
        assert_eq!(resolve("postgresql"), "postgresql.service");
        assert_eq!(resolve("redis"), "redis.service");
        assert_eq!(resolve("docker"), "docker.service");
        assert_eq!(resolve("unknown-service"), "unknown-service.service");
    }
}
