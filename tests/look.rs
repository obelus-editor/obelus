mod support;
use crossterm::event::KeyCode;
use obelus::{app::App, buffer::Buffer};

#[test]
fn look() {
    let path = std::path::PathBuf::from("/home/sunli/work/obelus-map-demo/src/lib.rs");
    let open = || {
        let mut app = App::new(vec![Buffer::open(&path).expect("o")]);
        app.working_directory_for_test("/home/sunli/work/obelus-map-demo".into());
        support::lay_out(&mut app, 70, 16);
        app
    };
    let goto = |app: &mut App, line: usize| {
        for _ in 0..line {
            support::press(app, KeyCode::Down);
        }
    };
    let show = |app: &mut App, what: &str| {
        println!("--- {what} ---");
        for row in support::text_block(&support::render(app, 70, 16)).lines() {
            println!("{row}");
        }
    };

    // The caret on the line just after the deletion at step_15 (line 97,
    // one-based), which is where the margin shows the seam.
    let mut app = open();
    goto(&mut app, 95);
    println!(
        "caret at {:?}",
        app.current_buffer().map(|b| b.cursor().line.get())
    );
    show(&mut app, "at line 96, before anything");
    support::press_alt(&mut app, 'f');
    show(&mut app, "after alt+f");

    let mut app = open();
    goto(&mut app, 95);
    println!(
        "caret at {:?}",
        app.current_buffer().map(|b| b.cursor().line.get())
    );
    support::press_alt(&mut app, 'd');
    show(&mut app, "after alt+d (hunk open)");
    support::press_alt(&mut app, 'f');
    show(&mut app, "then alt+f");
}
