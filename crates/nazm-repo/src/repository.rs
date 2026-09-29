//! The repository's inputs, read once per request, and the state they digest to.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::cargo::{self, Package};
use crate::catalogue::{self, Catalogue};
use crate::manifest::{Layout, SourceRoot};
use crate::{Problem, COMPILER};

const STATE_TAG: &[u8] = b"nazm.repository-state/1";

/// What the state is taken over, as it is read: each input framed by its class, its
/// repository-relative path and its exact bytes, in the order they are read.
#[derive(Debug)]
pub(crate) struct Inputs {
    hasher: blake3::Hasher,
    texts: BTreeMap<String, String>,
}

impl Inputs {
    fn new() -> Self {
        let mut i = Self {
            hasher: blake3::Hasher::new(),
            texts: BTreeMap::new(),
        };
        i.part(STATE_TAG);
        i.part(COMPILER.as_bytes());
        i
    }

    /// Length-prefixed, so no two sequences of parts frame to the same bytes.
    fn part(&mut self, bytes: &[u8]) {
        self.hasher.update(&(bytes.len() as u64).to_le_bytes());
        self.hasher.update(bytes);
    }

    pub(crate) fn frame(&mut self, class: &str, value: &str) {
        self.part(class.as_bytes());
        self.part(value.as_bytes());
    }

    /// Read `path` under `root`, framed into the state, or `None` if it is not a readable
    /// UTF-8 file — which is framed too, so a file's disappearance moves the state.
    pub(crate) fn read(&mut self, root: &Path, path: &str, class: &str) -> Option<String> {
        self.part(class.as_bytes());
        self.part(path.as_bytes());
        match std::fs::read(root.join(path)).map(String::from_utf8) {
            Ok(Ok(text)) => {
                self.part(b"+");
                self.part(text.as_bytes());
                self.texts.insert(path.to_owned(), text.clone());
                Some(text)
            }
            _ => {
                self.part(b"-");
                None
            }
        }
    }

    /// A file already read.
    pub(crate) fn text(&self, path: &str) -> Option<String> {
        self.texts.get(path).cloned()
    }

    fn finish(&self) -> String {
        self.hasher.finalize().to_hex().to_string()
    }
}

/// A published machine schema: the `schema` constant it declares, and its file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Schema {
    pub(crate) name: String,
    pub(crate) file: String,
}

/// Every input of one request, and the state they digest to. Request-local.
#[derive(Debug)]
pub(crate) struct Repository {
    pub(crate) root: PathBuf,
    pub(crate) layout: Layout,
    pub(crate) state: String,
    pub(crate) packages: Vec<Package>,
    pub(crate) catalogue: Catalogue,
    pub(crate) corpus: nazm_docs::Corpus,
    pub(crate) schemas: Vec<Schema>,
    /// Every `.nz` file directly in a source root: `(source root id, file name)` to text.
    pub(crate) sources: BTreeMap<(&'static str, String), String>,
}

impl Repository {
    /// Read every input the layout names, or every problem that makes the repository
    /// unreadable.
    pub(crate) fn load(root: &Path, layout: &Layout) -> Result<Self, Vec<Problem>> {
        let mut inputs = Inputs::new();
        let mut problems = Vec::new();
        for s in layout.sources {
            inputs.frame("source_root", &format!("{}\0{}", s.id, s.dir));
            for r in s.roots {
                inputs.frame("compilation_root", r);
            }
            for d in s.docs {
                inputs.frame("source_root_doc", d);
            }
        }
        inputs.frame("catalogue", layout.catalogue.unwrap_or_default());
        inputs.frame("schemas", layout.schemas.unwrap_or_default());

        let corpus = match nazm_docs::Corpus::load(root, layout.documents) {
            Ok(c) => Some(c),
            Err(p) => {
                problems.extend(p.into_iter().map(|p| {
                    Problem::new(
                        "documentation_unavailable",
                        format!("{} {}", p.document, p.kind),
                    )
                }));
                None
            }
        };
        if let Some(c) = &corpus {
            inputs.frame("documentation", c.state());
        }
        let packages = cargo::load(root, &mut inputs, &mut problems);
        let catalogue = catalogue::load(
            root,
            layout.catalogue,
            &packages,
            &mut inputs,
            &mut problems,
        );
        let schemas = schemas(root, layout.schemas, &mut inputs, &mut problems);
        let sources = sources(root, layout.sources, &mut inputs, &mut problems);

        if let Some(c) = &corpus {
            for p in &packages {
                for d in &p.docs {
                    if !section_exists(c, d) {
                        problems.push(Problem::new(
                            "broken_documentation_link",
                            format!("{} {d}", p.id()),
                        ));
                    }
                }
            }
            for s in layout.sources {
                for d in s.docs {
                    if !section_exists(c, d) {
                        problems.push(Problem::new(
                            "broken_documentation_link",
                            format!("sources:{} {d}", s.id),
                        ));
                    }
                }
            }
        }
        match corpus {
            Some(corpus) if problems.is_empty() => Ok(Self {
                root: root.to_path_buf(),
                layout: *layout,
                state: inputs.finish(),
                packages,
                catalogue,
                corpus,
                schemas,
                sources,
            }),
            _ => {
                problems.sort();
                problems.dedup();
                Err(problems)
            }
        }
    }

