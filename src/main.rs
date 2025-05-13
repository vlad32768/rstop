use crossterm::event::{self, Event, KeyCode};
use ratatui::prelude::Stylize;
use ratatui::{prelude::*, widgets::*};
use std::time::Instant;
use std::usize;
use std::{error::Error, time::Duration};
use sysinfo::{DiskUsage, ProcessRefreshKind, ProcessesToUpdate, System};

#[derive(Debug)]
enum SortBy {
    Name,
    CPU,
    PID,
    Memory,
}

#[derive(Debug)]
struct State {
    system: System,
    paused: bool,
    sort_by: SortBy,
    plot_x: f64,
    cpu_usage_all: Vec<(f64, f64)>,
    mem_usage_all: Vec<(f64, f64)>,
    t_state: TableState,
    sb_state: ScrollbarState,
    processes_data: ProcessesData,
}

type ProcessesData = Vec<(u32, String, f32, u64, DiskUsage)>;
fn create_processes_data(system: &System) -> ProcessesData {
    system
        .processes()
        .values()
        .map(|process| {
            (
                process.pid().as_u32(),
                process.name().to_str().unwrap().to_string(),
                process.cpu_usage(),
                process.memory(),
                process.disk_usage(),
            )
        })
        .collect()
}

impl State {
    pub fn new() -> Self {
        let mut system = System::new();
        system.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::everything(),
        );
        system.refresh_memory();

        let start_vec: Vec<(f64, f64)> = (0..200).map(|v| (v as f64, 0.0)).collect();

        let pd_start = create_processes_data(&system);

        Self {
            system,
            paused: false,
            plot_x: 199.0,
            sort_by: SortBy::CPU,
            cpu_usage_all: start_vec.clone(),
            mem_usage_all: start_vec,
            t_state: TableState::default().with_selected(0),
            sb_state: ScrollbarState::new(pd_start.len()).position(0),
            processes_data: pd_start,
        }
    }

    pub fn refresh(&mut self) {
        if !self.paused {
            self.system.refresh_processes_specifics(
                ProcessesToUpdate::All,
                true,
                ProcessRefreshKind::everything(),
            );
            self.system.refresh_memory();
            // self.system
            //     .refresh_cpu_specifics(CpuRefreshKind::everything());
            self.system.refresh_cpu_usage();
            //self.system.refresh_all();

            self.plot_x += 1.0;

            self.system.total_memory();

            self.cpu_usage_all.drain(0..1);
            self.cpu_usage_all
                .push((self.plot_x, self.system.global_cpu_usage() as f64));

            self.mem_usage_all.drain(0..1);
            self.mem_usage_all
                .push((self.plot_x, self.system.used_memory() as f64));

            //------------ update process table
            // save currently selected pid
            let selected_pid = if let Some(idx) = self.t_state.selected() {
                self.processes_data[idx].0
            } else {
                unreachable!();
            };

            // new process data + sort
            self.processes_data = create_processes_data(&self.system);
            self.sort_process_data();

            //restore the previous selection
            let new_selected_idx = if let Some(elem) = self
                .processes_data
                .iter()
                .enumerate()
                .find(|v| v.1.0 == selected_pid)
            {
                elem.0
            } else {
                0
            };
            self.t_state.select(Some(new_selected_idx));
            self.sb_state = self
                .sb_state
                .content_length(self.processes_data.len())
                .position(new_selected_idx);
        }
    }

    /// Calculates starting data index for plots
    pub fn start_data_idx(&self, r: Rect) -> usize {
        // actual data width for the plot = rect width - 2 borders - 3 digits - 1 axis -

        let widget_data_width = 2 * (r.width - 6) as usize;

        if self.cpu_usage_all.len() < widget_data_width {
            0
        } else {
            self.cpu_usage_all.len() - widget_data_width.clamp(0, usize::MAX)
        }
    }

    pub fn next_row(&mut self) {
        let i = match self.t_state.selected() {
            Some(i) => {
                if i >= self.processes_data.len() - 1 {
                    self.processes_data.len() - 1
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.t_state.select(Some(i));
        self.sb_state = self.sb_state.position(i);
    }

    pub fn previous_row(&mut self) {
        let i = match self.t_state.selected() {
            Some(i) => {
                if i == 0 {
                    0
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.t_state.select(Some(i));
        self.sb_state = self.sb_state.position(i);
    }
    fn sort_process_data(&mut self) {
        self.processes_data.sort_by(|a, b| match self.sort_by {
            SortBy::Name => b.1.cmp(&a.1),
            SortBy::CPU => b.2.total_cmp(&a.2),
            SortBy::PID => b.0.cmp(&a.0),
            SortBy::Memory => b.3.cmp(&a.3),
        });
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut terminal = ratatui::init();

    let mut state = State::new();

    let tick_rate = Duration::from_millis(1000);
    let mut last_tick = Instant::now();

    let result = loop {
        terminal.draw(|frame| ui(frame, &mut state))?;

        let timeout = tick_rate.saturating_sub(last_tick.elapsed());

        if event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Char('q') => break Ok(()),
                    KeyCode::Char(' ') => state.paused = !state.paused,
                    KeyCode::Char('c') => state.sort_by = SortBy::CPU,
                    KeyCode::Char('m') => state.sort_by = SortBy::Memory,
                    KeyCode::Char('n') => state.sort_by = SortBy::Name,
                    KeyCode::Char('p') => state.sort_by = SortBy::PID,
                    KeyCode::Up => state.previous_row(),
                    KeyCode::Down => state.next_row(),
                    _ => {}
                }
            }
        }
        if last_tick.elapsed() >= tick_rate {
            state.refresh();
            last_tick = Instant::now();
        }
    };

    ratatui::restore();
    result
}

/// returns truncated human-readable memory size + unit str
fn mem_human_readable(bytes: u64) -> (String, &'static str) {
    if bytes < 10240 {
        return (format!("{bytes}"), "");
    }
    if bytes < 1024 * 1024 * 10 {
        return (format!("{}", bytes / 1024), "K");
    }
    if bytes < 1024 * 1024 * 1024 * 10 {
        return (format!("{}", bytes / (1024 * 1024)), "M");
    }
    if bytes < 1024 * 1024 * 1024 * 1024 * 10 {
        return (
            format!("{:.1}", bytes as f64 / (1024 * 1024 * 1024) as f64),
            "G",
        );
    }
    (format!("{}", bytes / (1024 * 1024 * 1024 * 1024)), "T")
}

fn ui(frame: &mut Frame, state: &mut State) {
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

            render_table_widget_processes(state, frame, layout[1]);
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

            // CPU Plot
            render_plot_cpu_global(state, frame, upper[0]);

            // Mem plot
            let mem_start_idx = state.start_data_idx(upper[1]);
            let mem_plot = plot_mem(state, mem_start_idx);
            frame.render_widget(mem_plot, upper[1]);

            render_table_widget_processes(state, frame, layout[1]);
        }
        _ => {}
    }
}

fn render_plot_cpu_global(state: &State, frame: &mut Frame, area: Rect) {
    let start_data_idx = state.start_data_idx(area);
    let dataslice = &state.cpu_usage_all[start_data_idx..];
    let datasets = vec![
        Dataset::default()
            .name("Total")
            .marker(symbols::Marker::Braille)
            .graph_type(GraphType::Bar)
            .style(Style::default().fg(Color::Green))
            .data(dataslice),
    ];
    let last_cpu_usage = state.cpu_usage_all.last().unwrap().1;
    let cpu_frequency: f64 = state
        .system
        .cpus()
        .iter()
        .map(|cpu| cpu.frequency())
        .sum::<u64>() as f64
        / state.system.cpus().len() as f64;
    //let cpu_frequency = state.system.cpus()[0].frequency();

    let chart = Chart::new(datasets)
        .block(Block::bordered().title(format!(
            "CPU usage: {:.0}% total, {:.0} MHz",
            last_cpu_usage, cpu_frequency
        )))
        .legend_position(Some(LegendPosition::TopLeft))
        .x_axis(Axis::default().bounds([dataslice.first().unwrap().0, dataslice.last().unwrap().0]))
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
        );
    frame.render_widget(chart, area);
}

