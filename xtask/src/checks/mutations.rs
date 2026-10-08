//! The mutations-apply gate: every catalogue entry applies, exactly once, to the file it names.

use crate::plan;

/// No crate may **depend on** a code generator unless `rules.rs` says it can.
///
/// # What this establishes, and what it does not
///
/// It reads cargo metadata, so it is a statement about the dependency graph and nothing
/// else. **Emitting LLVM IR as text is code generation and this check cannot see it** —
/// `nazm-lir` generates a complete LLVM module with no codegen dependency at all, and
/// passes here trivially. Describing that as architecture validation would be describing
/// a vacuous result.
///
/// The capability question — *who may emit a backend artefact, and who may invoke the
/// toolchain* — is [`check_emission_reach`], which looks at call sites instead. The two
/// together are what `docs/architecture.md` §1 means by keeping Cranelift and LLVM behind
/// the LIR contract; neither is sufficient alone.
/// Every catalogued mutation still matches its file, exactly once.
///
/// A mutation whose pattern has drifted is reported `NOT INJECTED` — correctly, because a
/// defect that was never applied says nothing about the tests. But the runner only
/// discovers that after copying the tree, building the workspace and running the suite
/// once, which is minutes per entry inside the contained environment; and until it gets
/// there the catalogue's *size* is a claim about coverage that one of its entries is not
/// keeping.
///
/// This found a real one. `an-absolute-path-reaches-the-module-key` had matched nothing
/// since before N7: the line it names was rewritten to make both paths absolute, and the
/// pattern went on naming the old spelling. The catalogue said 85 and meant 84.
///
/// Pure file reads, no build, so it belongs in `check` where the cost is a few
/// milliseconds rather than in the run it protects.
pub(crate) fn check_mutations_apply() -> Result<(), Vec<String>> {
    const CATALOGUE: &str = "xtask/mutations/mutations.toml";
    let text = match std::fs::read_to_string(CATALOGUE) {
        Ok(t) => t,
        Err(e) => return Err(vec![format!("{CATALOGUE}: {e}")]),
    };
    let catalogue: toml::Value = match toml::from_str(&text) {
        Ok(v) => v,
        Err(e) => return Err(vec![format!("{CATALOGUE} is not valid TOML: {e}")]),
    };
    let Some(entries) = catalogue.get("mutation").and_then(toml::Value::as_array) else {
        return Err(vec![format!("{CATALOGUE} has no `[[mutation]]` entries")]);
    };

    let mut bad = Vec::new();
    // One read per file however many mutations name it: the catalogue is nearly a hundred
    // entries over a dozen files.
    let mut sources: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    for entry in entries {
        let field = |name: &str| entry.get(name).and_then(toml::Value::as_str);
        let (Some(name), Some(file), Some(find)) = (field("name"), field("file"), field("find"))
        else {
            bad.push(format!(
                "{CATALOGUE} has an entry missing `name`, `file` or `find`"
            ));
            continue;
        };
        let source = match sources.get(file) {
            Some(s) => s,
            None => match std::fs::read_to_string(file) {
                Ok(s) => sources.entry(file.to_owned()).or_insert(s),
                Err(e) => {
                    bad.push(format!("{name}: cannot read {file}: {e}"));
                    continue;
                }
            },
        };
        match source.matches(find).count() {
            1 => {}
            0 => bad.push(format!(
                "{name}: its pattern is not in {file} any more, so the run would report it \
                 NOT INJECTED after building the workspace to find out. Repoint it at the \
                 line that carries the rule now, or retire it"
            )),
            n => bad.push(format!(
                "{name}: its pattern appears {n} times in {file}, so the runner cannot say \
                 which one it means. Widen it with enough context to be unique"
            )),
        }
    }

    // The typed schema, and everything about killers and profiles that can be checked
    // without building: unknown fields, duplicate names, a killer or profile target
    // that does not exist, an ignored killer, a profile named without a killer and
    // without a reason. A stale killer fails here in milliseconds rather than in a
    // campaign an hour in; the campaign still re-checks against the built binary.
    match plan::load(std::path::Path::new(CATALOGUE)) {
        Err(e) => bad.push(e),
        Ok(cat) => bad.extend(plan::validate(std::path::Path::new("."), &cat)),
    }

    if bad.is_empty() {
        Ok(())
    } else {
        Err(bad)
    }
}
