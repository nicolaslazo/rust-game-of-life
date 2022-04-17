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
    layout::{Alignment, Constraint, Direction, Margin, Rect},
    style::{Modifier, Style},
    text::{Span, Spans},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame, Terminal,
};

const THIN_MARGIN: &Margin = &Margin {
    horizontal: 1,
    vertical: 1,
};

#[derive(Copy, Clone)]
struct Point {
    x: u16,
    y: u16,
}

impl Point {
    fn in_rect(self, rect: Rect) -> bool {
        self.x >= rect.x
            && self.x < rect.x + rect.width
            && self.y >= rect.y
            && self.y < rect.y + rect.height
    }
}

type ClickPosition = Point;
// Crossterm's alternatives are too verbose. Let's strip down all unnecessary complexity as soon as possible
#[derive(PartialEq)]
enum ClickType {
    Left,
    Right,
}

const DEFAULT_TICK: Duration = Duration::from_millis(250);
const TICK_STEP: Duration = Duration::from_millis(10);

enum GameEvent {
    KeyInput(KeyEvent),
    Click(ClickType, ClickPosition),
    Tick,
    TickSet(Duration),
    Resize(Rect),
}

struct App {
    state: Vec<Vec<bool>>,
    running: bool,
    tick_rate: Duration,
    dimensions: Rect,
    last_click: (usize, usize),
}

impl App {
    fn new<B: Backend>(frame: Frame<B>) -> App {
        let dimensions = Layout::default()
            .direction(Direction::Horizontal)
            .margin(2)
            .constraints([Constraint::Percentage(85), Constraint::Percentage(15)].as_ref())
            .split(frame.size())[0]
            .inner(THIN_MARGIN);

        App {
            state: vec![vec![false; dimensions.width as usize + 1]; dimensions.height as usize + 1],
            running: false,
            tick_rate: DEFAULT_TICK,
            dimensions,
            // TODO: For debugging purposes, delete later
            last_click: (0, 0),
        }
    }

    fn resize(&mut self, rect: Rect) {
        // we need to remove borders, again
        let dimensions = rect.inner(THIN_MARGIN);

        *self = App {
            state: vec![vec![false; dimensions.width as usize + 1]; dimensions.height as usize + 1],
            running: false,
            tick_rate: self.tick_rate,
            dimensions,
            last_click: self.last_click,
        }
    }

    fn on_tick(&mut self) {
        if !self.running {
            return;
        }

        // We don't want to effect any changes until all cells are evaluated
        let mut to_flip: Vec<(usize, usize)> = Vec::new();
        for (row_idx, row) in self.state.iter().enumerate() {
            for (col_idx, cell) in row.iter().enumerate() {
                // The data type wrangling in this area is atrocious, I wonder if there's any way to fix it
                // The + self.dimensions.w/h is to prevent overflow, not required for adding
                let neighbour_idxs = {
                    let top_row_idx = (row_idx + self.dimensions.height as usize - 1)
                        % self.dimensions.height as usize;
                    let bottom_row_idx = (row_idx as usize + 1) % self.dimensions.height as usize;
                    let left_col_idx = (col_idx + self.dimensions.width as usize - 1)
                        % self.dimensions.width as usize;
                    let right_col_idx = (col_idx as usize + 1) % self.dimensions.width as usize;

                    [
                        (top_row_idx, left_col_idx),
                        (top_row_idx, col_idx),
                        (top_row_idx, right_col_idx),
                        (row_idx, left_col_idx),
                        (row_idx, right_col_idx),
                        (bottom_row_idx, left_col_idx),
                        (bottom_row_idx, col_idx),
                        (bottom_row_idx, right_col_idx),
                    ]
                };

                let live_neighbour_count = neighbour_idxs
                    .iter()
                    .filter(|(nbr_row_idx, nbr_col_idx)| self.state[*nbr_row_idx][*nbr_col_idx])
                    .count();

                let mut flip_this = false;
                match (cell, live_neighbour_count) {
                    /* By this point the formatting or the game rules is starting to look weird,
                      but I can defend my decisions.

                      Why I don't like particularly is how I'm using a match statement which leads
                      to one of two possible decisions: set needs_flip to true, or do nothing.
                      Doing one thing or nothing (and skipping all the extra conditionals once we
                      reach a truthy evaluation) sounds like the job for a if/else if decision tree.
                      I wanted to go with a match because the pattern matching and guards make for
                      an idiomatic overview of the game rules.

                      If we decide to settle on a match then its more idiomatic use would be to set
                      the literal boolean for each cell in self.state, but that would involve
                      a lot of unnecessary writes to the Vec.

                      The flip_this boolean could be made redundant by push to to_flip
                      directly but that would clutter the match.

                      Any potential implementation would be faster and cleaner to implement
                      than taking the time to write this comment.
                      But this is a learning experience, and I'm a perfectionist.
                    */
                    (true, count) if count < 2 => flip_this = true, // Underpopulation
                    (true, count) if count > 3 => flip_this = true, // Overpopulation
                    (false, count) if count == 3 => flip_this = true, // Reproduction
                    (_, _) => {}
                }

                if flip_this {
                    to_flip.push((row_idx, col_idx))
                }
            }
        }

        to_flip.iter().for_each(|(row_idx, col_idx)| {
            self.state[*row_idx][*col_idx] = !self.state[*row_idx][*col_idx]
        });
    }

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
    thread::spawn(move || handle_game_events(event_handler_tx));

