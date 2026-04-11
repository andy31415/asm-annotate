//! Terminal User Interface for displaying source and assembly side-by-side.

use crate::cli::Cli;
use crate::commands::annotate::{AnnotationData, load_annotation_data};
use color_eyre::eyre::{Result, eyre};
use colored::Color as ColoredColor;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use log::{error, info};
use ratatui::{
    Frame, Terminal,
    backend::{Backend, CrosstermBackend},
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, Paragraph},
};
use std::collections::{BTreeMap, HashMap};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;
use std::time::Duration;
use tui_logger::{TuiLoggerWidget, TuiWidgetState};

// Maps a `colored::Color` to a `ratatui::style::Color`.
fn map_color(c: ColoredColor) -> Color {
    match c {
        ColoredColor::Black => Color::Black,
        ColoredColor::Red => Color::Red,
        ColoredColor::Green => Color::Green,
        ColoredColor::Yellow => Color::Yellow,
        ColoredColor::Blue => Color::Blue,
        ColoredColor::Magenta => Color::Magenta,
        ColoredColor::Cyan => Color::Cyan,
        ColoredColor::White => Color::White,
        ColoredColor::BrightBlack => Color::DarkGray,
        ColoredColor::BrightRed => Color::LightRed,
        ColoredColor::BrightGreen => Color::LightGreen,
        ColoredColor::BrightYellow => Color::LightYellow,
        ColoredColor::BrightBlue => Color::LightBlue,
        ColoredColor::BrightMagenta => Color::LightMagenta,
        ColoredColor::BrightCyan => Color::LightCyan,
        ColoredColor::BrightWhite => Color::Gray,
        ColoredColor::TrueColor { r, g, b } => Color::Rgb(r, g, b),
    }
}

/// Represents which pane is currently active and responding to scroll events.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum ActivePane {
    Source,
    Assembly,
}

/// Holds the main state of the TUI application.
struct AppState {
    cli_args: Cli,
    func_name: String,
    /// Source content lines (file headers excluded — see `source_file_headers`).
    source_lines: Vec<Line<'static>>,
    /// File header insertion points: (index into source_lines where header goes, raw path).
    source_file_headers: Vec<(usize, String)>,
    asm_lines: Vec<Line<'static>>,
    active_pane: ActivePane,
    source_scroll: u16,
    asm_scroll: u16,
    left_pane_width: u16, // Percentage
    show_help: bool,
    show_logger: bool,
    logger_state: TuiWidgetState,
    display_name: String,
    pre_post_context: usize,
    inter_context: usize,
}

// Helper to parse context string "N" or "N:M"
fn parse_context(context_str: &str) -> Result<(usize, usize)> {
    if let Ok(n) = context_str.parse::<usize>() {
        Ok((n, n))
    } else if let Some((n_str, m_str)) = context_str.split_once(':') {
        let n = n_str
            .parse::<usize>()
            .map_err(|_| eyre!("Invalid context number: {}", n_str))?;
        let m = m_str
            .parse::<usize>()
            .map_err(|_| eyre!("Invalid context number: {}", m_str))?;
        Ok((n, m))
    } else {
        Err(eyre!(
            "Invalid context format: {}. Use N or N:M",
            context_str
        ))
    }
}

impl AppState {
    /// Creates a new `AppState` from CLI arguments and initial annotation data.
    fn new(cli_args: &Cli, func_name: &str, data: AnnotationData) -> Result<Self> {
        let (pre_post_context, inter_context) = parse_context(&cli_args.context)?;
        let mut state = AppState {
            cli_args: cli_args.clone(),
            func_name: func_name.to_string(),
            source_lines: Vec::new(),
            source_file_headers: Vec::new(),
            asm_lines: Vec::new(),
            active_pane: ActivePane::Source,
            source_scroll: 0,
            asm_scroll: 0,
            left_pane_width: 50,
            show_help: false,
            show_logger: false,
            logger_state: TuiWidgetState::new().set_default_display_level(log::LevelFilter::Info),
            display_name: data.display_name.clone(),
            pre_post_context,
            inter_context,
        };
        state.update_data(data);
        Ok(state)
    }

