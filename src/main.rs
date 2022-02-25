use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use std::{
    error, io,
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};
use tui::{
    backend::CrosstermBackend,
    layout::Layout,
    layout::{Constraint, Direction},
    widgets::{Block, Borders},
    Terminal,
};

enum GameEvent<I> {
    Input(I),
    Tick,
}

fn main() -> Result<(), Box<dyn error::Error>> {
    enable_raw_mode().expect("Can enter raw mode");

    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture).expect("Can capture mouse input");
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).expect("Terminal is instantiated");

    let (tx, rx) = mpsc::channel();
    let tick_rate = Duration::from_millis(250);
    thread::spawn(move || -> ! {
        let mut last_tick = Instant::now();
        loop {
            let timeout = tick_rate
                .checked_sub(last_tick.elapsed())
                .unwrap_or_else(|| Duration::from_secs(0));

            if event::poll(timeout).expect("Events are properly polled") {
                if let Event::Key(key) = event::read().expect("Key inputs are detected") {
                    tx.send(GameEvent::Input(key))
                        .expect("GameEvent inputs can be sent to the consumer");
                }
            }

            if last_tick.elapsed() >= tick_rate && tx.send(GameEvent::Tick).is_ok() {
                last_tick = Instant::now()
            }
        }
    });

    let mut exit = false;

    loop {
        terminal.draw(|f| {
            let chunks = Layout::default()
                .direction(Direction::Horizontal)
                .margin(2)
                .constraints([Constraint::Percentage(85), Constraint::Percentage(15)].as_ref())
                .split(f.size());
            let block = Block::default().borders(Borders::ALL);
            f.render_widget(block, chunks[0]);
            let block = Block::default().borders(Borders::ALL);
            f.render_widget(block, chunks[1]);

            match rx.recv().unwrap() {
                GameEvent::Input(event) => {
                    if let KeyCode::Char('q') = event.code {
                        exit = true
                    }
                }
                GameEvent::Tick => {}
            }
        })?;

        if exit {
            break;
        }
    }

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    Ok(())
}
