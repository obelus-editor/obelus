use obelus::{
    app::App,
    buffer::Buffer,
    command::{Command, dispatch},
};
#[path = "support/mod.rs"]
mod support;
#[test]
fn probe() {
    let root = std::path::PathBuf::from("/home/sunli/Work/obelus");
    let mut app = App::new(vec![
        Buffer::open(&root.join("src/ui/reading.rs")).expect("open"),
    ]);
    app.working_directory_for_test(root);
    // Exactly the reader's own config.
    app.configure(obelus::config::Config {
        blame: false,
        ..Default::default()
    });
    let events = support::drive(&mut app);
    support::lay_out(&mut app, 84, 12);
    support::render(&mut app, 84, 12);
    for _ in 0..20 {
        support::press(&mut app, crossterm::event::KeyCode::Down);
    }

    for press in 1..=2 {
        dispatch::dispatch(&mut app, Command::HistoryLine);
        let dump = support::render(&mut app, 84, 12);
        println!(
            "press {press}: {:?}",
            support::text_block(&dump)
                .lines()
                .next_back()
                .map(str::trim)
        );
        // Whatever the walk sends while the reader sits there.
        let at = std::time::Instant::now();
        while at.elapsed() < std::time::Duration::from_millis(600) {
            match events.recv_timeout(std::time::Duration::from_millis(200)) {
                Ok(e) => app.handle(e),
                Err(_) => break,
            }
        }
    }
}
