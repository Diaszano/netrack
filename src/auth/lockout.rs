use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub const MAX_FAILS: u32 = 5;
pub const WINDOW: Duration = Duration::from_secs(15 * 60);

/// Failed-login counter per client IP: (failures, time of the first failure in the window).
#[derive(Default)]
pub struct Lockout {
    inner: Mutex<HashMap<IpAddr, (u32, Instant)>>,
}

impl Lockout {
    /// `Err(remaining)` while the IP is locked out.
    pub fn check(&self, ip: IpAddr, now: Instant) -> Result<(), Duration> {
        let mut map = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        // Drop expired entries so the map cannot grow without bound.
        map.retain(|_, (_, start)| now.saturating_duration_since(*start) < WINDOW);
        match map.get(&ip) {
            Some((n, start)) if *n >= MAX_FAILS => {
                Err(WINDOW.saturating_sub(now.saturating_duration_since(*start)))
            }
            _ => Ok(()),
        }
    }

    pub fn fail(&self, ip: IpAddr, now: Instant) {
        let mut map = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let e = map.entry(ip).or_insert((0, now));
        if now.saturating_duration_since(e.1) >= WINDOW {
            *e = (0, now);
        }
        e.0 += 1;
    }

    pub fn succeed(&self, ip: IpAddr) {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&ip);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const IP: IpAddr = IpAddr::V4(std::net::Ipv4Addr::new(1, 2, 3, 4));

    #[test]
    fn blocks_after_five_failures() {
        let l = Lockout::default();
        let t = Instant::now();
        for _ in 0..MAX_FAILS {
            l.fail(IP, t);
        }
        assert!(matches!(l.check(IP, t), Err(d) if d <= WINDOW));
    }

    #[test]
    fn success_resets() {
        let l = Lockout::default();
        let t = Instant::now();
        for _ in 0..4 {
            l.fail(IP, t);
        }
        l.succeed(IP);
        for _ in 0..4 {
            l.fail(IP, t);
        }
        assert!(l.check(IP, t).is_ok());
    }

    #[test]
    fn window_expires() {
        let l = Lockout::default();
        let t0 = Instant::now();
        for _ in 0..MAX_FAILS {
            l.fail(IP, t0);
        }
        assert!(l.check(IP, t0 + WINDOW + Duration::from_secs(1)).is_ok());
    }
}
