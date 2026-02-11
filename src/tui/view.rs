use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Tabs, Wrap},
    Frame,
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use super::app::{App, Focus, ModalKind, StatusLevel, Tab};
use super::tasks::{format_elapsed, spinner_frames};

pub fn draw(f: &mut Frame<'_>, app: &mut App) {
    let area = f.size();

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // header
            Constraint::Min(1),    // main
            Constraint::Length(2), // footer
        ])
        .split(area);

    draw_header(f, app, layout[0]);
    draw_main(f, app, layout[1]);
    draw_footer(f, app, layout[2]);

    if app.show_help {
        draw_help_modal(f, app, area);
    }

    // App-level modals should render above everything else.
    if app.modal.kind != ModalKind::None {
        draw_app_modal(f, app, area);
    }
}

fn draw_header(f: &mut Frame<'_>, app: &App, area: Rect) {
    let titles: Vec<Line> = Tab::ALL
        .iter()
        .map(|t| {
            let style = if *t == app.active_tab {
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            Line::from(Span::styled(t.title(), style))
        })
        .collect();

    // Make tab bar border brighter when focused so users understand focus.
    let border = if app.focus == Focus::TabBar {
        Style::default().fg(Color::White)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let tabs = Tabs::new(titles)
        .block(
            Block::default()
                .title(" Git Wiz ")
                .borders(Borders::ALL)
                .border_style(border),
        )
        .select(
            Tab::ALL
                .iter()
                .position(|t| *t == app.active_tab)
                .unwrap_or(0),
        )
        .style(Style::default().fg(Color::DarkGray))
        .highlight_style(
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )
        .divider(Span::raw(" | "));

    f.render_widget(tabs, area);
}

fn draw_main(f: &mut Frame<'_>, app: &mut App, area: Rect) {
    match app.active_tab {
        Tab::Generate => draw_generate_tab(f, app, area),
        Tab::Stage => draw_stage_tab(f, app, area),
        Tab::Diff => draw_diff_tab(f, app, area),
        Tab::Push => draw_push_tab(f, app, area),
        Tab::Release => draw_release_tab(f, app, area),
        Tab::Config => draw_config_tab(f, app, area),
    }
}

fn draw_generate_tab(f: &mut Frame<'_>, app: &mut App, area: Rect) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(44), Constraint::Min(1)])
        .split(area);

    let left = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(8),
            Constraint::Length(7),
            Constraint::Min(1),
        ])
        .split(cols[0]);

    // Context panel
    let info_block = Block::default()
        .title(" Context ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray));

    let info_text = Text::from(vec![
        Line::from(vec![
            Span::styled("Provider:    ", Style::default().fg(Color::DarkGray)),
            Span::styled(&app.provider_label, Style::default().fg(Color::White)),
        ]),
        Line::from(vec![
            Span::styled("Model:       ", Style::default().fg(Color::DarkGray)),
            Span::styled(&app.model_label, Style::default().fg(Color::White)),
        ]),
        Line::from(vec![
            Span::styled("Diff Source: ", Style::default().fg(Color::DarkGray)),
            Span::styled(&app.diff_source_label, Style::default().fg(Color::White)),
        ]),
        Line::from(vec![
            Span::styled("Summary:     ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                truncate_to_width(&app.diff_summary, 28),
                Style::default().fg(Color::White),
            ),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "Tip: ←/→ switches tabs (Alt+←/→ always). Tab cycles focus.",
            Style::default().fg(Color::DarkGray),
        )),
    ]);

    f.render_widget(
        Paragraph::new(info_text)
            .block(info_block)
            .wrap(Wrap { trim: true }),
        left[0],
    );

    // Actions panel (selectable)
    render_actions_list(f, app, left[1]);

    // Log panel
    render_log_panel(f, app, left[2]);

    // Editor
    let editor_border = if app.focus == Focus::CommitEditor {
        Style::default().fg(Color::White)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    app.commit_editor.set_block(
        Block::default()
            .title(" Commit Message ")
            .borders(Borders::ALL)
            .border_style(editor_border),
    );

    f.render_widget(app.commit_editor.widget(), cols[1]);
}

fn draw_stage_tab(f: &mut Frame<'_>, app: &mut App, area: Rect) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(44), Constraint::Min(1)])
        .split(area);

    let left = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(8), Constraint::Length(7), Constraint::Min(1)])
        .split(cols[0]);

    let info_block = Block::default()
        .title(" Stage / Unstage ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray));

    let info_text = Text::from(vec![
        Line::from(Span::styled(
            "Use the Actions list to stage/unstage changes.",
            Style::default().fg(Color::White),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "Patch actions open interactive git prompts.",
            Style::default().fg(Color::DarkGray),
        )),
        Line::from(Span::styled(
            "Tip: Tab to focus Actions, ↑/↓ select, Enter run.",
            Style::default().fg(Color::DarkGray),
        )),
    ]);

    f.render_widget(
        Paragraph::new(info_text)
            .block(info_block)
            .wrap(Wrap { trim: true }),
        left[0],
    );

    render_actions_list(f, app, left[1]);
    render_log_panel(f, app, left[2]);

    let details_block = Block::default()
        .title(" Details ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray));

    let details = Paragraph::new(Text::from(vec![
        Line::from(Span::styled(
            "Stage patch: git add -p (interactive)",
            Style::default().fg(Color::DarkGray),
        )),
        Line::from(Span::styled(
            "Stage all:   git add -A",
            Style::default().fg(Color::DarkGray),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "Unstage patch: git restore --staged -p (fallback: git reset -p)",
            Style::default().fg(Color::DarkGray),
        )),
        Line::from(Span::styled(
            "Unstage all:   git restore --staged . (fallback: git reset)",
            Style::default().fg(Color::DarkGray),
        )),
    ]))
    .block(details_block)
    .wrap(Wrap { trim: true });

    f.render_widget(details, cols[1]);
}

fn draw_diff_tab(f: &mut Frame<'_>, app: &mut App, area: Rect) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(44), Constraint::Min(1)])
        .split(area);

    let left = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(7), Constraint::Length(7), Constraint::Min(1)])
        .split(cols[0]);

    // Context panel for Diff tab
    let info_block = Block::default()
        .title(" Diff ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray));

    let info_text = Text::from(vec![
        Line::from(vec![
            Span::styled("Source: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                truncate_to_width(app.diff_view_source.label(), 28),
                Style::default().fg(Color::White),
            ),
        ]),
        Line::from(vec![
            Span::styled("Scroll: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                app.diff_scroll.to_string(),
                Style::default().fg(Color::White),
            ),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "Tip: Tab to focus Actions, then ↑/↓ and Enter.",
            Style::default().fg(Color::DarkGray),
        )),
        Line::from(Span::styled(
            "When not in Actions: ↑/↓ scroll, PgUp/PgDn faster, Home top.",
            Style::default().fg(Color::DarkGray),
        )),
    ]);

    f.render_widget(
        Paragraph::new(info_text)
            .block(info_block)
            .wrap(Wrap { trim: true }),
        left[0],
    );

    // Actions list on Diff tab (selectable)
    render_actions_list(f, app, left[1]);
    render_log_panel(f, app, left[2]);

    // Right: scrollable diff viewer
    let viewer_block = Block::default()
        .title(" Diff Viewer ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray));

    // Basic scrolling by lines.
    // Keep allocations proportional to the viewport rather than the whole diff.
    let total = app.diff_text.lines().count();

    let viewport_h = cols[1].height.saturating_sub(2) as usize; // account for borders
    let max_scroll = total.saturating_sub(viewport_h);

    let scroll = app.diff_scroll.min(max_scroll);

    let visible: Vec<Line> = if total == 0 {
        vec![Line::from(Span::styled(
            "[no diff loaded]",
            Style::default().fg(Color::DarkGray),
        ))]
    } else {
        app.diff_text
            .lines()
            .skip(scroll)
            .take(viewport_h)
            .map(|l| Line::from(Span::raw(l)))
            .collect()
    };

    let p = Paragraph::new(visible)
        .block(viewer_block)
        .wrap(Wrap { trim: false });

    f.render_widget(p, cols[1]);
}

fn draw_push_tab(f: &mut Frame<'_>, app: &mut App, area: Rect) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(44), Constraint::Min(1)])
        .split(area);

    let left = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(8), Constraint::Length(7), Constraint::Min(1)])
        .split(cols[0]);

    let info_block = Block::default()
        .title(" Push ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray));

    let info_text = Text::from(vec![
        Line::from(Span::styled(
            "Push branch and/or tags to remote.",
            Style::default().fg(Color::White),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "Tip: pushing v* tags triggers the Release workflow.",
            Style::default().fg(Color::DarkGray),
        )),
        Line::from(Span::styled(
            "Use 'Push specific tag' for safer releases.",
            Style::default().fg(Color::DarkGray),
        )),
    ]);

    f.render_widget(
        Paragraph::new(info_text)
            .block(info_block)
            .wrap(Wrap { trim: true }),
        left[0],
    );

    render_actions_list(f, app, left[1]);
    render_log_panel(f, app, left[2]);

    let details_block = Block::default()
        .title(" Notes ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray));

    let details = Paragraph::new(Text::from(vec![
        Line::from(Span::styled(
            "Push branch:",
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            "  - pushes current branch (sets upstream if missing)",
            Style::default().fg(Color::DarkGray),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "Push all tags:",
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            "  - runs git push --tags (may trigger releases)",
            Style::default().fg(Color::DarkGray),
        )),
    ]))
    .block(details_block)
    .wrap(Wrap { trim: true });

    f.render_widget(details, cols[1]);
}

fn draw_release_tab(f: &mut Frame<'_>, app: &mut App, area: Rect) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(44), Constraint::Min(1)])
        .split(area);

    let left = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(10), Constraint::Length(7), Constraint::Min(1)])
        .split(cols[0]);

    let info_block = Block::default()
        .title(" Release (CI) ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray));

    let pending = app
        .pending_release_version
        .as_ref()
        .map(|v| format!("v{}", v))
        .unwrap_or_else(|| "-".to_string());

    let info_text = Text::from(vec![
        Line::from(Span::styled(
            "This triggers GitHub Actions via tag push (v*).",
            Style::default().fg(Color::White),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled("Pending: ", Style::default().fg(Color::DarkGray)),
            Span::styled(pending, Style::default().fg(Color::White)),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "Guards: clean tree, origin exists, branch check, preflight checks.",
            Style::default().fg(Color::DarkGray),
        )),
    ]);

    f.render_widget(
        Paragraph::new(info_text)
            .block(info_block)
            .wrap(Wrap { trim: true }),
        left[0],
    );

    render_actions_list(f, app, left[1]);
    render_log_panel(f, app, left[2]);

    let details_block = Block::default()
        .title(" Flow ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray));

    let details = Paragraph::new(Text::from(vec![
        Line::from(Span::styled(
            "1) Preflight: fmt/clippy/test (before bump)",
            Style::default().fg(Color::DarkGray),
        )),
        Line::from(Span::styled(
            "2) Bump Cargo.toml + lockfile, stage + commit",
            Style::default().fg(Color::DarkGray),
        )),
        Line::from(Span::styled(
            "3) Tag vX.Y.Z and push tag to origin",
            Style::default().fg(Color::DarkGray),
        )),
        Line::from(Span::styled(
            "4) CI builds release assets + publishes to crates.io",
            Style::default().fg(Color::DarkGray),
        )),
    ]))
    .block(details_block)
    .wrap(Wrap { trim: true });

    f.render_widget(details, cols[1]);
}

fn draw_config_tab(f: &mut Frame<'_>, app: &mut App, area: Rect) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(44), Constraint::Min(1)])
        .split(area);

    let left = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(9), Constraint::Length(7), Constraint::Min(1)])
        .split(cols[0]);

    let info_block = Block::default()
        .title(" Config ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray));

    let info_text = Text::from(vec![
        Line::from(vec![
            Span::styled("Provider: ", Style::default().fg(Color::DarkGray)),
            Span::styled(&app.provider_label, Style::default().fg(Color::White)),
        ]),
        Line::from(vec![
            Span::styled("Model:    ", Style::default().fg(Color::DarkGray)),
            Span::styled(&app.model_label, Style::default().fg(Color::White)),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "Run setup wizard to configure provider + API key.",
            Style::default().fg(Color::DarkGray),
        )),
        Line::from(Span::styled(
            "Tip: Setup runs outside TUI and then returns here.",
            Style::default().fg(Color::DarkGray),
        )),
    ]);

    f.render_widget(
        Paragraph::new(info_text)
            .block(info_block)
            .wrap(Wrap { trim: true }),
        left[0],
    );

    render_actions_list(f, app, left[1]);
    render_log_panel(f, app, left[2]);

    let details_block = Block::default()
        .title(" Notes ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray));

    let details = Paragraph::new(Text::from(vec![
        Line::from(Span::styled(
            "Run setup wizard:",
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            "  - choose provider + model",
            Style::default().fg(Color::DarkGray),
        )),
        Line::from(Span::styled(
            "  - enter API key",
            Style::default().fg(Color::DarkGray),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "Clear config deletes local config file.",
            Style::default().fg(Color::DarkGray),
        )),
    ]))
    .block(details_block)
    .wrap(Wrap { trim: true });

    f.render_widget(details, cols[1]);
}

fn render_actions_list(f: &mut Frame<'_>, app: &App, area: Rect) {
    // Highlight the Actions panel border when focused so it's obvious where ↑/↓/Enter apply.
    let border_style = if app.focus == Focus::LeftPane {
        Style::default().fg(Color::White)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let actions_block = Block::default()
        .title(" Actions ")
        .borders(Borders::ALL)
        .border_style(border_style);

    let actions = app.actions_for_active_tab();
    let items: Vec<ListItem> = actions
        .iter()
        .enumerate()
        .map(|(idx, item)| {
            let is_selected = idx == app.action_index && app.focus == Focus::LeftPane;
            let prefix = if is_selected { "› " } else { "  " };

            let style = if is_selected {
                Style::default().fg(Color::Black).bg(Color::White)
            } else {
                Style::default().fg(Color::White)
            };

            ListItem::new(Line::from(Span::styled(
                format!("{}{}", prefix, item.label()),
                style,
            )))
        })
        .collect();

    let help_hint = if app.focus == Focus::LeftPane {
        "↑/↓ select  Enter run  Tab focus"
    } else {
        "Tab → Actions, then ↑/↓ and Enter   ? help"
    };

    let list = List::new(items)
        .block(actions_block)
        .style(Style::default().fg(Color::White))
        .highlight_style(Style::default().fg(Color::Black).bg(Color::White));

    f.render_widget(list, area);

    let hint_rect = Rect {
        x: area.x + 1,
        y: area.y + area.height.saturating_sub(1),
        width: area.width.saturating_sub(2),
        height: 1,
    };
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            help_hint,
            Style::default().fg(Color::DarkGray),
        ))),
        hint_rect,
    );
}

