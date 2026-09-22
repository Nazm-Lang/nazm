//! Where native objects are kept, and the rules that keep a damaged one from being
//! linked.
//!
//! # One file per entry, header and object together
//!
//! An object is binary and says nothing about itself. A directory of bare `.o` files
//! beside a directory of manifests would be two things that can disagree, and there is a
//! window — after one is written and before the other is — in which a reader sees a
//! complete-looking entry that is not one. So an entry is **one file**: a single line of
//! `nazm.object/1` JSON, a newline, and then the object's exact bytes.
//!
//! ```text
//! {"schema":"nazm.object/1","key":"…","llvm_input":"…", …}\n<object bytes>
//! ```
//!
//! Nothing in the header can contain a newline — it is JSON on one line — so the split is
//! unambiguous, and a truncated file fails either the header parse or the length check.
//!
//! # What a lookup must prove before it is a hit
//!
//! 1. the header parses, and its schema is one this build implements;
//! 2. every digest in it is in canonical form;
//! 3. the inputs it records reproduce the key it is filed under;
//! 4. that key is the one that was asked for;
//! 5. the bytes after the header are exactly as long as it claims;
//! 6. their digest is the one it claims.
//!
//! The file name is checked last in importance and first in time: it is how the entry is
//! found, and it is never why the entry is believed.
//!
//! # Publication is atomic or it did not happen
//!
//! Written under a name unique to this process and this call, flushed, then `rename`d.
//! Entries are immutable — the key covers every input — so two compilers racing on one
//! key both write and one rename lands second, with identical bytes either way. No lock,
//! no index, no sweep. A compiler killed mid-write leaves a `.part` file, which is not an
//! entry and which nothing looks for.
//!
//! # A miss is always allowed
//!
//! Every failure here is a miss, and a miss compiles. A cache whose damage could change
//! what a program means would be a cache that is part of the compiler; this one is
//! derived data, and `docs/architecture.md` §7.6 says so in the same words §7.4 used of
//! the semantic store.

use std::io::Write as _;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::object::{BackendIdentity, CodegenConfig, LlvmInput, ObjectFingerprint, ObjectKey};

/// The identifier every persisted object entry carries.
pub const SCHEMA: &str = "nazm.object/1";

/// The subdirectory of the derived-data root that holds them.
pub const DIRECTORY: &str = "native";

/// What a native cache lookup found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ObjectFound {
    /// A valid entry for exactly this key, and the object it holds.
    Hit(Vec<u8>),
    /// Nothing is filed under it.
    Miss,
    /// Something is, and it may not be linked.
    Rejected(ObjectInvalid),
}

/// Why a stored object could not be used. Every one is a miss.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ObjectInvalid {
    /// No newline, so there is no header and no object.
    NoHeader,
    /// The header is not JSON, or not this shape.
    Malformed(String),
    /// A schema this build does not implement.
    UnknownSchema(String),
    /// A digest that is not in the canonical written form.
    BadField(&'static str),
    /// The inputs recorded in the entry do not produce the key it is filed under.
    KeyDisagrees,
    /// It is a valid entry, for something else.
    WrongKey,
    /// The object is not as long as the header says — a truncated write, or a truncated
    /// read.
    LengthDisagrees,
    /// The object's bytes are not the ones the header describes.
    ObjectMoved,
}

impl std::fmt::Display for ObjectInvalid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoHeader => write!(f, "not a `{SCHEMA}` entry: it has no header line"),
            Self::Malformed(e) => write!(f, "not a `{SCHEMA}` entry: {e}"),
            Self::UnknownSchema(s) => write!(f, "entry schema `{s}` is not `{SCHEMA}`"),
            Self::BadField(w) => write!(f, "`{w}` is not in canonical form"),
            Self::KeyDisagrees => {
                f.write_str("its recorded inputs do not produce the key it is filed under")
            }
            Self::WrongKey => f.write_str("it is an entry for something else"),
            Self::LengthDisagrees => f.write_str("the object is not the length it claims"),
            Self::ObjectMoved => f.write_str("the object is not the one its digest describes"),
        }
    }
}

