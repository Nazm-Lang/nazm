//! `ContainerEngine` over the `docker` CLI.
//!
//! Shelling out rather than taking a Docker API client: it matches how `git` is already
//! invoked here, and a client would pull an async runtime into a crate that is entirely
//! synchronous.
//!
//! # Two traps this is shaped around
//!
//! **Killing the `docker` client is not killing the container.** `docker run` in the
//! foreground attaches a client to a container the daemon owns; terminating the client
//! detaches, and the container keeps running. So nothing here relies on client-process
//! termination as container termination — containers are created, started, and killed as
//! separate daemon operations, and a deadline that expires kills the *container*.
//!
//! **Every command needs a deadline.** A wedged daemon makes `docker inspect` hang
//! indefinitely, and a lifecycle with no bound anywhere becomes a run that never finishes
//! and never reports. Each invocation is bounded; a create that times out has an *unknown*
//! result and is resolved through ownership discovery rather than assumed not to have
//! happened.
//!
//! Argv construction is a pure function, so the security configuration can be asserted as
//! data without a daemon — see `tests/docker_argv.rs`.

use std::io::Read;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use super::{
    ContainerEngine, ContainerSpec, ContainerState, EngineError, Mount, MountSource, NetworkSpec,
    ResourceId, ResourceKind, VolumeSpec,
};

/// Per-command deadlines. Separate because they differ by an order of magnitude: an
/// inspect should be instant, an image pull should not be.
#[derive(Debug, Clone)]
pub struct Deadlines {
    pub create: Duration,
    pub start: Duration,
    pub inspect: Duration,
    pub kill: Duration,
    pub remove: Duration,
    pub list: Duration,
}

impl Default for Deadlines {
    fn default() -> Self {
        Self {
            create: Duration::from_secs(120),
            start: Duration::from_secs(60),
            inspect: Duration::from_secs(15),
            kill: Duration::from_secs(30),
            remove: Duration::from_secs(60),
            list: Duration::from_secs(15),
        }
    }
}

#[derive(Debug)]
pub struct DockerCli {
    binary: String,
    deadlines: Deadlines,
}

impl Default for DockerCli {
    fn default() -> Self {
        Self::new()
    }
}

impl DockerCli {
    #[must_use]
    pub fn new() -> Self {
        Self {
            binary: "docker".to_owned(),
            deadlines: Deadlines::default(),
        }
    }

    #[must_use]
    pub fn with_deadlines(mut self, deadlines: Deadlines) -> Self {
        self.deadlines = deadlines;
        self
    }

    /// Whether a daemon is reachable. Used to decide skip-versus-fail in the integration
    /// suite, never to decide whether a benchmark run may proceed.
    #[must_use]
    pub fn available(&self) -> bool {
        self.run(
            &[
                "info".to_owned(),
                "--format".to_owned(),
                "{{.ID}}".to_owned(),
            ],
            Duration::from_secs(10),
        )
        .is_ok()
    }

    fn run(&self, args: &[String], deadline: Duration) -> Result<String, EngineError> {
        let mut child = Command::new(&self.binary)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| EngineError::DaemonUnreachable(format!("could not run docker: {e}")))?;

        // Drain both pipes on threads. Polling `try_wait` while output accumulates in a
        // full pipe buffer deadlocks: the child blocks writing, the parent blocks waiting.
        let mut stdout_pipe = child.stdout.take();
        let mut stderr_pipe = child.stderr.take();
        let stdout_handle = std::thread::spawn(move || {
            let mut buf = String::new();
            if let Some(p) = stdout_pipe.as_mut() {
                let _ = p.read_to_string(&mut buf);
            }
            buf
        });
        let stderr_handle = std::thread::spawn(move || {
            let mut buf = String::new();
            if let Some(p) = stderr_pipe.as_mut() {
                let _ = p.read_to_string(&mut buf);
            }
            buf
        });

        let started = Instant::now();
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) => {
                    if started.elapsed() >= deadline {
                        let _ = child.kill();
                        let _ = child.wait();
                        return Err(EngineError::DaemonUnreachable(format!(
                            "`docker {}` exceeded its {}s deadline",
                            args.first().map_or("", String::as_str),
                            deadline.as_secs()
                        )));
                    }
                    std::thread::sleep(Duration::from_millis(20));
                }
                Err(e) => return Err(EngineError::DaemonUnreachable(e.to_string())),
            }
        };

        let stdout = stdout_handle.join().unwrap_or_default();
        let stderr = stderr_handle.join().unwrap_or_default();

        if status.success() {
            return Ok(stdout.trim().to_owned());
        }
        Err(classify(status.code().unwrap_or(-1), &stderr))
    }
}