    /// Updates the display data (source and assembly lines) based on new AnnotationData.
    fn update_data(&mut self, data: AnnotationData) {
        self.display_name = data.display_name;
        self.source_file_headers.clear();
        let source_reader = &data.source_reader;
        let items = &data.display_items;

        // --- Prepare Assembly Lines ---
        let asm_lines: Vec<Line<'static>> = items
            .iter()
            .map(|item| {
                let item_color = map_color(item.color);
                let asm_style = Style::default().fg(item_color);
                let bytes_str = if item.instruction.bytes.is_empty() {
                    "".to_string()
                } else {
                    format!("{:<16}  ", item.instruction.bytes)
                };
                Line::from(vec![
                    Span::raw(format!("    {:08x}  ", item.instruction.address)),
                    Span::styled(
                        bytes_str,
                        Style::default().fg(Color::Cyan).add_modifier(Modifier::DIM),
                    ),
                    Span::styled(
                        item.instruction.mnemonic.clone(),
                        asm_style.add_modifier(Modifier::BOLD),
                    ),
                ])
            })
            .collect();

        // --- Prepare Source Lines ---
        let mut source_map: BTreeMap<String, BTreeMap<usize, ()>> = BTreeMap::new();
        let mut file_line_color: HashMap<(String, usize), Color> = HashMap::new();

        for item in items {
            if let Some(ref src) = item.source {
                source_map
                    .entry(src.file.clone())
                    .or_default()
                    .insert(src.line, ());
                file_line_color
                    .entry((src.file.clone(), src.line))
                    .or_insert(map_color(item.color));
            }
        }

        let mut source_file_headers: Vec<(usize, String)> = Vec::new();
        let mut source_lines: Vec<Line<'static>> = Vec::new();
        for (file, lines) in &source_map {
            source_file_headers.push((source_lines.len(), file.clone()));

            if lines.is_empty() {
                continue;
            }

            let mut sorted_asm_lines: Vec<usize> = lines.keys().cloned().collect();
            sorted_asm_lines.sort();

            let mut ranges: Vec<(usize, usize)> = Vec::new();
            let mut i = 0;
            while i < sorted_asm_lines.len() {
                let current_asm_line = sorted_asm_lines[i];
                let context = if i == 0 {
                    self.pre_post_context
                } else {
                    self.inter_context
                };
                let start = std::cmp::max(1, current_asm_line.saturating_sub(context));
                let mut end = current_asm_line + context;
                let mut j = i + 1;
                while j < sorted_asm_lines.len() {
                    let next_asm_line = sorted_asm_lines[j];
                    let next_context = self.inter_context;
                    if std::cmp::max(1, next_asm_line.saturating_sub(next_context)) <= end + 1 {
                        end = next_asm_line + next_context;
                        j += 1;
                    } else {
                        break;
                    }
                }
                ranges.push((start, end));
                i = j;
            }

            // Adjust the end context for the last range
            if let Some(last_range) = ranges.last_mut() {
                let last_asm_line = *sorted_asm_lines.last().unwrap();
                last_range.1 = last_asm_line + self.pre_post_context;
            }

            let mut last_printed_line: Option<usize> = None;
            for (start, end) in ranges {
                if let Some(last) = last_printed_line
                    && start > last + 1
                {
                    let line_num_str = format!("{:>4}:", "");
                    source_lines.push(Line::from(Span::styled(
                        format!("{} ~", line_num_str),
                        Style::default().add_modifier(Modifier::DIM),
                    )));
                }

                for l in start..=end {
                    let color = file_line_color.get(&(file.clone(), l));
                    let line_content = source_reader
                        .read_line(file, l)
                        .unwrap_or(None)
                        .unwrap_or_default();
                    let is_main = lines.contains_key(&l);

                    let line_num_str = format!("{:>4}: ", l);
                    let base_style = color.map_or(Style::default(), |c| Style::default().fg(*c));

                    let styled_content = if is_main {
                        Line::from(vec![
                            Span::styled(line_num_str, base_style.add_modifier(Modifier::BOLD)),
                            Span::styled("▶ ", base_style.add_modifier(Modifier::BOLD)),
                            Span::styled(line_content, base_style),
                        ])
                    } else {
                        Line::from(vec![
                            Span::styled(
                                line_num_str,
                                Style::default().add_modifier(Modifier::DIM),
                            ),
                            Span::raw("  "),
                            Span::styled(line_content, Style::default().fg(Color::DarkGray)),
                        ])
                    };
                    source_lines.push(styled_content);
                }
                last_printed_line = Some(end);
            }
        }

        self.source_lines = source_lines;
        self.source_file_headers = source_file_headers;
        self.asm_lines = asm_lines;
    }

