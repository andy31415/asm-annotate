use crate::source_reader::SourceReader;
use crate::types::DisplayItem;
use color_eyre::eyre::Result;
use colored::Color as ColoredColor;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::{Backend, CrosstermBackend},
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Terminal,
};
use std::io;

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

pub fn run_tui(
    func_name: &str,
    items: &[DisplayItem],
    _source_reader: &SourceReader,
) -> Result<()> {
    // setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // create app and run it
    let res = run_app(&mut terminal, func_name, items);

    // restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    if let Err(err) = res {
        println!("{:?}", err)
    }

    Ok(())
}

fn run_app<B: Backend>(terminal: &mut Terminal<B>, func_name: &str, items: &[DisplayItem]) -> Result<()> {
    loop {
        terminal.draw(|f| {
            let chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(50), Constraint::Percentage(50)].as_ref())
                .split(f.size());

            // Prepare content for panes
            let mut source_lines: Vec<Line> = Vec::new();
            let mut asm_lines: Vec<Line> = Vec::new();
            let mut last_file: Option<String> = None;
            let mut last_line: Option<usize> = None;

            for item in items {
                // Assembly Line
                let item_color = map_color(item.color);
                let asm_style = Style::default().fg(item_color);

                let bytes_str = if item.instruction.bytes.is_empty() {
                    "".to_string()
                } else {
                    format!("{:<16}  ", item.instruction.bytes)
                };
                asm_lines.push(Line::from(vec![
                    Span::raw(format!("    {:08x}  ", item.instruction.address)),
                    Span::styled(
                        bytes_str,
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::DIM),
                    ),
                    Span::styled(
                        item.instruction.mnemonic.clone(),
                        asm_style.add_modifier(Modifier::BOLD),
                    ),
                ]));

                // Source Line
                if let Some(ref src) = item.source {
                    if item.is_new_file && last_file.as_ref() != Some(&src.file) {
                        source_lines.push(Line::from(Span::styled(
                            format!("-- {} --", src.file),
                            Style::default().add_modifier(Modifier::DIM | Modifier::ITALIC),
                        )));
                    }

                    if item.is_new_line && (last_file.as_ref() != Some(&src.file) || last_line != Some(src.line)) {
                        let line_num_str = format!("{:>4}: ", src.line);
                        let marker = "▶ ";
                        let empty_string = "".to_string();
                        let content = item.source_text.as_ref().unwrap_or(&empty_string);

                        source_lines.push(Line::from(vec![
                            Span::styled(line_num_str, asm_style.add_modifier(Modifier::BOLD)),
                            Span::styled(marker, asm_style.add_modifier(Modifier::BOLD)),
                            Span::styled(content.clone(), asm_style),
                        ]));
                    } else {
                        // Add empty line to keep source and asm in sync for scrolling
                        source_lines.push(Line::from(""));
                    }
                    last_file = Some(src.file.clone());
                    last_line = Some(src.line);
                } else {
                    // Add empty line to keep source and asm in sync for scrolling
                    source_lines.push(Line::from(""));
                }
            }

            let left_pane = Paragraph::new(source_lines)
                .block(Block::default().borders(Borders::ALL).title(format!("Source - {}", func_name)));
            f.render_widget(left_pane, chunks[0]);

            let right_pane = Paragraph::new(asm_lines)
                .block(Block::default().borders(Borders::ALL).title("Assembly"));
            f.render_widget(right_pane, chunks[1]);
        })?;

        if let Event::Key(key) = event::read()? {
            if key.code == KeyCode::Char('q') {
                return Ok(());
            }
        }
    }
}