fn render_log_panel(f: &mut Frame<'_>, app: &App, area: Rect) {
    let log_block = Block::default()
        .title(" Log ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray));

    let log_lines: Vec<Line> = app
        .logs
        .iter()
        .rev()
        .take(12)
        .rev()
        .map(|s| Line::from(Span::raw(s.as_str())))
        .collect();

    f.render_widget(
        Paragraph::new(log_lines)
            .block(log_block)
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn draw_footer(f: &mut Frame<'_>, app: &App, area: Rect) {
    let (label, color) = match &app.status {
        Some(s) => match s.level {
            StatusLevel::Info => ("INFO", Color::Cyan),
            StatusLevel::Success => ("OK", Color::Green),
            StatusLevel::Error => ("ERR", Color::Red),
        },
        None => ("", Color::DarkGray),
    };

    let msg = app
        .status
        .as_ref()
        .map(|s| s.message.as_str())
        .unwrap_or("");

    // Render a lightweight progress indicator when a background task is running.
    //
    // Note: the `tasks` module exposes helper functions for spinner frames and elapsed formatting.
    // The actual running task state is stored on the App (set by the TUI runtime).
    let progress_spans = if let Some(task) = app.running_task.as_ref() {
        let frames = spinner_frames();
        let spinner = frames[task.spinner_index % frames.len()];
        let elapsed = format_elapsed(task.started_at.elapsed());
        vec![
            Span::raw("  "),
            Span::styled(
                format!("{} {}", spinner, task.label),
                Style::default().fg(Color::White),
            ),
            Span::raw(" "),
            Span::styled(
                format!("({})", elapsed),
                Style::default().fg(Color::DarkGray),
            ),
        ]
    } else {
        vec![]
    };

    let mut line1_spans = vec![
        Span::styled(
            format!(" {} ", label),
            Style::default().fg(Color::Black).bg(color),
        ),
        Span::raw(" "),
        Span::styled(msg, Style::default().fg(Color::White)),
    ];
    line1_spans.extend(progress_spans);

    let line2_spans = vec![Span::styled(
        "←/→:Tabs  Alt+←/→:Tabs  Enter:Run/Commit  Tab:Focus  ?:Help  Esc:Quit",
        Style::default().fg(Color::DarkGray),
    )];

    let footer = Paragraph::new(Text::from(vec![
        Line::from(line1_spans),
        Line::from(line2_spans),
    ]))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray)),
        )
        .wrap(Wrap { trim: true });

    f.render_widget(footer, area);
}