    // Scrolls the active pane down by the given amount.
    fn scroll_down(&mut self, amount: u16, _pane_height: u16) {
        let min_visible_lines: u16 = 5;
        match self.active_pane {
            ActivePane::Source => {
                let content_height = self.source_lines.len() as u16;
                if content_height > min_visible_lines {
                    let max_scroll = content_height.saturating_sub(min_visible_lines);
                    self.source_scroll = self.source_scroll.saturating_add(amount).min(max_scroll);
                } else {
                    self.source_scroll = 0;
                }
            }
            ActivePane::Assembly => {
                let content_height = self.asm_lines.len() as u16;
                if content_height > min_visible_lines {
                    let max_scroll = content_height.saturating_sub(min_visible_lines);
                    self.asm_scroll = self.asm_scroll.saturating_add(amount).min(max_scroll);
                } else {
                    self.asm_scroll = 0;
                }
            }
        }
    }

    // Scrolls the active pane up by the given amount.
    fn scroll_up(&mut self, amount: u16) {
        match self.active_pane {
            ActivePane::Source => {
                self.source_scroll = self.source_scroll.saturating_sub(amount);
            }
            ActivePane::Assembly => {
                self.asm_scroll = self.asm_scroll.saturating_sub(amount);
            }
        }
    }
}

/// Sets up the terminal and runs the TUI application loop.
///
/// This function initializes the terminal, creates the application state, and handles
/// the main event loop. It also ensures the terminal is restored to its original
/// state upon exit.
///
/// # Arguments
///
/// * `cli_args` - The parsed command line arguments.
/// * `func_name` - The name of the function being annotated.
/// * `initial_data` - The initial data for assembly and source annotations.
/// * `file_change_rx` - A receiver for file change notifications, triggering data reload.
pub fn run_tui(
    cli_args: &Cli,
    func_name: &str,
    initial_data: AnnotationData,
    file_change_rx: Receiver<()>, // Add this parameter
) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let app_state = AppState::new(cli_args, func_name, initial_data)?;
    let res = run_app(&mut terminal, app_state, file_change_rx);

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    if let Err(err) = res {
        error!("TUI error: {:?}", err);
    }

    Ok(())
}

const PAGE_AMOUNT: u16 = 15;
const LOGGER_HEIGHT: u16 = 10;

// Main application loop: handles events and draws the UI.
fn run_app<B: Backend>(
    terminal: &mut Terminal<B>,
    mut app_state: AppState,
    file_change_rx: Receiver<()>, // Add this parameter
) -> Result<()> {
    loop {
        // Handle potential file changes
        if file_change_rx.try_recv().is_ok() {
            info!("Reloading annotation data due to file change...");
            match load_annotation_data(&app_state.cli_args, &app_state.func_name) {
                Ok(new_data) => {
                    app_state.update_data(new_data);
                }
                Err(e) => {
                    error!("Failed to reload annotation data: {}", e);
                }
            }
        }

        terminal.draw(|f| {
            let size = f.size();
            let main_chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints(
                    [
                        Constraint::Length(1), // Title
                        Constraint::Min(0),    // Content
                        Constraint::Length(if app_state.show_logger {
                            LOGGER_HEIGHT
                        } else {
                            0
                        }), // Logger
                    ]
                    .as_ref(),
                )
                .split(size);

            ui_title(f, &app_state, main_chunks[0]);

            let content_chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints(
                    [
                        Constraint::Percentage(app_state.left_pane_width),
                        Constraint::Percentage(100 - app_state.left_pane_width),
                    ]
                    .as_ref(),
                )
                .split(main_chunks[1]);

            ui_source_pane(f, &app_state, content_chunks[0]);
            ui_asm_pane(f, &app_state, content_chunks[1]);

            if app_state.show_logger {
                ui_logger(f, &app_state, main_chunks[2]);
            }

            if app_state.show_help {
                ui_help(f, size);
            }
        })?;

        // Poll for key events
        if event::poll(Duration::from_millis(100))?
            && let Event::Key(key) = event::read()?
        {
            if app_state.show_help {
                match key.code {
                    KeyCode::Char('?') | KeyCode::Esc | KeyCode::Char('q') => {
                        app_state.show_help = false;
                    }
                    _ => {}
                }
            } else {
                match key.code {
                    KeyCode::Char('?') => {
                        app_state.show_help = true;
                    }
                    KeyCode::Char('q') => return Ok(()),
                    KeyCode::Char('g') | KeyCode::Char('G') => {
                        app_state.show_logger = !app_state.show_logger;
                    }
                    KeyCode::Tab => {
                        app_state.active_pane = match app_state.active_pane {
                            ActivePane::Source => ActivePane::Assembly,
                            ActivePane::Assembly => ActivePane::Source,
                        };
                    }
                    KeyCode::Char('h') | KeyCode::Left if key.modifiers.is_empty() => {
                        app_state.active_pane = ActivePane::Source;
                    }
                    KeyCode::Char('l') | KeyCode::Right if key.modifiers.is_empty() => {
                        app_state.active_pane = ActivePane::Assembly;
                    }
                    KeyCode::Char('h') | KeyCode::Char('H') | KeyCode::Left
                        if key.modifiers.contains(KeyModifiers::SHIFT) =>
                    {
                        app_state.left_pane_width =
                            app_state.left_pane_width.saturating_sub(5).max(10);
                    }
                    KeyCode::Char('l') | KeyCode::Char('L') | KeyCode::Right
                        if key.modifiers.contains(KeyModifiers::SHIFT) =>
                    {
                        app_state.left_pane_width =
                            app_state.left_pane_width.saturating_add(5).min(90);
                    }
                    KeyCode::Char('j') | KeyCode::Down => {
                        let _pane_height = terminal.size().unwrap_or_default().height;
                        app_state.scroll_down(1, _pane_height);
                    }
                    KeyCode::Char('k') | KeyCode::Up => app_state.scroll_up(1),
                    KeyCode::PageDown => {
                        let _pane_height = terminal.size().unwrap_or_default().height;
                        app_state.scroll_down(PAGE_AMOUNT, _pane_height);
                    }
                    KeyCode::PageUp => app_state.scroll_up(PAGE_AMOUNT),
                    KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        let _pane_height = terminal.size().unwrap_or_default().height;
                        app_state.scroll_down(PAGE_AMOUNT, _pane_height);
                    }
                    KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        app_state.scroll_up(PAGE_AMOUNT);
                    }
                    _ => {}
                }
            }
        }
    }
}

