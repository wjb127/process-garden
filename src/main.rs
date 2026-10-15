#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use eframe::egui;
use std::{
    collections::{BTreeMap, HashSet},
    sync::mpsc,
    thread,
    time::Duration,
};
#[cfg(unix)]
use sysinfo::Signal;
use sysinfo::{Pid, ProcessesToUpdate, System};
mod city;
mod metrics;
mod night;

#[derive(Clone, Debug)]
struct Row {
    pid: u32,
    parent: u32,
    start: u64,
    name: String,
    exe: String,
    project: String,
    role: String,
    memory: u64,
    cpu: f32,
    seconds: u64,
    owned: bool,
}
#[derive(Default)]
struct Snapshot {
    rows: Vec<Row>,
    total: u64,
    used: u64,
    available: u64,
    swap: u64,
    cpu: Option<f32>,
    disks: Vec<metrics::Disk>,
}
enum Request {
    Terminate(Vec<Row>),
}
enum Event {
    Snapshot(Snapshot),
    Message(String),
}

fn role(name: &str, args: &str) -> String {
    let lower = name.to_lowercase();
    let name = lower.trim_end_matches(".exe");
    let args = args.to_lowercase();
    if ["codex", "claude", "chatgpt"].contains(&name) {
        "에이전트"
    } else if ["ollama", "ollama app", "llama-server", "lm studio", "lms"].contains(&name) {
        "로컬 LLM"
    } else if args.contains("mcp") || args.contains("modelcontextprotocol") {
        "MCP 도구"
    } else if [
        "workerd", "esbuild", "node", "uv", "uvx", "bun", "python", "pythonw", "cargo", "rustc",
    ]
    .contains(&name)
        || args.contains("wrangler")
    {
        "개발 도구"
    } else if ["chrome", "aside", "safari", "firefox", "msedge"]
        .iter()
        .any(|s| name.contains(s))
    {
        "브라우저"
    } else if ["qemu", "virtualmachine", "vmmem", "vmware", "virtualbox"]
        .iter()
        .any(|s| name.contains(s))
    {
        "가상 기기"
    } else if ["zsh", "bash", "fish", "login", "tmux", "cmux", "sleep"].contains(&name) {
        "터미널·대기"
    } else if name.contains("computeruse") || name.contains("node_repl") || name == "aside" {
        "자동화 도구"
    } else if name.starts_with("com.apple.webkit") {
        "웹 콘텐츠"
    } else if name.contains("mdworker") || name.contains("mds") || name.contains("spotlight") {
        "검색·인덱싱"
    } else if name.contains("python") || name == "cargo" || name == "rustc" || name == "npm" {
        "개발 도구"
    } else {
        "기타"
    }
    .into()
}
fn district(r: &Row) -> String {
    if !r.project.is_empty() {
        return r.project.clone();
    }
    if let Some((prefix, _)) = r.exe.split_once(".app/") {
        return format!("앱 / {}", prefix.rsplit('/').next().unwrap_or(&r.name));
    }
    if r.exe.starts_with("/System/")
        || r.exe.starts_with("/usr/libexec/")
        || r.exe.starts_with("/usr/sbin/")
    {
        return format!("macOS / {}", r.name);
    }
    format!(
        "{} / {}",
        if r.role == "기타" {
            "기타 앱"
        } else {
            &r.role
        },
        r.name
    )
}
fn classify(name: &str, args: &str, exe: &str) -> String {
    let base = role(name, args);
    if base != "기타" {
        return base;
    }
    if exe.starts_with("/System/")
        || exe.starts_with("/usr/libexec/")
        || exe.starts_with("/usr/sbin/")
    {
        "시스템 서비스".into()
    } else if exe.contains(".app/") {
        "일반 앱".into()
    } else {
        base
    }
}
fn in_scope(r: &Row, scope: &[(String, Vec<Row>)]) -> bool {
    scope
        .last()
        .is_none_or(|(_, rows)| rows.iter().any(|s| same_identity(r, s)))
}
fn project(text: &str) -> String {
    let normalized = text.replace('\\', "/");
    // ASCII markers must preserve byte offsets into the original Unicode path.
    let lower = normalized.to_ascii_lowercase();
    for marker in [
        "/project/",
        "/projects/",
        "/repos/",
        "/source/repos/",
        "/documents/codex/",
    ] {
        if let Some(index) = lower.find(marker) {
            let tail = &normalized[index + marker.len()..];
            let mut parts = tail.split(['/', ' ', '\n', '"']).filter(|s| !s.is_empty());
            if let Some(first) = parts.next() {
                if marker == "/documents/codex/"
                    && first.len() == 10
                    && first.as_bytes().get(4) == Some(&b'-')
                {
                    return parts
                        .next()
                        .map(|second| format!("{first}/{second}"))
                        .unwrap_or_else(|| first.into());
                }
                return first.into();
            }
        }
    }
    String::new()
}
fn rows(system: &System) -> Vec<Row> {
    let current_user = system
        .process(Pid::from_u32(std::process::id()))
        .and_then(|p| p.user_id());
    system
        .processes()
        .values()
        .map(|p| {
            let name = p.name().to_string_lossy().to_string();
            let args = p
                .cmd()
                .iter()
                .map(|s| s.to_string_lossy())
                .collect::<Vec<_>>()
                .join(" ");
            let cwd = p.cwd().map(|p| p.display().to_string()).unwrap_or_default();
            let proj = project(&cwd);
            let proj = if proj.is_empty() {
                project(&args)
            } else {
                proj
            };
            let exe = p.exe().map(|p| p.display().to_string()).unwrap_or_default();
            let role = classify(&name, &args, &exe);
            Row {
                pid: p.pid().as_u32(),
                parent: p.parent().map(|p| p.as_u32()).unwrap_or(0),
                start: p.start_time(),
                exe,
                role,
                name,
                project: proj,
                memory: p.memory(),
                cpu: p.cpu_usage(),
                seconds: p.run_time(),
                owned: p
                    .user_id()
                    .map(|u| current_user.is_some_and(|me| u == me))
                    .unwrap_or(false),
            }
        })
        .collect()
}
fn windows_system_process(row: &Row) -> bool {
    if !cfg!(windows) {
        return false;
    }
    let name = row.name.to_lowercase();
    let exe = row.exe.replace('\\', "/").to_lowercase();
    let root = std::env::var("SystemRoot")
        .unwrap_or_else(|_| "C:\\Windows".into())
        .replace('\\', "/")
        .to_lowercase();
    exe.starts_with(&(root + "/"))
        || [
            "system",
            "registry",
            "memory compression",
            "smss.exe",
            "csrss.exe",
            "wininit.exe",
            "winlogon.exe",
            "services.exe",
            "lsass.exe",
            "svchost.exe",
            "dwm.exe",
            "explorer.exe",
            "sihost.exe",
            "fontdrvhost.exe",
            "securityhealthservice.exe",
            "msmpeng.exe",
            "shellexperiencehost.exe",
            "startmenuexperiencehost.exe",
        ]
        .contains(&name.as_str())
}
fn protected(row: &Row, ancestors: &HashSet<u32>) -> bool {
    (row.pid <= 1 || (cfg!(windows) && row.pid <= 4))
        || windows_system_process(row)
        || !row.owned
        || row.start == 0
        || row.exe.is_empty()
        || ancestors.contains(&row.pid)
        || row.exe.starts_with("/System/")
        || row.exe.starts_with("/usr/libexec/")
        || [
            "launchd",
            "kernel_task",
            "WindowServer",
            "loginwindow",
            "Finder",
            "Dock",
        ]
        .contains(&row.name.as_str())
}
fn ancestors(system: &System) -> HashSet<u32> {
    let mut out = HashSet::new();
    let mut pid = std::process::id();
    while out.insert(pid) {
        match system.process(Pid::from_u32(pid)).and_then(|p| p.parent()) {
            Some(p) => pid = p.as_u32(),
            None => break,
        }
    }
    out
}
fn same_identity(a: &Row, b: &Row) -> bool {
    a.pid == b.pid && a.start == b.start && a.exe == b.exe && a.owned == b.owned
}
fn terminate(target: &Row) -> Result<String, String> {
    let mut s = System::new_all();
    s.refresh_processes(ProcessesToUpdate::All, true);
    let current = rows(&s)
        .into_iter()
        .find(|r| r.pid == target.pid)
        .ok_or("이미 종료됐거나 조회할 수 없는 프로세스야")?;
    if !same_identity(target, &current) {
        return Err("프로세스 신원이 바뀌었어. 다시 선택해 줘.".into());
    }
    if protected(&current, &ancestors(&s)) {
        return Err("보호 대상은 종료할 수 없어".into());
    }
    let p = s
        .process(Pid::from_u32(target.pid))
        .ok_or("프로세스 없음")?;
    #[cfg(windows)]
    let terminated = p.kill();
    #[cfg(unix)]
    let terminated = p.kill_with(Signal::Term) == Some(true);
    if !terminated {
        return Err("종료 요청 실패: 권한 또는 프로세스 상태를 확인해 줘".into());
    }
    for _ in 0..8 {
        thread::sleep(Duration::from_millis(250));
        s.refresh_processes(ProcessesToUpdate::Some(&[Pid::from_u32(target.pid)]), true);
        if s.process(Pid::from_u32(target.pid)).is_none_or(|p| {
            p.start_time() != target.start
                || p.status() == sysinfo::ProcessStatus::Zombie
                || p.status() == sysinfo::ProcessStatus::Dead
        }) {
            return Ok(format!("{} · PID {} 종료 확인", target.name, target.pid));
        }
    }
    Ok(format!(
        "PID {}에 종료 요청을 보냈지만 아직 실행 중이야. 자동 재시작 서비스일 수도 있어.",
        target.pid
    ))
}
fn worker(tx: mpsc::Sender<Event>, rx: mpsc::Receiver<Request>) {
    let mut s = System::new_all();
    let mut disks = sysinfo::Disks::new_with_refreshed_list();
    let mut cpu_ready = false;
    loop {
        s.refresh_processes(ProcessesToUpdate::All, true);
        s.refresh_memory();
        s.refresh_cpu_usage();
        disks.refresh(true);
        if tx
            .send(Event::Snapshot(Snapshot {
                rows: rows(&s),
                total: s.total_memory(),
                used: s.used_memory(),
                available: s.available_memory(),
                swap: s.used_swap(),
                cpu: cpu_ready.then(|| s.global_cpu_usage()),
                disks: disks
                    .list()
                    .iter()
                    .map(|d| metrics::Disk {
                        name: d.name().to_string_lossy().into(),
                        mount: d.mount_point().display().to_string(),
                        total: d.total_space(),
                        free: d.available_space(),
                    })
                    .collect(),
            }))
            .is_err()
        {
            break;
        }
        cpu_ready = true;
        match rx.recv_timeout(Duration::from_secs(3)) {
            Ok(Request::Terminate(targets)) => {
                let msg = targets
                    .iter()
                    .map(|r| {
                        terminate(r).unwrap_or_else(|e| format!("{} · PID {}: {e}", r.name, r.pid))
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                if tx.send(Event::Message(msg)).is_err() {
                    break;
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(_) => break,
        }
    }
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Processes,
    Disks,
    History,
}
struct App {
    snapshot: Snapshot,
    rx: mpsc::Receiver<Event>,
    tx: mpsc::Sender<Request>,
    query: String,
    filter: String,
    selected: Option<Row>,
    confirm: Option<Vec<Row>>,
    checked: BTreeMap<u32, Row>,
    anchor: Option<u32>,
    history: std::collections::VecDeque<f32>,
    message: String,
    sort: Sort,
    charts: bool,
    city: bool,
    buildings: bool,
    city_layout: city::LayoutCache,
    metrics: metrics::History,
    tab: Tab,
    scope: Vec<(String, Vec<Row>)>,
    group_project: bool,
}
impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let mut fonts = egui::FontDefinitions::default();
        let font_path = if cfg!(windows) {
            std::path::PathBuf::from(
                std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into()),
            )
            .join("Fonts/malgun.ttf")
        } else {
            std::path::PathBuf::from("/System/Library/Fonts/AppleSDGothicNeo.ttc")
        };
        if let Ok(bytes) = std::fs::read(font_path) {
            fonts
                .font_data
                .insert("korean".into(), egui::FontData::from_owned(bytes).into());
            fonts
                .families
                .entry(egui::FontFamily::Proportional)
                .or_default()
                .insert(0, "korean".into());
            cc.egui_ctx.set_fonts(fonts);
        }
        cc.egui_ctx.set_theme(egui::Theme::Dark);
        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = egui::Color32::from_rgb(13, 21, 34);
        visuals.window_fill = egui::Color32::from_rgb(18, 29, 45);
        visuals.extreme_bg_color = egui::Color32::from_rgb(8, 15, 26);
        visuals.selection.bg_fill = egui::Color32::from_rgb(24, 94, 112);
        visuals.override_text_color = Some(egui::Color32::from_rgb(216, 229, 240));
        cc.egui_ctx.set_visuals(visuals);
        let mut style = (*cc.egui_ctx.style()).clone();
        style.spacing.item_spacing = egui::vec2(10., 9.);
        style
            .text_styles
            .insert(egui::TextStyle::Body, egui::FontId::proportional(15.));
        cc.egui_ctx.set_style(style);
        let (tx_event, rx) = mpsc::channel();
        let (tx, rx_cmd) = mpsc::channel();
        thread::spawn(move || worker(tx_event, rx_cmd));
        Self {
            snapshot: Snapshot::default(),
            rx,
            tx,
            query: String::new(),
            filter: "전체".into(),
            selected: None,
            confirm: None,
            checked: BTreeMap::new(),
            anchor: None,
            history: std::collections::VecDeque::new(),
            message: "프로세스를 불러오는 중…".into(),
            sort: Sort::Memory,
            charts: true,
            city: true,
            buildings: true,
            city_layout: city::LayoutCache::default(),
            metrics: metrics::History::default(),
            tab: Tab::Processes,
            scope: vec![],
            group_project: !cfg!(windows),
        }
    }
}
fn gib(n: u64) -> String {
    format!("{:.1} GiB", n as f64 / 1073741824.)
}
#[derive(Clone, Copy, PartialEq)]
enum Sort {
    Memory,
    Cpu,
    Newest,
    Oldest,
    Importance,
}
impl Sort {
    fn label(self) -> &'static str {
        match self {
            Self::Memory => "메모리순",
            Self::Cpu => "CPU순",
            Self::Newest => "최신순",
            Self::Oldest => "오래된순",
            Self::Importance => "중요도순",
        }
    }
}
fn importance(r: &Row, guards: &HashSet<u32>) -> (u8, &'static str, &'static str) {
    if protected(r, guards) {
        (2, "보호", "시스템·다른 사용자·현재 앱 또는 신원 확인 불가")
    } else if r.role == "에이전트"
        || r.role == "로컬 LLM"
        || r.role == "가상 기기"
        || r.parent == 1
        || r.cpu > 10.
    {
        (
            1,
            "주의",
            "작업 세션·가상 기기·상시 서비스 가능성 또는 CPU 사용 중",
        )
    } else {
        (
            0,
            "일반",
            "사용자 프로세스. 작업 필요 여부는 직접 확인해야 해.",
        )
    }
}
fn sort_rows(entries: &mut [Row], sort: Sort, guards: &HashSet<u32>) {
    entries.sort_by(|a, b| {
        match sort {
            Sort::Memory => b.memory.cmp(&a.memory),
            Sort::Cpu => b.cpu.total_cmp(&a.cpu),
            Sort::Newest => b.start.cmp(&a.start),
            Sort::Oldest => a.start.cmp(&b.start),
            Sort::Importance => importance(b, guards).0.cmp(&importance(a, guards).0),
        }
        .then(a.pid.cmp(&b.pid))
    });
}
fn select_range(
    checked: &mut BTreeMap<u32, Row>,
    entries: &[Row],
    anchor: u32,
    end: u32,
    guards: &HashSet<u32>,
) {
    if let (Some(a), Some(b)) = (
        entries.iter().position(|r| r.pid == anchor),
        entries.iter().position(|r| r.pid == end),
    ) {
        for r in &entries[a.min(b)..=a.max(b)] {
            if !protected(r, guards) {
                checked.insert(r.pid, r.clone());
            }
        }
    }
}
impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        while let Ok(e) = self.rx.try_recv() {
            match e {
                Event::Snapshot(s) => {
                    self.metrics.record(&s, metrics::now());
                    self.history
                        .push_back(s.used as f32 / s.total.max(1) as f32);
                    if self.history.len() > 60 {
                        self.history.pop_front();
                    }
                    self.checked
                        .retain(|_, r| s.rows.iter().any(|x| same_identity(x, r)));
                    self.snapshot = s;
                    if self.message == "프로세스를 불러오는 중…" {
                        self.message =
                            "3초마다 갱신 · 프로세스 메모리는 작업 집합/RSS · 공유 메모리 중복 가능"
                                .into()
                    }
                }
                Event::Message(m) => {
                    self.metrics.mark(metrics::now());
                    self.message = m;
                }
            }
        }
        ctx.request_repaint_after(Duration::from_millis(500));
        let mut guards = HashSet::new();
        let mut pid = std::process::id();
        while guards.insert(pid) {
            match self.snapshot.rows.iter().find(|r| r.pid == pid) {
                Some(r) if r.parent > 0 => pid = r.parent,
                _ => break,
            }
        }
        egui::TopBottomPanel::top("header").show(ctx, |ui| {
            ui.add_space(10.);
            ui.horizontal(|ui| {
                ui.heading("Process Garden");
                ui.label("프로세스 정리");
            });
            ui.horizontal(|ui| {
                ui.label(format!("프로세스 {}", self.snapshot.rows.len()));
                ui.separator();
                ui.label(format!(
                    "RAM {} / {}",
                    gib(self.snapshot.used),
                    gib(self.snapshot.total)
                ));
                ui.separator();
                ui.label(format!("사용 가능 {}", gib(self.snapshot.available)));
                ui.separator();
                ui.label(format!("스왑/페이지파일 {}", gib(self.snapshot.swap)));
            });
            ui.add_space(6.);
            ui.add(
                egui::ProgressBar::new(
                    self.snapshot.used as f32 / self.snapshot.total.max(1) as f32,
                )
                .desired_height(5.),
            );
            ui.add_space(5.);
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.tab, Tab::Processes, "프로세스");
                ui.selectable_value(&mut self.tab, Tab::Disks, "디스크");
                ui.selectable_value(&mut self.tab, Tab::History, "최근 1시간");
            });
        });
        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            egui::ScrollArea::vertical().max_height(70.).show(ui, |ui| {
                ui.label(&self.message);
            });
        });
        if self.tab == Tab::Processes {
            egui::SidePanel::left("filters")
                .exact_width(140.)
                .show(ctx, |ui| {
                    ui.add_space(10.);
                    ui.strong("분류");
                    for category in [
                        "전체",
                        "에이전트",
                        "로컬 LLM",
                        "MCP 도구",
                        "개발 도구",
                        "브라우저",
                        "가상 기기",
                        "터미널·대기",
                        "자동화 도구",
                        "웹 콘텐츠",
                        "검색·인덱싱",
                        "시스템 서비스",
                        "일반 앱",
                        "기타",
                    ] {
                        let count = self
                            .snapshot
                            .rows
                            .iter()
                            .filter(|r| category == "전체" || r.role == category)
                            .count();
                        ui.selectable_value(
                            &mut self.filter,
                            category.into(),
                            format!("{category}  {count}"),
                        );
                    }
                    ui.separator();
                    ui.label("이름·PID·프로젝트로 검색");
                    ui.add_space(10.);
                    ui.small("부모가 1이어도 상시 서비스일 수 있어.");
                });
            egui::SidePanel::right("inspector").exact_width(270.).show(ctx,|ui|{
        ui.add_space(10.);ui.heading("프로세스 상세");
        if let Some(selected)=self.selected.clone() {
            let live=self.snapshot.rows.iter().find(|r|same_identity(r,&selected)).cloned();
            let r=live.as_ref().unwrap_or(&selected);
            ui.add_space(10.);ui.strong(&r.name);ui.label(format!("PID {} · 부모 {}",r.pid,r.parent));ui.label(&r.role);
            ui.separator();ui.label(format!("메모리 {:.1} MiB",r.memory as f64/1048576.));ui.label(format!("CPU {:.1}%",r.cpu));ui.label(format!("실행 시간 {}시간 {}분",r.seconds/3600,r.seconds%3600/60));
            ui.label(if r.project.is_empty(){"프로젝트 미확인"}else{&r.project});
            ui.collapsing("실행 파일",|ui|{ui.add(egui::Label::new(&r.exe).wrap());});
            ui.separator();
            let parent=self.snapshot.rows.iter().find(|x|x.pid==r.parent);
            ui.label(format!("부모: {}",parent.map(|p|p.name.as_str()).unwrap_or("조회 불가")));
            ui.strong("직접 자식");egui::ScrollArea::vertical().max_height(180.).show(ui,|ui|{for child in self.snapshot.rows.iter().filter(|x|x.parent==r.pid){if ui.selectable_label(false,format!("{} · {}",child.pid,child.name)).clicked(){self.selected=Some(child.clone())}}});
            ui.separator();let blocked=protected(r,&guards)||live.is_none();
            let (_,level,reason)=importance(r,&guards);ui.label(format!("중요도: {level}"));ui.small(reason);
            if ui.add_enabled(!blocked,egui::Button::new("선택 프로세스 종료…")).clicked(){self.confirm=Some(vec![r.clone()])}
            if live.is_none(){ui.label("종료됨 또는 조회 불가");}else if blocked{ui.label("시스템·다른 사용자·현재 앱은 보호돼.");}else{ui.small("종료 전 저장하지 않은 작업을 확인해 줘. 자식 프로세스는 함께 종료하지 않아.");}
        } else {ui.label("목록에서 프로세스를 선택해 줘.");}
    });
        }
        egui::CentralPanel::default().show(ctx, |ui| {
            if self.tab != Tab::Processes {
                egui::ScrollArea::vertical()
                    .id_salt("resource_tab")
                    .show(ui, |ui| match self.tab {
                        Tab::Disks => metrics::show_disks(ui, &self.snapshot),
                        Tab::History => self.metrics.show(ui, &self.snapshot),
                        Tab::Processes => {}
                    });
                return;
            }
            ui.horizontal_wrapped(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.query)
                        .hint_text("프로세스 검색")
                        .desired_width(220.),
                );
                egui::ComboBox::from_id_salt("sort")
                    .selected_text(self.sort.label())
                    .show_ui(ui, |ui| {
                        for sort in [
                            Sort::Memory,
                            Sort::Cpu,
                            Sort::Newest,
                            Sort::Oldest,
                            Sort::Importance,
                        ] {
                            ui.selectable_value(&mut self.sort, sort, sort.label());
                        }
                    });
                ui.checkbox(&mut self.group_project, "프로젝트별");
                ui.checkbox(&mut self.charts, "차트");
                ui.checkbox(&mut self.city, "도시 지도");
                ui.checkbox(&mut self.buildings, "3D 건물");
            });
            if !self.scope.is_empty() {
                ui.horizontal_wrapped(|ui| {
                    if ui.button("← 이전 구역").clicked() {
                        self.scope.pop();
                    }
                    if ui.button("전체 지도").clicked() {
                        self.scope.clear();
                    }
                    if let Some((label, _)) = self.scope.last() {
                        ui.label(format!("선택 구역: {label}"));
                    }
                });
            }
            ui.separator();
            let q = self.query.to_lowercase();
            let mut groups: BTreeMap<String, Vec<Row>> = BTreeMap::new();
            for r in &self.snapshot.rows {
                if (self.filter == "전체" || r.role == self.filter)
                    && in_scope(r, &self.scope)
                    && format!("{} {} {}", r.name, r.pid, r.project)
                        .to_lowercase()
                        .contains(&q)
                {
                    let group = if self.group_project {
                        district(r)
                    } else {
                        "전체 목록".into()
                    };
                    groups.entry(group).or_default().push(r.clone());
                }
            }
            if groups.is_empty() {
                ui.label("일치하는 프로세스가 없어.");
            }
            for entries in groups.values_mut() {
                sort_rows(entries, self.sort, &guards);
            }
            let visible: Vec<Row> = groups.values().flatten().cloned().collect();
            ui.horizontal_wrapped(|ui| {
                if ui.button("검색 결과 전체 선택").clicked() {
                    for r in &visible {
                        if !protected(r, &guards) {
                            self.checked.insert(r.pid, r.clone());
                        }
                    }
                }
                if ui.button("선택 해제").clicked() {
                    self.checked.clear();
                }
                if ui
                    .add_enabled(
                        !self.checked.is_empty(),
                        egui::Button::new(format!("{}개 종료…", self.checked.len())),
                    )
                    .clicked()
                {
                    self.confirm = Some(self.checked.values().cloned().collect());
                }
            });
            ui.small(
                "체크박스 또는 Ctrl/⌘ 클릭 · Shift 클릭으로 범위 선택 · 선택은 필터를 바꿔도 유지돼.",
            );
            if self.group_project {
                ui.small("정렬은 각 프로젝트 안에 적용돼. 전체 정렬은 프로젝트별을 꺼 줘.");
            }
            if self.city {
                if let Some(area) = city::show(
                    ui,
                    &visible,
                    self.selected.as_ref(),
                    &self.checked,
                    self.buildings,
                    &mut self.city_layout,
                    &format!(
                        "{}|{}|{:?}",
                        self.query,
                        self.filter,
                        self.scope.iter().map(|s| &s.0).collect::<Vec<_>>()
                    ),
                ) {
                    if ui.input(|i| i.modifiers.command) && area.rows.len() == 1 {
                        let r = &area.rows[0];
                        if !protected(r, &guards) && self.checked.remove(&r.pid).is_none() {
                            self.checked.insert(r.pid, r.clone());
                        }
                        self.selected = Some(r.clone());
                    } else {
                        self.selected = if area.rows.len() == 1 {
                            area.rows.first().cloned()
                        } else {
                            None
                        };
                        self.scope.push((area.label, area.rows));
                        ctx.request_repaint();
                    }
                }
            }
            if self.charts {
                egui::CollapsingHeader::new("리소스 차트")
                    .default_open(false)
                    .show(ui, |ui| {
                        ui.small("전체 RAM 사용률 · 최근 60개 표본 (약 3분)");
                        let (rect, _) = ui.allocate_exact_size(
                            egui::vec2(ui.available_width(), 44.),
                            egui::Sense::hover(),
                        );
                        let painter = ui.painter_at(rect);
                        painter.rect_filled(rect, 3., egui::Color32::from_rgb(8, 18, 30));
                        let points: Vec<_> = self
                            .history
                            .iter()
                            .enumerate()
                            .map(|(i, v)| {
                                egui::pos2(
                                    rect.left() + rect.width() * i as f32 / 59.,
                                    rect.bottom() - rect.height() * v.clamp(0., 1.),
                                )
                            })
                            .collect();
                        if points.len() > 1 {
                            painter.add(egui::Shape::line(
                                points,
                                egui::Stroke::new(2_f32, egui::Color32::from_rgb(40, 130, 190)),
                            ));
                        }
                        let mut top = visible.clone();
                        sort_rows(
                            &mut top,
                            if self.sort == Sort::Cpu {
                                Sort::Cpu
                            } else {
                                Sort::Memory
                            },
                            &guards,
                        );
                        ui.small(if self.sort == Sort::Cpu {
                            "현재 검색 결과 · CPU 상위 3개 (코어 합산 %)"
                        } else {
                            "현재 검색 결과 · 메모리 상위 3개 (RSS)"
                        });
                        let metric = |r: &Row| {
                            if self.sort == Sort::Cpu {
                                r.cpu
                            } else {
                                r.memory as f32 / 1048576.
                            }
                        };
                        let max = top.first().map(metric).unwrap_or(1.).max(1.);
                        for r in top.iter().take(3) {
                            let value = metric(r);
                            ui.add(egui::ProgressBar::new(value / max).text(format!(
                                "{} · {} · {:.1} {}",
                                r.name,
                                r.pid,
                                value,
                                if self.sort == Sort::Cpu { "%" } else { "MiB" }
                            )));
                        }
                    });
            }
            egui::ScrollArea::vertical()
                .id_salt(("process-list", self.scope.last().map(|s| s.0.clone())))
                .show(ui, |ui| {
                    for (group, entries) in groups {
                        let mem: u64 = entries.iter().map(|r| r.memory).sum();
                        egui::CollapsingHeader::new(format!(
                            "{}  · {}개 · {:.0} MiB",
                            group,
                            entries.len(),
                            mem as f64 / 1048576.
                        ))
                        .id_salt(&group)
                        .default_open(true)
                        .open(if self.scope.is_empty() {
                            None
                        } else {
                            Some(true)
                        })
                        .show(ui, |ui| {
                            egui::Grid::new((&group, "grid"))
                                .num_columns(6)
                                .striped(true)
                                .spacing([12., 8.])
                                .show(ui, |ui| {
                                    ui.label("");
                                    ui.strong("프로세스 / PID");
                                    ui.strong("MiB");
                                    ui.strong("CPU");
                                    ui.strong("경과");
                                    ui.strong("중요도");
                                    ui.end_row();
                                    for r in entries {
                                        let mut checked = self
                                            .checked
                                            .get(&r.pid)
                                            .is_some_and(|x| same_identity(x, &r));
                                        let response = ui.add_enabled(
                                            !protected(&r, &guards),
                                            egui::Checkbox::without_text(&mut checked),
                                        );
                                        if response.changed() {
                                            if ui.input(|i| i.modifiers.shift) {
                                                select_range(
                                                    &mut self.checked,
                                                    &visible,
                                                    self.anchor.unwrap_or(r.pid),
                                                    r.pid,
                                                    &guards,
                                                );
                                            } else if checked {
                                                self.checked.insert(r.pid, r.clone());
                                            } else {
                                                self.checked.remove(&r.pid);
                                            }
                                            self.anchor = Some(r.pid);
                                        }
                                        let name = format!("{}  · {}", r.name, r.pid);
                                        let is_selected = self
                                            .selected
                                            .as_ref()
                                            .is_some_and(|s| same_identity(s, &r));
                                        if ui
                                            .add_sized(
                                                [ui.available_width().min(220.).max(110.), 24.],
                                                egui::Button::selectable(
                                                    is_selected,
                                                    egui::RichText::new(name),
                                                )
                                                .truncate(),
                                            )
                                            .on_hover_text(format!(
                                                "{} · 부모 {}",
                                                r.role, r.parent
                                            ))
                                            .clicked()
                                        {
                                            if ui.input(|i| i.modifiers.shift) {
                                                select_range(
                                                    &mut self.checked,
                                                    &visible,
                                                    self.anchor.unwrap_or(r.pid),
                                                    r.pid,
                                                    &guards,
                                                );
                                            } else if ui.input(|i| i.modifiers.command)
                                                && !protected(&r, &guards)
                                            {
                                                if self.checked.remove(&r.pid).is_none() {
                                                    self.checked.insert(r.pid, r.clone());
                                                }
                                            }
                                            self.anchor = Some(r.pid);
                                            self.selected = Some(r.clone())
                                        }
                                        ui.label(format!("{:.1}", r.memory as f64 / 1048576.));
                                        ui.label(format!("{:.1}%", r.cpu));
                                        ui.label(format!(
                                            "{}h {}m",
                                            r.seconds / 3600,
                                            r.seconds % 3600 / 60
                                        ));
                                        let (_, level, reason) = importance(&r, &guards);
                                        ui.label(level).on_hover_text(reason);
                                        ui.end_row();
                                    }
                                });
                        });
                    }
                });
        });
        if let Some(targets) = self.confirm.clone() {
            egui::Window::new("프로세스를 종료할까?")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0., 0.])
                .show(ctx, |ui| {
                    ui.strong(format!("{}개 프로세스", targets.len()));
                    egui::ScrollArea::vertical()
                        .max_height(200.)
                        .show(ui, |ui| {
                            for r in &targets {
                                ui.label(format!("{} · PID {} · {}", r.name, r.pid, r.project));
                            }
                        });
                    ui.label("저장하지 않은 작업이 사라질 수 있어.");
                    ui.label(if cfg!(windows) {
                        "Windows 강제 종료(taskkill /F). 먼저 앱에서 정상 종료하는 것을 권장해."
                    } else {
                        "정상 종료 요청(SIGTERM)을 보내며 자동실행 설정은 바꾸지 않아."
                    });
                    ui.horizontal(|ui| {
                        if ui.button("취소").clicked() {
                            self.confirm = None
                        }
                        if ui.button("종료 요청").clicked() {
                            self.message =
                                if self.tx.send(Request::Terminate(targets.clone())).is_ok() {
                                    "종료 여부 확인 중…".into()
                                } else {
                                    "수집기가 종료됐어. 앱을 다시 실행해 줘.".into()
                                };
                            self.confirm = None;
                            self.checked.clear();
                        }
                    });
                });
        }
    }
}
fn main() -> eframe::Result {
    if let Some(path) = std::env::args()
        .skip(1)
        .find_map(|a| a.strip_prefix("--diagnostics=").map(str::to_owned))
    {
        let mut system = System::new_all();
        thread::sleep(Duration::from_secs(1));
        system.refresh_all();
        let data = rows(&system);
        let own = data.iter().filter(|r| r.owned).count();
        let protected_count = data
            .iter()
            .filter(|r| protected(r, &ancestors(&system)))
            .count();
        std::fs::write(
            path,
            format!(
                "processes={}\nowned={}\nprotected={}\ntotal_bytes={}\navailable_bytes={}\n",
                data.len(),
                own,
                protected_count,
                system.total_memory(),
                system.available_memory()
            ),
        )
        .expect("write diagnostics");
        return Ok(());
    }
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1180., 760.])
            .with_min_inner_size([1100., 700.]),
        ..Default::default()
    };
    eframe::run_native(
        "Process Garden",
        options,
        Box::new(|cc| Ok(Box::new(App::new(cc)))),
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    fn sleeper() -> std::process::Child {
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--ignored", "--exact", "tests::child_sleep_helper"])
                .creation_flags(0x08000000)
                .spawn()
                .unwrap()
        }
        #[cfg(unix)]
        {
            std::process::Command::new("/bin/sleep")
                .arg("60")
                .spawn()
                .unwrap()
        }
    }
    fn fixture(pid: u32, start: u64) -> Row {
        Row {
            pid,
            parent: 50,
            start,
            name: "sleep".into(),
            exe: "/bin/sleep".into(),
            project: String::new(),
            role: "기타".into(),
            memory: pid as u64,
            cpu: 0.,
            seconds: 0,
            owned: true,
        }
    }
    #[test]
    #[ignore]
    fn child_sleep_helper() {
        thread::sleep(Duration::from_secs(60));
    }
    #[test]
    fn windows_roles_and_paths() {
        assert_eq!(role("chrome.exe", ""), "브라우저");
        assert_eq!(role("llama-server.exe", ""), "로컬 LLM");
        assert_eq!(role("node.exe", "mcp server"), "MCP 도구");
        assert_eq!(
            project(r"C:\Users\me\Documents\Codex\2026-10-16\app\src"),
            "2026-10-16/app"
        );
        assert_eq!(project(r"D:\Projects\garden\src"), "garden");
        if cfg!(windows) {
            let mut r = fixture(100, 10);
            r.name = "dwm.exe".into();
            assert!(protected(&r, &HashSet::new()));
        }
    }
    #[test]
    fn unicode_project_path_preserves_offsets() {
        // Unicode lowercasing can change the byte length of İ before the marker.
        assert_eq!(project("/Users/İ/Projects/garden/src"), "garden");
        assert_eq!(project(r"C:\Users\사용자\PROJECTS\정원\src"), "정원");
    }
    #[test]
    fn unknown_ownership_and_identity_are_protected() {
        let guards = HashSet::new();
        let row = fixture(100, 10);
        assert!(!protected(&row, &guards));
        let mut changed = row.clone();
        changed.owned = false;
        assert!(protected(&changed, &guards));
        assert!(!same_identity(&row, &changed));
        changed = row.clone();
        changed.exe.clear();
        assert!(protected(&changed, &guards));
        assert!(!same_identity(&row, &changed));
        changed = row.clone();
        changed.start = 0;
        assert!(protected(&changed, &guards));
        changed = row.clone();
        changed.exe = "/System/Library/test".into();
        assert!(protected(&changed, &guards));
        changed.exe = "/usr/libexec/test".into();
        assert!(protected(&changed, &guards));
    }
    #[test]
    fn sorting_range_and_protection() {
        let mut items = vec![fixture(10, 100), fixture(20, 300), fixture(30, 200)];
        let guards = HashSet::from([30]);
        sort_rows(&mut items, Sort::Newest, &guards);
        assert_eq!(
            items.iter().map(|r| r.pid).collect::<Vec<_>>(),
            vec![20, 30, 10]
        );
        sort_rows(&mut items, Sort::Oldest, &guards);
        assert_eq!(items[0].pid, 10);
        let mut checked = BTreeMap::new();
        select_range(&mut checked, &items, 20, 10, &guards);
        assert_eq!(checked.keys().copied().collect::<Vec<_>>(), vec![10, 20]);
        assert_eq!(importance(&items[1], &guards).1, "보호");
    }
    #[test]
    fn batch_terminates_only_requested_children() {
        let mut children: Vec<_> = (0..3).map(|_| sleeper()).collect();
        let snapshot = rows(&System::new_all());
        for child in children.iter().take(2) {
            let row = snapshot.iter().find(|r| r.pid == child.id()).unwrap();
            assert!(terminate(row).unwrap().contains("종료 확인"));
        }
        assert!(children[2].try_wait().unwrap().is_none());
        children[2].kill().unwrap();
        for child in &mut children {
            child.wait().unwrap();
        }
    }
    #[test]
    fn categories() {
        assert_eq!(role("codex", ""), "에이전트");
        assert_eq!(role("node", "mcp server"), "MCP 도구");
        assert_eq!(role("zsh", ""), "터미널·대기");
        assert_eq!(role("mdworker_shared", ""), "검색·인덱싱");
        assert_eq!(
            classify("distnoted", "", "/usr/sbin/distnoted"),
            "시스템 서비스"
        );
        assert_eq!(
            classify(
                "Example",
                "",
                "/Applications/Example.app/Contents/MacOS/Example"
            ),
            "일반 앱"
        );
        let mut r = fixture(100, 123);
        r.exe = "/Applications/Example.app/Contents/Helpers/Child.app/Contents/MacOS/Child".into();
        assert_eq!(district(&r), "앱 / Example");
        let scope = vec![("Example".into(), vec![r.clone()])];
        assert!(in_scope(&r, &scope));
        r.start += 1;
        assert!(!in_scope(&r, &scope));
        assert_eq!(
            project("/Users/a/Project/ss-133-process-garden/src"),
            "ss-133-process-garden"
        );
    }
    #[test]
    fn own_child_termination_and_stale_pid_guard() {
        let mut child = sleeper();
        let s = System::new_all();
        let row = rows(&s).into_iter().find(|r| r.pid == child.id()).unwrap();
        let mut stale = row.clone();
        stale.start += 1;
        assert!(terminate(&stale).is_err());
        assert!(child.try_wait().unwrap().is_none());
        assert!(terminate(&row).unwrap().contains("종료 확인"));
        child.wait().unwrap();
    }
    #[test]
    fn self_is_protected() {
        let s = System::new_all();
        let me = rows(&s)
            .into_iter()
            .find(|r| r.pid == std::process::id())
            .unwrap();
        assert!(protected(&me, &ancestors(&s)));
        assert!(terminate(&me).is_err());
    }
}
