//! The container engine seam.
//!
//! One trait, two implementations: `DockerCli` shells out, `FakeEngine` models the rules
//! in memory. The split exists because almost everything worth testing about the attempt
//! lifecycle is *ordering* — teardown sequence, cleanup verification, what happens after a
//! partial startup failure — and none of that needs a daemon to get wrong.
//!
//! What the fake can and cannot establish is stated in `fake.rs`, and the boundary is
//! sharp: the fake proves the harness's logic, and proves nothing whatsoever about Docker.
//! The kill guarantee, read-only mounts, and role restrictions are claims about Docker and
//! PostgreSQL, and only the Docker-gated integration tests touch them.

pub mod docker;
pub mod fake;

use std::fmt;

use serde::{Deserialize, Serialize};

/// A resource the harness created and is therefore responsible for removing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceKind {
    Network,
    Volume,
    Container,
}

impl fmt::Display for ResourceKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Network => "network",
            Self::Volume => "volume",
            Self::Container => "container",
        })
    }
}

/// A named resource. Names are deterministic, so a collision on create is itself a leak
/// detector rather than something to work around with a random suffix.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ResourceId {
    pub kind: ResourceKind,
    pub name: String,
    /// The engine-assigned identifier, once a create has returned one.
    ///
    /// Names are a namespace someone else can take. Removing by name inspects one
    /// resource and then deletes whatever currently holds that name — a window a
    /// container exiting and being recreated is enough to open. Where the engine supports
    /// it, operations after creation address the resource by this instead.
    ///
    /// `None` before creation, and after an interrupted create where the client never
    /// received it — which is exactly the case that must resolve through ownership
    /// discovery rather than by assuming nothing was created.
    pub engine_id: Option<String>,
}

impl ResourceId {
    #[must_use]
    pub fn new(kind: ResourceKind, name: impl Into<String>) -> Self {
        Self {
            kind,
            name: name.into(),
            engine_id: None,
        }
    }

    #[must_use]
    pub fn with_engine_id(mut self, id: impl Into<String>) -> Self {
        self.engine_id = Some(id.into());
        self
    }

    /// What to address the resource by: the engine id when we have one, else the name.
    #[must_use]
    pub fn handle(&self) -> &str {
        self.engine_id.as_deref().unwrap_or(&self.name)
    }
}

impl fmt::Display for ResourceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.kind, self.name)
    }
}

/// Labels applied to every resource, so reconciliation is one query rather than a guess.
///
/// Recovery acts **only** on resources carrying these labels. A machine running unrelated
/// Docker projects — which this one is — must never have them touched by a benchmark
/// cleanup pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Labels {
    pub run: String,
    pub attempt: String,
    pub role: String,
    /// A random per-run token, and the **only** thing that establishes ownership.
    ///
    /// The ledger records an intention to create a name; it cannot establish that the
    /// thing now bearing that name is the thing the harness created. A create can fail
    /// with `NameConflict` precisely because something else already holds the name, and a
    /// cleanup pass that removed whatever it found there would delete a stranger's
    /// resource on the strength of its own intention.
    pub owner: String,
}

