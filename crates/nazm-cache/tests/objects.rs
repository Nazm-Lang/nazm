//! The native object store, and every way an entry may not be believed.
//!
//! # What these establish
//!
//! An object is binary. It does not parse, it does not carry a version, and a damaged one
//! does not announce itself — it becomes a linker failure somewhere else, or, worse, a
//! program. So every one of these asks the same question from a different direction:
//!
//! > can a file in this directory make the compiler link something it did not compile?
//!
//! The answer has to be no for each of them, and the shape of the answer has to be a
//! **miss**. An entry that fails validation is never an error: a cache is derived data,
//! and derived data that could fail a build would be a cache whose absence changed a
//! program's meaning.

// Slicing and indexing a file this test just wrote: a panic here is the test failing,
// which is what it is for.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::integer_division
)]

use nazm_cache::objects::{ObjectArtifact, ObjectHeader, SCHEMA};
use nazm_cache::{
    BackendIdentity, CodegenConfig, LlvmInput, ObjectFingerprint, ObjectFound, ObjectInvalid,
    ObjectKey, ObjectStore,
};

const IR: &[u8] = b"define hidden i64 @nz.m.a_2Enz.main() {\nentry:\n  ret i64 0\n}\n";
const OBJECT: &[u8] = b"\xcf\xfa\xed\xfe\x0c\x00\x00\x01 not really a Mach-O, but bytes";

fn tool() -> BackendIdentity {
    BackendIdentity::of("driver", "version 1\nTarget: aarch64\n")
}

fn config() -> CodegenConfig {
    CodegenConfig::of(&["-O0".to_owned(), "-triple".to_owned(), "aarch64".to_owned()])
}

fn entry() -> ObjectArtifact {
    ObjectArtifact::of(LlvmInput::of(IR), tool(), config(), OBJECT.to_vec())
}

fn key() -> ObjectKey {
    ObjectKey::of(LlvmInput::of(IR), tool(), config())
}

/// A store in a fresh directory, with one valid entry already in it.
fn warm() -> (tempfile::TempDir, ObjectStore) {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = ObjectStore::under(dir.path());
    store.put(&entry()).expect("publish");
    (dir, store)
}

/// The single file holding the entry for `key`.
fn file(store: &ObjectStore, key: &ObjectKey) -> std::path::PathBuf {
    let name = key.to_string();
    store
        .directory()
        .join(&name[..2])
        .join(format!("{name}.nzo"))
}

/// Rewrite an entry's header field, leaving the object bytes exactly as they were.
fn damage(store: &ObjectStore, key: &ObjectKey, edit: impl Fn(&mut ObjectHeader)) {
    let path = file(store, key);
    let raw = std::fs::read(&path).expect("read");
    let split = raw.iter().position(|b| *b == b'\n').expect("a header");
    let mut header: ObjectHeader = serde_json::from_slice(&raw[..split]).expect("a header");
    edit(&mut header);
    let mut out = serde_json::to_vec(&header).expect("header");
    out.push(b'\n');
    out.extend_from_slice(&raw[split + 1..]);
    std::fs::write(&path, out).expect("write");
}

#[test]
fn a_published_entry_comes_back_exactly() {
    let (_dir, store) = warm();
    assert_eq!(store.get(&key()), ObjectFound::Hit(OBJECT.to_vec()));
}

#[test]
fn nothing_is_filed_under_a_key_that_was_never_written() {
    let (_dir, store) = warm();
    let other = ObjectKey::of(LlvmInput::of(b"other ir"), tool(), config());
    assert_eq!(store.get(&other), ObjectFound::Miss);
}

#[test]
fn an_entry_carries_its_own_inputs_and_they_are_recomputed() {
    // The file name is a digest, and a digest match is evidence rather than proof. The
    // entry repeats what it was keyed on so that the claim can be checked instead of
    // assumed.
    let e = entry();
    assert_eq!(e.header.schema, SCHEMA);
    assert_eq!(e.header.key, key().to_string());
    assert_eq!(e.header.llvm_input, LlvmInput::of(IR).to_string());
    assert_eq!(e.header.tool, tool().to_string());
    assert_eq!(e.header.config, config().to_string());
    assert_eq!(e.header.object, ObjectFingerprint::of(OBJECT).to_string());
    assert_eq!(e.header.length, OBJECT.len() as u64);
}

