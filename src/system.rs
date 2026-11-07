//! Process resource statistics (Linux `/proc`; `None` elsewhere).

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProcessStats {
    /// Average CPU use since the process started, in percent of one core.
    pub cpu_percent: Option<f64>,
    pub rss_bytes: Option<u64>,
}

pub fn process_stats() -> ProcessStats {
    #[cfg(target_os = "linux")]
    {
        linux::read()
    }
    #[cfg(not(target_os = "linux"))]
    {
        ProcessStats {
            cpu_percent: None,
            rss_bytes: None,
        }
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use super::ProcessStats;

    // Fixed on every mainstream Linux target (USER_HZ / 4 KiB pages), which
    // avoids a libc dependency for sysconf.
    const CLK_TCK: f64 = 100.0;
    const PAGE_SIZE: u64 = 4096;

    pub fn read() -> ProcessStats {
        ProcessStats {
            cpu_percent: cpu_percent(),
            rss_bytes: rss_bytes(),
        }
    }

    fn rss_bytes() -> Option<u64> {
        let statm = std::fs::read_to_string("/proc/self/statm").ok()?;
        let pages: u64 = statm.split_whitespace().nth(1)?.parse().ok()?;
        Some(pages * PAGE_SIZE)
    }

    fn cpu_percent() -> Option<f64> {
        let stat = std::fs::read_to_string("/proc/self/stat").ok()?;
        // The command name may contain spaces; fields resume after the last ')'.
        let rest = &stat[stat.rfind(')')? + 1..];
        let f: Vec<&str> = rest.split_whitespace().collect();
        // Field numbers in proc(5) minus 3: utime=14, stime=15, starttime=22.
        let utime: f64 = f.get(11)?.parse().ok()?;
        let stime: f64 = f.get(12)?.parse().ok()?;
        let start: f64 = f.get(19)?.parse().ok()?;
        let uptime: f64 = std::fs::read_to_string("/proc/uptime")
            .ok()?
            .split_whitespace()
            .next()?
            .parse()
            .ok()?;
        let lifetime = uptime - start / CLK_TCK;
        (lifetime > 0.0).then(|| (utime + stime) / CLK_TCK / lifetime * 100.0)
    }
}
