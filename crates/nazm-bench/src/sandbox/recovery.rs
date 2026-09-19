//! Reconciling leftovers from a harness that did not get to tear down.
//!
//! A separate code path from teardown, and it needs the same two disciplines — ownership
//! before removal, containers before the things they reference. Writing the test first
//! made that concrete: iterating the ledger in creation order tried to remove the network
//! while containers were still attached, and the engine refused exactly as a daemon does.
//!
//! What recovery may act on: resources this run's ledger claims **and** that carry this
//! run's ownership token. Both. The ledger alone records an intention to create a name,
//! which is not evidence that the thing now holding that name is ours; the token alone
//! would miss a resource whose create returned after the process died.

use crate::engine::{self, ContainerEngine, EngineError, Ownership, ResourceId, ResourceKind};
use crate::sandbox::ledger::{Ledger, LedgerError};

/// What reconciliation did, in enough detail to decide whether to proceed.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Reconciliation {
    /// Ours, and now confirmed gone.
    pub removed: Vec<ResourceId>,
    /// Claimed by the ledger but already absent — the create never landed, or someone
    /// cleaned up. Not a problem.
    pub already_absent: Vec<ResourceId>,
    /// Claimed by the ledger but **not ours**. Never touched. This is the case that
    /// justifies the ownership token existing.
    pub foreign: Vec<(ResourceId, Option<String>)>,
    /// Could not be resolved. The run cannot safely continue past these.
    pub unresolved: Vec<(ResourceId, String)>,
}

impl Reconciliation {
    /// Whether it is safe to start a new attempt.
    ///
    /// Foreign resources do not block: they were never ours and their existence says
    /// nothing about our isolation. Unresolved ones do — an unresolved leftover is
    /// indistinguishable from state that will reach the next attempt.
    #[must_use]
    pub fn is_clear(&self) -> bool {
        self.unresolved.is_empty()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RecoveryError {
    #[error("could not read the resource ledger: {0}")]
    Ledger(#[from] LedgerError),
}

/// Remove this run's leftovers, in dependency order, checking ownership on every one.
pub fn reconcile<E: ContainerEngine + ?Sized>(
    engine: &E,
    ledger: &Ledger,
    owner: &str,
) -> Result<Reconciliation, RecoveryError> {
    let outstanding = ledger.outstanding()?;
    let mut out = Reconciliation::default();

    // Containers first, then everything they reference. The ledger is in creation order,
    // and removing in that order is refused: a network with attached endpoints and a
    // volume with a referencing container both hold on.
    let ordered = outstanding
        .iter()
        .filter(|e| e.resource.kind == ResourceKind::Container)
        .chain(
            outstanding
                .iter()
                .filter(|e| e.resource.kind != ResourceKind::Container),
        );

    for entry in ordered {
        let id = &entry.resource;
        match engine::ownership(engine, id, owner) {
            Ownership::Absent => {
                let _ = ledger.removed(&entry.attempt, id);
                out.already_absent.push(id.clone());
            }
            Ownership::Ours => {
                // Force: a container left running by a killed harness is the normal case
                // here, not an exception.
                match engine.remove(id, true) {
                    Ok(()) | Err(EngineError::NotFound { .. }) => {
                        let _ = ledger.removed(&entry.attempt, id);
                        out.removed.push(id.clone());
                    }
                    Err(e) => out.unresolved.push((id.clone(), e.to_string())),
                }
            }
            Ownership::Foreign { found } => out.foreign.push((id.clone(), found)),
            Ownership::Unknown { reason } => out.unresolved.push((id.clone(), reason)),
        }
    }

    Ok(out)
}
