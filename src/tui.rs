use crate::models::{Decision, ReviewItem, Risk};
use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, MouseButton,
    MouseEvent, MouseEventKind,
};
use crossterm::execute;
use crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Table, TableState, Wrap};
use ratatui::Terminal;
use std::io;
use tui_input::backend::crossterm::EventHandler;
use tui_input::Input;

pub fn accept_all_low(items: &mut [ReviewItem]) {
    for item in items.iter_mut() {
        if item.analysis.risk == Risk::Low && item.decision.is_none() {
            item.decision = Some(Decision::Accept);
        }
    }
}

pub fn progress(items: &[ReviewItem]) -> (usize, usize) {
    let total = items.len();
    let decided = items.iter().filter(|it| it.decision.is_some()).count();
    (decided, total)
}

enum AppMode {
    Normal,
    RejectInput,
}

#[derive(Clone, Copy, PartialEq)]
enum Focus {
    Queue,
    Detail,
}

pub type SendCallback = Box<dyn FnMut(&[ReviewItem])>;
pub type ProgressCallback = Box<dyn FnMut(usize, usize)>;

const SPLIT_HOVER_COLOR: Color = Color::Rgb(255, 170, 0);
const FOCUS_COLOR: Color = Color::Green;

pub struct App {
    items: Vec<ReviewItem>,
    table_state: TableState,
    mode: AppMode,
    focus: Focus,
    diff_scroll: u16,
    max_diff_scroll: u16,
    hover_split: bool,
    last_table_row: Option<usize>,
    reject_input: Input,
    queue_pct: u16,
    dragging: bool,
    on_send: SendCallback,
    on_progress: ProgressCallback,
}

impl App {
    pub fn new(
        items: Vec<ReviewItem>,
        on_send: SendCallback,
        on_progress: ProgressCallback,
    ) -> Self {
        let mut table_state = TableState::default();
        if !items.is_empty() {
            table_state.select(Some(0));
        }
        Self {
            items,
            table_state,
            mode: AppMode::Normal,
            focus: Focus::Queue,
            diff_scroll: 0,
            max_diff_scroll: 0,
            hover_split: false,
            last_table_row: Some(0),
            reject_input: Input::default(),
            queue_pct: 50,
            dragging: false,
            on_send,
            on_progress,
        }
    }

    fn selected_index(&self) -> Option<usize> {
        self.table_state.selected()
    }

    fn report_progress(&mut self) {
        let (d, t) = progress(&self.items);
        (self.on_progress)(d, t);
    }

    pub fn run(&mut self) -> io::Result<()> {
        terminal::enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        self.report_progress();
        let result = self.event_loop(&mut terminal);

        terminal::disable_raw_mode()?;
        execute!(
            terminal.backend_mut(),
            LeaveAlternateScreen,
            DisableMouseCapture
        )?;
        terminal.show_cursor()?;
        result
    }

    fn event_loop(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    ) -> io::Result<()> {
        loop {
            terminal.draw(|f| self.draw(f))?;
            let area = terminal.size().unwrap_or_default();
            let area = Rect::new(0, 0, area.width, area.height);
            match event::read()? {
                Event::Key(key) => match self.mode {
                    AppMode::Normal => {
                        if self.handle_normal_key(key) {
                            return Ok(());
                        }
                    }
                    AppMode::RejectInput => self.handle_reject_key(key),
                },
                Event::Mouse(mouse) => self.handle_mouse(mouse, area),
                _ => {}
            }
        }
    }

