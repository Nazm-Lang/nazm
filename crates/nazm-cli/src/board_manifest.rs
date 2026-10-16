//! `nazm build --board FILE` (Gate 3, `docs/spec.md`, *Boards*): a board's manifest read into the
//! description the linker script, the runtime and the entry are generated from.
//!
//! The keys are closed, as an attribute's vocabulary is: one this does not know, a value of the
//! wrong kind or out of range, or a board whose instruction set is not the target's refuses the
//! build naming the key (`N0626`) — never a board half-understood. The built-in boards print as
//! manifests ([`nazm_lir::runtime::board::Board::manifest`]) that read back as themselves.

use std::borrow::Cow;

use nazm_lir::runtime::board::{
    Board, Controller, Image, Interrupts, Isa, Memory, Region, Stop, Uart, STACK, TIMER_INTID,
};

/// Why a manifest is refused: the key, and what is wrong with it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Refused {
    pub(crate) key: String,
    pub(crate) why: String,
    /// `N0626`, or `N0850` for a stack the board's RAM cannot hold (M1).
    pub(crate) code: nazm_diag::Code,
}

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "error[{}]: `{}`: {}", self.code, self.key, self.why)
    }
}

fn refuse(key: &str, why: impl Into<String>) -> Refused {
    Refused {
        key: key.to_owned(),
        why: why.into(),
        code: nazm_diag::codes::BOARD_MANIFEST,
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
            "interrupts",
            "watchdog",
            "memory",
            "init",
            "image",
        ],
    )?;
    let name = string(&doc, "name")?;
    let written_isa = match string(&doc, "isa")?.as_str() {
        "aarch64" => Isa::Aarch64,
        "riscv64" => Isa::Riscv64,
        "thumbv7m" => Isa::Thumbv7m,
        "thumbv8m.main" => Isa::Thumbv8m,
        "riscv32" => Isa::Riscv32,
        other => {
            return Err(refuse(
                "isa",
                format!(
                    "`{other}` is not `aarch64`, `riscv64`, `thumbv7m`, `thumbv8m.main` or \
                     `riscv32`"
                ),
            ))
        }
    };
    if written_isa != isa {
        return Err(refuse(
            "isa",
            format!("the board is not `{triple}`'s architecture"),
        ));
    }
    // A microcontroller's flash and RAM (M1): with them the image runs from flash, so it starts
    // where flash does and a load address, if written, says so too.
    let memory = match doc.get("memory") {
        None => None,
        Some(_) if !isa.microcontroller() => {
            return Err(refuse(
                "memory",
                "a 64-bit board's image is loaded whole; flash and RAM are a microcontroller's",
            ))
        }
        Some(_) => Some(memory(table(&doc, "memory")?)?),
    };
    let load_address = match (memory, doc.get("load_address")) {
        (Some(m), None) => m.flash.origin,
        (Some(m), Some(_)) => {
            let a = address(&doc, "load_address")?;
            if a != m.flash.origin {
                return Err(refuse(
                    "load_address",
                    "an image that runs from flash starts where `memory.flash` does",
                ));
            }
            a
        }
        (None, _) => address(&doc, "load_address")?,
    };
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
        "cmsdk" => Uart::Cmsdk,
        other => {
            return Err(refuse(
                "uart.kind",
                format!("`{other}` is not `pl011`, `ns16550a` or `cmsdk`"),
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
            "semihosting" if !matches!(isa, Isa::Riscv64 | Isa::Riscv32) => {
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
        // The arena's runtime parts pass sizes as 64-bit words, a Cortex-M's C sizes are 32: not
        // this board's in 1.0 (Gate 3).
        Some(_) if isa.microcontroller() => {
            return Err(refuse(
                "heap",
                "a microcontroller board has no heap in 1.0: the arena's runtime is a 64-bit \
                 board's",
            ))
        }
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
    let interrupts = match doc.get("interrupts") {
        None => None,
        Some(_) => Some(interrupts(table(&doc, "interrupts")?, isa)?),
    };
    // A watchdog on the scheduler's ticks (Gate 3): it counts the timer's, so it needs one.
    let watchdog = match doc.get("watchdog") {
        None => None,
        Some(_) => {
            let w = table(&doc, "watchdog")?;
            allow(w, "watchdog.", &["ticks"])?;
            let ticks = address(w, "ticks").map_err(|r| prefix("watchdog.", r))?;
            if ticks == 0 {
                return Err(refuse(
                    "watchdog.ticks",
                    "a watchdog waits at least one tick",
                ));
            }
            if !interrupts
                .as_ref()
                .is_some_and(|i: &Interrupts| i.timer_period_us.is_some())
            {
                return Err(refuse(
                    "watchdog",
                    "a watchdog counts the board timer's ticks: give `[interrupts]` a `timer_period_us`",
                ));
            }
            Some(ticks)
        }
    };
    // The board's own start (M1): a Nazm function of the program, by name, which the build finds
    // and checks (`N0851`).
    let init = match doc.get("init") {
        None => None,
        Some(_) => {
            let f = string(&doc, "init")?;
            let mut chars = f.chars();
            let ident = chars
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
                && chars.all(|c| c.is_ascii_alphanumeric() || c == '_');
            if !ident {
                return Err(refuse(
                    "init",
                    "the name of a function: letters, digits and `_`",
                ));
            }
            Some(Cow::Owned(f))
        }
    };
    // What the chip's boot ROM expects of the image (M1).
    let image = match doc.get("image") {
        None => None,
        Some(_) => {
            let t = table(&doc, "image")?;
            allow(t, "image.", &["format"])?;
            match string(t, "format")
                .map_err(|r| prefix("image.", r))?
                .as_str()
            {
                "rp2350" if isa == Isa::Thumbv8m && memory.is_some() => Some(Image::Rp2350),
                "rp2350" => {
                    return Err(refuse(
                        "image.format",
                        "the RP2350's image is a `thumbv8m.main` board's, run from flash: give it \
                         `[memory]`",
                    ))
                }
                other => return Err(refuse("image.format", format!("`{other}` is not `rp2350`"))),
            }
        }
    };
    // The stack is in RAM (M1): a manifest whose stack RAM cannot hold is `N0850`, as an image
    // that does not fit is.
    if let Some(m) = memory {
        if stack > m.ram.length {
            return Err(Refused {
                key: "stack".to_owned(),
                why: format!(
                    "the stack of {stack:#x} bytes does not fit the board's RAM, {:#x} bytes; it \
                     is over by {:#x}",
                    m.ram.length,
                    stack - m.ram.length
                ),
                code: nazm_diag::codes::BOARD_MEMORY,
            });
        }
    }
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
        watchdog,
        interrupts,
        emulator: Cow::Borrowed(""),
        hardware: false,
        memory,
        init,
        image,
    })
}

/// `[memory]` (M1): `flash` and `ram`, each `{ origin, length }`, word-aligned, inside the 32-bit
/// address space, and apart.
fn memory(t: &toml::Table) -> Result<Memory, Refused> {
    allow(t, "memory.", &["flash", "ram"])?;
    let region = |key: &str| -> Result<Region, Refused> {
        let at = format!("memory.{key}");
        let r = table(t, key).map_err(|r| prefix("memory.", r))?;
        allow(r, &format!("{at}."), &["origin", "length"])?;
        let origin = address(r, "origin").map_err(|e| prefix(&format!("{at}."), e))?;
        let length = address(r, "length").map_err(|e| prefix(&format!("{at}."), e))?;
        if length == 0 || origin % 4 != 0 || length % 4 != 0 {
            return Err(refuse(
                &at,
                "a region is a positive number of bytes from a word-aligned origin, a multiple of 4",
            ));
        }
        if origin.saturating_add(length) > 1 << 32 {
            return Err(refuse(&at, "a region ends within the 32-bit address space"));
        }
        Ok(Region { origin, length })
    };
    let flash = region("flash")?;
    let ram = region("ram")?;
    if flash.origin < ram.end() && ram.origin < flash.end() {
        return Err(refuse("memory.ram", "RAM and flash overlap"));
    }
    Ok(Memory { flash, ram })
}

/// `[interrupts]` (Gate 3): a GICv2 on AArch64 or an NVIC on Cortex-M, the timer's period, and the
/// further lines by name.
fn interrupts(t: &toml::Table, isa: Isa) -> Result<Interrupts, Refused> {
    if matches!(isa, Isa::Riscv64 | Isa::Riscv32) {
        return Err(refuse(
            "interrupts",
            "a RISC-V board's machine-mode interrupts are not this runtime's in 1.0; AArch64's \
             GICv2 and Cortex-M's NVIC are",
        ));
    }
    let named = string(t, "controller").map_err(|r| prefix("interrupts.", r))?;
    let controller = match (named.as_str(), isa) {
        ("gicv2", Isa::Aarch64) => {
            allow(
                t,
                "interrupts.",
                &[
                    "controller",
                    "distributor",
                    "cpu",
                    "timer_period_us",
                    "lines",
                ],
            )?;
            Controller::Gicv2 {
                distributor: address(t, "distributor").map_err(|r| prefix("interrupts.", r))?,
                cpu: address(t, "cpu").map_err(|r| prefix("interrupts.", r))?,
            }
        }
        ("nvic", Isa::Thumbv7m | Isa::Thumbv8m) => {
            allow(
                t,
                "interrupts.",
                &["controller", "clock_hz", "timer_period_us", "lines"],
            )?;
            let clock_hz = address(t, "clock_hz").map_err(|r| prefix("interrupts.", r))?;
            if clock_hz == 0 {
                return Err(refuse("interrupts.clock_hz", "a clock runs at least 1 Hz"));
            }
            Controller::Nvic { clock_hz }
        }
        (other, _) => {
            let want = if isa == Isa::Aarch64 { "gicv2" } else { "nvic" };
            return Err(refuse(
                "interrupts.controller",
                format!("`{other}` is not this board's: its controller is `{want}`"),
            ));
        }
    };
    let timer_period_us = match t.get("timer_period_us") {
        None => None,
        Some(_) => {
            let p = address(t, "timer_period_us").map_err(|r| prefix("interrupts.", r))?;
            if p == 0 {
                return Err(refuse(
                    "interrupts.timer_period_us",
                    "a period is at least one microsecond",
                ));
            }
            // SysTick counts down from a 24-bit reload: a period longer than that is not one.
            if let Controller::Nvic { clock_hz } = controller {
                let ticks = (u128::from(clock_hz) * u128::from(p))
                    .checked_div(1_000_000)
                    .unwrap_or(0);
                if ticks == 0 || ticks > 0x0100_0000 {
                    return Err(refuse(
                        "interrupts.timer_period_us",
                        "SysTick counts at most 2^24 of the clock's ticks: a shorter period, or \
                         a period of at least one tick",
                    ));
                }
            }
            Some(p)
        }
    };
    // A GICv2's lines are INTIDs from 16 to 1019, the timer's 27 excepted; an NVIC's are its
    // external interrupts from 0 to 239.
    let (low, high) = match controller {
        Controller::Gicv2 { .. } => (16, 1019),
        Controller::Nvic { .. } => (0, 239),
    };
    let mut lines = Vec::new();
    if t.contains_key("lines") {
        let l = table(t, "lines").map_err(|r| prefix("interrupts.", r))?;
        for (name, v) in l {
            let key = format!("interrupts.lines.{name}");
            let id = match v {
                toml::Value::Integer(n) => u32::try_from(*n).ok(),
                _ => None,
            };
            let gic_timer =
                matches!(controller, Controller::Gicv2 { .. }) && id == Some(TIMER_INTID);
            match id {
                Some(id) if (low..=high).contains(&id) && !gic_timer => {
                    if name == "timer" {
                        return Err(refuse(&key, "`timer` is the board timer's name"));
                    }
                    if lines.iter().any(|(_, i)| *i == id) {
                        return Err(refuse(&key, format!("interrupt {id} is named twice")));
                    }
                    lines.push((name.clone(), id));
                }
                _ => {
                    let not = if matches!(controller, Controller::Gicv2 { .. }) {
                        format!(", and not {TIMER_INTID}, the timer's")
                    } else {
                        String::new()
                    };
                    return Err(refuse(
                        &key,
                        format!("an interrupt number from {low} to {high}{not}"),
                    ));
                }
            }
        }
    }
    Ok(Interrupts {
        controller,
        timer_period_us,
        lines,
    })
}

fn prefix(p: &str, r: Refused) -> Refused {
    Refused {
        key: format!("{p}{}", r.key),
        why: r.why,
        code: r.code,
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
