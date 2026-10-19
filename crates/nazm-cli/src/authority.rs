//! The program's authority (R1, 5b.1, option C; `docs/user/spec.md`, *What `main` holds, and how it
//! ends*): the capabilities its `main` holds — every authority a program has enters there — said
//! by `nazm build` and `nazm inspect`, so a reader learns what a program may touch without
//! reading its body. Read from `main`'s signature as the parser gives it, so the `OutCap` option C
//! gives a `main` that writes none is there, marked as given rather than written.

use serde_json::{json, Value};

/// One capability `main` holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Held {
    /// The capability's type, as source writes it: `IoCap`.
    pub(crate) capability: String,
    /// Given by option C rather than written.
    pub(crate) implicit: bool,
}

/// What each capability lets a program touch, in a reader's words.
fn reaches(capability: &str) -> &'static str {
    match capability {
        "OutCap" => "prints to standard output and error",
        "IoCap" => "files, arguments, the terminal and the process's exit",
        "NetCap" => "the network",
        "ProcessCap" => "other programs and the environment",
        "SpawnCap" => "threads",
        "RandomCap" => "the system's randomness",
        "TimeCap" => "the clocks",
        "ForeignCap" => "C functions",
        "VouchCap" => "vouching for values' provenance",
        "MmioCap" => "device memory",
        _ => "an authority this build does not describe",
    }
}

/// The capabilities `text`'s top-level `main` holds, in the order its parameters are; `None`
/// where there is no `main` — a library has no authority of its own to report.
pub(crate) fn of_main(text: &str) -> Option<Vec<Held>> {
    let (program, _) = nazm_core::__syntax::parse_source(text);
    program
        .functions
        .iter()
        .find(|f| f.method_of.is_none() && f.name == "main")
        .map(|main| {
            main.params
                .iter()
                .filter(|p| p.ty.name.ends_with("Cap"))
                .map(|p| Held {
                    capability: p.ty.name.clone(),
                    implicit: p.span.start == p.span.end,
                })
                .collect()
        })
}

/// The line `nazm build` prints: `authority: OutCap (given to main): prints to standard output
/// and error`, each capability after a `; `, or `authority: none`.
pub(crate) fn line(held: &[Held]) -> String {
    if held.is_empty() {
        return "authority: none".to_owned();
    }
    let parts: Vec<String> = held
        .iter()
        .map(|h| {
            let given = if h.implicit { " (given to main)" } else { "" };
            format!("{}{given}: {}", h.capability, reaches(&h.capability))
        })
        .collect();
    format!("authority: {}", parts.join("; "))
}

/// The `authority` member of `nazm.inspect/2`.
pub(crate) fn json(held: &[Held]) -> Value {
    Value::Array(
        held.iter()
            .map(|h| {
                json!({
                    "capability": h.capability,
                    "implicit": h.implicit,
                    "reaches": reaches(&h.capability),
                })
            })
            .collect(),
    )
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn main_s_written_and_given_capabilities_are_read_from_its_signature() {
        let given = of_main("fn main() { println(\"hi\") }\n").unwrap();
        assert_eq!(
            given,
            [Held {
                capability: "OutCap".to_owned(),
                implicit: true
            }]
        );
        assert_eq!(
            line(&given),
            "authority: OutCap (given to main): prints to standard output and error"
        );
        let written = of_main("fn main(io: IoCap, net: NetCap, n: Int) -> Int { 0 }\n").unwrap();
        assert_eq!(
            line(&written),
            "authority: IoCap: files, arguments, the terminal and the process's exit; NetCap: the network"
        );
        assert_eq!(
            // A parameter named `out` that is not one: option C gives nothing.
            line(&of_main("fn main(out: Int) -> Int { out }\n").unwrap()),
            "authority: none"
        );
        assert_eq!(of_main("pub fn helper() -> Int { 0 }\n"), None);
    }
}