    pub(crate) fn source_root(&self, id: &str) -> Option<&'static SourceRoot> {
        self.layout.sources.iter().find(|s| s.id == id)
    }

    pub(crate) fn package(&self, name: &str) -> Option<&Package> {
        self.packages.iter().find(|p| p.name == name)
    }

    /// The package a repository path is in: the member whose directory is its longest
    /// prefix, as Cargo owns files.
    pub(crate) fn owner(&self, path: &str) -> Option<&Package> {
        self.packages
            .iter()
            .filter(|p| path.starts_with(&p.prefix()))
            .max_by_key(|p| p.prefix().len())
    }
}

fn section_exists(c: &nazm_docs::Corpus, id: &str) -> bool {
    matches!(c.section(id, None), nazm_docs::SectionResult::Available(_))
}

/// Every `*.json` directly in the schema directory, by the `schema` constant it declares.
fn schemas(
    root: &Path,
    dir: Option<&str>,
    inputs: &mut Inputs,
    problems: &mut Vec<Problem>,
) -> Vec<Schema> {
    let Some(dir) = dir else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for name in files(root, dir, ".json") {
        let file = format!("{dir}/{name}");
        let Some(text) = inputs.read(root, &file, "schema") else {
            continue;
        };
        let declared = serde_json::from_str::<serde_json::Value>(&text)
            .ok()
            .and_then(|v| {
                v.pointer("/properties/schema/const")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned)
            });
        match declared {
            Some(n) => out.push(Schema { name: n, file }),
            None => problems.push(Problem::new("schema_without_name", file)),
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    for w in out.windows(2) {
        if let [a, b] = w {
            if a.name == b.name {
                problems.push(Problem::new("duplicate_schema", a.name.clone()));
            }
        }
    }
    out
}

fn sources(
    root: &Path,
    roots: &[SourceRoot],
    inputs: &mut Inputs,
    problems: &mut Vec<Problem>,
) -> BTreeMap<(&'static str, String), String> {
    let mut out = BTreeMap::new();
    for s in roots {
        for name in files(root, s.dir, ".nz") {
            if let Some(text) = inputs.read(root, &format!("{}/{name}", s.dir), "nazm_source") {
                out.insert((s.id, name), text);
            }
        }
        for r in s.roots {
            if !out.contains_key(&(s.id, (*r).to_owned())) {
                problems.push(Problem::new("missing_root", format!("{}/{r}", s.dir)));
            }
        }
    }
    out
}

/// The names of the files directly in `dir` ending in `suffix`, in byte order.
fn files(root: &Path, dir: &str, suffix: &str) -> Vec<String> {
    let mut out: Vec<String> = std::fs::read_dir(root.join(dir))
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().is_file())
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| n.ends_with(suffix))
        .collect();
    out.sort();
    out
}
