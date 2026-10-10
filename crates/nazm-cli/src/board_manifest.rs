//! `nazm build --board FILE` (Gate 3, `docs/spec.md`, *Boards*): a board's manifest read into the
//! description the linker script, the runtime and the entry are generated from.
//!
//! The keys are closed, as an attribute's vocabulary is: one this does not know, a value of the
//! wrong kind or out of range, or a board whose instruction set is not the target's refuses the
//! build naming the key (`N0626`) — never a board half-understood. The built-in boards print as
//! manifests ([`nazm_lir::runtime::board::Board::manifest`]) that read back as themselves.

use std::borrow::Cow;

use nazm_lir::runtime::board::{Board, Isa, Stop, Uart, STACK};

/// Why a manifest is refused: the key, and what is wrong with it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Refused {
    pub(crate) key: String,
    pub(crate) why: String,
}

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "error[N0626]: `{}`: {}", self.key, self.why)
    }
}

fn refuse(key: &str, why: impl Into<String>) -> Refused {
    Refused {
        key: key.to_owned(),
        why: why.into(),
    }
}

/// The board `text` describes, for `triple` — whose architecture it must be.
pub(crate) fn read(text: &str, triple: &str, isa: Isa) -> Result<Board, Refused> {
    let doc: toml::Table = text
        .parse()
        .map_err(|e: toml::de::Error| refuse("(the file)", format!("not TOML: {}", e.message())))?;
    allow(
        &doc,
        "",
        &[
            "name",
            "isa",
            "load_address",
            "stack",
            "uart",
            "stop",
            "heap",
        ],
    )?;
    let name = string(&doc, "name")?;
    let written_isa = match string(&doc, "isa")?.as_str() {
        "aarch64" => Isa::Aarch64,
        "riscv64" => Isa::Riscv64,
        other => {
            return Err(refuse(
                "isa",
                format!("`{other}` is not `aarch64` or `riscv64`"),
            ))
        }
    };
    if written_isa != isa {
        return Err(refuse(
            "isa",
            format!("the board is not `{triple}`'s architecture"),
        ));
    }
    let load_address = address(&doc, "load_address")?;
    let stack = match doc.get("stack") {
        None => STACK,
        Some(_) => {
            let s = address(&doc, "stack")?;
            if s < 0x1000 || s % 16 != 0 {
                return Err(refuse(
                    "stack",
                    "a stack is at least 4 KiB and a multiple of 16 bytes",
                ));
            }
            s
        }
    };
    let uart_t = table(&doc, "uart")?;
    allow(uart_t, "uart.", &["kind", "address"])?;
    let uart_kind = match string(uart_t, "kind")
        .map_err(|r| prefix("uart.", r))?
        .as_str()
    {
        "pl011" => Uart::Pl011,
        "ns16550a" => Uart::Ns16550a,
        other => {
            return Err(refuse(
                "uart.kind",
                format!("`{other}` is not `pl011` or `ns16550a`"),
            ))
        }
    };
    let uart = address(uart_t, "address").map_err(|r| prefix("uart.", r))?;
    let stop_t = table(&doc, "stop")?;
    allow(stop_t, "stop.", &["kind", "address"])?;
    let stop =
        match string(stop_t, "kind")
            .map_err(|r| prefix("stop.", r))?
            .as_str()
        {
            // ARM semihosting stops an AArch64 machine; RISC-V's is not this runtime's.
            "semihosting" if isa == Isa::Aarch64 => {
                if stop_t.contains_key("address") {
                    return Err(refuse("stop.address", "semihosting has no address"));
                }
                Stop::Semihosting
            }
            "semihosting" => return Err(refuse(
                "stop.kind",
                "semihosting stops an AArch64 board; a RISC-V board stops through `sifive-test`",
            )),
            "sifive-test" => Stop::SifiveTest {
                address: address(stop_t, "address").map_err(|r| prefix("stop.", r))?,
            },
            other => {
                return Err(refuse(
                    "stop.kind",
                    format!("`{other}` is not `semihosting` or `sifive-test`"),
                ))
            }
        };
    let heap = match doc.get("heap") {
        None => None,
        Some(_) => {
            let h = table(&doc, "heap")?;
            allow(h, "heap.", &["size"])?;
            let size = address(h, "size").map_err(|r| prefix("heap.", r))?;
            if size == 0 || size % 16 != 0 {
                return Err(refuse(
                    "heap.size",
                    "a heap is a positive multiple of 16 bytes",
                ));
            }
            Some(size)
        }
    };
    Ok(Board {
        triple: Cow::Owned(triple.to_owned()),
        name: Cow::Owned(name),
        isa,
        load_address,
        uart,
        uart_kind,
        stop,
        stack,
        heap,
        emulator: Cow::Borrowed(""),
        hardware: false,
    })
}

fn prefix(p: &str, r: Refused) -> Refused {
    Refused {
        key: format!("{p}{}", r.key),
        why: r.why,
    }
}

/// Every key of `t` is one of `known`.
fn allow(t: &toml::Table, at: &str, known: &[&str]) -> Result<(), Refused> {
    match t.keys().find(|k| !known.contains(&k.as_str())) {
        Some(k) => Err(refuse(
            &format!("{at}{k}"),
            format!(
                "not a key a board has; the keys here are {}",
                known.join(", ")
            ),
        )),
        None => Ok(()),
    }
}

fn string(t: &toml::Table, key: &str) -> Result<String, Refused> {
    match t.get(key) {
        Some(toml::Value::String(s)) => Ok(s.clone()),
        Some(_) => Err(refuse(key, "a string")),
        None => Err(refuse(key, "missing")),
    }
}

/// A non-negative integer: an address or a size.
fn address(t: &toml::Table, key: &str) -> Result<u64, Refused> {
    match t.get(key) {
        Some(toml::Value::Integer(n)) => {
            u64::try_from(*n).map_err(|_| refuse(key, "an address or size is never negative"))
        }
        Some(_) => Err(refuse(key, "an integer: an address or a size")),
        None => Err(refuse(key, "missing")),
    }
}

fn table<'t>(t: &'t toml::Table, key: &str) -> Result<&'t toml::Table, Refused> {
    match t.get(key) {
        Some(toml::Value::Table(x)) => Ok(x),
        Some(_) => Err(refuse(key, "a table")),
        None => Err(refuse(key, "missing")),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use nazm_lir::runtime::board::BOARDS;

    #[test]
    fn every_built_in_board_reads_back_from_its_own_manifest() {
        for b in &BOARDS {
            let read = read(&b.manifest(), &b.triple, b.isa).unwrap();
            assert_eq!(
                Board {
                    emulator: b.emulator.clone(),
                    ..read
                },
                *b,
                "{}",
                b.manifest()
            );
        }
    }
}
