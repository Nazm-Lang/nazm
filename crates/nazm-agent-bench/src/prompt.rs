//! The prompt: one system text and one layout for every task, arm and provider.
//!
//! The arms differ only in the bytes between the context tags. [`fixed`] is the same request
//! with those bytes removed — what the benchmark's own instructions cost — so a reduction is
//! never credited with the instructions both arms share.

use crate::suite::{Arm, Task};

/// The system text. It names both context shapes, so neither arm is told something the other
/// is not. It asks for the answer only: no reasoning is requested and none is scored.
pub const SYSTEM: &str = "You answer questions about a software repository using only the \
repository context in the user message. The context is either whole files, each introduced by a \
line of the form `==> path <==`, or a structured JSON task context; whichever you are given is \
everything you know about the repository. Do not use outside knowledge of this repository. Reply \
with exactly one JSON object of the shape the task's answer contract shows, and nothing else: no \
prose, no Markdown fence, no explanation. Answer every field. If the context does not contain \
what the task needs, set \"insufficient_context\" to true and leave the answer's fields empty.";

const OPEN: &str = "<repository_context>\n";
const CLOSE: &str = "\n</repository_context>\n\n";
const CONTRACT: &str = "\n\nReply with one JSON object of exactly this shape:\n";

/// One request's two messages. `max_output_tokens` is the same for every request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub system: String,
    pub user: String,
}

/// The user message for `task` with `context`'s bytes.
#[must_use]
pub fn user(task: &Task, context: &str) -> String {
    let mut s = String::with_capacity(context.len() + 2048);
    s.push_str(OPEN);
    s.push_str(context);
    s.push_str(CLOSE);
    s.push_str(&task.wording);
    s.push_str(CONTRACT);
    s.push_str(&task.contract());
    s
}

/// The request for `task` under `arm`.
#[must_use]
pub fn request(task: &Task, arm: Arm) -> Request {
    Request {
        system: SYSTEM.to_owned(),
        user: user(task, &task.context(arm).text),
    }
}

/// The request with no context: the fixed part both arms share.
#[must_use]
pub fn fixed(task: &Task) -> Request {
    Request {
        system: SYSTEM.to_owned(),
        user: user(task, ""),
    }
}

/// The digest of the template itself: the system text and the layout, without any task.
#[must_use]
pub fn digest() -> String {
    crate::digest(
        [crate::PROMPT_VERSION, SYSTEM, OPEN, CLOSE, CONTRACT]
            .join("\u{0}")
            .as_bytes(),
    )
}
