//! That work which waits is not held up by work that does not.
//!
//! The one claim in this crate's doc that is about behaviour rather than
//! about shape: a language server's answer is parsed on the runtime, and a
//! parse holds the worker it is on for as long as it takes. What that must
//! not do is hold up the agent, the clock, or the next server.

use std::{sync::mpsc, time::Duration};

/// How long the held worker is held for.
///
/// Long enough that a machine under load cannot close the gap by accident
/// and short enough not to be felt in the suite.
const HELD: Duration = Duration::from_millis(300);

/// A worker held by one answer does not hold up the others.
///
/// Deliberate break: `Builder::new_multi_thread().worker_threads(1)` in
/// `handle`, which is the one-thread runtime the doc is arguing against.
/// The second task then cannot be polled until the first lets its worker
/// go, so what comes back first is the parse and this fails. (A
/// current-thread runtime breaks it further: nothing is polled at all
/// without somebody blocking on it, and the test times out rather than
/// failing.)
#[test]
fn a_worker_held_by_one_answer_does_not_hold_up_the_rest() {
    // The worker count is tokio's own, one per core, so on a single-core
    // machine there is no second worker and nothing here to show. Skipped
    // rather than asserted: the claim is about having somewhere else to
    // run, and a machine with nowhere else is not a machine it is wrong
    // about.
    if std::thread::available_parallelism().is_ok_and(|cores| cores.get() < 2) {
        return;
    }

    let handle = obelus_runtime::handle();
    let (tell, hear) = mpsc::channel();

    // What a server's answer is: parsed where it is read, holding its
    // worker for as long as the parse takes. `sleep` rather than a real
    // parse because what is under test is the holding, not the parsing.
    let (began, heard_it_began) = mpsc::channel();
    let parsing = tell.clone();
    handle.spawn(async move {
        // Said before the worker is held, so the task below is spawned
        // into a runtime that is already holding one. Without this the
        // order the two are polled in is the runtime's to choose, and a
        // one-worker runtime could run the second first and pass.
        let _ = began.send(());
        std::thread::sleep(HELD);
        let _ = parsing.send("the parse");
    });
    heard_it_began
        .recv_timeout(HELD * 4)
        .expect("the parse never started");

    // And anything else that was waiting on this runtime.
    handle.spawn(async move {
        let _ = tell.send("the rest");
    });

    let first = hear
        .recv_timeout(HELD * 4)
        .expect("neither of them came back");
    assert_eq!(
        first, "the rest",
        "the rest of the work waited for the parse to finish"
    );
}