/// Map docker's stderr onto the typed taxonomy.
///
/// Text-matching is unavoidable here — the CLI does not expose structured errors — but it
/// is confined to this one function so the rest of the code matches on a type. Anything
/// unrecognised stays `CliFailure` rather than being guessed into a specific variant,
/// because a wrong guess sends an attempt down the wrong disposition.
#[must_use]
pub fn classify(code: i32, stderr: &str) -> EngineError {
    let s = stderr.to_lowercase();
    if s.contains("cannot connect to the docker daemon")
        || s.contains("is the docker daemon running")
    {
        return EngineError::DaemonUnreachable(stderr.trim().to_owned());
    }
    if s.contains("no space left on device") {
        return EngineError::NoSpaceLeft;
    }
    if s.contains("already in use by container") || s.contains("already exists") {
        return EngineError::NameConflict {
            id: ResourceId::new(ResourceKind::Container, "<named in stderr>"),
        };
    }
    if s.contains("pool overlaps") || s.contains("overlaps with other one") {
        return EngineError::SubnetOverlap {
            subnet: "<named in stderr>".to_owned(),
        };
    }
    if s.contains("could not find an available, non-overlapping ipv4 address pool") {
        return EngineError::AddressPoolExhausted;
    }
    if s.contains("no such container")
        || s.contains("no such network")
        || s.contains("no such volume")
    {
        return EngineError::NotFound {
            id: ResourceId::new(ResourceKind::Container, "<named in stderr>"),
        };
    }
    if s.contains("removal of container") && s.contains("already in progress") {
        return EngineError::RemovalInProgress {
            id: ResourceId::new(ResourceKind::Container, "<named in stderr>"),
        };
    }
    if s.contains("has active endpoints") || s.contains("volume is in use") {
        return EngineError::StillReferenced {
            id: ResourceId::new(ResourceKind::Container, "<named in stderr>"),
        };
    }
    if s.contains("manifest unknown")
        || s.contains("pull access denied")
        || s.contains("not found: manifest")
    {
        return EngineError::ImagePullFailed {
            image: "<named in stderr>".to_owned(),
            reason: stderr.trim().to_owned(),
        };
    }
    EngineError::CliFailure {
        code,
        stderr: stderr.trim().to_owned(),
    }
}

fn label_args(spec_labels: &super::Labels) -> Vec<String> {
    spec_labels
        .as_pairs()
        .into_iter()
        .flat_map(|(k, v)| ["--label".to_owned(), format!("{k}={v}")])
        .collect()
}

/// Argv for `docker network create`. Pure, so the configuration is assertable as data.
#[must_use]
pub fn network_create_argv(spec: &NetworkSpec) -> Vec<String> {
    let mut a = vec![
        "network".to_owned(),
        "create".to_owned(),
        "--driver".to_owned(),
        "bridge".to_owned(),
    ];
    if spec.internal {
        // Removes the route out of the bridge. The candidate has no reason to reach the
        // internet, and egress would make cost incomparable across time and let a task
        // statement leave the machine.
        a.push("--internal".to_owned());
    }
    a.extend(["--subnet".to_owned(), spec.subnet.clone()]);
    a.extend(label_args(&spec.labels));
    a.push(spec.name.clone());
    a
}

#[must_use]
pub fn volume_create_argv(spec: &VolumeSpec) -> Vec<String> {
    let mut a = vec!["volume".to_owned(), "create".to_owned()];
    a.extend(label_args(&spec.labels));
    a.push(spec.name.clone());
    a
}