impl std::error::Error for ObjectInvalid {}

/// The header of one entry: its key, and every input that produced it.
///
/// Nothing here is informational. Every field is recomputed or compared during
/// [`read`](ObjectArtifact::read), so there is no field in an entry that can lie.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObjectHeader {
    /// The shape these bytes are in. First, so a reader can refuse before the rest means
    /// anything.
    pub schema: String,
    /// The key it is filed under, repeated so a match can be verified rather than assumed.
    pub key: String,
    /// The digest of the LLVM text that was compiled.
    pub llvm_input: String,
    /// Which backend compiled it.
    pub tool: String,
    /// The resolved configuration it was compiled under.
    pub config: String,
    /// The digest of the object that follows.
    pub object: String,
    /// How many bytes follow the header line.
    pub length: u64,
}

/// A completed object compilation, ready to be written down or just read back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectArtifact {
    pub header: ObjectHeader,
    pub bytes: Vec<u8>,
}

impl ObjectArtifact {
    /// Record an object produced from these inputs.
    #[must_use]
    pub fn of(
        input: LlvmInput,
        tool: BackendIdentity,
        config: CodegenConfig,
        bytes: Vec<u8>,
    ) -> Self {
        let key = ObjectKey::of(input, tool, config);
        Self {
            header: ObjectHeader {
                schema: SCHEMA.to_owned(),
                key: key.to_string(),
                llvm_input: input.to_string(),
                tool: tool.to_string(),
                config: config.to_string(),
                object: ObjectFingerprint::of(&bytes).to_string(),
                length: bytes.len() as u64,
            },
            bytes,
        }
    }

    /// The key this entry belongs under.
    #[must_use]
    pub fn key(&self) -> Option<ObjectKey> {
        ObjectKey::parse(&self.header.key)
    }

    /// The exact bytes of an entry.
    ///
    /// # Errors
    ///
    /// Only if serialising the header fails, which for its shape it does not.
    pub fn to_bytes(&self) -> Result<Vec<u8>, serde_json::Error> {
        let mut out = serde_json::to_vec(&self.header)?;
        out.push(b'\n');
        out.extend_from_slice(&self.bytes);
        Ok(out)
    }

    /// Read one back and establish that it is the entry that was asked for.
    ///
    /// # Errors
    ///
    /// [`ObjectInvalid`] for anything this build may not link. Every variant is a miss.
    pub fn read(raw: &[u8], wanted: &ObjectKey) -> Result<Vec<u8>, ObjectInvalid> {
        let split = raw
            .iter()
            .position(|b| *b == b'\n')
            .ok_or(ObjectInvalid::NoHeader)?;
        let (head, rest) = raw.split_at(split);
        let bytes = rest.get(1..).unwrap_or_default();

        // The schema on its own and first: a future version may not have the shape this
        // build's `Deserialize` expects, and "failed to parse" would be the wrong thing to
        // conclude about a newer file.
        #[derive(Deserialize)]
        struct Envelope {
            schema: String,
        }
        let envelope: Envelope =
            serde_json::from_slice(head).map_err(|e| ObjectInvalid::Malformed(e.to_string()))?;
        if envelope.schema != SCHEMA {
            return Err(ObjectInvalid::UnknownSchema(envelope.schema));
        }
        let header: ObjectHeader =
            serde_json::from_slice(head).map_err(|e| ObjectInvalid::Malformed(e.to_string()))?;

        let filed = ObjectKey::parse(&header.key).ok_or(ObjectInvalid::BadField("key"))?;
        let input =
            LlvmInput::parse(&header.llvm_input).ok_or(ObjectInvalid::BadField("llvm_input"))?;
        let tool = BackendIdentity::parse(&header.tool).ok_or(ObjectInvalid::BadField("tool"))?;
        let config =
            CodegenConfig::parse(&header.config).ok_or(ObjectInvalid::BadField("config"))?;
        let object =
            ObjectFingerprint::parse(&header.object).ok_or(ObjectInvalid::BadField("object"))?;

        // The inputs must produce the key the entry claims, and that key must be the one
        // asked for. Two separate failures: the first says the file disagrees with
        // itself, the second that it is somebody else's entry.
        if ObjectKey::of(input, tool, config) != filed {
            return Err(ObjectInvalid::KeyDisagrees);
        }
        if filed != *wanted {
            return Err(ObjectInvalid::WrongKey);
        }

        // And only now the object itself. A linker discovers a damaged object as a
        // failure somewhere else entirely; this discovers it as a miss.
        if header.length != bytes.len() as u64 {
            return Err(ObjectInvalid::LengthDisagrees);
        }
        if ObjectFingerprint::of(bytes) != object {
            return Err(ObjectInvalid::ObjectMoved);
        }
        Ok(bytes.to_vec())
    }
}

