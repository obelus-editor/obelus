//! This terminal drawing an Obelus that runs on another machine: `ob
//! --connect`, and `connect-to-server` from `ob`.
//!
//! The terminal is taken over the way `ob` takes it over, and what it can
//! do is asked the same way -- pictures, the keyboard -- and said to the
//! Obelus there, which draws for a terminal because of it. What arrives is
//! cells, kept here in a screen of the terminal's own size and drawn at the
//! end of every frame; what the reader does goes back as `ob`'s loop would
//! have heard it (`obelus_front::client`).
//!
//! **A terminal can change which Obelus it draws**, the way a window can:
//! [`drawing`] goes from this machine's to one on another and back, on the
//! one terminal and the one thread that reads it.

use std::sync::mpsc::{Receiver, Sender};

use anyhow::{Context as _, Result};
use obelus_app::{
    app::{self, App},
    event::Event,
};
use obelus_front::{
    Closed, Measured, Showing, Update,
    client::{Afar, Left, Next},
    link::Front,
};
use ratatui::{buffer::Buffer, layout::Rect};

/// What a terminal draws: Obelus on this machine, or one on another.
pub(crate) enum Start {
    /// This process's own.
    Here(Box<App>),
    /// One at the other end of a connection.
    Afar(Afar),
}

/// What the terminal was found able to do, and what it is.
pub(crate) struct Terminal<'a> {
    /// Whether the wheel is reported.
    pub(crate) mouse: bool,
    /// Whether a paste arrives in one go.
    pub(crate) paste: bool,
    /// Whether `ctrl+enter` arrives as itself.
    pub(crate) ctrl_enter: bool,
    /// What it draws pictures with, which the first of this machine's
    /// Obelus is handed as asked and every one after is told.
    pub(crate) images: Option<obelus_ui::image::Images>,
    /// What it is, to an Obelus on another machine.
    pub(crate) front: Front,
    /// What it hears.
    pub(crate) events: &'a Receiver<Event>,
    /// A way into what it hears.
    pub(crate) nudge: &'a Sender<Event>,
}

/// Draws `start` on the terminal, and whichever Obelus the reader goes to
/// from it, until the reader leaves -- leaving the last of this machine's
/// in `kept`, for the caller to let go of once the terminal is back.
///
/// # Errors
///
/// Where an Obelus would not start or would not draw, or a server would
/// not let the terminal in.
pub(crate) fn drawing(
    start: Start,
    terminal: &mut ratatui::DefaultTerminal,
    mut found: Terminal<'_>,
    kept: &mut Option<Box<App>>,
) -> Result<()> {
    let told = found
        .images
        .as_ref()
        .and_then(obelus_ui::image::Images::of_the_terminal);
    let mut next = start;
    // What waits for a machine that went, while the page saying so is up.
    let mut waiting: Option<std::sync::Arc<std::sync::atomic::AtomicBool>> = None;
    loop {
        next = match next {
            Start::Here(mut app) => {
                if !found.mouse {
                    // The one thing that goes wrong here and stays wrong:
                    // the wheel keeps sending arrow keys, and nothing on
                    // screen would otherwise say why.
                    app.amiss("The wheel will not be reported, so it moves the cursor");
                }
                if !found.paste {
                    app.amiss("Pasting will arrive a character at a time, not in one go");
                }
                let images = found.images.take().unwrap_or_else(|| match told {
                    Some((protocol, cell)) => obelus_ui::image::Images::told(protocol, cell),
                    None => obelus_ui::image::Images::none(),
                });
                app.use_images(images);
                app.ctrl_enter_arrives(found.ctrl_enter);
                app.start(found.nudge.clone());
                let ran = app::run(terminal, &mut app, found.events);
                if let Some(waiting) = waiting.take() {
                    waiting.store(true, std::sync::atomic::Ordering::Release);
                }
                // What was drawn last is handed out rather than let go of
                // here: letting go of it stops its servers, each of which
                // may take a moment, and the terminal is given back goes first.
                let leaving =
                    ran.and_then(|()| obelus_front::client::from_here(&app, env!("OBELUS_BUILD")));
                let afar = match leaving {
                    Ok(Some(afar)) => afar,
                    left => {
                        *kept = Some(app);
                        return left.map(|_| ());
                    }
                };
                // Let go of before the other is drawn: its servers, its
                // agent and its claims go with it.
                drop(app);
                Start::Afar(afar)
            }
            Start::Afar(afar) => {
                // Not a screen with nothing on it while a machine that is
                // not there is waited on: the page saying so, at once.
                if let Some(page) = obelus_front::client::first(&afar, found.nudge)? {
                    match page {
                        Next::Done => return Ok(()),
                        Next::Here(app, wait) => {
                            waiting = wait;
                            next = Start::Here(app);
                            terminal.clear()?;
                            continue;
                        }
                    }
                }
                let left = afar_drawn(&afar, terminal, &found)?;
                match obelus_front::client::after(&afar, left, found.nudge)? {
                    Next::Done => return Ok(()),
                    Next::Here(app, wait) => {
                        waiting = wait;
                        Start::Here(app)
                    }
                }
            }
        };
        terminal.clear()?;
    }
}

/// What the drawing thread is told: what arrived, and when it ends.
enum Told {
    /// Something on the screen.
    Update(Box<Update>),
    /// The terminal is this size now.
    Resized,
    /// There is no more connection.
    Ended,
}

/// Where what arrives is handed to the drawing thread.
struct Arriving(Sender<Told>);