fn draw_help_modal(f: &mut Frame<'_>, app: &App, area: Rect) {
    let width = (area.width as f32 * 0.70) as u16;
    let height = (area.height as f32 * 0.70) as u16;

    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;

    let modal = Rect {
        x,
        y,
        width,
        height,
    };

    // Make the modal opaque by clearing anything behind it first.
    f.render_widget(Clear, modal);

    let mut lines: Vec<Line> = vec![
        Line::from(Span::styled(
            "Git Wiz — Help",
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled("Global: ", Style::default().fg(Color::DarkGray)),
            Span::styled("Esc", Style::default().fg(Color::White)),
            Span::styled(" quit  ", Style::default().fg(Color::DarkGray)),
            Span::styled("Ctrl+C", Style::default().fg(Color::White)),
            Span::styled(" quit  ", Style::default().fg(Color::DarkGray)),
            Span::styled("?", Style::default().fg(Color::White)),
            Span::styled(" toggle help", Style::default().fg(Color::DarkGray)),
        ]),
        Line::from(vec![
            Span::styled("Tabs:   ", Style::default().fg(Color::DarkGray)),
            Span::styled("←/→", Style::default().fg(Color::White)),
            Span::styled(
                " switch (when not editing)  ",
                Style::default().fg(Color::DarkGray),
            ),
            Span::styled("Alt+←/→", Style::default().fg(Color::White)),
            Span::styled(" always switch", Style::default().fg(Color::DarkGray)),
        ]),
        Line::from(vec![
            Span::styled("Focus:  ", Style::default().fg(Color::DarkGray)),
            Span::styled("Tab", Style::default().fg(Color::White)),
            Span::styled(
                " cycle focus (TabBar / panels / editor)",
                Style::default().fg(Color::DarkGray),
            ),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            format!("Current tab: {}", app.active_tab.title()),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
    ];

    match app.active_tab {
        Tab::Generate => {
            lines.extend([
                Line::from(vec![
                    Span::styled("Generate: ", Style::default().fg(Color::DarkGray)),
                    Span::styled("g", Style::default().fg(Color::White)),
                    Span::styled(
                        " generate commit message from staged changes",
                        Style::default().fg(Color::DarkGray),
                    ),
                ]),
                Line::from(vec![
                    Span::styled("Commit:   ", Style::default().fg(Color::DarkGray)),
                    Span::styled("Enter", Style::default().fg(Color::White)),
                    Span::styled(
                        " commit using the textarea content",
                        Style::default().fg(Color::DarkGray),
                    ),
                ]),
                Line::from(vec![
                    Span::styled("Clear:    ", Style::default().fg(Color::DarkGray)),
                    Span::styled("c", Style::default().fg(Color::White)),
                    Span::styled(
                        " clear the commit message editor",
                        Style::default().fg(Color::DarkGray),
                    ),
                ]),
                Line::from(""),
                Line::from(Span::styled(
                    "Tip: When the editor is focused, arrow keys move the cursor.",
                    Style::default().fg(Color::DarkGray),
                )),
                Line::from(Span::styled(
                    "Use Alt+←/→ to switch tabs anytime.",
                    Style::default().fg(Color::DarkGray),
                )),
            ]);
        }
        _ => {
            lines.push(Line::from(Span::styled(
                "This tab is wired via the Actions list. Tab focus to Actions and press Enter.",
                Style::default().fg(Color::DarkGray),
            )));
        }
    }

    let block = Block::default()
        .title(" Help ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::White));

    let p = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: true })
        .style(Style::default().fg(Color::White).bg(Color::Black));

    f.render_widget(p, modal);
}

fn draw_app_modal(f: &mut Frame<'_>, app: &App, area: Rect) {
    // Centered modal (slightly smaller than help)
    let width = (area.width as f32 * 0.55) as u16;
    let height = (area.height as f32 * 0.35) as u16;

    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;

    let modal = Rect {
        x,
        y,
        width,
        height,
    };

    f.render_widget(Clear, modal);

    let title = if app.modal.title.is_empty() {
        " Dialog ".to_string()
    } else {
        format!(" {} ", app.modal.title)
    };

    let border = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::White));

    match app.modal.kind {
        ModalKind::Confirm => {
            let lines = vec![
                Line::from(Span::styled(
                    &app.modal.message,
                    Style::default().fg(Color::White),
                )),
                Line::from(""),
                Line::from(Span::styled(
                    "Enter: confirm   Esc: cancel",
                    Style::default().fg(Color::DarkGray),
                )),
            ];

            let p = Paragraph::new(lines)
                .block(border)
                .wrap(Wrap { trim: true })
                .style(Style::default().fg(Color::White).bg(Color::Black));

            f.render_widget(p, modal);
        }
        ModalKind::TextInput => {
            // Render message + a simple input box line
            let prompt_lines = vec![
                Line::from(Span::styled(
                    &app.modal.message,
                    Style::default().fg(Color::White),
                )),
                Line::from(""),
                Line::from(vec![
                    Span::styled("Input: ", Style::default().fg(Color::DarkGray)),
                    Span::styled(&app.modal.input_value, Style::default().fg(Color::White)),
                ]),
                Line::from(""),
                Line::from(Span::styled(
                    "Type, Backspace to edit. Enter: accept   Esc: cancel",
                    Style::default().fg(Color::DarkGray),
                )),
            ];

            let p = Paragraph::new(prompt_lines)
                .block(border)
                .wrap(Wrap { trim: true })
                .style(Style::default().fg(Color::White).bg(Color::Black));

            f.render_widget(p, modal);
        }
        ModalKind::None => {}
    }
}

fn truncate_to_width(s: &str, max: usize) -> String {
    if UnicodeWidthStr::width(s) <= max {
        return s.to_string();
    }
    if max <= 1 {
        return "…".to_string();
    }

    let mut out = String::new();
    let mut width = 0usize;
    for ch in s.chars() {
        let ch_w = UnicodeWidthChar::width(ch).unwrap_or(0);
        if width + ch_w >= max.saturating_sub(1) {
            break;
        }
        out.push(ch);
        width += ch_w;
    }
    out.push('…');
    out
}
