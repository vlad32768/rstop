use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::prelude::Stylize;
use ratatui::{prelude::*, widgets::*};
use std::time::Instant;
use std::{error::Error, time::Duration};
use sysinfo::{
    DiskUsage, Pid, Process, ProcessRefreshKind, ProcessesToUpdate, SUPPORTED_SIGNALS, System,
    Users,
};

#[derive(Clone, Copy, PartialEq, Debug)]
enum SortBy {
    Pid,
    User,
    Name,
    Cpu,
    Memory,
    Io,
}

#[derive(Debug)]
enum Mode {
    Normal,
    Kill,
}

#[derive(Debug)]
struct State {
    mode: Mode,
    system: System,
    paused: bool,
    sort_by: SortBy,
    sort_ascending: bool,
    plot_x: f64,
    cpu_usage_all: Vec<(f64, f64)>,
    mem_usage_all: Vec<(f64, f64)>,
    t_state: TableState,
    sb_state: ScrollbarState,
    processes_data: ProcessesData,
    deb_show: bool,
}
/// (pid,uname,proc_name,cpu_usage,mem_usage,disk_usage)
type ProcessesData = Vec<(u32, String, String, f32, u64, DiskUsage)>;
fn create_processes_data(system: &System) -> ProcessesData {
    let users = Users::new_with_refreshed_list();
    system
        .processes()
        .values()
        .map(|process| {
            (
                process.pid().as_u32(),
                {
                    if let Some(uid) = process.user_id() {
                        if let Some(user) = users.get_user_by_id(uid) {
                            user.name().to_string()
                        } else {
                            "[Unknown]".to_string()
                        }
                    } else {
                        "[Unknown]".to_string()
                    }
                },
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
            mode: Mode::Normal,
            system,
            paused: false,
            plot_x: 199.0,
            sort_by: SortBy::Cpu,
            sort_ascending: false,
            cpu_usage_all: start_vec.clone(),
            mem_usage_all: start_vec,
            t_state: TableState::default().with_selected(0),
            sb_state: ScrollbarState::new(pd_start.len()).position(0),
            processes_data: pd_start,

            deb_show: false,
        }
    }

    pub fn get_selected_process(&self) -> Option<&Process> {
        let sel_pid = self.processes_data[self.t_state.selected().unwrap_or(0)].0;
        self.system.process(Pid::from_u32(sel_pid))
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

    /// Calculates starting data index for plots.
    /// If data array fits completely, starting index is 0.
    /// If not, starting index is last - data symbol length.
    /// Data symbol length depends on rect width and addition.
    ///
    /// addition is number of symbols used for non-data drawing.
    ///           Usually 2 * borders + 1 * axis_line + max number of symbols in a label.
    pub fn start_data_idx(&self, r: Rect, addition: usize) -> usize {
        // actual data width for the plot = rect width - 2 borders - 3 digits - 1 axis -

        let widget_data_width = 2 * ((r.width as usize).saturating_sub(addition));

        if self.cpu_usage_all.len() < widget_data_width {
            0
        } else {
            self.cpu_usage_all.len() - widget_data_width.clamp(1, usize::MAX)
        }
    }

    /// Select next row in process table; advance `n` rows
    pub fn next_row(&mut self, n: usize) {
        let i = match self.t_state.selected() {
            Some(i) => {
                if i + n >= self.processes_data.len() - 1 {
                    self.processes_data.len() - 1
                } else {
                    i + n
                }
            }
            None => 0,
        };
        self.select_row(i);
    }

    /// Select previous row in process table; decrease `n` rows
    pub fn previous_row(&mut self, n: usize) {
        let i = match self.t_state.selected() {
            Some(i) => i.saturating_sub(n),
            None => 0,
        };
        self.select_row(i);
    }

    /// Select row by index `n`.
    pub fn select_row(&mut self, row_no: usize) {
        self.t_state.select(Some(row_no));
        self.sb_state = self.sb_state.position(row_no);
    }

    fn sort_process_data(&mut self) {
        self.processes_data.sort_by(|a, b| match self.sort_by {
            SortBy::Pid => b.0.cmp(&a.0),
            SortBy::User => b.1.cmp(&a.1),
            SortBy::Name => b.2.cmp(&a.2),
            SortBy::Cpu => b.3.total_cmp(&a.3),
            SortBy::Memory => b.4.cmp(&a.4),
            SortBy::Io => b.5.read_bytes.cmp(&a.5.read_bytes),
        });
        if self.sort_ascending {
            self.processes_data.reverse();
        }
    }

    fn set_sort_by(&mut self, sort_by: SortBy) {
        if self.sort_by == sort_by {
            self.sort_ascending = !self.sort_ascending;
        } else {
            self.sort_by = sort_by;
        }
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
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Char('q') => break Ok(()),
                        KeyCode::Char(' ') => state.paused = !state.paused,
                        KeyCode::Char('1') => state.set_sort_by(SortBy::Pid),
                        KeyCode::Char('2') => state.set_sort_by(SortBy::User),
                        KeyCode::Char('3') => state.set_sort_by(SortBy::Name),
                        KeyCode::Char('4') => state.set_sort_by(SortBy::Cpu),
                        KeyCode::Char('5') => state.set_sort_by(SortBy::Memory),
                        KeyCode::Char('6') => state.set_sort_by(SortBy::Io),
                        KeyCode::Char('k') => state.mode = Mode::Kill,
                        KeyCode::Char('y') => match state.mode {
                            Mode::Kill => {
                                state.get_selected_process().unwrap().kill();
                                state.mode = Mode::Normal;
                            }
                            _ => {}
                        },
                        KeyCode::Char('n') => state.mode = Mode::Normal,
                        KeyCode::Char('d') => state.deb_show = !state.deb_show,
                        KeyCode::Up => state.previous_row(1),
                        KeyCode::Down => state.next_row(1),
                        KeyCode::PageUp => state.previous_row(10),
                        KeyCode::PageDown => state.next_row(10),
                        KeyCode::Home => state.select_row(0),
                        KeyCode::End => state.select_row(state.processes_data.len() - 1),
                        _ => {}
                    }
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
                .margin(0)
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

            render_plot_mem(state, frame, upper[1]);
            render_table_widget_processes(state, frame, layout[1]);
        }
        _ => {}
    }
}

