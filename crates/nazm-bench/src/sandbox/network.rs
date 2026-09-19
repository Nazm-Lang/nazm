//! Subnet allocation: small, configurable, and collision-checked at allocation time.
//!
//! Deliberately not a subsystem. The harness runs one attempt at a time, so it needs one
//! free /24 at a time, and the whole job is: pick a slot from a configured block, confirm
//! nothing is already using it, and fail clearly if the slot is occupied.
//!
//! # Why not just let Docker choose
//!
//! Measured on the development machine (see `docs/runbook.md`): fourteen of Docker's
//! fifteen default `172.x/16` pools are already consumed by unrelated projects, and the
//! documented fallback pool overlaps this machine's own LAN. Whether Docker would actually
//! hand out a conflicting subnet is **not established** — its IPAM does some host-route
//! avoidance — and the point of allocating explicitly is that the answer stops mattering.
//!
//! # What this does not do
//!
//! It never inspects, modifies, or removes a network it did not create. An occupied slot
//! is reported, not reclaimed: on a machine running other people's projects, a cleanup
//! pass that reasons from "this looks like ours" eventually deletes something that is not.

use std::fmt;

use serde::{Deserialize, Serialize};

/// The block slots are carved from. Configurable because a default that is free today on
/// one machine is not a reservation anywhere.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubnetBlock {
    /// First two octets, e.g. `[10, 201]` for `10.201.x.0/24`.
    pub prefix: [u8; 2],
    /// Third-octet values that may be used, in order.
    pub slots: Vec<u8>,
}

impl Default for SubnetBlock {
    fn default() -> Self {
        // 10.201.x.0/24. Measured free on the development machine: no 10.x routes on the
        // host. Four utun interfaces exist though, and a VPN could add one later — which
        // is exactly why allocation re-checks rather than trusting this default.
        Self {
            prefix: [10, 201],
            slots: (1..=32).collect(),
        }
    }
}

impl SubnetBlock {
    #[must_use]
    pub fn subnet_for(&self, slot: u8) -> Subnet {
        Subnet {
            prefix: self.prefix,
            slot,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Subnet {
    prefix: [u8; 2],
    slot: u8,
}

impl Subnet {
    #[must_use]
    pub fn cidr(self) -> String {
        format!("{}.{}.{}.0/24", self.prefix[0], self.prefix[1], self.slot)
    }

    #[must_use]
    pub fn gateway(self) -> String {
        format!("{}.{}.{}.1", self.prefix[0], self.prefix[1], self.slot)
    }

    #[must_use]
    pub fn slot(self) -> u8 {
        self.slot
    }
}

impl fmt::Display for Subnet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.cidr())
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum AllocationError {
    #[error(
        "every slot in the configured block is occupied ({tried} tried). Either a previous \
         run leaked networks — check the resource ledger and `docker network ls` — or the \
         block is too small for this machine. The harness will not reclaim a network it \
         did not create."
    )]
    BlockExhausted { tried: usize },
    #[error(
        "subnet {subnet} is in use by a network this harness did not create. Configure a \
         different block rather than removing it."
    )]
    Occupied { subnet: String },
}

/// Pick the first slot in the block whose subnet nothing is currently using.
///
/// `in_use` is whatever the engine reports, foreign networks included — the check is
/// against reality, not against the ledger, because a collision with someone else's
/// network is exactly the case the ledger cannot know about.
pub fn allocate(block: &SubnetBlock, in_use: &[String]) -> Result<Subnet, AllocationError> {
    for &slot in &block.slots {
        let candidate = block.subnet_for(slot);
        if !in_use.iter().any(|s| overlaps(s, &candidate.cidr())) {
            return Ok(candidate);
        }
    }
    Err(AllocationError::BlockExhausted {
        tried: block.slots.len(),
    })
}

/// Whether two CIDRs share any address.
///
/// Deliberately simple: both sides are compared as prefixes over the first three octets,
/// which is exact for the /24s this allocates and conservative — it reports overlap for a
/// containing /16 — for the wider networks Docker hands out by default. Being
/// conservative here costs a slot; being wrong costs a broken network.
fn overlaps(existing: &str, candidate: &str) -> bool {
    let Some((e_addr, e_bits)) = split_cidr(existing) else {
        return false;
    };
    let Some((c_addr, _)) = split_cidr(candidate) else {
        return false;
    };

    // Compare over the coarser of the two prefixes.
    let octets = match e_bits {
        0..=8 => 1,
        9..=16 => 2,
        _ => 3,
    };
    e_addr.iter().take(octets).eq(c_addr.iter().take(octets))
}

fn split_cidr(s: &str) -> Option<([u8; 4], u8)> {
    let (addr, bits) = s.split_once('/')?;
    let mut octets = [0u8; 4];
    let mut parts = addr.split('.');
    for o in &mut octets {
        *o = parts.next()?.parse().ok()?;
    }
    Some((octets, bits.parse().ok()?))
}