fn plot_mem(state: &State, start_idx: usize) -> Chart {
    let mem_usage_all = &state.mem_usage_all[start_idx..];
    let datasets = vec![
        Dataset::default()
            .name("Memory")
            .marker(symbols::Marker::Braille)
            .graph_type(GraphType::Bar)
            .style(Style::default().fg(Color::Magenta))
            .data(mem_usage_all),
    ];
    //let last_mem_usage = state.mem_usage_all.last().unwrap().1;

    let (total_mem, tot_unit) = mem_human_readable(state.system.total_memory());
    let (used_mem, u_unit) = mem_human_readable(state.system.used_memory());
    Chart::new(datasets)
        .block(Block::bordered().title(format!(
            "Mem usage: {}{}/{}{}",
            used_mem, u_unit, total_mem, tot_unit
        )))
        .legend_position(Some(LegendPosition::TopLeft))
        .x_axis(Axis::default().bounds([
            mem_usage_all.first().unwrap().0,
            mem_usage_all.last().unwrap().0,
        ]))
        .y_axis(
            Axis::default()
                .bounds([0.0, state.system.total_memory() as f64])
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

fn render_table_widget_processes(state: &mut State, frame: &mut Frame, area: Rect) {
    let rows: Vec<Row> = state
        .processes_data
        .iter()
        .map(|(pid, name, cpu, mem, du)| {
            let (mem_str, mem_unit) = mem_human_readable(*mem);
            Row::new(vec![
                format!("{pid}"),
                name.clone(), //fixme
                format!("{:.1}", cpu),
                format!("{} {}", mem_str, mem_unit),
                format!("{}/{}", du.read_bytes, du.written_bytes),
            ])
        })
        .collect();

    let header = Row::new(vec!["PID", "Name", "CPU%", "MEM", "Disk R/W"])
        .style(Style::new().bold())
        .bottom_margin(1);

    let table = Table::new(
        rows,
        [
            Constraint::Length(6),
            Constraint::Length(20),
            Constraint::Length(5),
            Constraint::Length(6),
            Constraint::Percentage(20),
        ],
    )
    .header(header)
    .block(
        Block::new()
            .title("Processes")
            .title(state.processes_data.len().to_string())
            .borders(Borders::ALL),
    )
    .style(Style::new().fg(Color::White))
    .row_highlight_style(Style::default().bg(Color::DarkGray));
    frame.render_stateful_widget(table, area, &mut state.t_state);

    frame.render_stateful_widget(
        Scrollbar::default().orientation(ScrollbarOrientation::VerticalRight), //.begin_symbol(None)
        // .end_symbol(None)
        area.inner(Margin {
            vertical: 1,
            horizontal: 1,
        }),
        &mut state.sb_state,
    );
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
