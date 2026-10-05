//! Process/container readings for investigating externally terminated services.
//! SIGKILL cannot be caught, so resource readings precede a possible termination.
use std::time::Duration;

pub fn start() {
    #[cfg(target_os = "linux")]
    tokio::spawn(async {
        let mut interval = tokio::time::interval(Duration::from_secs(10));
        loop {
            interval.tick().await;
            let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
            let current = read_number("/sys/fs/cgroup/memory.current");
            let limit = read_number("/sys/fs/cgroup/memory.max");
            let events =
                std::fs::read_to_string("/sys/fs/cgroup/memory.events").unwrap_or_default();
            tracing::info!(
                pid = std::process::id(),
                resident_bytes = ?proc_bytes(&status, "VmRSS:"),
                peak_resident_bytes = ?proc_bytes(&status, "VmHWM:"),
                cgroup_memory_bytes = ?current,
                cgroup_memory_limit_bytes = ?limit,
                cgroup_oom_kills = ?event_count(&events, "oom_kill"),
                "Water runtime resources"
            );
        }
    });
}

fn read_number(path: &str) -> Option<u64> {
    std::fs::read_to_string(path).ok()?.trim().parse().ok()
}

fn proc_bytes(status: &str, key: &str) -> Option<u64> {
    let line = status.lines().find(|line| line.starts_with(key))?;
    let mut fields = line.split_whitespace();
    fields.next()?;
    let kib: u64 = fields.next()?.parse().ok()?;
    (fields.next()? == "kB")
        .then(|| kib.checked_mul(1024))
        .flatten()
}

fn event_count(events: &str, key: &str) -> Option<u64> {
    events.lines().find_map(|line| {
        let mut fields = line.split_whitespace();
        (fields.next()? == key)
            .then(|| fields.next()?.parse().ok())
            .flatten()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_or_unrecognized_readings_stay_unknown() {
        assert_eq!(proc_bytes("VmRSS: 128 kB\n", "VmRSS:"), Some(131072));
        assert_eq!(proc_bytes("VmRSS: 128 MB\n", "VmRSS:"), None);
        assert_eq!(proc_bytes("", "VmRSS:"), None);
        assert_eq!(event_count("oom 2\noom_kill 1\n", "oom_kill"), Some(1));
        assert_eq!(event_count("oom 2\n", "oom_kill"), None);
    }
}