impl Labels {
    #[must_use]
    pub fn as_pairs(&self) -> [(String, String); 4] {
        [
            ("nazm.run".to_owned(), self.run.clone()),
            ("nazm.attempt".to_owned(), self.attempt.clone()),
            ("nazm.role".to_owned(), self.role.clone()),
            ("nazm.owner".to_owned(), self.owner.clone()),
        ]
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkSpec {
    pub name: String,
    pub subnet: String,
    pub labels: Labels,
    /// No route out of the bridge. The candidate has no reason to reach the internet, and
    /// egress would make cost incomparable across time and permit exfiltrating the task.
    pub internal: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VolumeSpec {
    pub name: String,
    pub labels: Labels,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mount {
    pub source: MountSource,
    pub target: String,
    pub read_only: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MountSource {
    Volume(String),
    /// A host path. Used only for read-only, harness-authored inputs whose permissions are
    /// not load-bearing — never for anything whose bytes are graded, because host binds
    /// traverse VirtioFS on this platform and it is inconsistent about mode bits.
    HostPath(String),
    Tmpfs {
        size_mb: u32,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContainerSpec {
    pub name: String,
    pub image: String,
    pub labels: Labels,
    pub network: Option<String>,
    pub network_alias: Option<String>,
    pub mounts: Vec<Mount>,
    pub env: Vec<(String, String)>,
    pub command: Vec<String>,
    pub memory_mb: u32,
    /// Thousandths of a CPU. An integer because this feeds the infrastructure cost line,
    /// where `cost.rs` keeps everything exact, and because a float here would force the
    /// spec to drop `Eq` and stop being comparable in tests.
    pub milli_cpus: u32,
    pub pids_limit: u32,
    /// Reap orphaned children. Without it a long-lived PID 1 accumulates zombies until the
    /// pids limit is hit, and the failure looks like the candidate's fault.
    pub init: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContainerState {
    pub running: bool,
    pub exit_code: Option<i32>,
    pub oom_killed: bool,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
}

/// Typed failures, so the outcome mapping is a `match` over a type rather than a scrape
/// over stderr text that changes between Docker releases.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EngineError {
    #[error("the container daemon is unreachable: {0}")]
    DaemonUnreachable(String),
    #[error("could not pull image {image}: {reason}")]
    ImagePullFailed { image: String, reason: String },
    #[error("{id} already exists")]
    NameConflict { id: ResourceId },
    #[error("subnet {subnet} overlaps an existing network")]
    SubnetOverlap { subnet: String },
    #[error("the address pool is exhausted")]
    AddressPoolExhausted,
    #[error("{id} does not exist")]
    NotFound { id: ResourceId },
    #[error("{id} is still referenced and cannot be removed yet")]
    StillReferenced { id: ResourceId },
    #[error("{id} is mid-removal")]
    RemovalInProgress { id: ResourceId },
    #[error("no space left on the container filesystem")]
    NoSpaceLeft,
    #[error("docker exited {code}: {stderr}")]
    CliFailure { code: i32, stderr: String },
}

impl EngineError {
    /// Whether this failure means the whole run should stop rather than voiding one
    /// attempt. Grinding out voids against a dead daemon produces nothing but cost.
    #[must_use]
    pub fn is_fatal_to_run(&self) -> bool {
        matches!(self, Self::DaemonUnreachable(_) | Self::NoSpaceLeft)
    }
}

/// What the harness can ask a container engine to do.
///
/// Deliberately small. Every method here is one the lifecycle actually needs; a wider
/// surface would be a wider fake, and the fake's value comes from modelling a few rules
/// exactly rather than many rules approximately.
pub trait ContainerEngine: fmt::Debug {
    fn create_network(&self, spec: &NetworkSpec) -> Result<ResourceId, EngineError>;
    fn create_volume(&self, spec: &VolumeSpec) -> Result<ResourceId, EngineError>;
    fn create_container(&self, spec: &ContainerSpec) -> Result<ResourceId, EngineError>;
    fn start(&self, id: &ResourceId) -> Result<(), EngineError>;
    /// Send SIGKILL. Not SIGTERM: a PID 1 that ignores SIGTERM leaves its children
    /// unsignalled, and the namespace teardown that kills detached descendants only
    /// happens once PID 1 is actually gone.
    fn kill(&self, id: &ResourceId) -> Result<(), EngineError>;
    fn inspect(&self, id: &ResourceId) -> Result<ContainerState, EngineError>;
    fn remove(&self, id: &ResourceId, force: bool) -> Result<(), EngineError>;
    /// Every resource carrying this label pair, whatever created it.
    fn list_by_label(&self, key: &str, value: &str) -> Result<Vec<ResourceId>, EngineError>;
    /// Subnets currently in use, for the allocation-time collision check.
    fn subnets_in_use(&self) -> Result<Vec<String>, EngineError>;

    /// The `nazm.owner` label on a resource, if it has one.
    ///
    /// `Ok(None)` means the resource exists and is not ours. That is a different answer
    /// from `Err(NotFound)`, and conflating them is how a cleanup pass ends up deleting a
    /// stranger's container.
    fn owner_of(&self, id: &ResourceId) -> Result<Option<String>, EngineError>;
}

/// Whether a resource may be removed by the harness.
///
/// Every removal goes through this. A negative or unknown answer is never upgraded to a
/// removal: the cost of leaving a leaked resource is a quarantined run, and the cost of
/// guessing wrong the other way is destroying someone else's work.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ownership {
    /// Carries our token. Safe to remove.
    Ours,
    /// Exists, but carries a different token or none. **Never remove.**
    Foreign { found: Option<String> },
    /// Does not exist. Nothing to remove, which is the desired end state anyway.
    Absent,
    /// The engine could not say. Treated as foreign for removal purposes.
    Unknown { reason: String },
}

impl Ownership {
    #[must_use]
    pub fn may_remove(&self) -> bool {
        matches!(self, Self::Ours)
    }

    /// Whether the resource is already gone, so a failure to remove it is not a leak.
    #[must_use]
    pub fn is_absent(&self) -> bool {
        matches!(self, Self::Absent)
    }
}

/// Establish ownership before acting on a resource.
pub fn ownership<E: ContainerEngine + ?Sized>(
    engine: &E,
    id: &ResourceId,
    our_token: &str,
) -> Ownership {
    match engine.owner_of(id) {
        Ok(Some(token)) if token == our_token => Ownership::Ours,
        Ok(found) => Ownership::Foreign { found },
        Err(EngineError::NotFound { .. }) => Ownership::Absent,
        Err(e) => Ownership::Unknown {
            reason: e.to_string(),
        },
    }
}
