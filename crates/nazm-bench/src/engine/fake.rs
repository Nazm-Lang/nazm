//! An in-memory container engine that models the rules which make ordering matter.
//!
//! # What this can and cannot establish
//!
//! It proves the **harness's logic**: teardown order, cleanup verification, recovery after
//! a partial startup failure, and what happens when an operation fails at each step. All
//! of that is ordering, and ordering is where this code will actually be wrong.
//!
//! It proves **nothing about Docker**. The kill guarantee, read-only mount enforcement,
//! the restricted database role, and network isolation are claims about Docker and
//! PostgreSQL, and only the Docker-gated integration tests touch them. A fake that always
//! succeeds would be worse than no fake; this one is worth having because it refuses the
//! same operations a real daemon refuses.
//!
//! The four modelled rules, each of which turns a real ordering bug into a unit-test
//! failure:
//!
//! * a network with attached endpoints cannot be removed
//! * a volume referenced by a container cannot be removed
//! * a running container cannot be removed without force
//! * a subnet that overlaps an existing network cannot be allocated

use std::collections::BTreeMap;
use std::sync::Mutex;

use super::{
    ContainerEngine, ContainerSpec, ContainerState, EngineError, Labels, MountSource, NetworkSpec,
    ResourceId, ResourceKind, VolumeSpec,
};

#[derive(Debug, Clone)]
struct FakeContainer {
    running: bool,
    exit_code: Option<i32>,
    oom_killed: bool,
    network: Option<String>,
    volumes: Vec<String>,
    labels: Labels,
}

#[derive(Debug, Default)]
struct World {
    networks: BTreeMap<String, (String, Labels)>,
    volumes: BTreeMap<String, Labels>,
    containers: BTreeMap<String, FakeContainer>,
    /// Queued failures, applied in order to matching operations.
    faults: Vec<(Op, EngineError)>,
    /// Every operation attempted, so a test can assert on teardown *order*.
    log: Vec<String>,
}

/// Which operation a queued fault applies to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    CreateNetwork,
    CreateVolume,
    CreateContainer,
    Start,
    Kill,
    Inspect,
    Remove,
    List,
}

#[derive(Debug, Default)]
pub struct FakeEngine {
    world: Mutex<World>,
}

impl FakeEngine {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Make the next matching operation fail. Used to drive every row of the failure
    /// taxonomy without a daemon.
    pub fn fail_next(&self, op: Op, err: EngineError) {
        if let Ok(mut w) = self.world.lock() {
            w.faults.push((op, err));
        }
    }

    /// The operation log, in order. This is what makes teardown sequence testable.
    #[must_use]
    pub fn log(&self) -> Vec<String> {
        self.world.lock().map(|w| w.log.clone()).unwrap_or_default()
    }

    /// Everything that currently exists — the leak check for tests.
    #[must_use]
    pub fn surviving(&self) -> Vec<ResourceId> {
        let Ok(w) = self.world.lock() else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for name in w.containers.keys() {
            out.push(ResourceId::new(ResourceKind::Container, name));
        }
        for name in w.networks.keys() {
            out.push(ResourceId::new(ResourceKind::Network, name));
        }
        for name in w.volumes.keys() {
            out.push(ResourceId::new(ResourceKind::Volume, name));
        }
        out
    }

    /// Plant a resource the harness did not create, to test that recovery leaves foreign
    /// resources alone.
    pub fn plant_foreign_network(&self, name: &str, subnet: &str) {
        if let Ok(mut w) = self.world.lock() {
            let labels = Labels {
                run: "someone-else".to_owned(),
                attempt: "someone-else".to_owned(),
                role: "unrelated".to_owned(),
            };
            w.networks
                .insert(name.to_owned(), (subnet.to_owned(), labels));
        }
    }

    fn take_fault(w: &mut World, op: Op) -> Option<EngineError> {
        let idx = w.faults.iter().position(|(o, _)| *o == op)?;
        Some(w.faults.remove(idx).1)
    }
}

