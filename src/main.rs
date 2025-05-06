use crossterm::{
    event::{self, Event, KeyCode},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::prelude::Stylize;
use ratatui::{prelude::*, widgets::*};
use std::{error::Error, time::Duration};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, RefreshKind, System};

#[derive(Debug)]
struct State {
    system: System,
    paused: bool,
    plot_x: f64,
    cpu_usage_all: Vec<(f64, f64)>,
    mem_usage_all: Vec<(f64, f64)>,
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

        let start_vec: Vec<(f64, f64)> = (0..200).map(|v| (v as f64, 0.0)).collect();

        Self {
            system,
            paused: false,
            plot_x: 200.0,
            cpu_usage_all: start_vec.clone(),
            mem_usage_all: start_vec,
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

            self.plot_x += 1.0;

            self.system.total_memory();

            self.cpu_usage_all.drain(0..1);
            self.cpu_usage_all
                .push((self.plot_x, self.system.global_cpu_usage() as f64));

            self.mem_usage_all.drain(0..1);
            self.mem_usage_all.push((
                self.plot_x,
                self.system.used_memory() as f64 / (1024 * 1024) as f64,
            ));
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

fn mem_human_readable(bytes: u64) -> String {
    if bytes < 10240 {
        return format!("{bytes}");
    }
    if bytes < 1024 * 1024 * 10 {
        return format!("{} K", bytes / 1024);
    }
    if bytes < 1024 * 1024 * 1024 * 10 {
        return format!("{} M", bytes / (1024 * 1024));
    }
    if bytes < 1024 * 1024 * 1024 * 1024 * 10 {
        return format!("{} G", bytes / (1024 * 1024 * 1024));
    }
    format!("{} T", bytes / (1024 * 1024 * 1024 * 1024))
}

fn ui(frame: &mut Frame, state: &State) {
    match 2 {
        1 => {
            let layout = Layout::default()
                .direction(Direction::Vertical)
                .margin(1)
                .constraints([Constraint::Length(3), Constraint::Min(5)])
                .split(frame.area());

            // Split upper rect
            let upper = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                .split(layout[0]);

            // Memory gauge
            let gauge = gauge_mem_simple(state);
            frame.render_widget(gauge, upper[1]);

            // CPU gauge
            let gauge = gauge_cpu_simple(state);
            frame.render_widget(gauge, upper[0]);

            let table = table_widget_processes(state);
            frame.render_widget(table, layout[1]);
        }
        2 => {
            let layout = Layout::default()
                .direction(Direction::Vertical)
                .margin(1)
                .constraints([Constraint::Length(20), Constraint::Min(5)])
                .split(frame.area());

            // Split upper rect
            let upper = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                .split(layout[0]);

            // Memory gauge
            let plot = plot_cpu_global(state);
            frame.render_widget(plot, upper[0]);

            // CPU gauge
            let gauge = gauge_mem_simple(state);
            frame.render_widget(gauge, upper[1]);

            let table = table_widget_processes(state);
            frame.render_widget(table, layout[1]);
        }
        _ => {}
    }
}

fn plot_cpu_global(state: &State) -> Chart {
    let datasets = vec![
        Dataset::default()
            .name("Total")
            .marker(symbols::Marker::Braille)
            .graph_type(GraphType::Bar)
            .style(Style::default().fg(Color::Green))
            .data(&state.cpu_usage_all),
    ];
    let last_cpu_usage = state.cpu_usage_all.last().unwrap().1;
    Chart::new(datasets)
        .block(Block::bordered().title(format!("CPU usage: {:.0}% total", last_cpu_usage)))
        .x_axis(Axis::default().bounds([
            state.cpu_usage_all.first().unwrap().0,
            state.cpu_usage_all.last().unwrap().0,
        ]))
        .y_axis(
            Axis::default()
                .bounds([0.0, 100.0])
                .style(Style::default().gray())
                .labels([
                    "0".bold(),
                    "25".into(),
                    "50".bold(),
                    "75".into(),
                    "100".bold(),
                ]),
        )
}

fn table_widget_processes(state: &State) -> Table {
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
    table
}

///
///
fn gauge_cpu_simple(state: &State) -> Gauge {
    let cpu_usage = state.system.global_cpu_usage() as f64;
    let gauge = Gauge::default()
        .block(Block::new().title("CPU").borders(Borders::ALL))
        .gauge_style(Style::new().fg(Color::Magenta))
        .ratio(cpu_usage / 100.0)
        .label(format!("{:.1}%", cpu_usage));
    gauge
}

fn gauge_mem_simple(state: &State) -> Gauge {
    let memory_usage = state.system.used_memory() as f64 / state.system.total_memory() as f64;
    let gauge = Gauge::default()
        .block(Block::new().title("Memory").borders(Borders::ALL))
        .gauge_style(Style::new().fg(Color::Magenta))
        .ratio(memory_usage)
        .label(format!("{:.1}%", memory_usage * 100.0));
    gauge
}
