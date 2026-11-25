//! A soft brake for expensive daemon work. Existing work owns its deadlines
//! and may drain while admission is held.
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

const SAMPLE_INTERVAL: Duration = Duration::from_secs(2);
const CALM_INTERVAL: Duration = Duration::from_secs(30);

#[derive(Default)]
struct Admission {
    held: bool,
    last_sample: Option<Instant>,
    calm_since: Option<Instant>,
}

impl Admission {
    fn observe(&mut self, now: Instant, available: Option<f64>, stall: Option<f64>) {
        if available.is_some_and(|pct| pct <= 8.0) || stall.is_some_and(|pct| pct >= 10.0) {
            self.held = true;
            self.calm_since = None;
        } else if self.held {
            if available.is_none_or(|pct| pct >= 10.0) && stall.is_none_or(|pct| pct < 5.0) {
                let calm_since = self.calm_since.get_or_insert(now);
                if now.duration_since(*calm_since) >= CALM_INTERVAL {
                    self.held = false;
                }
            } else {
                self.calm_since = None;
            }
        }
    }
}

fn headroom_pct(meminfo: &str) -> Option<f64> {
    let read = |key| -> Option<f64> {
        meminfo.lines().find_map(|line| {
            line.strip_prefix(key)?
                .split_whitespace()
                .next()?
                .parse()
                .ok()
        })
    };
    let total = read("MemTotal:")?;
    let headroom =
        read("MemFree:")? + read("Buffers:")? + read("Cached:")? + read("SReclaimable:")?;
    (total > 0.0).then_some(100.0 * headroom / total)
}

fn full_stall(pressure: &str) -> Option<f64> {
    pressure.lines().find_map(|line| {
        line.strip_prefix("full ")?
            .split_whitespace()
            .find_map(|field| field.strip_prefix("avg10=")?.parse().ok())
    })
}

fn local_pressure_path() -> Option<&'static Path> {
    static PATH: OnceLock<Option<PathBuf>> = OnceLock::new();
    PATH.get_or_init(|| {
        let groups = std::fs::read_to_string("/proc/self/cgroup").ok()?;
        let group = groups.lines().find_map(|line| line.strip_prefix("0::/"))?;
        Some(
            Path::new("/sys/fs/cgroup")
                .join(group)
                .join("memory.pressure"),
        )
    })
    .as_deref()
}

fn read_full_stall(path: &Path) -> Option<f64> {
    full_stall(&std::fs::read_to_string(path).ok()?)
}

static ADMISSION: Mutex<Admission> = Mutex::new(Admission {
    held: false,
    last_sample: None,
    calm_since: None,
});

pub(crate) fn resource_kill() {
    let mut admission = ADMISSION.lock().expect("memory admission poisoned");
    admission.held = true;
    admission.calm_since = None;
    eprintln!("scheduler: isolated push was OOM-killed; waiting for sustained calm before admitting uploads and inspections");
}

pub(crate) fn held() -> bool {
    let mut admission = ADMISSION.lock().expect("memory admission poisoned");
    let now = Instant::now();
    if admission
        .last_sample
        .is_none_or(|at| now.duration_since(at) >= SAMPLE_INTERVAL)
    {
        admission.last_sample = Some(now);
        let available = std::fs::read_to_string("/proc/meminfo")
            .ok()
            .as_deref()
            .and_then(headroom_pct);
        let stall = read_full_stall(Path::new("/proc/pressure/memory"))
            .into_iter()
            .chain(local_pressure_path().and_then(read_full_stall))
            .reduce(f64::max);
        let was_held = admission.held;
        admission.observe(now, available, stall);
        if was_held != admission.held {
            if admission.held {
                eprintln!("scheduler: memory pressure holding new uploads and inspections (available={available:?}%, full stall={stall:?}%)");
            } else {
                eprintln!(
                    "scheduler: memory pressure cleared; resuming upload and inspection admission"
                );
            }
        }
    }
    admission.held
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pressure_enters_immediately_and_requires_continuous_calm_to_release() {
        let mut admission = Admission::default();
        let now = Instant::now();
        admission.observe(now, Some(40.0), Some(10.0));
        assert!(admission.held);
        admission.observe(now, Some(12.0), Some(1.0));
        admission.observe(now + Duration::from_secs(20), Some(9.0), Some(1.0));
        admission.observe(now + Duration::from_secs(30), Some(12.0), Some(1.0));
        assert!(admission.held);
        admission.observe(now + Duration::from_secs(60), Some(12.0), Some(1.0));
        assert!(!admission.held);
        admission.observe(now + Duration::from_secs(62), Some(7.0), None);
        assert!(admission.held);
    }

    #[test]
    fn reads_available_memory_and_full_stalls() {
        assert_eq!(
            headroom_pct(
                "MemTotal: 1000 kB\nMemFree: 50 kB\nBuffers: 25 kB\nCached: 25 kB\nSReclaimable: 25 kB",
            ),
            Some(12.5)
        );
        assert_eq!(
            full_stall("some avg10=15.0\nfull avg10=3.5 avg60=0.0"),
            Some(3.5)
        );
    }
}