/// Argv for `docker create`.
///
/// `create`, never `run`: a foreground `run` attaches a client whose death does not stop
/// the container, and the exit code would come from the client rather than the workload.
#[must_use]
pub fn container_create_argv(spec: &ContainerSpec) -> Vec<String> {
    let mut a = vec!["create".to_owned(), "--name".to_owned(), spec.name.clone()];

    if spec.init {
        a.push("--init".to_owned());
    }
    if let Some(net) = &spec.network {
        a.extend(["--network".to_owned(), net.clone()]);
    }
    if let Some(alias) = &spec.network_alias {
        a.extend(["--network-alias".to_owned(), alias.clone()]);
    }

    a.extend(["--memory".to_owned(), format!("{}m", spec.memory_mb)]);
    // Equal to --memory, so the limit is a real limit rather than a swap-thrashing
    // latency cliff. A memory cap that can be exceeded by swapping is not a cap, and
    // `ResourceExhausted` would then be reporting something that never happened.
    a.extend(["--memory-swap".to_owned(), format!("{}m", spec.memory_mb)]);
    a.extend([
        "--cpus".to_owned(),
        format!("{:.3}", f64::from(spec.milli_cpus) / 1000.0),
    ]);
    a.extend(["--pids-limit".to_owned(), spec.pids_limit.to_string()]);

    a.push("--cap-drop".to_owned());
    a.push("ALL".to_owned());
    a.extend(["--security-opt".to_owned(), "no-new-privileges".to_owned()]);

    for m in &spec.mounts {
        a.push("--mount".to_owned());
        a.push(mount_arg(m));
    }
    for (k, v) in &spec.env {
        a.extend(["--env".to_owned(), format!("{k}={v}")]);
    }
    a.extend(label_args(&spec.labels));
    a.push(spec.image.clone());
    a.extend(spec.command.iter().cloned());
    a
}

fn mount_arg(m: &Mount) -> String {
    let ro = if m.read_only { ",readonly" } else { "" };
    match &m.source {
        MountSource::Volume(v) => format!("type=volume,source={v},target={}{ro}", m.target),
        MountSource::HostPath(p) => format!("type=bind,source={p},target={}{ro}", m.target),
        MountSource::Tmpfs { size_mb } => {
            format!("type=tmpfs,target={},tmpfs-size={}m", m.target, size_mb)
        }
    }
}

impl ContainerEngine for DockerCli {
    fn create_network(&self, spec: &NetworkSpec) -> Result<ResourceId, EngineError> {
        let out = self.run(&network_create_argv(spec), self.deadlines.create)?;
        Ok(ResourceId::new(ResourceKind::Network, &spec.name).with_engine_id(out))
    }

    fn create_volume(&self, spec: &VolumeSpec) -> Result<ResourceId, EngineError> {
        self.run(&volume_create_argv(spec), self.deadlines.create)?;
        // `docker volume create` echoes the name; volumes have no separate id.
        Ok(ResourceId::new(ResourceKind::Volume, &spec.name).with_engine_id(&spec.name))
    }

    fn create_container(&self, spec: &ContainerSpec) -> Result<ResourceId, EngineError> {
        let out = self.run(&container_create_argv(spec), self.deadlines.create)?;
        Ok(ResourceId::new(ResourceKind::Container, &spec.name).with_engine_id(out))
    }

    fn start(&self, id: &ResourceId) -> Result<(), EngineError> {
        self.run(
            &["start".to_owned(), id.handle().to_owned()],
            self.deadlines.start,
        )?;
        Ok(())
    }

    fn kill(&self, id: &ResourceId) -> Result<(), EngineError> {
        // SIGKILL explicitly. With SIGTERM a PID 1 that ignores it leaves its children
        // running; once PID 1 is actually gone the kernel signals every remaining task in
        // the namespace, which is what reaches detached and re-parented descendants.
        let args = [
            "kill".to_owned(),
            "--signal".to_owned(),
            "KILL".to_owned(),
            id.handle().to_owned(),
        ];
        match self.run(&args, self.deadlines.kill) {
            Ok(_) => Ok(()),
            // Already stopped is the desired state, not a failure.
            Err(EngineError::CliFailure { stderr, .. }) if stderr.contains("is not running") => {
                Ok(())
            }
            Err(e) => Err(e),
        }
    }

    fn inspect(&self, id: &ResourceId) -> Result<ContainerState, EngineError> {
        let args = [
            "inspect".to_owned(),
            "--format".to_owned(),
            "{{.State.Running}}|{{.State.ExitCode}}|{{.State.OOMKilled}}|{{.State.StartedAt}}|{{.State.FinishedAt}}"
                .to_owned(),
            id.handle().to_owned(),
        ];
        let out = self.run(&args, self.deadlines.inspect)?;
        let f: Vec<&str> = out.split('|').collect();
        let get = |i: usize| f.get(i).copied().unwrap_or_default();

        // Docker's zero time for a container that never ran. Parsed naively it yields a
        // large negative duration, which a `max(0, …)` would silently turn into zero
        // runtime — so it is rejected here and the cost line records it as missing.
        let real_time = |s: &str| {
            let s = s.trim();
            (!s.is_empty() && !s.starts_with("0001-01-01")).then(|| s.to_owned())
        };

        Ok(ContainerState {
            running: get(0) == "true",
            exit_code: get(1).parse().ok(),
            oom_killed: get(2) == "true",
            started_at: real_time(get(3)),
            finished_at: real_time(get(4)),
        })
    }