impl ContainerEngine for FakeEngine {
    fn create_network(&self, spec: &NetworkSpec) -> Result<ResourceId, EngineError> {
        let mut w = self
            .world
            .lock()
            .map_err(|_| EngineError::DaemonUnreachable("poisoned".into()))?;
        w.log.push(format!("create_network {}", spec.name));
        if let Some(e) = Self::take_fault(&mut w, Op::CreateNetwork) {
            return Err(e);
        }
        let id = ResourceId::new(ResourceKind::Network, &spec.name);
        if w.networks.contains_key(&spec.name) {
            return Err(EngineError::NameConflict { id });
        }
        if w.networks.values().any(|(s, _)| *s == spec.subnet) {
            return Err(EngineError::SubnetOverlap {
                subnet: spec.subnet.clone(),
            });
        }
        w.networks.insert(
            spec.name.clone(),
            (spec.subnet.clone(), spec.labels.clone()),
        );
        Ok(id)
    }

    fn create_volume(&self, spec: &VolumeSpec) -> Result<ResourceId, EngineError> {
        let mut w = self
            .world
            .lock()
            .map_err(|_| EngineError::DaemonUnreachable("poisoned".into()))?;
        w.log.push(format!("create_volume {}", spec.name));
        if let Some(e) = Self::take_fault(&mut w, Op::CreateVolume) {
            return Err(e);
        }
        let id = ResourceId::new(ResourceKind::Volume, &spec.name);
        if w.volumes.contains_key(&spec.name) {
            return Err(EngineError::NameConflict { id });
        }
        w.volumes.insert(spec.name.clone(), spec.labels.clone());
        Ok(id)
    }

    fn create_container(&self, spec: &ContainerSpec) -> Result<ResourceId, EngineError> {
        let mut w = self
            .world
            .lock()
            .map_err(|_| EngineError::DaemonUnreachable("poisoned".into()))?;
        w.log.push(format!("create_container {}", spec.name));
        if let Some(e) = Self::take_fault(&mut w, Op::CreateContainer) {
            return Err(e);
        }
        let id = ResourceId::new(ResourceKind::Container, &spec.name);
        if w.containers.contains_key(&spec.name) {
            return Err(EngineError::NameConflict { id });
        }
        let volumes = spec
            .mounts
            .iter()
            .filter_map(|m| match &m.source {
                MountSource::Volume(v) => Some(v.clone()),
                MountSource::HostPath(_) | MountSource::Tmpfs { .. } => None,
            })
            .collect();
        w.containers.insert(
            spec.name.clone(),
            FakeContainer {
                running: false,
                exit_code: None,
                oom_killed: false,
                network: spec.network.clone(),
                volumes,
                labels: spec.labels.clone(),
            },
        );
        Ok(id)
    }

    fn start(&self, id: &ResourceId) -> Result<(), EngineError> {
        let mut w = self
            .world
            .lock()
            .map_err(|_| EngineError::DaemonUnreachable("poisoned".into()))?;
        w.log.push(format!("start {}", id.name));
        if let Some(e) = Self::take_fault(&mut w, Op::Start) {
            return Err(e);
        }
        let c = w
            .containers
            .get_mut(&id.name)
            .ok_or_else(|| EngineError::NotFound { id: id.clone() })?;
        c.running = true;
        Ok(())
    }

    fn kill(&self, id: &ResourceId) -> Result<(), EngineError> {
        let mut w = self
            .world
            .lock()
            .map_err(|_| EngineError::DaemonUnreachable("poisoned".into()))?;
        w.log.push(format!("kill {}", id.name));
        if let Some(e) = Self::take_fault(&mut w, Op::Kill) {
            return Err(e);
        }
        let c = w
            .containers
            .get_mut(&id.name)
            .ok_or_else(|| EngineError::NotFound { id: id.clone() })?;
        c.running = false;
        c.exit_code = Some(137);
        Ok(())
    }