// --- §45: every way a stored object may be wrong -----------------------------------------

#[test]
fn a_truncated_object_is_a_miss() {
    let (_dir, store) = warm();
    let path = file(&store, &key());
    let raw = std::fs::read(&path).expect("read");
    std::fs::write(&path, &raw[..raw.len() - 8]).expect("truncate");
    assert_eq!(
        store.get(&key()),
        ObjectFound::Rejected(ObjectInvalid::LengthDisagrees)
    );
}

#[test]
fn an_object_whose_bytes_were_edited_is_a_miss() {
    // Same length, different content: the one corruption a length check cannot see, and
    // the reason the digest of the object is stored beside it.
    let (_dir, store) = warm();
    let path = file(&store, &key());
    let mut raw = std::fs::read(&path).expect("read");
    let last = raw.len() - 1;
    raw[last] ^= 0xff;
    std::fs::write(&path, raw).expect("edit");
    assert_eq!(
        store.get(&key()),
        ObjectFound::Rejected(ObjectInvalid::ObjectMoved)
    );
}

#[test]
fn an_entry_moved_to_another_key_s_name_is_a_miss() {
    // Filed where it does not belong. The name is how an entry is found and never why it
    // is believed.
    let (_dir, store) = warm();
    let other = ObjectKey::of(LlvmInput::of(b"other ir"), tool(), config());
    let target = file(&store, &other);
    std::fs::create_dir_all(target.parent().expect("parent")).expect("mkdir");
    std::fs::rename(file(&store, &key()), &target).expect("move");
    assert_eq!(
        store.get(&other),
        ObjectFound::Rejected(ObjectInvalid::WrongKey)
    );
}

#[test]
fn a_header_claiming_other_inputs_is_a_miss() {
    // Three separate rewrites, because a key that ignored any one of its three fields
    // would still pass the other two.
    for edit in [
        (|h: &mut ObjectHeader| h.llvm_input = LlvmInput::of(b"something else").to_string())
            as fn(&mut ObjectHeader),
        |h: &mut ObjectHeader| {
            h.tool = BackendIdentity::of("driver", "version 2").to_string();
        },
        |h: &mut ObjectHeader| {
            h.config = CodegenConfig::of(&["-O2".to_owned()]).to_string();
        },
    ] {
        let (_dir, store) = warm();
        damage(&store, &key(), edit);
        assert_eq!(
            store.get(&key()),
            ObjectFound::Rejected(ObjectInvalid::KeyDisagrees),
            "an entry whose recorded inputs do not produce its own name was believed"
        );
    }
}

#[test]
fn a_header_claiming_another_object_is_a_miss() {
    let (_dir, store) = warm();
    damage(&store, &key(), |h| {
        h.object = ObjectFingerprint::of(b"a different object").to_string();
    });
    // The object digest is part of neither the key nor its recomputation, so this is not
    // a key disagreement — it is the entry disagreeing with the bytes it carries.
    assert_eq!(
        store.get(&key()),
        ObjectFound::Rejected(ObjectInvalid::ObjectMoved)
    );
}

#[test]
fn a_digest_that_is_not_in_canonical_form_is_a_miss() {
    let (_dir, store) = warm();
    damage(&store, &key(), |h| h.tool = "NOT A DIGEST".to_owned());
    assert_eq!(
        store.get(&key()),
        ObjectFound::Rejected(ObjectInvalid::BadField("tool"))
    );
}

#[test]
fn an_unknown_schema_is_refused_rather_than_reinterpreted() {
    // The one place where reading a future file's fields as the current ones fails
    // silently and successfully.
    let (_dir, store) = warm();
    damage(&store, &key(), |h| h.schema = "nazm.object/2".to_owned());
    assert_eq!(
        store.get(&key()),
        ObjectFound::Rejected(ObjectInvalid::UnknownSchema("nazm.object/2".to_owned()))
    );
}

#[test]
fn a_file_with_no_header_is_a_miss() {
    let (_dir, store) = warm();
    std::fs::write(file(&store, &key()), OBJECT).expect("write");
    assert_eq!(
        store.get(&key()),
        ObjectFound::Rejected(ObjectInvalid::NoHeader)
    );
}