// Renders the title bar.
fn ui_title(f: &mut Frame, app_state: &AppState, area: Rect) {
    let title_line = Line::from(vec![
        Span::raw("Annotating Function: "),
        Span::styled(
            app_state.display_name.clone(),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" (Press ‘?’ for help, ‘G’ to toggle logs)"),
    ]);
    let title_paragraph = Paragraph::new(title_line).alignment(Alignment::Center);
    f.render_widget(title_paragraph, area);
}

// Renders the source code pane.
fn ui_source_pane(f: &mut Frame, app_state: &AppState, area: Rect) {
    let border_style = if app_state.active_pane == ActivePane::Source && !app_state.show_help {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default()
    };

    // Build display lines with file headers sized to the available pane width.
    // Borders consume 2 columns; "-- " + " --" consume 6 more.
    let path_budget = area.width.saturating_sub(8) as usize;
    let header_style = Style::default().add_modifier(Modifier::DIM | Modifier::ITALIC);
    let mut display_lines: Vec<Line<'static>> =
        Vec::with_capacity(app_state.source_lines.len() + app_state.source_file_headers.len());
    let mut content_idx = 0usize;
    for (insert_at, raw_path) in &app_state.source_file_headers {
        while content_idx < *insert_at {
            display_lines.push(app_state.source_lines[content_idx].clone());
            content_idx += 1;
        }
        display_lines.push(Line::from(Span::styled(
            format!("-- {} --", short_path_by_width(raw_path, path_budget)),
            header_style,
        )));
    }
    while content_idx < app_state.source_lines.len() {
        display_lines.push(app_state.source_lines[content_idx].clone());
        content_idx += 1;
    }

    let pane = Paragraph::new(display_lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Source")
                .border_style(border_style),
        )
        .scroll((app_state.source_scroll, 0));
    f.render_widget(pane, area);
}

// Renders the assembly code pane.
fn ui_asm_pane(f: &mut Frame, app_state: &AppState, area: Rect) {
    let border_style = if app_state.active_pane == ActivePane::Assembly && !app_state.show_help {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default()
    };
    let pane = Paragraph::new(app_state.asm_lines.clone())
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Assembly")
                .border_style(border_style),
        )
        .scroll((app_state.asm_scroll, 0));
    f.render_widget(pane, area);
}

