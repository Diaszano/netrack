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
    /// Reserves one attempt under a single lock, so parallel requests cannot all slip past the
    /// limit before any failure is recorded. `Err(remaining)` while the IP is locked out.
    pub fn try_begin(&self, ip: IpAddr, now: Instant) -> Result<(), Duration> {
        let mut map = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        // Drop expired entries so the map cannot grow without bound.
        map.retain(|_, (_, start)| now.saturating_duration_since(*start) < WINDOW);
        let e = map.entry(ip).or_insert((0, now));
        if e.0 >= MAX_FAILS {
            return Err(WINDOW.saturating_sub(now.saturating_duration_since(e.1)));
        }
        e.0 += 1;
        Ok(())
    }

    /// A correct password clears the reserved attempts.
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
    fn blocks_after_five_attempts() {
        let l = Lockout::default();
        let t = Instant::now();
        for _ in 0..MAX_FAILS {
            assert!(l.try_begin(IP, t).is_ok());
        }
        assert!(matches!(l.try_begin(IP, t), Err(d) if d <= WINDOW));
    }

    #[test]
    fn success_resets() {
        let l = Lockout::default();
        let t = Instant::now();
        for _ in 0..4 {
            l.try_begin(IP, t).unwrap();
        }
        l.succeed(IP);
        for _ in 0..4 {
            l.try_begin(IP, t).unwrap();
        }
        assert!(l.try_begin(IP, t).is_ok());
    }

    #[test]
    fn window_expires() {
        let l = Lockout::default();
        let t0 = Instant::now();
        for _ in 0..MAX_FAILS {
            l.try_begin(IP, t0).unwrap();
        }
        assert!(
            l.try_begin(IP, t0 + WINDOW + Duration::from_secs(1))
                .is_ok()
        );
    }
}