fn render_plot_cpu_global(state: &State, frame: &mut Frame, area: Rect) {
    let start_data_idx = state.start_data_idx(area, 6);
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
    // let cpu_frequency: f64 = state
    //     .system
    //     .cpus()
    //     .iter()
    //     .map(|cpu| cpu.frequency())
    //     .sum::<u64>() as f64
    //     / state.system.cpus().len() as f64;
    //
    //let cpu_frequency = state.system.cpus()[0].frequency();

    let chart = Chart::new(datasets)
        .block(Block::bordered().title(format!(
            "CPU usage: {:.0}% total", // {:.0} MHz",
            last_cpu_usage             //, cpu_frequency
        )))
        //.legend_position(Some(LegendPosition::TopLeft))
        .legend_position(None)
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

fn render_plot_mem(state: &State, frame: &mut Frame, area: Rect) {
    let t_m = state.system.total_memory();
    let mem_labels: Vec<_> = (1..=5)
        .rev()
        .map(|x| {
            if x == 5 {
                "0".to_string()
            } else {
                mem_human_readable(t_m / x)
            }
        })
        .collect();

    let max_label_len = mem_labels.iter().map(|x| x.len()).max().unwrap();

    let start_idx = state.start_data_idx(area, 3 + max_label_len);

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

    let total_mem = mem_human_readable(t_m);
    let used_mem = mem_human_readable(state.system.used_memory());
    let used_swap = mem_human_readable(state.system.used_swap());
    let total_swap = mem_human_readable(state.system.total_swap());

    let mem_plot = Chart::new(datasets)
        .block(Block::bordered().title(format!(
            "Mem usage:{used_mem}/{total_mem}; Swap:{used_swap}/{total_swap}"
        )))
        //.legend_position(Some(LegendPosition::TopLeft))
        .legend_position(None)
        .x_axis(Axis::default().bounds([
            mem_usage_all.first().unwrap().0,
            mem_usage_all.last().unwrap().0,
        ]))
        .y_axis(
            Axis::default()
                .bounds([0.0, state.system.total_memory() as f64])
                .style(Style::default().gray())
                .labels(mem_labels.clone()),
        );
    frame.render_widget(mem_plot, area);
}

fn render_table_widget_processes(state: &mut State, frame: &mut Frame, area: Rect) {
    let rows: Vec<Row> = state
        .processes_data
        .iter()
        .map(|(pid, user_name, name, cpu, mem, du)| {
            let mem_str = mem_human_readable(*mem);
            let du_str = {
                let rbs = mem_human_readable(du.read_bytes);
                let wbs = mem_human_readable(du.written_bytes);
                format!("{rbs}/{wbs}")
            };

            Row::new(vec![
                format!("{pid}"),
                user_name.clone(),
                name.clone(), //fixme
                format!("{:.1}", cpu),
                format!("{}", mem_str),
                du_str,
            ])
        })
        .collect();

    const HEADER_NAMES: [&str; 6] = ["PID", "User", "Name", "CPU%", "MEM", "Disk R/W"];

    let header_vec = HEADER_NAMES
        .iter()
        .enumerate()
        .map(|(n, &x)| {
            //let sort_order = if state.sort_ascending {'🠭'} else {'🠯'}; // 	↑↓ ⇧ ⇩⇧⇩⇪  🠱 🠳 🠭 🠯 ▼▲
            let sort_order = if state.sort_ascending { '▲' } else { '▼' }; // 	↑↓ ⇧ ⇩⇧⇩⇪  🠱 🠳 🠭 🠯 ▼▲

            if n == state.sort_by as usize {
                Text::from(format!("{x}{sort_order}")).bg(Color::Blue)
            } else {
                Text::from(x)
            }
        })
        .collect::<Vec<_>>();

    let header = Row::new(header_vec)
        .style(Style::new().bold())
        .bottom_margin(1);

    let table = Table::new(
        rows,
        [
            Constraint::Length(6),
            Constraint::Length(10),
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
    //---------- kill msgbox ------------
    if let Mode::Kill = state.mode {
        let rect = centered_rect(60, 20, area);
        let kill_block = Block::default()
            .title("Kill process (Y/N)?")
            .borders(Borders::ALL)
            .style(Style::default().fg(Color::White).bg(Color::Red));

        let proc = state.get_selected_process().unwrap();

        let cmdline = if proc.cmd().is_empty() {
            "[None]"
        } else {
            proc.cmd()[0].to_str().unwrap()
        };
        let mem_str = mem_human_readable(proc.memory());
        let kill_string = format!(
            "PID:{}\nName:{}\nCommand:{}\nMem:{}",
            proc.pid().as_u32(),
            proc.name().to_str().unwrap(),
            cmdline,
            mem_str
        );
        let kill_text = Paragraph::new(kill_string)
            .block(kill_block)
            .wrap(Wrap { trim: true });
        frame.render_widget(Clear, rect);
        frame.render_widget(kill_text, rect);
    }
    if state.deb_show {
        //-------- Debug wnd------
        let rect = centered_rect(60, 60, area);
        let debug_blk = Block::default()
            .title("Debug info")
            .borders(Borders::ALL)
            .style(Style::default().fg(Color::Green).bg(Color::DarkGray));
        let deb_text = vec![
            Line::from(format!("Supported signals:{:?}", SUPPORTED_SIGNALS)),
            Line::from(System::long_os_version().unwrap()),
        ];
        let deb_par = Paragraph::new(deb_text)
            .block(debug_blk)
            .wrap(Wrap { trim: true });
        frame.render_widget(Clear, rect);
        frame.render_widget(deb_par, rect);
    }
}

/// Simple CPU usage gauge
fn gauge_cpu_simple(state: &State) -> Gauge {
    let cpu_usage = state.system.global_cpu_usage() as f64;
    Gauge::default()
        .block(Block::new().title("CPU").borders(Borders::ALL))
        .gauge_style(Style::new().fg(Color::Magenta))
        .ratio(cpu_usage / 100.0)
        .label(format!("{:.1}%", cpu_usage))
}

///Simple Mem usage gauge
fn gauge_mem_simple(state: &State) -> Gauge {
    let memory_usage = state.system.used_memory() as f64 / state.system.total_memory() as f64;
    Gauge::default()
        .block(Block::new().title("Memory").borders(Borders::ALL))
        .gauge_style(Style::new().fg(Color::Magenta))
        .ratio(memory_usage)
        .label(format!("{:.1}%", memory_usage * 100.0))
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    // Cut the given rectangle into three vertical pieces
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    // Then cut the middle vertical piece into three width-wise pieces
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1] // Return the middle chunk
}

// /// returns truncated human-readable memory size + unit str
// fn mem_human_readable(bytes: u64) -> (String, &'static str) {
//     if bytes < 10240 {
//         return (format!("{bytes}"), "");
//     }
//     if bytes < 1024 * 1024 * 10 {
//         return (format!("{}", bytes / 1024), "K");
//     }
//     if bytes < 1024 * 1024 * 1024 * 10 {
//         return (format!("{}", bytes / (1024 * 1024)), "M");
//     }
//     if bytes < 1024 * 1024 * 1024 * 1024 * 10 {
//         return (
//             format!("{:.1}", bytes as f64 / (1024 * 1024 * 1024) as f64),
//             "G",
//         );
//     }
//     (format!("{}", bytes / (1024 * 1024 * 1024 * 1024)), "T")
// }

fn mem_human_readable(bytes: u64) -> String {
    if bytes < 10240 {
        return format!("{bytes}");
    }
    if bytes < 1024 * 1024 * 10 {
        return format!("{}K", bytes / 1024);
    }
    if bytes < 1024 * 1024 * 1024 * 10 {
        return format!("{}M", bytes / (1024 * 1024));
    }
    if bytes < 1024 * 1024 * 1024 * 1024 * 10 {
        return format!("{:.1}G", bytes as f64 / (1024 * 1024 * 1024) as f64);
    }
    format!("{}T", bytes / (1024 * 1024 * 1024 * 1024))
}