// Renders the logger pane if visible.
fn ui_logger(f: &mut Frame, app_state: &AppState, area: Rect) {
    let logger_widget = TuiLoggerWidget::default()
        .block(
            Block::default()
                .title("Logs (G: Close)")
                .borders(Borders::ALL),
        )
        .state(&app_state.logger_state);
    f.render_widget(logger_widget, area);
}

// Renders the help popup if visible.
fn ui_help(f: &mut Frame, size: Rect) {
    let help_text = Text::from(vec![
        Line::from(Span::styled(
            "Keybindings",
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from("? / Esc / q: Toggle Help"),
        Line::from("q: Quit (when help is not visible)"),
        Line::from("j / Down: Scroll Down (in active pane)"),
        Line::from("k / Up: Scroll Up (in active pane)"),
        Line::from("h / Left: Activate Source Pane"),
        Line::from("l / Right: Activate Assembly Pane"),
        Line::from("g / G: Toggle Logger Pane"),
        Line::from("Tab: Cycle through Source/Assembly"),
        Line::from("PgDown / Ctrl+D: Page Down"),
        Line::from("PgUp / Ctrl+U: Page Up"),
        Line::from("Shift + Left / H: Decrease Source Pane Width"),
        Line::from("Shift + Right / L: Increase Source Pane Width"),
    ]);
    let block = Block::default()
        .title(" Help ")
        .borders(Borders::ALL)
        .style(Style::default().fg(Color::White).bg(Color::DarkGray));

    let area = centered_rect(60, 18, size);
    let help_paragraph = Paragraph::new(help_text)
        .block(block)
        .alignment(Alignment::Left);
    f.render_widget(Clear, area);
    f.render_widget(help_paragraph, area);
}

/// Helper function to create a centered rectangle.
fn centered_rect(percent_x: u16, height: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Length(height),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

// Shortens a path to fit within `max_chars` characters.
// Keeps as many trailing components as will fit, prefixed with "…".
// If even just the filename exceeds `max_chars`, truncates from the left with "…".
fn short_path_by_width(path_str: &str, max_chars: usize) -> String {
    if path_str.chars().count() <= max_chars {
        return path_str.to_string();
    }
    let path = Path::new(path_str);
    let components: Vec<&std::ffi::OsStr> = path.components().map(|c| c.as_os_str()).collect();
    // Drop leading components one at a time (prefix with "…") until it fits.
    // Iterating from start=1 gives the most components that still fit.
    for start in 1..components.len() {
        let mut candidate = PathBuf::from("…");
        for component in components.iter().skip(start) {
            candidate.push(component);
        }
        let s = candidate.to_string_lossy();
        if s.chars().count() <= max_chars {
            return s.into_owned();
        }
    }
    // Nothing fits — hard-truncate from the left
    let chars: Vec<char> = path_str.chars().collect();
    if chars.len() <= max_chars {
        path_str.to_string()
    } else {
        let keep = max_chars.saturating_sub(1); // 1 for "…"
        format!(
            "…{}",
            &chars[chars.len() - keep..].iter().collect::<String>()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_short_path_by_width_fits() {
        assert_eq!(short_path_by_width("/a/b.c", 20), "/a/b.c");
        assert_eq!(short_path_by_width("short.c", 20), "short.c");
    }

    #[test]
    fn test_short_path_by_width_truncates() {
        // "/a/b/c/d.c" is 10 chars; budget 8 → drop leading "/a" → "…/b/c/d.c" (9 chars still > 8)
        // → drop "/a/b" → "…/c/d.c" (7 chars ≤ 8) ✓
        assert_eq!(short_path_by_width("/a/b/c/d.c", 8), "…/c/d.c");
        // Wider budget → can fit one extra component
        assert_eq!(short_path_by_width("/a/b/c/d.c", 9), "…/b/c/d.c");
        // Full path fits
        assert_eq!(short_path_by_width("/a/b/c/d.c", 10), "/a/b/c/d.c");
    }

    #[test]
    fn test_short_path_by_width_hard_truncate() {
        // Even the filename alone exceeds budget
        assert_eq!(short_path_by_width("/very/long/filename.c", 4), "…e.c");
    }

    #[test]
    fn test_parse_context() {
        assert_eq!(parse_context("3").unwrap(), (3, 3));
        assert_eq!(parse_context("2:5").unwrap(), (2, 5));
        assert!(parse_context("2:").is_err());
        assert!(parse_context(":5").is_err());
        assert!(parse_context("a:5").is_err());
        assert!(parse_context("2:b").is_err());
        assert!(parse_context("2:5:1").is_err());
        assert!(parse_context("").is_err());
    }
}
