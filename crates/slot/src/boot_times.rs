//! The uptime at each step of startup, in the log the startup scripts keep (`SLOT_BOOT_TIMES`,
//! Slot/Logs/boot-times.log): where the time from power on to the shelf goes (issue #44).
use std::fs::File;
use std::io::Write;
use std::sync::{Mutex, OnceLock};

/// The log, opened once: startup marks dozens of steps, and each open is a trip to the card.
fn log() -> Option<&'static Mutex<File>> {
    static LOG: OnceLock<Option<Mutex<File>>> = OnceLock::new();
    LOG.get_or_init(|| {
        let path = std::path::PathBuf::from(std::env::var_os("SLOT_BOOT_TIMES")?);
        // The boot scripts may always supply the path. Opt in before opening
        // or reading uptime for native startup timing marks.
        if !crate::session::trace() && !path.parent().and_then(|p| p.parent())
            .is_some_and(|root| root.join("POWER_DIAGNOSTICS").exists()) { return None; }
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .ok()
            .map(Mutex::new)
    })
    .as_ref()
}

pub fn mark(step: &str) {
    let Some(log) = log() else {
        return;
    };
    let Some(up) = std::fs::read_to_string("/proc/uptime")
        .ok()
        .and_then(|raw| slot_power::uptime_seconds(&raw))
    else {
        return;
    };
    let mut f = log.lock().unwrap_or_else(|e| e.into_inner());
    let _ = writeln!(f, "{up} slot: {step}");
}
