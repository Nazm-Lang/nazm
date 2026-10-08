//! Where a build's time went (`--timings`), and the build report (`--build-report`).

use super::*;

/// Where a build's time went (N48): each phase's wall time since the one before, in the order
/// the build ran them. `--timings` prints them as `nazm.timings/1`, one line on stderr.
pub(super) struct Clock {
    start: std::time::Instant,
    last: std::sync::Mutex<(std::time::Instant, Vec<(&'static str, f64)>)>,
}

impl Clock {
    pub(super) fn start() -> Self {
        let now = std::time::Instant::now();
        Self {
            start: now,
            last: std::sync::Mutex::new((now, Vec::new())),
        }
    }

    pub(super) fn mark(&self, phase: &'static str) {
        if let Ok(mut g) = self.last.lock() {
            let now = std::time::Instant::now();
            let ms = now.duration_since(g.0).as_secs_f64() * 1000.0;
            g.1.push((phase, ms));
            g.0 = now;
        }
    }

    pub(super) fn report(&self) -> String {
        let total = self.start.elapsed().as_secs_f64() * 1000.0;
        let phases = self
            .last
            .lock()
            .map(|g| {
                g.1.iter()
                    .map(|(p, ms)| format!("{{\"phase\":\"{p}\",\"ms\":{ms:.3}}}"))
                    .collect::<Vec<_>>()
                    .join(",")
            })
            .unwrap_or_default();
        format!("{{\"schema\":\"nazm.timings/1\",\"phases\":[{phases}],\"total_ms\":{total:.3}}}")
    }
}

/// One `nazm.build-report/1` object.
///
/// Structured because the reader is as likely to be a script or an agent as a person, and
/// versioned because anything a script reads is an interface. Off unless asked for: a
/// compiler that reports on itself on every run is a compiler whose output has to be
/// filtered before it can be read.
///
/// # Why `semantic.reused` is always zero here
///
/// It is not an oversight and it is not a placeholder. `nazm check` may skip a module's
/// bodies because all it needs afterwards is the interface; `nazm build` needs the
/// *resolution* those bodies produce, because that is what lowering reads, and nothing
/// persists a resolution. So a build checks everything, every time, and says so rather
/// than leaving a reader to infer it from a missing field. `docs/roadmap.md` records what
/// reusing it would require.
///
/// # Why `link.performed` is always true
///
/// Same reason, stated the same way. The report is printed after a successful link, and
/// the link runs on every build: there is no executable cache and no incremental linker.
pub(super) fn report(root: crate::root::How, modules: usize, t: Native) -> String {
    format!(
        r#"{{"schema":"nazm.build-report/1","root":"{}","semantic":{{"checked":{modules},"reused":0}},"native":{{"units":{},"compiled":{},"reused":{},"rejected":{},"stored":{},"instances":{{"units":{},"compiled":{},"reused":{}}}}},"link":{{"performed":true}}}}"#,
        root.as_str(),
        t.units,
        t.compiled,
        t.reused,
        t.rejected,
        t.stored,
        t.instances,
        t.instances_compiled,
        t.instances_reused,
    )
}
