use crossterm::{
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, MouseButton,
        MouseEvent, MouseEventKind,
    },
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
    layout::{Alignment, Constraint, Direction, Rect},
    style::{Modifier, Style},
    text::{Span, Spans},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame, Terminal,
};

struct ClickPosition {
    x: u16,
    y: u16,
}

enum GameEvent {
    KeyInput(KeyEvent),
    LeftClick(ClickPosition),
    RightClick(ClickPosition),
    Tick,
    Resize(Rect),
}

struct App {
    state: Vec<Vec<bool>>,
    running: bool,
    dimensions: Rect,
    tick_rate: Duration,
    last_click: (usize, usize),
}

impl App {
    fn new<B: Backend>(frame: Frame<B>) -> App {
        let dimensions = Layout::default()
            .direction(Direction::Horizontal)
            .margin(2)
            .constraints([Constraint::Percentage(85), Constraint::Percentage(15)].as_ref())
            .split(frame.size())[0];

        App {
            state: vec![vec![false; dimensions.width as usize - 1]; dimensions.height as usize - 1],
            running: false,
            dimensions,
            tick_rate: Duration::from_millis(250),
            last_click: (0, 0),
        }
    }

    fn resize(&mut self, rect: Rect) {
        self.state = vec![vec![false; rect.width.into()]; rect.height.into()];
        self.dimensions = rect;
    }

    fn on_tick(&mut self) {}

    fn add_cell(&mut self, pos: ClickPosition) {
        self.state[pos.y as usize][pos.x as usize] = true;
    }
    fn remove_cell(&mut self, pos: ClickPosition) {
        self.state[pos.y as usize][pos.x as usize] = false;
    }
}

fn main() -> Result<(), Box<dyn error::Error>> {
    enable_raw_mode().expect("Can enter raw mode");

    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let app = App::new(terminal.get_frame());
    let res = run_app(&mut terminal, app);

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
) -> Result<(), Box<dyn error::Error>> {
    let (mut tx, rx) = mpsc::channel();

    let event_handler_tx = tx.clone();
    thread::spawn(move || handle_game_events(event_handler_tx, &app.tick_rate));

    let mut exit = false;

    loop {
        terminal.draw(|f| ui(f, &mut app, &mut tx))?;

        match rx.recv().unwrap() {
            // GameEvent handler/consumer
            GameEvent::KeyInput(event) => match event.code {
                KeyCode::Char('+') => app.tick_rate += Duration::from_millis(10),
                KeyCode::Char('-') => app.tick_rate -= Duration::from_millis(10),
                KeyCode::Enter => app.running = !app.running,

                KeyCode::Char('q') => exit = true,

                _ => {}
            },

            GameEvent::LeftClick(position)
                if !app.running
                    && app.dimensions.intersects(Rect {
                        x: position.x,
                        y: position.y,
                        width: 0,
                        height: 0,
                    }) =>
            {
                let x_offset = app.dimensions.x + 1;
                let y_offset = app.dimensions.y + 1;
                app.last_click = (position.x as usize, position.y as usize);
                app.add_cell(ClickPosition {
                    x: position.x - x_offset,
                    y: position.y - y_offset,
                })
            }

            GameEvent::RightClick(position)
                if !app.running
                    && app.dimensions.intersects(Rect {
                        x: position.x,
                        y: position.y,
                        width: 0,
                        height: 0,
                    }) =>
            {
                let x_offset = app.dimensions.x + 1;
                let y_offset = app.dimensions.y + 1;
                app.remove_cell(ClickPosition {
                    x: position.x - x_offset,
                    y: position.y - y_offset,
                })
            }

            GameEvent::Tick => app.on_tick(),
            GameEvent::Resize(rect) => app.resize(rect),

            _ => {}
        }

        if exit {
            break;
        }
    }
    Ok(())
}

fn handle_game_events(tx: Sender<GameEvent>, tick_rate: &Duration) {
    // Reads for inputs and generates ticks
    let mut last_tick = Instant::now();

    loop {
        let timeout = tick_rate
            .checked_sub(last_tick.elapsed())
            .unwrap_or_else(|| Duration::from_secs(0));

        if event::poll(timeout).expect("Events are properly polled") {
            match event::read().expect("Key inputs are detected") {
                Event::Key(key) => {
                    tx.send(GameEvent::KeyInput(key))
                        .expect("GameEvent keys can be sent to the consumer");
                }

                Event::Mouse(MouseEvent {
                    kind: MouseEventKind::Down(MouseButton::Left),
                    column,
                    row,
                    ..
                }) => {
                    tx.send(GameEvent::LeftClick(ClickPosition { x: column, y: row }))
                        .expect("Left clicks can be sent to the consumer");
                }

                Event::Mouse(MouseEvent {
                    kind: MouseEventKind::Down(MouseButton::Right),
                    column,
                    row,
                    ..
                }) => {
                    tx.send(GameEvent::RightClick(ClickPosition { x: column, y: row }))
                        .expect("Right clicks can be sent to the consumer");
                }

                _ => {}
            }
        }

        if last_tick.elapsed() >= *tick_rate && tx.send(GameEvent::Tick).is_ok() {
            last_tick = Instant::now()
        }
    }
}

fn ui<B: Backend>(f: &mut Frame<B>, app: &mut App, event_tx: &mut Sender<GameEvent>) {
    let chunks = Layout::default() // Basic layout
        .direction(Direction::Horizontal)
        .margin(2)
        .constraints([Constraint::Percentage(85), Constraint::Percentage(15)].as_ref())
        .split(f.size());

    if chunks[0] != app.dimensions {
        event_tx
            .send(GameEvent::Resize(chunks[0]))
            .expect("Can send resize events");
    }

    let game = Paragraph::new(vec![Spans::from(vec![Span::styled(
        format!(
            "{}",
            app.state
                .iter()
                .flat_map(|row| {
                    row.iter().map(|x| {
                        if *x {
                            return "█";
                        }
                        " "
                    })
                })
                .collect::<String>()
        ),
        Style::default().add_modifier(Modifier::BOLD),
    )])])
    .alignment(Alignment::Left)
    .block(Block::default().borders(Borders::ALL))
    .wrap(Wrap { trim: false });

    f.render_widget(game, chunks[0]);

    let run_pause_str = match app.running {
        // Controls rendering
        false => "  Run",
        true => "  Pause",
    };

    let instructions = Paragraph::new(vec![
        Spans::from(vec![Span::raw(format!("{:?}", app.dimensions))]),
        Spans::from(vec![Span::raw(format!(
            "{:?}",
            (app.state.len(), app.state[0].len())
        ))]),
        Spans::from(vec![Span::raw(format!("{:?}", app.last_click))]),
        Spans::from(vec![Span::raw(format!(
            "{}",
            app.state
                .iter()
                .map(|row| row.iter().filter(|x| **x).count())
                .sum::<usize>()
        ))]),
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
            app.tick_rate.as_millis()
        ))]),
        Spans::from(vec![Span::raw("")]),
        Spans::from(vec![Span::raw(" [q]")]),
        Spans::from(vec![Span::raw("  Exit")]),
    ])
    .alignment(Alignment::Left)
    .block(Block::default().borders(Borders::ALL).title("Controls"));
    f.render_widget(instructions, chunks[1]);
}