    let mut exit = false;

    loop {
        terminal.draw(|f| ui(f, &mut app, &mut tx))?;

        match rx.recv().unwrap() {
            // GameEvent handler/consumer
            GameEvent::KeyInput(event) => match event.code {
                KeyCode::Enter => app.running = !app.running,

                KeyCode::Char('q') => exit = true,

                _ => {}
            },

            GameEvent::Click(button, position)
                if !app.running && position.in_rect(app.dimensions) =>
            {
                let x_offset = app.dimensions.x;
                let y_offset = app.dimensions.y;
                let offset_position = ClickPosition {
                    x: position.x - x_offset,
                    y: position.y - y_offset,
                };
                app.last_click = (position.x as usize, position.y as usize);

                if button == ClickType::Left {
                    app.add_cell(offset_position);
                } else {
                    app.remove_cell(offset_position);
                }
            }

            GameEvent::Tick => app.on_tick(),
            GameEvent::TickSet(new_tick_rate) => app.tick_rate = new_tick_rate,
            GameEvent::Resize(rect) => app.resize(rect),

            _ => {}
        }

        if exit {
            break;
        }
    }
    Ok(())
}

fn handle_game_events(tx: Sender<GameEvent>) {
    // Reads for inputs and generates ticks
    let mut tick_rate = DEFAULT_TICK;
    let mut last_tick = Instant::now();

    loop {
        let timeout = tick_rate
            .checked_sub(last_tick.elapsed())
            .unwrap_or_else(|| Duration::from_secs(0));

        if event::poll(timeout).expect("Events are properly polled") {
            match event::read().expect("Key inputs are detected") {
                Event::Key(key) => match key.code {
                    KeyCode::Char('+') => {
                        tick_rate += TICK_STEP;

                        tx.send(GameEvent::TickSet(tick_rate))
                            .expect("Can increase tick rate");
                    }
                    KeyCode::Char('-') => {
                        if tick_rate > Duration::from_millis(30) {
                            // TODO: Figure out a way to drop events so the buffer doesn't get clogged with ticks at really high rates
                            tick_rate -= TICK_STEP
                        }

                        tx.send(GameEvent::TickSet(tick_rate))
                            .expect("Can increase tick rate");
                    }
                    _ => tx
                        .send(GameEvent::KeyInput(key))
                        .expect("GameEvent keys can be sent to the consumer"),
                },

                Event::Mouse(MouseEvent {
                    kind:
                        button @ (MouseEventKind::Down(MouseButton::Left)
                        | MouseEventKind::Down(MouseButton::Right)),
                    column,
                    row,
                    ..
                }) => {
                    let click_type = if button == MouseEventKind::Down(MouseButton::Left) {
                        ClickType::Left
                    } else {
                        ClickType::Right
                    };

                    tx.send(GameEvent::Click(
                        click_type,
                        ClickPosition { x: column, y: row },
                    ))
                    .expect("Clicks can be sent to the consumer");
                }

                _ => {}
            }
        }

        if last_tick.elapsed() >= tick_rate && tx.send(GameEvent::Tick).is_ok() {
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

    if chunks[0].inner(THIN_MARGIN) != app.dimensions {
        event_tx
            .send(GameEvent::Resize(chunks[0]))
            .expect("Can send resize events");
    }

    /* A tui-rs Canvas sounds like the more obvious tool for this case
      but that dot system doesn't conform to a simple grid system like Paragrah does
    */
    let game = Paragraph::new(vec![Spans::from(vec![Span::styled(
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
            .collect::<String>(),
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
        Spans::from(vec![Span::raw("")]),
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
            "  Tick rate = {}ms",
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