/// A local directory of compiled objects.
#[derive(Debug, Clone)]
pub struct ObjectStore {
    dir: PathBuf,
}

impl ObjectStore {
    /// The store at `dir`. Nothing is created until something is written.
    #[must_use]
    pub fn at(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// Where a project's objects belong: `.nazm/native` under its source root.
    ///
    /// Beside the semantic store and never mixed with it. The two answer different
    /// questions and one may hit while the other misses, so an entry of one must never be
    /// reachable by a lookup of the other — which the separate directories and the
    /// separate key domains both ensure.
    #[must_use]
    pub fn for_project(source_root: &Path) -> Self {
        Self::at(source_root.join(crate::store::DIRECTORY).join(DIRECTORY))
    }

    /// The store under an explicitly chosen derived-data root.
    #[must_use]
    pub fn under(root: &Path) -> Self {
        Self::at(root.join(DIRECTORY))
    }

    #[must_use]
    pub fn directory(&self) -> &Path {
        &self.dir
    }

    /// Where the entry for `key` lives.
    ///
    /// One level of fan-out on the first byte, as the semantic store does, so a large
    /// project does not put thousands of files in one directory.
    fn path(&self, key: &ObjectKey) -> PathBuf {
        let name = key.to_string();
        let prefix = name.get(..2).unwrap_or("00").to_owned();
        self.dir.join(prefix).join(format!("{name}.nzo"))
    }

    /// Look for a usable object.
    #[must_use]
    pub fn get(&self, key: &ObjectKey) -> ObjectFound {
        let Ok(raw) = std::fs::read(self.path(key)) else {
            return ObjectFound::Miss;
        };
        match ObjectArtifact::read(&raw, key) {
            Ok(bytes) => ObjectFound::Hit(bytes),
            Err(e) => ObjectFound::Rejected(e),
        }
    }

    /// Publish an object, atomically.
    ///
    /// # Errors
    ///
    /// Anything the filesystem refused. A caller treats it as "not stored" and carries
    /// on: failing to write a cache entry is not a reason to fail a build.
    pub fn put(&self, artifact: &ObjectArtifact) -> Result<(), std::io::Error> {
        let key = artifact.key().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "an entry must carry the key it is filed under",
            )
        })?;
        let final_path = self.path(&key);
        let Some(parent) = final_path.parent() else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "a store needs a directory",
            ));
        };
        std::fs::create_dir_all(parent)?;

        let bytes = artifact
            .to_bytes()
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

        // Unique to this process and this call, so two compilers writing one entry at the
        // same time cannot write to one temporary file. `.part` and not `.nzo`, so a
        // reader looking for an entry cannot find one that is still being written.
        let temp = parent.join(format!(
            "{}.{}.{}.part",
            artifact.header.key,
            std::process::id(),
            COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));

        let write = (|| -> Result<(), std::io::Error> {
            let mut f = std::fs::File::create(&temp)?;
            f.write_all(&bytes)?;
            // Flushed before the rename, so what lands is a complete entry rather than a
            // name pointing at bytes the kernel has not written yet.
            f.sync_all()?;
            drop(f);
            std::fs::rename(&temp, &final_path)
        })();

        if write.is_err() {
            // Only this call's own temporary file. Nothing else in the directory is this
            // function's business.
            let _ = std::fs::remove_file(&temp);
        }
        write
    }
}

static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