    fn handle_normal_key(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Char('q') => return true,
            KeyCode::Char('j') => self.move_table_cursor(1),
            KeyCode::Char('k') => self.move_table_cursor(-1),
            KeyCode::Down => match self.focus {
                Focus::Detail => self.scroll_diff(1),
                Focus::Queue => self.move_table_cursor(1),
            },
            KeyCode::Up => match self.focus {
                Focus::Detail => self.scroll_diff(-1),
                Focus::Queue => self.move_table_cursor(-1),
            },
            KeyCode::PageDown if self.focus == Focus::Detail => self.scroll_diff(10),
            KeyCode::PageUp if self.focus == Focus::Detail => self.scroll_diff(-10),
            KeyCode::Home if self.focus == Focus::Detail => {
                self.diff_scroll = 0;
            }
            KeyCode::End if self.focus == Focus::Detail => {
                self.diff_scroll = self.max_diff_scroll;
            }
            KeyCode::Enter => {
                self.focus = Focus::Detail;
            }
            KeyCode::Esc => {
                self.focus = Focus::Queue;
            }
            KeyCode::Char('a') => {
                if let Some(i) = self.selected_index() {
                    self.items[i].decision = Some(Decision::Accept);
                    self.items[i].reject_reason = None;
                    self.report_progress();
                }
            }
            KeyCode::Char('A') => {
                accept_all_low(&mut self.items);
                self.report_progress();
            }
            KeyCode::Char('r') if self.selected_index().is_some() => {
                self.reject_input = Input::default();
                self.mode = AppMode::RejectInput;
            }
            KeyCode::Char('s') => {
                let rejects: Vec<ReviewItem> = self
                    .items
                    .iter()
                    .filter(|it| it.decision == Some(Decision::Reject))
                    .cloned()
                    .collect();
                (self.on_send)(&rejects);
                return true;
            }
            KeyCode::Char('[') => {
                self.queue_pct = self.queue_pct.saturating_sub(10).max(20);
            }
            KeyCode::Char(']') => {
                self.queue_pct = (self.queue_pct + 10).min(80);
            }
            _ => {}
        }
        false
    }

    fn scroll_diff(&mut self, delta: i32) {
        let next = (self.diff_scroll as i32 + delta).max(0);
        self.diff_scroll = (next as u16).min(self.max_diff_scroll);
    }

    fn move_table_cursor(&mut self, delta: i32) {
        if delta > 0 {
            self.table_state.select_next();
        } else {
            self.table_state.select_previous();
        }
        let new = self.table_state.selected();
        if new != self.last_table_row {
            self.diff_scroll = 0;
            self.last_table_row = new;
        }
    }

    fn handle_reject_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Enter => {
                let reason = self.reject_input.value().to_string();
                if !reason.is_empty() {
                    if let Some(i) = self.selected_index() {
                        self.items[i].decision = Some(Decision::Reject);
                        self.items[i].reject_reason = Some(reason);
                        self.report_progress();
                    }
                }
                self.mode = AppMode::Normal;
            }
            KeyCode::Esc => {
                self.mode = AppMode::Normal;
            }
            _ => {
                self.reject_input.handle_event(&Event::Key(key));
            }
        }
    }

    fn handle_mouse(&mut self, mouse: MouseEvent, area: Rect) {
        let split_x = (area.width as u32 * self.queue_pct as u32 / 100) as u16;
        let near_split = (mouse.column as i16 - split_x as i16).unsigned_abs() <= 1;
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) if near_split => {
                self.dragging = true;
            }
            MouseEventKind::Drag(MouseButton::Left) if self.dragging => {
                let pct = (mouse.column as u32 * 100 / area.width.max(1) as u32) as u16;
                self.queue_pct = pct.clamp(20, 80);
                self.hover_split = true;
            }
            MouseEventKind::Up(MouseButton::Left) => {
                self.dragging = false;
                self.hover_split = near_split;
            }
            MouseEventKind::Moved => {
                self.hover_split = near_split;
            }
            _ => {}
        }
    }

    fn draw(&mut self, f: &mut ratatui::Frame) {
        let area = f.area();

        let outer = Layout::vertical([
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(area);

        f.render_widget(
            Paragraph::new("vouch — you vouch, AI helps")
                .style(Style::default().fg(Color::White).bg(Color::DarkGray)),
            outer[0],
        );

        f.render_widget(
            Paragraph::new(
                " j↓ k↑ a:accept A:all r:reject s:send q:quit  Enter:detail Esc:queue  [/]:resize",
            )
            .style(Style::default().fg(Color::White).bg(Color::DarkGray)),
            outer[2],
        );

        let body = Layout::horizontal([
            Constraint::Percentage(self.queue_pct),
            Constraint::Percentage(100 - self.queue_pct),
        ])
        .split(outer[1]);

        self.draw_queue(f, body[0]);
        self.draw_detail(f, body[1]);

        // Hover-on-split: paint the shared boundary column amber.
        if self.hover_split {
            let q = body[0];
            let d = body[1];
            let buf = f.buffer_mut();
            let cols = [q.right().saturating_sub(1), d.left()];
            let style = Style::default().fg(SPLIT_HOVER_COLOR);
            for x in cols.iter().copied() {
                if x < buf.area.right() {
                    for y in q.top()..q.bottom() {
                        if y < buf.area.bottom() {
                            buf[(x, y)].set_style(style);
                        }
                    }
                }
            }
        }

        if matches!(self.mode, AppMode::RejectInput) {
            self.draw_reject_modal(f, area);
        }
    }

    fn pane_block(&self, title: &str, focused: bool) -> Block<'static> {
        let mut block = Block::default()
            .borders(Borders::ALL)
            .title(title.to_string());
        if focused {
            block = block
                .border_type(ratatui::widgets::BorderType::Thick)
                .border_style(Style::default().fg(FOCUS_COLOR));
        }
        block
    }

    fn draw_queue(&mut self, f: &mut ratatui::Frame, area: Rect) {
        let header = Row::new(["Risk", "Conf", "Intent", "Files", "Decision"])
            .style(Style::default().add_modifier(Modifier::BOLD))
            .bottom_margin(0);

        let rows: Vec<Row> = self
            .items
            .iter()
            .map(|it| {
                let decision = match &it.decision {
                    Some(Decision::Accept) => "accept",
                    Some(Decision::Reject) => "reject",
                    None => "—",
                };
                Row::new([
                    Cell::from(it.analysis.risk.badge()),
                    Cell::from(it.analysis.confidence.badge()),
                    Cell::from(it.semantic.intent.chars().take(50).collect::<String>()),
                    Cell::from(
                        it.semantic
                            .files
                            .join(", ")
                            .chars()
                            .take(40)
                            .collect::<String>(),
                    ),
                    Cell::from(decision),
                ])
            })
            .collect();

        let widths = [
            Constraint::Length(4),
            Constraint::Length(4),
            Constraint::Percentage(40),
            Constraint::Percentage(30),
            Constraint::Length(8),
        ];

        let block = self.pane_block("Queue", self.focus == Focus::Queue);
        let table = Table::new(rows, widths)
            .header(header)
            .block(block)
            .row_highlight_style(
                Style::default()
                    .bg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD),
            );

        f.render_stateful_widget(table, area, &mut self.table_state);
    }

    fn draw_detail(&mut self, f: &mut ratatui::Frame, area: Rect) {
        let block = self.pane_block("Detail", self.focus == Focus::Detail);
        let inner = block.inner(area);
        f.render_widget(block, area);

        let selected = self.selected_index().and_then(|i| self.items.get(i));
        let Some(it) = selected else {
            return;
        };

        let detail_layout =
            Layout::vertical([Constraint::Length(6), Constraint::Min(0)]).split(inner);

        let header_text = vec![
            Line::from(Span::styled(
                it.semantic.intent.clone(),
                Style::default().add_modifier(Modifier::BOLD),
            )),
            Line::from(format!(
                "Risk: {} {}  ({})",
                it.analysis.risk.badge(),
                format!("{:?}", it.analysis.risk).to_lowercase(),
                it.analysis.risk_reason,
            )),
            Line::from(format!(
                "Confidence: {} {}",
                it.analysis.confidence.badge(),
                format!("{:?}", it.analysis.confidence).to_lowercase(),
            )),
            Line::from(format!("Summary: {}", it.analysis.summary_ko)),
            Line::from(format!("Files: {}", it.semantic.files.join(", "))),
            Line::from(format!(
                "Decision: {}",
                match &it.decision {
                    Some(d) => format!("{:?}", d).to_lowercase(),
                    None => "(none)".into(),
                }
            )),
        ];
        f.render_widget(
            Paragraph::new(header_text).wrap(Wrap { trim: false }),
            detail_layout[0],
        );

        let diff_lines: Vec<Line> = it
            .semantic
            .merged_diff
            .lines()
            .map(|line| {
                let style = if line.starts_with('+') && !line.starts_with("+++") {
                    Style::default().fg(Color::Green)
                } else if line.starts_with('-') && !line.starts_with("---") {
                    Style::default().fg(Color::Red)
                } else if line.starts_with("@@") {
                    Style::default().fg(Color::Cyan)
                } else {
                    Style::default()
                };
                Line::from(Span::styled(line.to_string(), style))
            })
            .collect();

        let diff_area = detail_layout[1];
        let diff_block = Block::default().borders(Borders::TOP);
        let diff_inner_height = diff_block.inner(diff_area).height;
        let diff_inner_width = diff_block.inner(diff_area).width;

        let paragraph = Paragraph::new(diff_lines)
            .block(diff_block)
            .wrap(Wrap { trim: false });

        // Compute wrapped line count to bound the scroll, so we don't
        // scroll past the end into an empty void.
        let total_lines = paragraph.line_count(diff_inner_width) as u16;
        self.max_diff_scroll = total_lines.saturating_sub(diff_inner_height);
        if self.diff_scroll > self.max_diff_scroll {
            self.diff_scroll = self.max_diff_scroll;
        }

        f.render_widget(paragraph.scroll((self.diff_scroll, 0)), diff_area);
    }

    fn draw_reject_modal(&self, f: &mut ratatui::Frame, area: Rect) {
        let modal_area = centered_rect(70, 5, area);
        f.render_widget(Clear, modal_area);
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow))
            .title("Reject reason (Enter to submit, Esc to cancel)");
        let inner = block.inner(modal_area);
        f.render_widget(block, modal_area);

        let width = inner.width.saturating_sub(1) as usize;
        let scroll = self.reject_input.visual_scroll(width);
        f.render_widget(
            Paragraph::new(self.reject_input.value()).scroll((0, scroll as u16)),
            inner,
        );
        f.set_cursor_position(Position::new(
            inner.x + (self.reject_input.visual_cursor().saturating_sub(scroll)) as u16,
            inner.y,
        ));
    }
}