#[test]
fn a_header_that_is_not_json_is_a_miss() {
    let (_dir, store) = warm();
    let mut raw = b"not json at all\n".to_vec();
    raw.extend_from_slice(OBJECT);
    std::fs::write(file(&store, &key()), raw).expect("write");
    assert!(matches!(
        store.get(&key()),
        ObjectFound::Rejected(ObjectInvalid::Malformed(_))
    ));
}

#[test]
fn an_incomplete_publication_is_not_an_entry() {
    // A compiler killed between the write and the rename leaves a `.part` file. Nothing
    // looks for one, so it is not found, and the build compiles.
    let dir = tempfile::tempdir().expect("tempdir");
    let store = ObjectStore::under(dir.path());
    let name = key().to_string();
    let parent = store.directory().join(&name[..2]);
    std::fs::create_dir_all(&parent).expect("mkdir");
    let raw = entry().to_bytes().expect("bytes");
    std::fs::write(
        parent.join(format!("{name}.1.0.part")),
        &raw[..raw.len() / 2],
    )
    .expect("half an entry");
    assert_eq!(store.get(&key()), ObjectFound::Miss);
}

#[test]
fn an_empty_object_still_round_trips() {
    // Not a useful object, and exactly the value a truncation could produce, so the
    // length and the digest must agree about it rather than treating zero as absent.
    let dir = tempfile::tempdir().expect("tempdir");
    let store = ObjectStore::under(dir.path());
    let e = ObjectArtifact::of(LlvmInput::of(IR), tool(), config(), Vec::new());
    store.put(&e).expect("publish");
    assert_eq!(store.get(&key()), ObjectFound::Hit(Vec::new()));
}

#[test]
fn an_object_containing_a_newline_is_read_back_whole() {
    // The header is one line and the split is on the first newline, so an object full of
    // them must be unaffected. Mach-O and ELF both contain `\n` bytes routinely.
    let dir = tempfile::tempdir().expect("tempdir");
    let store = ObjectStore::under(dir.path());
    let bytes: Vec<u8> = (0u8..=255).cycle().take(4096).collect();
    let e = ObjectArtifact::of(LlvmInput::of(IR), tool(), config(), bytes.clone());
    store.put(&e).expect("publish");
    assert_eq!(store.get(&key()), ObjectFound::Hit(bytes));
}

// --- §21: two compilers, one key ---------------------------------------------------------

#[test]
fn concurrent_writers_of_one_key_cannot_make_a_malformed_entry() {
    // Entries are immutable because the key covers every input, so both write, one
    // rename lands second, and the bytes are the same either way. No lock is needed and
    // none is taken.
    let dir = tempfile::tempdir().expect("tempdir");
    let store = ObjectStore::under(dir.path());
    let threads: Vec<_> = (0..8)
        .map(|_| {
            let store = store.clone();
            std::thread::spawn(move || store.put(&entry()))
        })
        .collect();
    for t in threads {
        t.join().expect("thread").expect("publish");
    }
    assert_eq!(store.get(&key()), ObjectFound::Hit(OBJECT.to_vec()));

    // And nothing was left behind that a reader could mistake for an entry.
    let parent = store.directory().join(&key().to_string()[..2]);
    let entries: Vec<_> = std::fs::read_dir(&parent)
        .expect("read")
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(entries.len(), 1, "{entries:?}");
}

#[test]
fn a_store_writes_nothing_outside_its_own_directory() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = ObjectStore::under(&dir.path().join("derived"));
    store.put(&entry()).expect("publish");
    let top: Vec<_> = std::fs::read_dir(dir.path())
        .expect("read")
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(top, vec!["derived".to_owned()]);
}

#[test]
fn the_two_stores_do_not_share_a_directory() {
    // A semantic result and an object answer different questions, and an entry of one
    // must never be reachable by a lookup of the other.
    let dir = tempfile::tempdir().expect("tempdir");
    assert_ne!(
        ObjectStore::under(dir.path()).directory(),
        nazm_cache::Store::under(dir.path()).directory()
    );
}
