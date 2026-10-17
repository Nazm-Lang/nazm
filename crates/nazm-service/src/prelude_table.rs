//! The prelude's standard modules as the loader looks for them (R1.5, D9): each module's public
//! names, the methods its functions answer to, and the prelude modules it imports.
//!
//! Read from `library/std`'s prelude modules once, here, rather than by parsing every one of
//! them on every run — a third of a small program's `nazm check` (performance leadership 7.2).
//! `the_prelude_table_is_read_from_the_prelude_sources` reads them again and fails, printing
//! this table as it must now read, whenever a prelude module's public items change.

use crate::load::PreludeEntry;

pub(crate) const TABLE: &[PreludeEntry] = &[
    PreludeEntry {
        name: "hash",
        names: &[
            "hash_finish",
            "hash_bytes",
            "hash_u64",
            "hash_combine",
            "Hash",
        ],
        methods: &["finish", "bytes", "u64", "combine", "hash", "same"],
        imports: &[],
    },
    PreludeEntry {
        name: "hashmap",
        names: &[
            "hashmap_new",
            "hashmap_single",
            "hashmap_with_seed",
            "hashmap_len",
            "hashmap_set",
            "hashmap_get",
            "hashmap_at",
            "hashmap_at_or",
            "hashmap_has",
            "hashmap_remove",
            "hashmap_keys",
            "hashmap_values",
            "hashmap_top",
            "HashMap",
            "Slot",
            "Map",
        ],
        methods: &[
            "single",
            "with_seed",
            "at",
            "at_or",
            "has",
            "remove",
            "keys",
            "values",
            "top",
        ],
        imports: &["hash", "ordmap"],
    },
    PreludeEntry {
        name: "option",
        names: &["option_is_some", "option_is_none", "option_unwrap_or"],
        methods: &["is_some", "is_none", "unwrap_or"],
        imports: &[],
    },
    PreludeEntry {
        name: "ordmap",
        names: &[
            "ordmap_new",
            "ordmap_len",
            "ordmap_get",
            "ordmap_at",
            "ordmap_at_or",
            "ordmap_has",
            "ordmap_set",
            "ordmap_remove",
            "ordmap_first",
            "ordmap_last",
            "ordmap_floor",
            "ordmap_ceiling",
            "ordmap_keys",
            "ordmap_values",
            "ordmap_entries",
            "ordmap_range",
            "OrdEntry",
            "OrdMap",
            "Ord",
        ],
        methods: &[
            "at", "at_or", "has", "remove", "first", "last", "floor", "ceiling", "keys", "values",
            "entries", "range", "compare",
        ],
        imports: &[],
    },
    PreludeEntry {
        name: "result",
        names: &[
            "result_is_ok",
            "result_is_err",
            "result_unwrap_or",
            "result_ok",
            "result_err",
        ],
        methods: &["is_ok", "is_err", "unwrap_or", "ok", "err"],
        imports: &[],
    },
    PreludeEntry {
        name: "sort",
        names: &[
            "vec_sort_by",
            "hashmap_sort_by",
            "ints_sort",
            "strs_sort",
            "vec_sort_in_place_by",
            "ints_sort_in_place",
            "strs_sort_in_place",
        ],
        methods: &[
            "sort_by",
            "sort_by",
            "sort",
            "sort",
            "sort_in_place_by",
            "sort_in_place",
            "sort_in_place",
        ],
        imports: &["hash", "hashmap"],
    },
    PreludeEntry {
        name: "text",
        names: &[
            "text_starts_with",
            "text_ends_with",
            "text_find",
            "text_contains",
            "text_repeat",
            "text_split_end",
            "text_split_next",
            "text_split",
            "text_is_space",
            "text_trim",
            "text_parse_int",
            "text_trim_start",
            "text_trim_end",
            "text_is_digit",
            "text_is_alpha",
            "text_to_upper",
            "text_to_lower",
            "text_replace",
            "text_count",
            "text_pad_left",
            "text_pad_right",
            "text_lines",
            "text_compare",
        ],
        methods: &[
            "starts_with",
            "ends_with",
            "find",
            "contains",
            "repeat",
            "split_end",
            "split_next",
            "split",
            "is_space",
            "trim",
            "parse_int",
            "trim_start",
            "trim_end",
            "is_digit",
            "is_alpha",
            "to_upper",
            "to_lower",
            "replace",
            "count",
            "pad_left",
            "pad_right",
            "lines",
            "compare",
        ],
        imports: &[],
    },
];

/// The table as Rust, as [`TABLE`] is written.
#[cfg(test)]
fn render(table: &[crate::load::PreludeRow]) -> String {
    let list = |xs: &[String]| {
        let items: Vec<String> = xs.iter().map(|x| format!("{x:?}")).collect();
        format!("&[{}]", items.join(", "))
    };
    let mut out = String::from("pub(crate) const TABLE: &[PreludeEntry] = &[\n");
    for (name, names, methods, imports) in table {
        out.push_str(&format!(
            "    PreludeEntry {{\n        name: {name:?},\n        names: {},\n        methods: {},\n        imports: {},\n    }},\n",
            list(names),
            list(methods),
            list(imports)
        ));
    }
    out.push_str("];\n");
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_prelude_table_is_read_from_the_prelude_sources() {
        let read = crate::load::prelude_table_from_sources();
        let owned = |xs: &[&str]| xs.iter().map(|x| (*x).to_owned()).collect::<Vec<_>>();
        let written: Vec<crate::load::PreludeRow> = super::TABLE
            .iter()
            .map(|e| {
                (
                    e.name.to_owned(),
                    owned(e.names),
                    owned(e.methods),
                    owned(e.imports),
                )
            })
            .collect();
        assert!(
            written == read,
            "a prelude module's public items changed: crates/nazm-service/src/prelude_table.rs \
             must read (then `cargo fmt`):\n{}",
            super::render(&read)
        );
    }
}