fn centered_rect(percent_x: u16, height: u16, area: Rect) -> Rect {
    let popup_layout = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(height),
        Constraint::Fill(1),
    ])
    .split(area);
    Layout::horizontal([
        Constraint::Percentage((100 - percent_x) / 2),
        Constraint::Percentage(percent_x),
        Constraint::Percentage((100 - percent_x) / 2),
    ])
    .split(popup_layout[1])[1]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::*;

    fn sample_items() -> Vec<ReviewItem> {
        vec![
            ReviewItem {
                semantic: SemanticHunk {
                    id: "s0".into(),
                    intent: "SQL injection".into(),
                    files: vec!["auth.py".into()],
                    raw_hunk_ids: vec!["r0".into()],
                    merged_diff: "+eval(user_input)".into(),
                },
                analysis: Analysis {
                    id: "s0".into(),
                    risk: Risk::High,
                    risk_reason: "eval".into(),
                    confidence: Confidence::Confident,
                    summary_ko: "위험한 eval 사용".into(),
                },
                decision: None,
                reject_reason: None,
            },
            ReviewItem {
                semantic: SemanticHunk {
                    id: "s1".into(),
                    intent: "rename var".into(),
                    files: vec!["utils.py".into()],
                    raw_hunk_ids: vec!["r1".into()],
                    merged_diff: "-old_name\n+new_name".into(),
                },
                analysis: Analysis {
                    id: "s1".into(),
                    risk: Risk::Low,
                    risk_reason: "rename".into(),
                    confidence: Confidence::Confident,
                    summary_ko: "변수 이름 변경".into(),
                },
                decision: None,
                reject_reason: None,
            },
        ]
    }

    #[test]
    fn accept_sets_decision() {
        let mut items = sample_items();
        items[0].decision = Some(Decision::Accept);
        assert_eq!(items[0].decision, Some(Decision::Accept));
    }

    #[test]
    fn accept_all_low_only_affects_low_risk() {
        let mut items = sample_items();
        accept_all_low(&mut items);
        assert!(items[0].decision.is_none());
        assert_eq!(items[1].decision, Some(Decision::Accept));
    }

    #[test]
    fn progress_calculation() {
        let mut items = sample_items();
        assert_eq!(progress(&items), (0, 2));
        items[0].decision = Some(Decision::Accept);
        assert_eq!(progress(&items), (1, 2));
        items[1].decision = Some(Decision::Reject);
        assert_eq!(progress(&items), (2, 2));
    }
}
