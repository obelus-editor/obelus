//! The one runtime Obelus's waiting is done on.
//!
//! Obelus's loop is a thread blocked on a channel, and that does not
//! change: one owner of the application's state, and no `.await` between a
//! key arriving and the screen it produced. What is here is for the other
//! side -- the work that is *waiting* rather than working. A language
//! server's pipe, an agent's connection, the clock behind an animation, a
//! download. None of those needs a thread; each of them needed one, because
//! there was nowhere else to put them.
//!
//! One runtime for all of it, rather than one per thing that wanted async.
//! There were two before this -- the agent's connection built one, and the
//! tools Obelus offers an agent built another -- each on a thread whose
//! whole job was to own it.
//!
//! Multi-threaded, and this is the reason: a language server's answer is
//! parsed where it is read, and semantic tokens for a two-thousand-line
//! file is four hundred kilobytes of JSON that takes twenty milliseconds to
//! parse. On one thread that stalls every other thing waiting on this
//! runtime -- the agent, the clock, the other servers. It is measured, not
//! guessed: `tests/runtime.rs`.
//!
//! The worker count is tokio's own, which is one per core. A parked worker
//! waits in the same epoll every other thread waits in and costs a stack
//! that is never touched; counting them against the threads this replaces
//! compares numbers rather than costs.

pub mod cancel;

use std::sync::OnceLock;

use tokio::runtime::{Handle, Runtime};

/// The runtime, built the first time anything asks for it.
static RUNTIME: OnceLock<Runtime> = OnceLock::new();

/// Where to spawn work that waits.
///
/// Built on first use so that a session which never opens a file, starts a
/// server or talks to an agent never builds one -- and so that a test can
/// ask for it without arranging anything, which is what keeps the code
/// under test the same code that runs.
///
/// Panics only if the runtime cannot be built at all, which means the
/// process has no threads to give: the same footing as the thread the
/// terminal is read on, which is also expected rather than handled.
pub fn handle() -> &'static Handle {
    RUNTIME
        .get_or_init(|| Runtime::new().expect("building the runtime Obelus waits on"))
        .handle()
}
