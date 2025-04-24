use crossterm::{
    event::{self, Event, KeyCode},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{prelude::*, widgets::*};
use std::{error::Error, time::Duration};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, RefreshKind, System};

#[derive(Debug)]
struct State {
    system: System,
    paused: bool,
}

impl State {
    fn new() -> Self {
        let mut system = System::new();
        system.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::everything(),
        );
        system.refresh_memory();
        Self {
            system,
            paused: false,
        }
    }

    fn refresh(&mut self) {
        if !self.paused {
            self.system.refresh_processes_specifics(
                ProcessesToUpdate::All,
                true,
                ProcessRefreshKind::everything(),
            );
            self.system.refresh_memory();
        }
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut terminal = ratatui::init();

    let mut state = State::new();

    let result = loop {
        terminal.draw(|frame| ui(frame, &state))?;

        if event::poll(Duration::from_millis(1000))? {
            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Char('q') => break Ok(()),
                    KeyCode::Char(' ') => state.paused = !state.paused,
                    _ => {}
                }
            }
        }

        state.refresh();
    };

    ratatui::restore();
    result
}

fn mem_human_readable(bytes:u64) -> String {
    if bytes < 10240 {
        return format!("{bytes}")
    }
    if bytes < 1024*1024*10 {
        return format!("{} K",bytes/1024)
    }
    if bytes < 1024*1024*1024*10 {
        return format!("{} M",bytes/(1024*1024))
    }
    if bytes < 1024*1024*1024*1024*10 {
        return format!("{} G",bytes/(1024*1024*1024))
    }
    format!("{} T",bytes/(1024*1024*1024*1024))
}

fn ui(frame: &mut Frame, state: &State) {
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([Constraint::Length(3), Constraint::Min(5)])
        .split(frame.area());

    // Memory gauge
    let memory_usage = state.system.used_memory() as f64 / state.system.total_memory() as f64;
    let gauge = Gauge::default()
        .block(Block::new().title("Memory").borders(Borders::ALL))
        .gauge_style(Style::new().fg(Color::Magenta))
        .ratio(memory_usage)
        .label(format!("{:.1}%", memory_usage * 100.0));
    frame.render_widget(gauge, layout[0]);

    // Processes table
    let processes = state.system.processes();
    let mut processes_data: Vec<_> = processes
        .values()
        .map(|process| {
            (
                process.pid().to_string(),
                process.name().to_str().unwrap().to_string(),
                process.cpu_usage(),
                process.memory(),
            )
        })
        .collect();

    processes_data.sort_by(|a, b| b.2.total_cmp(&a.2));

    let rows: Vec<Row> = processes_data
        .into_iter()
        .map(|(pid, name, cpu, mem)| {
            Row::new(vec![
                pid,
                name,
                format!("{:.1}", cpu),
                mem_human_readable(mem),
            ])
        })
        .collect();

    let header = Row::new(vec!["PID", "Name", "CPU%", "MEM"])
        .style(Style::new().bold())
        .bottom_margin(1);

    let table = Table::new(
        rows,
        [
            Constraint::Percentage(10),
            Constraint::Percentage(40),
            Constraint::Percentage(25),
            Constraint::Percentage(25),
        ],
    )
    .header(header)
    .block(Block::new().title("Processes").borders(Borders::ALL))
    .style(Style::new().fg(Color::White));

    frame.render_widget(table, layout[1]);
}