    fn inspect(&self, id: &ResourceId) -> Result<ContainerState, EngineError> {
        let mut w = self
            .world
            .lock()
            .map_err(|_| EngineError::DaemonUnreachable("poisoned".into()))?;
        w.log.push(format!("inspect {}", id.name));
        if let Some(e) = Self::take_fault(&mut w, Op::Inspect) {
            return Err(e);
        }
        let c = w
            .containers
            .get(&id.name)
            .ok_or_else(|| EngineError::NotFound { id: id.clone() })?;
        Ok(ContainerState {
            running: c.running,
            exit_code: c.exit_code,
            oom_killed: c.oom_killed,
            started_at: Some("2026-09-19T00:00:00Z".to_owned()),
            finished_at: (!c.running).then(|| "2026-09-19T00:01:00Z".to_owned()),
        })
    }

    fn remove(&self, id: &ResourceId, force: bool) -> Result<(), EngineError> {
        let mut w = self
            .world
            .lock()
            .map_err(|_| EngineError::DaemonUnreachable("poisoned".into()))?;
        w.log.push(format!("remove {} {}", id.kind, id.name));
        if let Some(e) = Self::take_fault(&mut w, Op::Remove) {
            return Err(e);
        }
        match id.kind {
            ResourceKind::Container => {
                let c = w
                    .containers
                    .get(&id.name)
                    .ok_or_else(|| EngineError::NotFound { id: id.clone() })?;
                if c.running && !force {
                    return Err(EngineError::StillReferenced { id: id.clone() });
                }
                w.containers.remove(&id.name);
            }
            ResourceKind::Network => {
                if !w.networks.contains_key(&id.name) {
                    return Err(EngineError::NotFound { id: id.clone() });
                }
                // The rule that makes teardown order matter.
                if w.containers
                    .values()
                    .any(|c| c.network.as_deref() == Some(id.name.as_str()))
                {
                    return Err(EngineError::StillReferenced { id: id.clone() });
                }
                w.networks.remove(&id.name);
            }
            ResourceKind::Volume => {
                if !w.volumes.contains_key(&id.name) {
                    return Err(EngineError::NotFound { id: id.clone() });
                }
                if w.containers.values().any(|c| c.volumes.contains(&id.name)) {
                    return Err(EngineError::StillReferenced { id: id.clone() });
                }
                w.volumes.remove(&id.name);
            }
        }
        Ok(())
    }

    fn list_by_label(&self, key: &str, value: &str) -> Result<Vec<ResourceId>, EngineError> {
        let mut w = self
            .world
            .lock()
            .map_err(|_| EngineError::DaemonUnreachable("poisoned".into()))?;
        w.log.push(format!("list_by_label {key}={value}"));
        if let Some(e) = Self::take_fault(&mut w, Op::List) {
            return Err(e);
        }
        let matches = |l: &Labels| match key {
            "nazm.run" => l.run == value,
            "nazm.attempt" => l.attempt == value,
            "nazm.role" => l.role == value,
            _ => false,
        };
        let mut out = Vec::new();
        for (name, c) in &w.containers {
            if matches(&c.labels) {
                out.push(ResourceId::new(ResourceKind::Container, name));
            }
        }
        for (name, (_, l)) in &w.networks {
            if matches(l) {
                out.push(ResourceId::new(ResourceKind::Network, name));
            }
        }
        for (name, l) in &w.volumes {
            if matches(l) {
                out.push(ResourceId::new(ResourceKind::Volume, name));
            }
        }
        Ok(out)
    }

    fn subnets_in_use(&self) -> Result<Vec<String>, EngineError> {
        let w = self
            .world
            .lock()
            .map_err(|_| EngineError::DaemonUnreachable("poisoned".into()))?;
        Ok(w.networks.values().map(|(s, _)| s.clone()).collect())
    }
}
