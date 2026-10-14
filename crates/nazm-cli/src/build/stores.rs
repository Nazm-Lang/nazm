//! Which object store a unit is filed in (R1.5, D12, `docs/architecture.md` §7.113).

use nazm_cache::ObjectStore;

/// The two object stores a build files into: the project's `.nazm/native/`, and the user cache,
/// which holds the units every project shares — the runtime's and the standard library's. Either
/// is `None` where the build keeps no objects there.
#[derive(Debug, Clone, Default)]
pub(crate) struct Stores {
    pub(crate) project: Option<ObjectStore>,
    pub(crate) user: Option<ObjectStore>,
}

impl Stores {
    /// The store a unit is filed in: a shared unit in the user cache when there is one, and in the
    /// project's otherwise; every other unit in the project's. The key is the same in either.
    pub(crate) fn for_unit(&self, shared: bool) -> Option<&ObjectStore> {
        if shared {
            self.user.as_ref().or(self.project.as_ref())
        } else {
            self.project.as_ref()
        }
    }
}
