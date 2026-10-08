//! Tasks: starting one on its own thread, and joining a scope's tasks.

use super::counters::{SCHED_JOINED, SCHED_SPAWNED};
use super::value::Value;
use super::*;

impl Interpreter {
    /// Start one task and hand back the handle its scope will join.
    ///
    /// One OS thread per task, and `docs/spec.md` says so rather than implying anything
    /// else. Each task gets its **own** interpreter over the same program: the state
    /// shared between tasks is exactly the channels they were handed, which is what the
    /// capture rule is for.
    ///
    /// Each task gets its own iteration and depth budgets rather than a share of the
    /// parent's. Splitting them would make a program's own limit depend on how many
    /// tasks happened to be running, which is not something a person can reason about.
    pub(super) fn start_task(
        &self,
        callee: nazm_sema::FnId,
        args: Vec<Value>,
        span: Span,
    ) -> Result<JoinHandle<Result<(), RuntimeError>>, RuntimeError> {
        let Some(name) = self.core.function(callee).map(|f| f.name.clone()) else {
            return Err(fail(
                codes::UNDEFINED_FUNCTION,
                span,
                "this task's function is not in the program",
            ));
        };
        let core = Arc::clone(&self.core);
        let max_depth = self.max_depth;
        let iterations_left = self.iterations_left;
        let task_name = name.clone();

        std::thread::Builder::new()
            .name(name.clone())
            // The same reservation the main evaluation thread gets, for the same reason:
            // the depth limit has to be reached before the stack is, or a deep recursion
            // inside a task crashes the process with no diagnostic at all.
            .stack_size(stack_for(max_depth))
            .spawn(move || {
                let mut task = Self {
                    core: Arc::clone(&core),
                    pending: Vec::new(),
                    depth: 0,
                    max_depth,
                    iterations_left,
                    calls: 0,
                    // A task has no arguments of its own: `arg(i)` is the program's
                    // command line, and a task is not a program.
                    args: Vec::new(),
                };
                // Looked up in the program this task shares, which is the one the callee
                // was resolved in. Reported rather than indexed, because a panic in a task
                // loses every other task's diagnostic along with its own.
                let Some(f) = core.function(callee) else {
                    return Err(RuntimeError {
                        diagnostic: Box::new(
                            Diagnostic::error(
                                codes::INTERPRETER_UNAVAILABLE,
                                Span::synthetic(),
                                format!("a task was started for `{task_name}`, which is not there"),
                            )
                            .help("this is a bug in the interpreter"),
                        ),
                    });
                };
                task.call_body(f, frame(f, args)).map(|_| ())
            })
            .inspect(|_| {
                SCHED_SPAWNED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            })
            .map_err(|e| RuntimeError {
                diagnostic: Box::new(
                    Diagnostic::error(
                        codes::INTERPRETER_UNAVAILABLE,
                        span,
                        format!("could not start a task for `{name}`: {e}"),
                    )
                    .help(
                        "each task is an operating-system thread; this is a limit of the \
                         machine, not of the program",
                    ),
                ),
            })
    }

    /// Wait for every task of one scope.
    ///
    /// Every handle is joined even after one has failed. Returning at the first failure
    /// would leave the rest running, and a scope that does not contain its tasks is not
    /// a scope.
    ///
    /// A task that fails ends the whole program, because that is what the specification
    /// promises — tasks do not have private failure modes. The first failure is reported
    /// and the rest are treated as consequences of it.
    pub(super) fn join_all(
        tasks: Vec<JoinHandle<Result<(), RuntimeError>>>,
    ) -> Result<(), RuntimeError> {
        SCHED_JOINED.fetch_add(
            u64::try_from(tasks.len()).unwrap_or(u64::MAX),
            std::sync::atomic::Ordering::Relaxed,
        );
        let mut first: Option<RuntimeError> = None;
        for handle in tasks {
            let outcome = match handle.join() {
                Ok(outcome) => outcome,
                // A panic is an interpreter bug, not a program error, but it still has to
                // come out as a diagnostic: resuming the unwind here would lose the other
                // tasks still waiting to be joined.
                Err(_) => Err(RuntimeError {
                    diagnostic: Box::new(
                        Diagnostic::error(
                            codes::INTERPRETER_UNAVAILABLE,
                            nazm_span::Span::synthetic(),
                            "a task ended abnormally",
                        )
                        .help("this is a bug in the interpreter"),
                    ),
                }),
            };
            if let Err(e) = outcome {
                first.get_or_insert(e);
            }
        }
        match first {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }
}
