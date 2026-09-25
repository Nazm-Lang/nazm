//! The compiler's driver library. N16.
//!
//! Three things that need the filesystem and are needed by more than one front door:
//!
//! - [`root`] — where a compilation's durable identities are measured from;
//! - [`load`] — which files a compilation has, read from disk or from open buffers;
//! - [`service`] — the long-lived language service an editor talks to through `nazm lsp`.
//!
//! The first two were `nazm-cli`'s until N16 and moved here unchanged in what they decide,
//! so the command line and the language service cannot disagree about a module graph.
//! Nothing here decides what a program means: that is `nazm-core`'s, reached through
//! [`nazm_core::analyse`], the same parse and check every command runs.

pub mod load;
pub mod root;
pub mod service;

pub use service::{Definition, Hover, LanguageService, Location, Published, Target};
