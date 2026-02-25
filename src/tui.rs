#![cfg(feature = "tui")]

use crate::{app, config};
use anyhow::Context;
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::{execute, terminal};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Text};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Terminal;
use std::io;
use tokio::sync::mpsc;

#[derive(Debug, Clone)]
struct ChatLine {
    role: &'static str,
    text: String,
}

#[derive(Debug, Clone)]
enum StreamMsg {
    Chunk(String),
    Done,
    Error(String),
}

pub async fn run_tui(cfg: Option<&config::Config>, model_override: Option<String>) -> anyhow::Result<()> {
    let http = reqwest::Client::builder()
        .user_agent(concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION")))
        .build()
        .context("failed to build HTTP client")?;

    let provider_name = cfg
        .and_then(|c| c.provider.clone())
        .unwrap_or_else(|| "google".to_string());
    let provider = app::build_provider(&http, cfg, &provider_name).await?;

    let mut model = model_override
        .or_else(|| cfg.and_then(|c| c.model.clone()))
        .unwrap_or_else(|| "gemini-1.5-flash".to_string());

    enable_raw_mode().context("enable raw mode")?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen).context("enter alt screen")?;
    terminal::enable_raw_mode().ok();

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).context("create terminal")?;

    let (ev_tx, mut ev_rx) = mpsc::unbounded_channel::<Event>();
    std::thread::spawn(move || {
        loop {
            match crossterm::event::read() {
                Ok(ev) => {
                    if ev_tx.send(ev).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    let mut input = String::new();
    let mut lines: Vec<ChatLine> = vec![ChatLine {
        role: "system",
        text: "Type a message and press Enter. Commands: /quit, /clear, /model <name>".to_string(),
    }];

    let mut active_stream: Option<mpsc::UnboundedReceiver<StreamMsg>> = None;

    let mut ticker = tokio::time::interval(std::time::Duration::from_millis(33));

    let res = loop {
        tokio::select! {
            _ = ticker.tick() => {
                if let Err(e) = draw(&mut terminal, &model, &lines, &input) {
                    break Err(e);
                }
            }
            Some(ev) = ev_rx.recv() => {
                match ev {
                    Event::Key(key) => {
                        if handle_key(key, &mut input, &mut lines, &mut model, &provider, &mut active_stream).await? {
                            break Ok(());
                        }
                    }
                    Event::Resize(_, _) => {}
                    _ => {}
                }
            }
            Some(msg) = async {
                match &mut active_stream {
                    Some(rx) => rx.recv().await,
                    None => None,
                }
            } => {
                match msg {
                    StreamMsg::Chunk(t) => {
                        if let Some(last) = lines.last_mut() {
                            if last.role == "assistant" {
                                last.text.push_str(&t);
                            }
                        }
                    }
                    StreamMsg::Done => {
                        active_stream = None;
                    }
                    StreamMsg::Error(e) => {
                        active_stream = None;
                        lines.push(ChatLine{role:"error", text: e});
                    }
                }
            }
        }
    };

    disable_raw_mode().ok();
    execute!(terminal.backend_mut(), LeaveAlternateScreen).ok();
    terminal.show_cursor().ok();

    res
}

async fn handle_key(
    key: KeyEvent,
    input: &mut String,
    lines: &mut Vec<ChatLine>,
    model: &mut String,
    provider: &Box<dyn crate::provider::Provider + Send + Sync>,
    active_stream: &mut Option<mpsc::UnboundedReceiver<StreamMsg>>,
) -> anyhow::Result<bool> {
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        return Ok(true);
    }

    match key.code {
        KeyCode::Esc => return Ok(true),
        KeyCode::Char(c) => input.push(c),
        KeyCode::Backspace => {
            input.pop();
        }
        KeyCode::Enter => {
            let msg = input.trim().to_string();
            input.clear();
            if msg.is_empty() {
                return Ok(false);
            }

            if msg == "/quit" {
                return Ok(true);
            }
            if msg == "/clear" {
                lines.clear();
                return Ok(false);
            }
            if let Some(rest) = msg.strip_prefix("/model ") {
                *model = rest.trim().to_string();
                lines.push(ChatLine{role:"system", text: format!("model set to: {}", model)});
                return Ok(false);
            }

            if active_stream.is_some() {
                lines.push(ChatLine{role:"system", text: "(streaming in progress; wait for completion)".to_string()});
                return Ok(false);
            }

            lines.push(ChatLine{role:"user", text: msg.clone()});
            lines.push(ChatLine{role:"assistant", text: String::new()});

            let req = crate::provider::ChatRequest {
                model: model.clone(),
                prompt: msg,
                include_directories: Vec::new(),
            };

            let mut stream = provider
                .stream_chat(req)
                .await
                .context("failed to start stream")?;

            let (tx, rx) = mpsc::unbounded_channel::<StreamMsg>();
            *active_stream = Some(rx);

            tokio::spawn(async move {
                use tokio_stream::StreamExt;
                while let Some(item) = stream.next().await {
                    match item {
                        Ok(chunk) => {
                            if tx.send(StreamMsg::Chunk(chunk.text)).is_err() {
                                return;
                            }
                        }
                        Err(e) => {
                            let _ = tx.send(StreamMsg::Error(format!("{e:#}")));
                            return;
                        }
                    }
                }
                let _ = tx.send(StreamMsg::Done);
            });
        }
        _ => {}
    }

    Ok(false)
}

fn draw(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    model: &str,
    lines: &[ChatLine],
    input: &str,
) -> anyhow::Result<()> {
    terminal.draw(|f| {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(3)])
            .split(f.area());

        let mut text = Text::default();
        for l in lines {
            let role = format!("{}: ", l.role);
            let style = match l.role {
                "user" => Style::default().add_modifier(Modifier::BOLD),
                "assistant" => Style::default(),
                "error" => Style::default().add_modifier(Modifier::BOLD),
                _ => Style::default(),
            };
            text.lines.push(Line::styled(role, style));
            text.lines.extend(Text::from(l.text.clone()).lines);
            text.lines.push(Line::from(""));
        }

        let chat = Paragraph::new(text)
            .block(Block::default().borders(Borders::ALL).title(format!("gemini tui — model: {model}")))
            .wrap(Wrap { trim: false });

        let input_w = Paragraph::new(input.to_string())
            .block(Block::default().borders(Borders::ALL).title("input"));

        f.render_widget(chat, chunks[0]);
        f.render_widget(input_w, chunks[1]);

        let x = chunks[1].x + 1 + input.chars().count() as u16;
        let y = chunks[1].y + 1;
        f.set_cursor_position((x.min(chunks[1].x + chunks[1].width.saturating_sub(2)), y));
    })?;
    Ok(())
}
