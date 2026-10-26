//! `ob --connect`: this terminal drawing an Obelus that runs on another
//! machine.
//!
//! The terminal is taken over the way `ob` takes it over, and what it can
//! do is asked the same way -- pictures, the keyboard -- and said to the
//! Obelus there, which draws for a terminal because of it. What arrives is
//! cells, kept here in a screen of the terminal's own size and drawn at the
//! end of every frame; what the reader does goes back as `ob`'s loop would
//! have heard it (`obelus_front::client`).

use std::{
    process::ExitCode,
    sync::{Arc, mpsc::Sender},
};

use anyhow::{Context as _, Result};
use obelus_app::event::Event;
use obelus_front::{Closed, Measured, Showing, Update, link::Front};
use ratatui::{buffer::Buffer, layout::Rect};

/// What the terminal's own thread is told: what arrived, and when it ends.
enum Told {
    /// Something on the screen.
    Update(Box<Update>),
    /// The terminal is this size now.
    Resized,
    /// There is no more connection, and why.
    Ended(Result<()>),
}

/// Where what arrives is handed to the terminal's thread.
struct Terminal(Sender<Told>);

impl Showing for Terminal {
    fn show(&self, update: Update) -> Result<(), Closed> {
        self.0
            .send(Told::Update(Box::new(update)))
            .map_err(|_| Closed)
    }

    // The terminal is drawn at the end of a frame, which arrives like
    // everything else.
    fn wake(&self) {}
}

/// Draws an Obelus on another machine on this terminal, until it ends or
/// the reader leaves.
///
/// # Errors
///
/// Where there is no key, no terminal, or the server would not let it in.
pub(crate) fn connect(at: String, key: Option<String>, path: Option<String>) -> Result<ExitCode> {
    let key = obelus_front::client::key(&at, key.or_else(|| std::env::var("OBELUS_KEY").ok()))?;
    // Asked of this terminal before it is taken over, the way `ob` asks.
    let images = obelus_ui::image::Images::detect(obelus_app::event::over_a_network());
    let pictures = images
        .of_the_terminal()
        .and_then(|(protocol, cell)| Some((pictures(protocol)?, cell)));
    let keyboard = crate::ctrl_enter_arrives();

    let mut terminal = ratatui::try_init()?;
    let mouse = crate::enable_mouse();
    if !crate::enable_paste() {
        tracing::warn!("pasting will arrive a character at a time");
    }
    let protocol = crate::enable_keyboard();

    let size = terminal.size()?;
    let measured = Arc::new(Measured::default());
    measured.resized(size.width, size.height, 0, 0);

    let (told, telling) = std::sync::mpsc::channel::<Told>();
    // What the terminal says, measured first where it is a resize: the
    // size goes with the event, and the screen here is remade at it.
    let (heard, hearing) = obelus_app::event::channel();
    obelus_app::event::spawn_terminal_reader(heard);
    let (done_to, done) = std::sync::mpsc::channel::<Event>();
    let (sizing, resized) = (Arc::clone(&measured), told.clone());
    std::thread::Builder::new()
        .name("obelus terminal to server".to_string())
        .spawn(move || {
            while let Ok(event) = hearing.recv() {
                if matches!(event, Event::Resize)
                    && let Ok((columns, rows)) = crossterm::terminal::size()
                {
                    sizing.resized(columns, rows, 0, 0);
                    let _ = resized.send(Told::Resized);
                }
                if done_to.send(event).is_err() {
                    return;
                }
            }
        })
        .context("a thread to hear the terminal on")?;

    let ending = told.clone();
    obelus_front::client::start(
        obelus_front::client::Afar {
            at,
            key,
            path,
            build: env!("OBELUS_BUILD"),
        },
        Front::Terminal { pictures, keyboard },
        Arc::new(Terminal(told)),
        Arc::clone(&measured),
        done,
        Box::new(move |ended| {
            let _ = ending.send(Told::Ended(ended));
        }),
    )?;

    let outcome = draw(&mut terminal, &measured, &telling);
    crate::give_back(mouse, protocol);
    outcome.map(|()| ExitCode::SUCCESS)
}

/// Keeps the screen as what arrives says it is, and draws it at the end of
/// every frame, until there is no more connection.
fn draw<B>(
    terminal: &mut ratatui::Terminal<B>,
    measured: &Measured,
    telling: &std::sync::mpsc::Receiver<Told>,
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
            Told::Ended(ended) => return ended,
        }
    }
    Ok(())
}

/// How this terminal draws a picture, in the words that cross -- or
/// nothing for the way that is not one (`Halfblocks`).
const fn pictures(
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
        told.send(Told::Ended(Ok(()))).expect("told");
        draw(&mut terminal, &measured, &telling).expect("drawn");
        let buffer = terminal.backend().buffer();
        assert_eq!(buffer[(1, 1)].symbol(), "a", "what arrived was not drawn");
        assert_eq!(
            terminal.get_cursor_position().expect("a caret"),
            ratatui::layout::Position::new(2, 1)
        );
    }
}
