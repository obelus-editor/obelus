//! What an agent is started with, where the reader and the pool of build
//! jobs name the same variable.
//!
//! A binary of its own for the reason `jobs` is: the pool is process-wide,
//! and a test beside the agents' would be joining and leaving it while one
//! of them started an agent. One test, so that nothing in here takes turns.

mod support;

use std::{
    path::Path,
    time::{Duration, Instant},
};

use obelus_app::app::App;

/// What the reader adds to an agent's environment has the last word over
/// what Obelus lends it for the pool.
///
/// The reader named it on purpose -- a build of their own that wants its
/// own `-j` -- and Obelus's is a default it put there without asking.
///
/// Broken deliberately by handing the agent the reader's variables before
/// the pool's in `link::start`: the agent was told the pool's.
#[test]
fn what_the_reader_adds_has_the_last_word_over_the_pool() {
    let scratch = support::Scratch::new("started-with");
    let settings = scratch.join("config.toml");
    std::fs::write(
        &settings,
        "[environment.fake]\nOBELUS_FAKE_WORD = \"here\"\nCARGO_MAKEFLAGS = \"the-readers\"\n",
    )
    .expect("the settings");
    let log = scratch.join("asked.log");

    let (sender, events) = std::sync::mpsc::channel();
    let mut app = App::new(Vec::new());
    support::lay_out(&mut app, 60, 12);
    app.config_file_for_test(settings);
    app.start(sender);
    assert!(
        obelus_jobs::lent()
            .iter()
            .any(|(name, _)| *name == "CARGO_MAKEFLAGS"),
        "there is no pool for the reader's word to be put over"
    );

    app.talk_to(
        "fake",
        Path::new(support::sh()),
        &[
            "tests/fixtures/fake-agent.sh".to_string(),
            format!("log={}", log.display()),
        ],
    );
    let deadline = Instant::now() + Duration::from_secs(180);
    let said = loop {
        let said = std::fs::read_to_string(&log).unwrap_or_default();
        if said.contains("pool ") {
            break said;
        }
        assert!(
            Instant::now() < deadline,
            "the agent never said what it was started with"
        );
        if let Ok(event) = events.recv_timeout(Duration::from_millis(20)) {
            app.handle(event);
        }
    };
    assert!(
        said.contains("pool the-readers"),
        "the pool's word took the place of the reader's:\n{said}"
    );

    // And out of the pool again, so that nothing on this machine is told
    // about one this test made.
    app.configure(
        obelus_config::Config {
            build_jobs: "unlimited".to_string(),
            ..obelus_config::Config::default()
        },
        vec!["build_jobs"],
    );
}