    fn remove(&self, id: &ResourceId, force: bool) -> Result<(), EngineError> {
        let mut args = match id.kind {
            ResourceKind::Container => vec!["rm".to_owned()],
            ResourceKind::Network => vec!["network".to_owned(), "rm".to_owned()],
            ResourceKind::Volume => vec!["volume".to_owned(), "rm".to_owned()],
        };
        if force && id.kind != ResourceKind::Network {
            args.push("--force".to_owned());
        }
        if id.kind == ResourceKind::Container {
            // Also remove anonymous volumes the image declared, which otherwise
            // accumulate invisibly — the postgres image declares one.
            args.push("--volumes".to_owned());
        }
        args.push(id.handle().to_owned());

        match self.run(&args, self.deadlines.remove) {
            Ok(_) | Err(EngineError::NotFound { .. }) => Ok(()),
            Err(e) => Err(e),
        }
    }

    fn list_by_label(&self, key: &str, value: &str) -> Result<Vec<ResourceId>, EngineError> {
        let mut out = Vec::new();
        for (kind, prefix) in [
            (
                ResourceKind::Container,
                vec!["ps".to_owned(), "--all".to_owned()],
            ),
            (
                ResourceKind::Network,
                vec!["network".to_owned(), "ls".to_owned()],
            ),
            (
                ResourceKind::Volume,
                vec!["volume".to_owned(), "ls".to_owned()],
            ),
        ] {
            let mut args = prefix;
            args.extend([
                "--filter".to_owned(),
                format!("label={key}={value}"),
                "--format".to_owned(),
                "{{.Name}}".to_owned(),
            ]);
            // `docker ps` uses .Names, the others .Name.
            if kind == ResourceKind::Container {
                let last = args.len() - 1;
                if let Some(slot) = args.get_mut(last) {
                    *slot = "{{.Names}}".to_owned();
                }
            }
            for line in self.run(&args, self.deadlines.list)?.lines() {
                let name = line.trim();
                if !name.is_empty() {
                    out.push(ResourceId::new(kind, name));
                }
            }
        }
        Ok(out)
    }

    fn subnets_in_use(&self) -> Result<Vec<String>, EngineError> {
        let ids = self.run(
            &["network".to_owned(), "ls".to_owned(), "--quiet".to_owned()],
            self.deadlines.list,
        )?;
        let mut out = Vec::new();
        for id in ids.lines().filter(|l| !l.trim().is_empty()) {
            let args = [
                "network".to_owned(),
                "inspect".to_owned(),
                "--format".to_owned(),
                "{{range .IPAM.Config}}{{.Subnet}} {{end}}".to_owned(),
                id.trim().to_owned(),
            ];
            // A network removed between listing and inspecting is not an error here.
            if let Ok(line) = self.run(&args, self.deadlines.inspect) {
                out.extend(line.split_whitespace().map(str::to_owned));
            }
        }
        Ok(out)
    }

    fn owner_of(&self, id: &ResourceId) -> Result<Option<String>, EngineError> {
        let template = match id.kind {
            // Containers keep labels under `.Config.Labels`; networks and volumes put
            // them at `.Labels`. Using `.Labels` on a container is not an empty result but
            // a template *error*, which surfaces as `Unknown` ownership and refuses every
            // removal. That is how this was found: the first run against a real daemon
            // quarantined rather than deleting the wrong thing.
            ResourceKind::Container => "{{index .Config.Labels \"nazm.owner\"}}",
            ResourceKind::Network | ResourceKind::Volume => "{{index .Labels \"nazm.owner\"}}",
        };
        let mut args = match id.kind {
            ResourceKind::Container => vec!["inspect".to_owned()],
            ResourceKind::Network => vec!["network".to_owned(), "inspect".to_owned()],
            ResourceKind::Volume => vec!["volume".to_owned(), "inspect".to_owned()],
        };
        args.extend([
            "--format".to_owned(),
            template.to_owned(),
            id.handle().to_owned(),
        ]);

        match self.run(&args, self.deadlines.inspect) {
            Ok(v) if v.trim().is_empty() || v.trim() == "<no value>" => Ok(None),
            Ok(v) => Ok(Some(v.trim().to_owned())),
            Err(EngineError::NotFound { .. }) => Err(EngineError::NotFound { id: id.clone() }),
            Err(e) => Err(e),
        }
    }
}
