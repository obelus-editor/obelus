//! The machine's pool of build jobs, as the settings put Obelus in it and
//! take it out.
//!
//! A binary of its own because what a program is told about the pool is
//! process-wide: a test beside the agents' would be joining and leaving
//! while one of them started an agent and asked what it had been told. One
//! test, so that nothing in here takes turns either.

mod support;

use obelus_app::app::App;

/// How many tokens the pool's record says are going round.
///
/// Read out of the record rather than worked out the way Obelus works it
/// out: a test that worked it out would be asking the rule under test what
/// the answer is.
fn tokens() -> Option<usize> {
    let record = obelus_logging::state_directory()
        .expect("somewhere to keep state")
        .join("jobs")
        .join("pool");
    std::fs::read_to_string(record)
        .ok()?
        .lines()
        .nth(1)?
        .parse()
        .ok()
}

fn with(share: &str) -> obelus_config::Config {
    obelus_config::Config {
        build_jobs: share.to_string(),
        ..obelus_config::Config::default()
    }
}

/// The setting puts this Obelus in the pool, sizes it as a share of the
/// machine, and `unlimited` takes it out again -- so that a program started
/// afterwards is told nothing and sizes itself.
///
/// Broken deliberately three ways. Dropping `settle_the_pool` from
/// `apply_config` leaves Obelus out of any pool. Resizing only on the way in
/// leaves `all` at half the machine. And keeping the pool on `unlimited`
/// goes on telling every program where it is.
#[test]
fn the_setting_puts_obelus_in_the_pool_and_takes_it_out() {
    let (sender, _events) = std::sync::mpsc::channel();
    let mut app = App::new(Vec::new());
    app.events_for_test(sender);
    support::lay_out(&mut app, 60, 12);
    let cpus = std::thread::available_parallelism().map_or(1, std::num::NonZero::get);

    app.configure(with("half"), vec!["build_jobs"]);
    assert!(
        obelus_jobs::lent()
            .iter()
            .any(|(name, value)| *name == "CARGO_MAKEFLAGS" && value.contains("--jobserver-auth=")),
        "half the machine is not a pool: {:?}",
        obelus_jobs::lent()
    );
    assert_eq!(tokens(), Some((cpus / 2).max(1) - 1));

    app.configure(with("all"), vec!["build_jobs"]);
    assert_eq!(tokens(), Some(cpus - 1), "the pool was not resized");

    app.configure(with("unlimited"), vec!["build_jobs"]);
    assert!(
        obelus_jobs::lent().is_empty(),
        "a program started now would still be told about a pool: {:?}",
        obelus_jobs::lent()
    );
    assert_eq!(tokens(), None, "the pool outlived the only Obelus in it");

    // And the page says what each share comes to here, in the reader's
    // words rather than the file's.
    assert_eq!(
        app.called("build_jobs", "all").as_deref(),
        Some(format!("All {cpus} CPUs").as_str())
    );
    assert_eq!(
        app.called("build_jobs", "unlimited").as_deref(),
        Some("Unlimited")
    );
}
