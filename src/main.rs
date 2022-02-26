use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use std::{
    error, io,
    sync::mpsc::{self, Sender},
    thread,
    time::{Duration, Instant},
};
use tui::{
    backend::{Backend, CrosstermBackend},
    layout::Layout,
    layout::{Alignment, Constraint, Direction},
    text::{Span, Spans},
    widgets::{Block, Borders, Paragraph},
    Terminal,
};

enum GameEvent<T> {
    Input(T),
    Tick,
}

struct App {}

impl App {
    fn new() -> App {
        App {}
    }
    fn on_tick(&mut self) {}
}

fn main() -> Result<(), Box<dyn error::Error>> {
    enable_raw_mode().expect("Can enter raw mode");

    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let app = App::new();
    let res = run_app(&mut terminal, app, Duration::from_millis(250));

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    if let Err(err) = res {
        println!("{:?}", err);
    }

    Ok(())
}

fn run_app<B: Backend>(
    terminal: &mut Terminal<B>,
    mut app: App,
    tick_rate: Duration,
) -> Result<(), Box<dyn error::Error>> {
    let (tx, rx) = mpsc::channel();

    thread::spawn(move || handle_game_events(tx, tick_rate));

    let mut exit = false;

    loop {
        terminal
            .draw(|f| {
                let chunks = Layout::default()
                    .direction(Direction::Horizontal)
                    .margin(2)
                    .constraints([Constraint::Percentage(85), Constraint::Percentage(15)].as_ref())
                    .split(f.size());
                let block = Block::default().borders(Borders::ALL);
                f.render_widget(block, chunks[0]);

                let instructions = Paragraph::new(vec![
                    Spans::from(vec![Span::raw(" [Left click]")]),
                    Spans::from(vec![Span::raw(" Add cell")]),
                    Spans::from(vec![Span::raw("")]),
                    Spans::from(vec![Span::raw(" [Right click]")]),
                    Spans::from(vec![Span::raw(" Delete cell")]),
                    Spans::from(vec![Span::raw("")]),
                    Spans::from(vec![Span::raw(" [Enter]")]),
                    Spans::from(vec![Span::raw(" Run")]),
                    Spans::from(vec![Span::raw("")]),
                    Spans::from(vec![Span::raw(" [-, +]")]),
                    Spans::from(vec![Span::raw(" Tick rate = 250")]),
                    Spans::from(vec![Span::raw("")]),
                    Spans::from(vec![Span::raw(" [q]")]),
                    Spans::from(vec![Span::raw(" Exit")]),
                ])
                .alignment(Alignment::Left)
                .block(Block::default().borders(Borders::ALL).title("Controls"));
                f.render_widget(instructions, chunks[1]);

                match rx.recv().unwrap() {
                    GameEvent::Input(event) => {
                        if let KeyCode::Char('q') = event.code {
                            exit = true
                        }
                    }
                    GameEvent::Tick => app.on_tick(),
                }
            })
            .expect("Can draw to terminal");

        if exit {
            break;
        }
    }
    Ok(())
}

fn handle_game_events(tx: Sender<GameEvent<event::KeyEvent>>, tick_rate: Duration) {
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
}
