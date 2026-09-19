//! The attempt lifecycle: create in order, tear down in reverse, verify nothing survived.
//!
//! Teardown order is not stylistic. A network with attached endpoints cannot be removed,
//! and neither can a volume a container still references — so containers must go first,
//! and the fake engine models both rules precisely to turn an ordering mistake into a unit
//! test failure rather than a leak.
//!
//! The candidate is killed before the database. It is the untrusted party, and it must not
//! get to observe its database dying and react to it.
//!
//! Every resource is recorded as *intended* before it is created. See `ledger.rs` for why
//! after-the-fact recording protects nothing.

use crate::engine::{
    self, ContainerEngine, ContainerSpec, EngineError, Labels, Mount, MountSource, NetworkSpec,
    Ownership, ResourceId, ResourceKind, VolumeSpec,
};
use crate::sandbox::ledger::{Ledger, LedgerError};
use crate::sandbox::network::{self, AllocationError, Subnet, SubnetBlock};
use crate::sandbox::postgres::Credentials;

/// How the harness should proceed after an attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Disposition {
    /// Teardown verified clean. The next attempt may run.
    Clean,
    /// Resources survived, or verification could not be performed. **The next attempt is
    /// refused, not run-and-voided.**
    ///
    /// A leaked resource is by definition state that can reach the next attempt. Running
    /// anyway converts a detectable instrument fault into a silent independence
    /// violation, and independence violations cannot be detected after the fact — so
    /// there is no recovering the result later. Stopping beats a void nobody can audit.
    Quarantine {
        surviving: Vec<ResourceId>,
        reason: String,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum SandboxError {
    #[error("subnet allocation failed: {0}")]
    Allocation(#[from] AllocationError),
    #[error("resource ledger failed: {0}")]
    Ledger(#[from] LedgerError),
    #[error("container engine failed: {0}")]
    Engine(#[from] EngineError),
}

/// Names are deterministic, so a collision on create is itself a leak detector.
#[derive(Debug, Clone)]
pub struct Names {
    pub network: String,
    pub db: String,
    pub db_volume: String,
    pub candidate: String,
    pub work_volume: String,
}

impl Names {
    #[must_use]
    pub fn for_attempt(run: &str, attempt: &str) -> Self {
        let p = format!("nazm-{run}-{attempt}");
        Self {
            network: format!("{p}-net"),
            db: format!("{p}-db"),
            db_volume: format!("{p}-pgdata"),
            candidate: format!("{p}-cand"),
            work_volume: format!("{p}-work"),
        }
    }
}

/// Configuration fixed by the pre-registration, not chosen per attempt.
#[derive(Debug, Clone)]
pub struct Limits {
    pub candidate_memory_mb: u32,
    pub candidate_milli_cpus: u32,
    pub db_memory_mb: u32,
    pub db_milli_cpus: u32,
    pub pids_limit: u32,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            candidate_memory_mb: 2048,
            candidate_milli_cpus: 2000,
            db_memory_mb: 1024,
            db_milli_cpus: 1000,
            pids_limit: 512,
        }
    }
}

#[derive(Debug, Clone)]
pub struct AttemptConfig {
    pub run: String,
    pub attempt: String,
    /// Random per-run ownership token. The only thing that authorises a removal.
    pub owner: String,
    pub candidate_image: String,
    pub postgres_image: String,
    pub block: SubnetBlock,
    pub limits: Limits,
    /// Harness-side scratch, mounted read-only into the database container for its init
    /// script. Never writable from any container.
    pub scratch: std::path::PathBuf,
}

/// One attempt's live resources, created in dependency order.
#[derive(Debug)]
pub struct Sandbox<'a, E: ContainerEngine> {
    engine: &'a E,
    ledger: &'a Ledger,
    cfg: AttemptConfig,
    names: Names,
    subnet: Subnet,
    credentials: Credentials,
    /// Reverse creation order. Teardown walks this, so it cannot drift from what was made.
    created: Vec<ResourceId>,
}

impl<'a, E: ContainerEngine> Sandbox<'a, E> {
    /// Allocate a subnet and bring up the network, database, and candidate.
    ///
    /// On any failure the caller still gets a `Sandbox` back inside the error path via
    /// [`Self::start`]'s partial construction — see `start`, which tears down what it
    /// built before returning. A half-built attempt that leaks is the same problem as one
    /// that leaks after running.
    pub fn start(
        engine: &'a E,
        ledger: &'a Ledger,
        cfg: AttemptConfig,
    ) -> Result<Self, (SandboxError, Disposition)> {
        let names = Names::for_attempt(&cfg.run, &cfg.attempt);

        let in_use = match engine.subnets_in_use() {
            Ok(v) => v,
            Err(e) => return Err((SandboxError::Engine(e), Disposition::Clean)),
        };
        let subnet = match network::allocate(&cfg.block, &in_use) {
            Ok(s) => s,
            // Nothing was created, so nothing can have leaked.
            Err(e) => return Err((SandboxError::Allocation(e), Disposition::Clean)),
        };

        let credentials = match Credentials::generate() {
            Ok(c) => c,
            Err(e) => {
                return Err((
                    SandboxError::Engine(EngineError::DaemonUnreachable(format!(
                        "could not generate database credentials: {e}"
                    ))),
                    Disposition::Clean,
                ))
            }
        };

        let mut sb = Self {
            engine,
            ledger,
            cfg,
            names,
            subnet,
            credentials,
            created: Vec::new(),
        };

        match sb.bring_up() {
            Ok(()) => Ok(sb),
            Err(e) => {
                // Partial startup still owns whatever it managed to create.
                let disposition = sb.teardown();
                Err((e, disposition))
            }
        }
    }

    fn labels(&self, role: &str) -> Labels {
        Labels {
            run: self.cfg.run.clone(),
            attempt: self.cfg.attempt.clone(),
            role: role.to_owned(),
            owner: self.cfg.owner.clone(),
        }
    }

    /// Record the intent, create, record the creation. In that order, always.
    fn track(
        &mut self,
        id: ResourceId,
        create: impl FnOnce(&E) -> Result<ResourceId, EngineError>,
    ) -> Result<(), SandboxError> {
        self.ledger.intend(&self.cfg.attempt, &id)?;
        match create(self.engine) {
            // Keep what the engine returned, not what we asked for. Later operations
            // address the resource by its engine-assigned id where there is one, so a
            // name that changes hands cannot redirect a removal.
            Ok(created) => {
                self.ledger.created(&self.cfg.attempt, &created)?;
                self.created.push(created);
                Ok(())
            }
            // The name is taken by something we did not create. Record that we do *not*
            // own it, so no later reconciliation reads the intent as a claim. Ownership is
            // re-checked before any removal regardless; this just stops the entry from
            // lingering as an unresolved question.
            Err(e @ EngineError::NameConflict { .. }) => {
                self.ledger.disowned(&self.cfg.attempt, &id)?;
                Err(SandboxError::Engine(e))
            }
            // Any other failure leaves the result genuinely unknown -- the create may have
            // taken effect before the error. The entry stays `Intended` and reconciliation
            // resolves it by asking the engine who owns the name.
            Err(e) => Err(SandboxError::Engine(e)),
        }
    }

    /// Remove a resource only after confirming we own it.
    ///
    /// Returns whether the resource is gone. A foreign or unknown owner is never removed
    /// and never counted as cleaned: the cost of leaving it is a quarantined run, and the
    /// cost of guessing wrong is destroying someone else's work.
    fn remove_if_ours(&self, id: &ResourceId, force: bool) -> Result<bool, String> {
        match engine::ownership(self.engine, id, &self.cfg.owner) {
            Ownership::Absent => Ok(true),
            Ownership::Ours => match self.engine.remove(id, force) {
                Ok(()) | Err(EngineError::NotFound { .. }) => Ok(true),
                Err(e) => Err(format!("{id}: {e}")),
            },
            Ownership::Foreign { found } => Err(format!(
                "{id} carries owner {found:?}, not ours -- refusing to remove it"
            )),
            Ownership::Unknown { reason } => Err(format!(
                "{id}: ownership could not be established ({reason})"
            )),
        }
    }

    fn init_dir(&self) -> std::path::PathBuf {
        self.cfg.scratch.join(&self.cfg.attempt).join("pginit")
    }

    fn bring_up(&mut self) -> Result<(), SandboxError> {
        // Written before the database container is created: the image runs it during
        // bootstrap, on a Unix socket, before TCP is ever reachable.
        let attempt_scratch = self.cfg.scratch.join(&self.cfg.attempt);
        crate::sandbox::postgres::write_init_script(&attempt_scratch, &self.credentials).map_err(
            |e| {
                SandboxError::Engine(EngineError::DaemonUnreachable(format!(
                    "could not write the database init script: {e}"
                )))
            },
        )?;

        let net = ResourceId::new(ResourceKind::Network, &self.names.network);
        let spec = NetworkSpec {
            name: self.names.network.clone(),
            subnet: self.subnet.cidr(),
            labels: self.labels("net"),
            internal: true,
        };
        self.track(net, |e| e.create_network(&spec))?;

        for (name, role) in [
            (self.names.db_volume.clone(), "pgdata"),
            (self.names.work_volume.clone(), "work"),
        ] {
            let id = ResourceId::new(ResourceKind::Volume, &name);
            let spec = VolumeSpec {
                name,
                labels: self.labels(role),
            };
            self.track(id, |e| e.create_volume(&spec))?;
        }

        let db = ResourceId::new(ResourceKind::Container, &self.names.db);
        let db_spec = self.db_spec();
        self.track(db.clone(), |e| e.create_container(&db_spec))?;
        self.engine.start(&db)?;

        let cand = ResourceId::new(ResourceKind::Container, &self.names.candidate);
        let cand_spec = self.candidate_spec();
        self.track(cand.clone(), |e| e.create_container(&cand_spec))?;
        self.engine.start(&cand)?;

        Ok(())
    }

    /// Credentials for this attempt. The candidate receives only the restricted role.
    #[must_use]
    pub fn credentials(&self) -> &Credentials {
        &self.credentials
    }

    fn db_spec(&self) -> ContainerSpec {
        ContainerSpec {
            name: self.names.db.clone(),
            image: self.cfg.postgres_image.clone(),
            labels: self.labels("db"),
            network: Some(self.names.network.clone()),
            network_alias: Some("db".to_owned()),
            mounts: vec![
                Mount {
                    source: MountSource::Volume(self.names.db_volume.clone()),
                    target: "/var/lib/postgresql/data".to_owned(),
                    read_only: false,
                },
                // Read-only, and harness-authored. The only host bind in the design, and
                // nothing about its bytes is graded.
                Mount {
                    source: MountSource::HostPath(self.init_dir().to_string_lossy().into_owned()),
                    target: "/docker-entrypoint-initdb.d".to_owned(),
                    read_only: true,
                },
            ],
            // Never POSTGRES_USER: the official image creates that role WITH SUPERUSER,
            // which would silently undo the restricted-role decision. The application
            // role is created by the init script above instead.
            env: vec![
                (
                    "POSTGRES_PASSWORD".to_owned(),
                    self.credentials.superuser_password.clone(),
                ),
                // Read by the init script through a psql variable, so it never appears as
                // literal text in any SQL.
                (
                    "APP_PASSWORD".to_owned(),
                    self.credentials.app_password.clone(),
                ),
                (
                    "POSTGRES_INITDB_ARGS".to_owned(),
                    "--auth-host=scram-sha-256".to_owned(),
                ),
            ],
            command: Vec::new(),
            memory_mb: self.cfg.limits.db_memory_mb,
            milli_cpus: self.cfg.limits.db_milli_cpus,
            pids_limit: self.cfg.limits.pids_limit,
            init: true,
            // The minimum the official entrypoint needs to chown its data directory and
            // drop from root to the `postgres` user. Verified by removing them: the
            // container dies with "failed switching to 'postgres'". Notably absent:
            // SYS_ADMIN, SYS_PTRACE, NET_ADMIN, and everything else.
            cap_add: ["CHOWN", "SETUID", "SETGID", "FOWNER", "DAC_OVERRIDE"]
                .iter()
                .map(|s| (*s).to_owned())
                .collect(),
        }
    }

    fn candidate_spec(&self) -> ContainerSpec {
        ContainerSpec {
            name: self.names.candidate.clone(),
            image: self.cfg.candidate_image.clone(),
            labels: self.labels("cand"),
            network: Some(self.names.network.clone()),
            network_alias: None,
            mounts: vec![Mount {
                source: MountSource::Volume(self.names.work_volume.clone()),
                target: "/work".to_owned(),
                read_only: false,
            }],
            env: vec![("DATABASE_URL".to_owned(), self.credentials.candidate_url())],
            command: vec!["sleep".to_owned(), "infinity".to_owned()],
            memory_mb: self.cfg.limits.candidate_memory_mb,
            milli_cpus: self.cfg.limits.candidate_milli_cpus,
            pids_limit: self.cfg.limits.pids_limit,
            init: true,
            // None. The candidate is the untrusted party and needs no capability at all.
            cap_add: Vec::new(),
        }
    }

    #[must_use]
    pub fn names(&self) -> &Names {
        &self.names
    }

    #[must_use]
    pub fn subnet(&self) -> Subnet {
        self.subnet
    }

    /// Kill the candidate. Used on hard deadline.
    ///
    /// SIGKILL, not SIGTERM: once PID 1 is gone the kernel signals every remaining task in
    /// the PID namespace, which is what reaches detached and re-parented descendants. A
    /// PID 1 that ignores SIGTERM leaves them running.
    pub fn kill_candidate(&self) -> Result<(), EngineError> {
        self.engine.kill(&ResourceId::new(
            ResourceKind::Container,
            &self.names.candidate,
        ))
    }

    /// Tear down in reverse creation order and verify nothing survived.
    ///
    /// Consumes nothing and returns a `Disposition` rather than a `Result`, because every
    /// outcome here needs a decision about the *next* attempt, and an error type would let
    /// a caller `?` past it.
    pub fn teardown(&mut self) -> Disposition {
        // Containers first: a network with attached endpoints and a volume with a
        // referencing container both refuse removal.
        let (containers, rest): (Vec<_>, Vec<_>) = self
            .created
            .iter()
            .rev()
            .cloned()
            .partition(|r| r.kind == ResourceKind::Container);

        let mut problems = Vec::new();

        for id in &containers {
            // Candidate before database: the untrusted party must not observe its
            // database dying and react.
            if engine::ownership(self.engine, id, &self.cfg.owner).may_remove() {
                let _ = self.engine.kill(id);
            }
            if let Err(problem) = self.remove_if_ours(id, true) {
                problems.push(problem);
            }
        }
        for id in &rest {
            if let Err(problem) = self.remove_if_ours(id, false) {
                problems.push(problem);
            }
        }

        self.verify_clean(problems)
    }

    /// Prove nothing survived, rather than trusting that removal returned zero.
    fn verify_clean(&self, mut problems: Vec<String>) -> Disposition {
        let surviving = match self.engine.list_by_label("nazm.attempt", &self.cfg.attempt) {
            Ok(v) => v,
            Err(e) => {
                // Cannot verify. Explicitly not "probably fine": an unverifiable teardown
                // is indistinguishable from a leaked one, and only one of those is safe.
                return Disposition::Quarantine {
                    surviving: Vec::new(),
                    reason: format!("cleanup could not be verified: {e}"),
                };
            }
        };

        if surviving.is_empty() && problems.is_empty() {
            for id in &self.created {
                let _ = self.ledger.removed(&self.cfg.attempt, id);
            }
            return Disposition::Clean;
        }

        if !surviving.is_empty() {
            problems.push(format!("{} resource(s) survived teardown", surviving.len()));
        }
        Disposition::Quarantine {
            surviving,
            reason: problems.join("; "),
        }
    }
}
