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
    style::Color,
    symbols::Marker,
    text::{Span, Spans},
    widgets::{
        canvas::{Canvas, Points},
        Block, Borders, Paragraph,
    },
    Terminal,
};

enum GameEvent<T> {
    Input(T),
    Tick,
    Resize(usize, usize),
}

struct App {
    state: Vec<Vec<bool>>,
}

impl App {
    fn new() -> App {
        App {
            // Kind of sucks how the app starts in an invalid state that shouldn't exist
            // but we can't reserve any memory until the UI is initialised and we know our dimensions
            state: Vec::new(),
        }
    }

    fn resize(&mut self, w: usize, h: usize) {
        self.state = vec![vec![false; w]; h];
    }

    fn on_tick(&mut self) {}

    fn cells(&self) -> Vec<(f64, f64)> {
        // I chose not to implement Iterator as it would require tracking state and everything.
        // No fancy uses here this is just for the UI to know whick blocks to paint white
        if self.state.is_empty() {
            // Covers the initial struct state before first UI call, just in case
            return Vec::<(f64, f64)>::new();
        }
        self.state
            .iter()
            .enumerate()
            .flat_map(move |(row_i, row)| {
                row.iter()
                    .enumerate()
                    .filter(|(_, cell)| **cell)
                    .map(move |(cell_i, _)| (cell_i as f64, row_i as f64))
            })
            .collect()
    }
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
    mut tick_rate: Duration,
) -> Result<(), Box<dyn error::Error>> {
    let (tx, rx) = mpsc::channel();

    let thread_tx = tx.clone();
    thread::spawn(move || handle_game_events(thread_tx, tick_rate));

    let mut current_game_dimensions: Option<(u16, u16)> = None;

    let mut game_running = false;
    let mut exit = false;

    loop {
        terminal.draw(|f| {
            let chunks = Layout::default() // Basic layout
                .direction(Direction::Horizontal)
                .margin(2)
                .constraints([Constraint::Percentage(85), Constraint::Percentage(15)].as_ref())
                .split(f.size());

            let new_game_dimensions = (chunks[0].width, chunks[0].height); // Check for resize in the UI side
            if Some(new_game_dimensions) != current_game_dimensions
                && tx
                    .send(GameEvent::Resize(
                        new_game_dimensions.0.into(),
                        new_game_dimensions.1.into(),
                    ))
                    .is_ok()
            {
                current_game_dimensions = Some(new_game_dimensions);
            }

            let canvas = Canvas::default() // Game rendering
                .marker(Marker::Block)
                .block(Block::default().borders(Borders::ALL))
                .paint(|ctx| {
                    ctx.draw(&Points {
                        coords: app.cells().as_slice(),
                        color: Color::White,
                    })
                });
            f.render_widget(canvas, chunks[0]);

            let run_pause_str = match &game_running {
                // Controls rendering
                false => "  Run",
                true => "  Pause",
            };

            let instructions = Paragraph::new(vec![
                Spans::from(vec![Span::raw(" [Left click]")]),
                Spans::from(vec![Span::raw("  Add cell")]),
                Spans::from(vec![Span::raw("")]),
                Spans::from(vec![Span::raw(" [Right click]")]),
                Spans::from(vec![Span::raw("  Delete cell")]),
                Spans::from(vec![Span::raw("")]),
                Spans::from(vec![Span::raw(" [Enter]")]),
                Spans::from(vec![Span::raw(run_pause_str)]),
                Spans::from(vec![Span::raw("")]),
                Spans::from(vec![Span::raw(" [-, +]")]),
                Spans::from(vec![Span::raw(format!(
                    "  Tick rate = {}",
                    &tick_rate.as_millis()
                ))]),
                Spans::from(vec![Span::raw("")]),
                Spans::from(vec![Span::raw(" [q]")]),
                Spans::from(vec![Span::raw("  Exit")]),
            ])
            .alignment(Alignment::Left)
            .block(Block::default().borders(Borders::ALL).title("Controls"));
            f.render_widget(instructions, chunks[1]);

            match rx.recv().unwrap() {
                // GameEvent handler/consumer
                GameEvent::Input(event) => match event.code {
                    KeyCode::Char('+') => tick_rate += Duration::from_millis(10),
                    KeyCode::Char('-') => tick_rate -= Duration::from_millis(10),
                    KeyCode::Enter => game_running = !game_running,

                    KeyCode::Char('q') => exit = true,

                    _ => {}
                },

                GameEvent::Tick => app.on_tick(),
                GameEvent::Resize(w, h) => app.resize(w, h),
            }
        })?;

        if exit {
            break;
        }
    }
    Ok(())
}

fn handle_game_events(tx: Sender<GameEvent<event::KeyEvent>>, tick_rate: Duration) {
    // Reads for inputs and generates ticks
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
