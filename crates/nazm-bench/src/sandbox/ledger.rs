//! Write-ahead resource ownership.
//!
//! The window this closes: the harness issues a create, the process dies before the call
//! returns, and a resource now exists that nothing knows the name of. Labels do not help —
//! the resource may carry them, but a crashed harness is not around to query for them, and
//! the next run has no way to distinguish "I made this" from "someone else's project made
//! this" without a record it wrote itself.
//!
//! So the name is written and flushed **before** the create is issued. Deterministic names
//! make that possible: the harness knows what it is about to call the thing.
//!
//! The ledger is append-only and one JSON object per line, so a partially written final
//! line from a hard kill costs one entry rather than the file.
//!
//! **Recovery acts only on resources this ledger claims.** This machine runs unrelated
//! Docker projects — fourteen networks' worth at last count — and a cleanup pass that
//! reasoned from anything looser than "I recorded creating this" would eventually delete
//! someone else's database.

use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::engine::ResourceId;

/// A resource's lifecycle, as recorded rather than as believed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    /// Written before the create call. If the harness dies here, the resource may or may
    /// not exist — recovery must check rather than assume either way.
    Intended,
    /// The create returned successfully.
    Created,
    /// Removal was verified, not merely requested.
    Removed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub run: String,
    pub attempt: String,
    pub resource: ResourceId,
    pub state: State,
}

#[derive(Debug, thiserror::Error)]
pub enum LedgerError {
    #[error("could not open the resource ledger at {path}: {source}")]
    Open {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("could not record {resource} as {state:?}: {source}")]
    Write {
        resource: ResourceId,
        state: State,
        source: std::io::Error,
    },
    #[error("could not read the resource ledger at {path}: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
}

#[derive(Debug)]
pub struct Ledger {
    path: PathBuf,
    run: String,
}

impl Ledger {
    pub fn open(path: impl Into<PathBuf>, run: impl Into<String>) -> Result<Self, LedgerError> {
        let path = path.into();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|source| LedgerError::Open {
                path: path.clone(),
                source,
            })?;
        }
        // Touch it now so a later append cannot be the first thing that fails.
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|source| LedgerError::Open {
                path: path.clone(),
                source,
            })?;
        Ok(Self {
            path,
            run: run.into(),
        })
    }

    /// Record an intention to create, and flush it to disk before returning.
    ///
    /// The caller must not issue the create until this returns `Ok`. That ordering is the
    /// entire point: a record written afterwards protects nothing.
    pub fn intend(&self, attempt: &str, resource: &ResourceId) -> Result<(), LedgerError> {
        self.append(attempt, resource, State::Intended)
    }

    pub fn created(&self, attempt: &str, resource: &ResourceId) -> Result<(), LedgerError> {
        self.append(attempt, resource, State::Created)
    }

    /// Record a **verified** removal. Never called on the strength of a remove call
    /// returning zero — only after the resource is confirmed gone.
    pub fn removed(&self, attempt: &str, resource: &ResourceId) -> Result<(), LedgerError> {
        self.append(attempt, resource, State::Removed)
    }

    fn append(
        &self,
        attempt: &str,
        resource: &ResourceId,
        state: State,
    ) -> Result<(), LedgerError> {
        let entry = Entry {
            run: self.run.clone(),
            attempt: attempt.to_owned(),
            resource: resource.clone(),
            state,
        };
        let line = serde_json::to_string(&entry).map_err(|e| LedgerError::Write {
            resource: resource.clone(),
            state,
            source: std::io::Error::other(e),
        })?;

        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .map_err(|source| LedgerError::Write {
                resource: resource.clone(),
                state,
                source,
            })?;
        writeln!(f, "{line}").map_err(|source| LedgerError::Write {
            resource: resource.clone(),
            state,
            source,
        })?;
        // fsync: an entry sitting in the page cache when the machine dies is an entry that
        // was never written, and this file's only job is surviving that.
        f.sync_all().map_err(|source| LedgerError::Write {
            resource: resource.clone(),
            state,
            source,
        })?;
        Ok(())
    }

    /// Resources this ledger claims and has not verified removed.
    ///
    /// Includes `Intended` entries whose create may never have happened — recovery checks
    /// existence rather than assuming, because both answers are possible and guessing
    /// wrong in one direction leaks and in the other deletes.
    pub fn outstanding(&self) -> Result<Vec<Entry>, LedgerError> {
        let file = File::open(&self.path).map_err(|source| LedgerError::Read {
            path: self.path.clone(),
            source,
        })?;

        let mut latest: Vec<Entry> = Vec::new();
        for line in BufReader::new(file).lines() {
            let Ok(line) = line else { break };
            if line.trim().is_empty() {
                continue;
            }
            // A truncated final line from a hard kill is skipped, not fatal.
            let Ok(entry) = serde_json::from_str::<Entry>(&line) else {
                continue;
            };
            if let Some(existing) = latest.iter_mut().find(|e| e.resource == entry.resource) {
                *existing = entry;
            } else {
                latest.push(entry);
            }
        }

        latest.retain(|e| e.state != State::Removed);
        Ok(latest)
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}
