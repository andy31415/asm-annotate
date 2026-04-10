use crate::source_reader::SourceReader;
use crate::types::DisplayItem;
use color_eyre::eyre::Result;
use colored::Color as ColoredColor;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use log::error;
use ratatui::{
    Terminal,
    backend::{Backend, CrosstermBackend},
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};
use std::collections::{BTreeMap, HashMap};
use std::io;
use std::path::{Path, PathBuf};

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

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum ActivePane {
    Source,
    Assembly,
}

struct AppState {
    source_lines: Vec<Line<'static>>,
    asm_lines: Vec<Line<'static>>,
    active_pane: ActivePane,
    source_scroll: u16,
    asm_scroll: u16,
}

impl AppState {
    fn new(items: &[DisplayItem], source_reader: &SourceReader, context_lines: usize) -> Self {
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

        // --- Prepare Source Lines (SideBySideRenderer logic) ---
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

        let mut source_lines: Vec<Line<'static>> = Vec::new();
        for (file, lines) in &source_map {
            source_lines.push(Line::from(Span::styled(
                format!("-- {} --", short_path(file, 3)),
                Style::default().add_modifier(Modifier::DIM | Modifier::ITALIC),
            )));

            if lines.is_empty() {
                continue;
            }

            let mut sorted_asm_lines: Vec<usize> = lines.keys().cloned().collect();
            sorted_asm_lines.sort();

            let mut ranges: Vec<(usize, usize)> = Vec::new();
            let mut i = 0;
            while i < sorted_asm_lines.len() {
                let current_asm_line = sorted_asm_lines[i];
                let start = std::cmp::max(1, current_asm_line.saturating_sub(context_lines));
                let mut end = current_asm_line + context_lines;
                let mut j = i + 1;
                while j < sorted_asm_lines.len() {
                    let next_asm_line = sorted_asm_lines[j];
                    if std::cmp::max(1, next_asm_line.saturating_sub(context_lines)) <= end + 1 {
                        end = next_asm_line + context_lines;
                        j += 1;
                    } else {
                        break;
                    }
                }
                ranges.push((start, end));
                i = j;
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
                            Span::raw("    "),
                            Span::styled(line_content, Style::default().fg(Color::DarkGray)),
                        ])
                    };
                    source_lines.push(styled_content);
                }
                last_printed_line = Some(end);
            }
        }

        AppState {
            source_lines,
            asm_lines,
            active_pane: ActivePane::Source,
            source_scroll: 0,
            asm_scroll: 0,
        }
    }

    fn scroll_down(&mut self, amount: u16) {
        match self.active_pane {
            ActivePane::Source => {
                self.source_scroll = self.source_scroll.saturating_add(amount);
            }
            ActivePane::Assembly => {
                self.asm_scroll = self.asm_scroll.saturating_add(amount);
            }
        }
    }

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

pub fn run_tui(func_name: &str, items: &[DisplayItem], source_reader: &SourceReader) -> Result<()> {
    // setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let app_state = AppState::new(items, source_reader, 5); // 5 lines of context
    let res = run_app(&mut terminal, func_name, app_state);

    // restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    if let Err(err) = res {
        error!("{:?}", err);
    }

    Ok(())
}

const PAGE_AMOUNT: u16 = 15;

fn run_app<B: Backend>(
    terminal: &mut Terminal<B>,
    func_name: &str,
    mut app_state: AppState,
) -> Result<()> {
    loop {
        terminal.draw(|f| {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(1), Constraint::Min(0)].as_ref())
                .split(f.size());

            let title_line = Line::from(vec![
                Span::raw("Annotating Function: "),
                Span::styled(
                    func_name,
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                ),
            ]);
            let title_paragraph = Paragraph::new(title_line).alignment(Alignment::Center);
            f.render_widget(title_paragraph, chunks[0]);

            let content_chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(50), Constraint::Percentage(50)].as_ref())
                .split(chunks[1]);

            let source_border_style = if app_state.active_pane == ActivePane::Source {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default()
            };
            let asm_border_style = if app_state.active_pane == ActivePane::Assembly {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default()
            };

            let left_pane = Paragraph::new(app_state.source_lines.clone())
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title("Source")
                        .border_style(source_border_style),
                )
                .scroll((app_state.source_scroll, 0));
            f.render_widget(left_pane, content_chunks[0]);

            let right_pane = Paragraph::new(app_state.asm_lines.clone())
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title("Assembly")
                        .border_style(asm_border_style),
                )
                .scroll((app_state.asm_scroll, 0));
            f.render_widget(right_pane, content_chunks[1]);
        })?;

        if let Event::Key(key) = event::read()? {
            match key.code {
                KeyCode::Char('q') => return Ok(()),
                KeyCode::Char('h') | KeyCode::Left => app_state.active_pane = ActivePane::Source,
                KeyCode::Char('l') | KeyCode::Right => app_state.active_pane = ActivePane::Assembly,
                KeyCode::Char('j') | KeyCode::Down => app_state.scroll_down(1),
                KeyCode::Char('k') | KeyCode::Up => app_state.scroll_up(1),
                KeyCode::PageDown => app_state.scroll_down(PAGE_AMOUNT),
                KeyCode::PageUp => app_state.scroll_up(PAGE_AMOUNT),
                KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    app_state.scroll_down(PAGE_AMOUNT);
                }
                KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    app_state.scroll_up(PAGE_AMOUNT);
                }
                _ => {}
            }
        }
    }
}

// Helper to shorten paths
fn short_path(path_str: &str, depth: usize) -> String {
    let path = Path::new(path_str);
    let components: Vec<&std::ffi::OsStr> = path.components().map(|c| c.as_os_str()).collect();
    if components.len() > depth {
        let start_index = components.len() - depth;
        let mut result = PathBuf::from("…");
        for component in components.iter().skip(start_index) {
            result.push(component);
        }
        result.to_string_lossy().to_string()
    } else {
        path_str.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_short_path_longer_than_depth() {
        assert_eq!(short_path("/a/b/c/d.c", 3), "…/b/c/d.c");
    }

    #[test]
    fn test_short_path_shorter_than_depth() {
        // Fewer components than depth - returned as-is
        assert_eq!(short_path("/a/b.c", 3), "/a/b.c");
        assert_eq!(short_path("short.c", 3), "short.c");
    }
}
