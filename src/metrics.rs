use eframe::egui::*;
use std::collections::VecDeque;
#[derive(Clone)]
pub struct Disk {
    pub name: String,
    pub mount: String,
    pub total: u64,
    pub free: u64,
}
#[derive(Clone)]
struct Sample {
    at: f64,
    ram: f64,
    swap: f64,
    free: Option<f64>,
    cpu: Option<f64>,
}
#[derive(Default)]
pub struct History {
    samples: VecDeque<Sample>,
    marks: VecDeque<f64>,
}
pub fn now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}
fn gib(n: u64) -> f64 {
    n as f64 / 1073741824.
}
fn startup(disks: &[Disk]) -> Option<&Disk> {
    if cfg!(windows) {
        let drive = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into());
        return disks.iter().find(|d| {
            d.mount
                .trim_end_matches(['/', '\\'])
                .eq_ignore_ascii_case(&drive)
        });
    }
    disks
        .iter()
        .find(|d| d.mount == "/System/Volumes/Data")
        .or_else(|| disks.iter().find(|d| d.mount == "/"))
}
impl History {
    fn prune(&mut self, time: f64) {
        while self.samples.front().is_some_and(|s| s.at < time - 3600.) || self.samples.len() > 1500
        {
            self.samples.pop_front();
        }
        while self.marks.front().is_some_and(|t| *t < time - 3600.) || self.marks.len() > 500 {
            self.marks.pop_front();
        }
    }
    pub fn record(&mut self, s: &super::Snapshot, time: f64) {
        self.samples.push_back(Sample {
            at: time,
            ram: gib(s.used),
            swap: gib(s.swap),
            free: startup(&s.disks).map(|d| gib(d.free)),
            cpu: s.cpu.map(f64::from),
        });
        self.prune(time);
    }
    pub fn mark(&mut self, time: f64) {
        self.marks.push_back(time);
        self.prune(time);
    }
    pub fn show(&self, ui: &mut Ui, snapshot: &super::Snapshot) {
        ui.spacing_mut().item_spacing.y = 4.;
        let time = now();
        ui.heading("최근 1시간");
        ui.label("앱 실행 후 수집 · 최근 60분만 메모리에 보관 · 노란 세로선은 종료 요청 처리 시점");
        ui.small(
            "프로세스 종료가 디스크 공간을 바로 늘리지는 않아. 스왑 회수 시점도 macOS가 결정해.",
        );
        for (title, kind, color) in [
            ("RAM 사용량", 0, Color32::from_rgb(85, 199, 216)),
            ("시동 디스크 여유 공간", 2, Color32::from_rgb(245, 207, 104)),
            ("스왑 사용량", 1, Color32::from_rgb(194, 146, 240)),
            ("CPU 사용률", 3, Color32::from_rgb(119, 216, 148)),
        ] {
            let unit = if kind == 3 { "%" } else { "GiB" };
            let delta_unit = if kind == 3 { "%p" } else { "GiB" };
            let points: Vec<_> = self
                .samples
                .iter()
                .filter(|s| s.at >= time - 3600.)
                .filter_map(|s| match kind {
                    0 => Some((s.at, s.ram)),
                    1 => Some((s.at, s.swap)),
                    2 => s.free.map(|v| (s.at, v)),
                    _ => s.cpu.map(|v| (s.at, v)),
                })
                .collect();
            ui.separator();
            if let (Some(first), Some(last)) = (points.first(), points.last()) {
                ui.strong(format!(
                    "{title}  {:.2} {unit}   · 측정 구간 변화 {:+.2} {delta_unit}",
                    last.1,
                    last.1 - first.1
                ));
            } else {
                ui.strong(title);
                ui.label("측정값 없음");
            }
            let lo = if kind == 0 || kind == 3 {
                0.
            } else {
                (points.iter().map(|p| p.1).fold(f64::INFINITY, f64::min) - 0.5).max(0.)
            };
            let lo = if lo.is_finite() { lo } else { 0. };
            let hi = if kind == 0 {
                gib(snapshot.total).max(1.)
            } else if kind == 3 {
                100.
            } else {
                points.iter().map(|p| p.1).fold(lo + 1., f64::max) + 0.5
            };
            ui.small(format!(
                "세로축 {lo:.1}–{hi:.1} {unit}{}",
                if kind == 0 || kind == 3 {
                    ""
                } else {
                    " · 변화 확인용 자동 범위"
                }
            ));
            let (rect, response) =
                ui.allocate_exact_size(vec2(ui.available_width(), 55.), Sense::hover());
            let p = ui.painter_at(rect);
            p.rect_filled(rect, 3., Color32::from_rgb(8, 16, 27));
            for frac in [0., 0.5, 1.] {
                let y = rect.bottom() - rect.height() * frac;
                p.line_segment(
                    [pos2(rect.left(), y), pos2(rect.right(), y)],
                    Stroke::new(1., Color32::from_rgb(37, 46, 58)),
                );
            }
            let x = |t: f64| rect.right() - ((time - t) / 3600.) as f32 * rect.width();
            for t in &self.marks {
                p.line_segment(
                    [pos2(x(*t), rect.top()), pos2(x(*t), rect.bottom())],
                    Stroke::new(1., Color32::from_rgba_unmultiplied(250, 204, 97, 100)),
                );
            }
            let vertices: Vec<_> = points
                .iter()
                .map(|(t, v)| {
                    pos2(
                        x(*t),
                        rect.bottom() - ((v - lo) / (hi - lo)).clamp(0., 1.) as f32 * rect.height(),
                    )
                })
                .collect();
            if vertices.len() > 1 {
                p.add(Shape::line(vertices, Stroke::new(2., color)));
            } else if let Some(point) = vertices.first() {
                p.circle_filled(*point, 3., color);
            }
            if let Some(pos) = response.hover_pos() {
                let target = time - (rect.right() - pos.x) as f64 / rect.width() as f64 * 3600.;
                if let Some((at, value)) = points
                    .iter()
                    .min_by(|a, b| (a.0 - target).abs().total_cmp(&(b.0 - target).abs()))
                {
                    response.on_hover_text(format!(
                        "{:.1}분 전 · {value:.3} {unit}",
                        (time - at) / 60.
                    ));
                }
            }
            ui.horizontal(|ui| {
                ui.small("60분 전");
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.small("현재");
                });
            });
        }
    }
}
pub fn show_disks(ui: &mut Ui, snapshot: &super::Snapshot) {
    ui.heading("디스크 / 볼륨 현황");
    ui.small(
        "APFS 볼륨은 공간을 공유할 수 있어 아래 용량을 합산하지 않아. 파일 삭제는 수행하지 않아.",
    );
    for d in &snapshot.disks {
        ui.label(format!("{} · {}", d.name, d.mount));
        ui.add(
            ProgressBar::new(1. - d.free as f32 / d.total.max(1) as f32).text(format!(
                "여유 {:.1} / 전체 {:.1} GiB",
                gib(d.free),
                gib(d.total)
            )),
        );
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn one_hour_retention_and_startup_disk() {
        let mut h = History::default();
        let mut s = super::super::Snapshot::default();
        s.disks = vec![
            Disk {
                name: "external".into(),
                mount: "/Volumes/external".into(),
                total: 100,
                free: 70,
            },
            Disk {
                name: "data".into(),
                mount: if cfg!(windows) {
                    format!(
                        "{}\\",
                        std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into())
                    )
                } else {
                    "/System/Volumes/Data".into()
                },
                total: 200,
                free: 50,
            },
        ];
        h.record(&s, 0.);
        h.record(&s, 3600.);
        assert_eq!(h.samples.len(), 2);
        h.mark(1.);
        h.record(&s, 3601.);
        assert_eq!(h.samples.len(), 2);
        assert_eq!(startup(&s.disks).unwrap().free, 50);
        h.record(&s, 7202.);
        assert_eq!(h.samples.len(), 1);
        assert!(h.marks.is_empty());
    }
}