impl Showing for Arriving {
    fn show(&self, update: Update) -> Result<(), Closed> {
        self.0
            .send(Told::Update(Box::new(update)))
            .map_err(|_| Closed)
    }

    // The terminal is drawn at the end of a frame, which arrives like
    // everything else.
    fn wake(&self) {}
}

/// Draws the Obelus at `afar` until there is no more connection, and says
/// why.
fn afar_drawn(
    afar: &Afar,
    terminal: &mut ratatui::DefaultTerminal,
    found: &Terminal<'_>,
) -> Result<Left> {
    let size = terminal.size()?;
    let measured = Measured::default();
    measured.resized(size.width, size.height, 0, 0);
    let (told, telling) = std::sync::mpsc::channel::<Told>();
    let sizing = &measured;
    std::thread::scope(|scope| {
        // The channel moves to the drawing thread, which is the one that
        // reads it.
        let drawn = std::thread::Builder::new()
            .name("obelus terminal".to_string())
            .spawn_scoped(scope, move || draw(terminal, sizing, &telling))
            .context("a thread to draw on")?;
        let resized = told.clone();
        let left = obelus_front::client::run(
            afar,
            &found.front,
            &Arriving(told.clone()),
            &measured,
            found.events,
            found.nudge,
            // Measured as it is heard, so the size goes with the resize and
            // the screen here is remade at it.
            &|event| {
                if matches!(event, Event::Resize)
                    && let Ok((columns, rows)) = crossterm::terminal::size()
                {
                    measured.resized(columns, rows, 0, 0);
                    let _ = resized.send(Told::Resized);
                }
            },
        );
        let _ = told.send(Told::Ended);
        let drew = drawn
            .join()
            .unwrap_or_else(|_| Err(anyhow::anyhow!("the drawing stopped")));
        drew?;
        left
    })
}

/// Keeps the screen as what arrives says it is, and draws it at the end of
/// every frame, until there is no more connection.
fn draw<B>(
    terminal: &mut ratatui::Terminal<B>,
    measured: &Measured,
    telling: &Receiver<Told>,
) -> Result<()>
where
    B: ratatui::backend::Backend,
    B::Error: std::error::Error + Send + Sync + 'static,
{
    let area = |measured: &Measured| {
        let size = measured.size();
        Rect::new(0, 0, size.width, size.height)
    };
    let mut screen = Buffer::empty(area(measured));
    let mut caret = None;
    while let Ok(told) = telling.recv() {
        match told {
            Told::Update(update) => match *update {
                Update::Cell { x, y, cell } => {
                    if let Some(at) = screen.cell_mut((x, y)) {
                        *at = *cell;
                    }
                }
                Update::Cleared => screen.reset(),
                Update::Caret(at) => caret = at,
                Update::Frame => {
                    terminal.draw(|frame| {
                        let into = frame.buffer_mut();
                        for y in 0..screen.area.height {
                            for x in 0..screen.area.width {
                                if let (Some(to), Some(from)) =
                                    (into.cell_mut((x, y)), screen.cell((x, y)))
                                {
                                    *to = from.clone();
                                }
                            }
                        }
                        if let Some(at) = caret {
                            frame.set_cursor_position(at);
                        }
                    })?;
                }
                // What only a window draws, which a terminal is not sent
                // and has no way to draw.
                _ => {}
            },
            Told::Resized => screen = Buffer::empty(area(measured)),
            Told::Ended => return Ok(()),
        }
    }
    Ok(())
}

/// How this terminal draws a picture, in the words that cross -- or
/// nothing for the way that is not one (`Halfblocks`).
pub(crate) const fn pictures(
    protocol: ratatui_image::picker::ProtocolType,
) -> Option<obelus_front::link::Pictures> {
    use obelus_front::link::Pictures;
    use ratatui_image::picker::ProtocolType;
    match protocol {
        ProtocolType::Kitty => Some(Pictures::Kitty),
        ProtocolType::Iterm2 => Some(Pictures::Iterm2),
        ProtocolType::Sixel => Some(Pictures::Sixel),
        ProtocolType::Halfblocks => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What arrives is drawn on the terminal at the end of a frame, the
    /// caret where it was said to be, and the end of the connection ends
    /// the drawing.
    ///
    /// Broken deliberately by drawing the frame without what arrived (the
    /// copy into the frame left out): the terminal stayed blank.
    #[test]
    fn what_arrives_is_drawn_at_the_end_of_a_frame() {
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(4, 2)).expect("a terminal");
        let measured = Measured::default();
        measured.resized(4, 2, 0, 0);
        let (told, telling) = std::sync::mpsc::channel();
        let mut cell = ratatui::buffer::Cell::default();
        cell.set_symbol("a");
        for update in [
            Update::Cell {
                x: 1,
                y: 1,
                cell: Box::new(cell),
            },
            Update::Caret(Some(ratatui::layout::Position::new(2, 1))),
            Update::Frame,
        ] {
            told.send(Told::Update(Box::new(update))).expect("told");
        }
        told.send(Told::Ended).expect("told");
        draw(&mut terminal, &measured, &telling).expect("drawn");
        let buffer = terminal.backend().buffer();
        assert_eq!(buffer[(1, 1)].symbol(), "a", "what arrived was not drawn");
        assert_eq!(
            terminal.get_cursor_position().expect("a caret"),
            ratatui::layout::Position::new(2, 1)
        );
    }
}
