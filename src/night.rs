use super::{Row, city::Selection};
use eframe::egui::*;
use std::collections::BTreeMap;

pub struct Camera {
    pub renderer: Option<std::sync::Arc<super::city_gpu::Renderer>>,
    pub gpu_error: Option<String>,
    angle: f32,
    tilt: f32,
    zoom: f32,
    pan: Vec2,
    edit: bool,
    offsets: BTreeMap<String, Vec2>,
    dragging: Option<String>,
    rendered: [f32; 2],
}
impl Default for Camera {
    fn default() -> Self {
        Self {
            renderer: None,
            gpu_error: None,
            angle: 0.,
            tilt: 0.58,
            zoom: 1.,
            pan: Vec2::ZERO,
            edit: false,
            offsets: BTreeMap::new(),
            dragging: None,
            rendered: [0., 0.58],
        }
    }
}
type V = [f32; 3];
struct Projection {
    mesh: std::cell::RefCell<Vec<f32>>,
    canvas: Rect,
    gpu: bool,
    center: Pos2,
    scale: f32,
    angle: f32,
    tilt: f32,
}
impl Projection {
    fn distance(&self, p: V) -> f32 {
        // 수평을 유지하는 궤도 카메라. 시야 앞쪽 물체에 원근감을 적용한다.
        (2400. - self.depth(p) * self.tilt.cos() - p[1] * self.tilt.sin()).max(100.)
    }
    fn point(&self, p: V) -> Pos2 {
        let (s, c) = self.angle.sin_cos();
        let perspective = 2400. / self.distance(p);
        self.center
            + vec2(
                (p[0] * c - p[2] * s) * self.scale,
                ((p[0] * s + p[2] * c) * self.tilt.sin() - p[1] * self.tilt.cos()) * self.scale,
            ) * perspective
    }
    fn depth(&self, p: V) -> f32 {
        p[0] * self.angle.sin() + p[2] * self.angle.cos()
    }
}
struct Hit {
    points: Vec<Pos2>,
    name: String,
    members: Vec<Row>,
    key: Option<String>,
}
fn contains(points: &[Pos2], p: Pos2) -> bool {
    let mut inside = false;
    for i in 0..points.len() {
        let a = points[i];
        let b = points[(i + 1) % points.len()];
        if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
            inside = !inside;
        }
    }
    inside
}
fn poly(p: &Painter, pr: &Projection, v: &[V], color: Color32) -> Vec<Pos2> {
    let points: Vec<_> = v.iter().map(|v| pr.point(*v)).collect();
    if pr.gpu {
        let mut mesh = pr.mesh.borrow_mut();
        for i in 1..v.len() - 1 {
            for j in [0, i, i + 1] {
                let point = points[j];
                // 카메라 깊이를 전달하여 그리는 순서와 무관하게 앞면을 표시한다.
                let depth = 1. - 200. / pr.distance(v[j]);
                mesh.extend_from_slice(&[
                    (point.x - pr.canvas.left()) / pr.canvas.width() * 2. - 1.,
                    1. - (point.y - pr.canvas.top()) / pr.canvas.height() * 2.,
                    depth,
                    color.r() as f32 / 255.,
                    color.g() as f32 / 255.,
                    color.b() as f32 / 255.,
                    color.a() as f32 / 255.,
                ]);
            }
        }
    } else {
        p.add(Shape::convex_polygon(points.clone(), color, Stroke::NONE));
    }
    points
}
fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}
fn live_members(old: &[Row], live: &[Row]) -> Vec<Row> {
    old.iter()
        .filter_map(|o| live.iter().find(|r| super::same_identity(o, r)).cloned())
        .collect()
}
pub fn show(
    ui: &mut Ui,
    frozen: &[Row],
    live: &[Row],
    selected: Option<&Row>,
    checked: &BTreeMap<u32, Row>,
    camera: &mut Camera,
    captured: f64,
) -> Option<Selection> {
    ui.add_space(6.);
    ui.horizontal(|ui| {
        ui.strong(RichText::new("MIDNIGHT SEOUL").color(rgb(233, 189, 112)));
        ui.small("프로젝트별 야경 지도");
    });
    ui.horizontal_wrapped(|ui| {
        if ui.button("＋").on_hover_text("확대").clicked() {
            camera.zoom = (camera.zoom * 1.2).min(4.);
        }
        if ui.button("−").on_hover_text("축소").clicked() {
            camera.zoom = (camera.zoom / 1.2).max(0.4);
        }
        if ui.button("◀ 회전").clicked() {
            camera.angle -= std::f32::consts::FRAC_PI_2;
        }
        if ui.button("회전 ▶").clicked() {
            camera.angle += std::f32::consts::FRAC_PI_2;
        }
        for (label, angle, tilt) in [
            ("정면", 0., 0.35),
            ("측면", std::f32::consts::FRAC_PI_2, 0.35),
            ("위에서", 0., 1.48),
        ] {
            if ui.button(label).clicked() {
                camera.angle = angle;
                camera.tilt = tilt;
            }
        }
        if ui.button("전체 보기").clicked() {
            camera.angle = 0.;
            camera.tilt = 0.58;
            camera.zoom = 1.;
            camera.pan = Vec2::ZERO;
        }
        ui.toggle_value(&mut camera.edit, "배치 편집");
        if camera.edit && ui.button("배치 초기화").clicked() {
            camera.offsets.clear();
        }
        ui.small(format!("{:.0}%", camera.zoom * 100.));
        let mut degrees = camera.tilt.to_degrees();
        if ui
            .add(
                Slider::new(&mut degrees, 15.0..=85.0)
                    .suffix("°")
                    .text("높낮이"),
            )
            .changed()
        {
            camera.tilt = degrees.to_radians();
        }
    });
    if camera.gpu_error.is_some() {
        ui.colored_label(Color32::YELLOW, "GPU 초기화 실패 · 기본 지도 표시 중");
    }
    ui.small("드래그 이동 · 휠 확대 · 우클릭 드래그 회전 · 건물/구역 클릭으로 목록 보기");
    let height = (ui.available_height() * 0.6).clamp(260., 460.);
    let (canvas, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::click_and_drag());
    let painter = ui.painter_at(canvas);
    painter.rect_filled(canvas, 5., rgb(10, 17, 26));
    if response.hovered() {
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        if scroll != 0. {
            camera.zoom = (camera.zoom * (scroll * 0.003).exp()).clamp(0.4, 4.);
        }
    }
    if response.dragged_by(PointerButton::Secondary) {
        let d = ui.input(|i| i.pointer.delta());
        camera.angle += d.x * 0.008;
    }
    let blend = 1. - (-12. * ui.input(|i| i.stable_dt).min(0.1)).exp();
    let targets = [camera.angle, camera.tilt];
    for (value, target) in camera.rendered.iter_mut().zip(targets) {
        *value += (target - *value) * blend;
        if (target - *value).abs() > 0.001 {
            ui.ctx().request_repaint();
        }
    }
    let mut groups: BTreeMap<String, Vec<Row>> = BTreeMap::new();
    for r in frozen.iter().filter(|r| r.memory > 0) {
        groups
            .entry(super::district(r))
            .or_default()
            .push(r.clone());
    }
    let mut groups: Vec<_> = groups.into_iter().collect();
    groups.sort_by(|a, b| {
        b.1.iter()
            .map(|r| r.memory)
            .sum::<u64>()
            .cmp(&a.1.iter().map(|r| r.memory).sum::<u64>())
            .then(a.0.cmp(&b.0))
    });
    if groups.len() > 16 {
        let rest = groups.split_off(15).into_iter().flat_map(|g| g.1).collect();
        groups.push(("나머지 구역".into(), rest));
    }
    if groups.is_empty() {
        painter.text(
            canvas.center(),
            Align2::CENTER_CENTER,
            "표시할 프로세스가 없어",
            FontId::proportional(14.),
            Color32::GRAY,
        );
        return None;
    }
    let cols = (groups.len() as f32).sqrt().ceil() as usize;
    let rows = groups.len().div_ceil(cols);
    let width = cols as f32 * 150.;
    let depth = rows as f32 * 170.;
    let (s, c) = camera.rendered[0].sin_cos();
    let projected_width = width * c.abs() + depth * s.abs();
    let projected_depth = width * s.abs() + depth * c.abs();
    let scale = (canvas.width() / (projected_width + 160.)).min(
        canvas.height()
            / (projected_depth * camera.rendered[1].sin() + 240. * camera.rendered[1].cos() + 120.),
    ) * 0.8
        * camera.zoom;
    let pr = Projection {
        mesh: Default::default(),
        canvas,
        gpu: camera.renderer.is_some(),
        center: canvas.center() + vec2(0., -15.) + camera.pan,
        scale,
        angle: camera.rendered[0],
        tilt: camera.rendered[1],
    };
    if response.dragged_by(PointerButton::Primary) {
        let d = ui.input(|i| i.pointer.delta());
        if let Some(key) = camera.dragging.as_ref().filter(|_| camera.edit) {
            let rx = d.x / scale;
            let rz = d.y / scale / camera.tilt.sin();
            *camera.offsets.entry(key.clone()).or_default() += vec2(
                rx * camera.angle.cos() + rz * camera.angle.sin(),
                -rx * camera.angle.sin() + rz * camera.angle.cos(),
            );
        } else {
            camera.pan += d;
        }
        ui.ctx().request_repaint();
    }
    poly(
        &painter,
        &pr,
        &[
            [-width / 2. - 15., -3., -depth / 2. - 15.],
            [width / 2. + 15., -3., -depth / 2. - 15.],
            [width / 2. + 15., -3., depth / 2. + 15.],
            [-width / 2. - 15., -3., depth / 2. + 15.],
        ],
        rgb(19, 30, 40),
    );
    for x in (-((width / 2.) as i32)..(width / 2.) as i32).step_by(25) {
        painter.line_segment(
            [
                pr.point([x as f32, -2., -depth / 2.]),
                pr.point([x as f32, -2., depth / 2.]),
            ],
            Stroke::new(0.5, rgb(29, 43, 54)),
        );
    }
    let mut hits = vec![];
    struct Building {
        pos: V,
        w: f32,
        d: f32,
        h: f32,
        old: Vec<Row>,
        name: String,
        key: String,
    }
    let mut buildings = vec![];
    let mut labels = vec![];
    for (g, (name, rs)) in groups.iter().enumerate() {
        let x = (g % cols) as f32 * 150. - width / 2. + 75.;
        let z = (g / cols) as f32 * 170. - depth / 2. + 85.;
        let points = poly(
            &painter,
            &pr,
            &[
                [x - 68., 0., z - 73.],
                [x + 68., 0., z - 73.],
                [x + 68., 0., z + 73.],
                [x - 68., 0., z + 73.],
            ],
            rgb(27, 42, 53),
        );
        hits.push(Hit {
            points,
            name: name.clone(),
            members: live_members(rs, live),
            key: None,
        });
        labels.push((pr.point([x, 0., z + 78.]), name.clone(), rs.clone()));
        let mut sorted = rs.clone();
        sorted.sort_by_key(|r| (std::cmp::Reverse(r.memory), r.pid));
        let rest = if sorted.len() > 9 {
            Some(sorted.split_off(8))
        } else {
            None
        };
        let mut tiles: Vec<Vec<Row>> = sorted.into_iter().map(|r| vec![r]).collect();
        if let Some(rest) = rest {
            tiles.push(rest);
        }
        for (i, old) in tiles.into_iter().enumerate() {
            let key = if old.len() == 1 {
                format!("{}:{}:{}", old[0].pid, old[0].start, old[0].exe)
            } else {
                format!("{name}/rest")
            };
            let offset = camera.offsets.get(&key).copied().unwrap_or_default();
            let mem = old.iter().map(|r| r.memory).sum::<u64>() as f32 / 1048576.;
            buildings.push(Building {
                pos: [
                    x - 50. + (i % 3) as f32 * 38. + offset.x,
                    0.,
                    z - 56. + (i / 3) as f32 * 42. + offset.y,
                ],
                w: 26.,
                d: 29.,
                h: 12. + (mem / 32. + 1.).ln() * 17.,
                name: format!(
                    "{name} / {}",
                    if old.len() == 1 {
                        old[0].name.clone()
                    } else {
                        format!("{}개 더 보기", old.len())
                    }
                ),
                old,
                key,
            });
        }
    }
    buildings.sort_by(|a, b| pr.depth(a.pos).total_cmp(&pr.depth(b.pos)));
    let now = ui.input(|i| i.time);
    for b in buildings {
        let [x, _, z] = b.pos;
        let w = b.w;
        let d = b.d;
        let h = b.h.min(150.);
        let members = live_members(&b.old, live);
        let cpu = members.iter().map(|r| r.cpu).sum::<f32>();
        let active = members.iter().any(|r| {
            checked
                .get(&r.pid)
                .is_some_and(|c| super::same_identity(c, r))
                || selected.is_some_and(|s| super::same_identity(s, r))
        });
        poly(
            &painter,
            &pr,
            &[
                [x + 5., 1., z + 4.],
                [x + w + 15., 1., z + 4.],
                [x + w + h * 0.35, 1., z + d + h * 0.3],
                [x + 8., 1., z + d + h * 0.3],
            ],
            Color32::from_black_alpha(48),
        );
        let mut faces = vec![
            (
                vec![[x, 0., z], [x + w, 0., z], [x + w, h, z], [x, h, z]],
                rgb(36, 46, 57),
            ),
            (
                vec![
                    [x, 0., z + d],
                    [x + w, 0., z + d],
                    [x + w, h, z + d],
                    [x, h, z + d],
                ],
                rgb(42, 53, 64),
            ),
            (
                vec![[x, 0., z], [x, 0., z + d], [x, h, z + d], [x, h, z]],
                rgb(28, 38, 48),
            ),
            (
                vec![
                    [x + w, 0., z],
                    [x + w, 0., z + d],
                    [x + w, h, z + d],
                    [x + w, h, z],
                ],
                rgb(32, 43, 53),
            ),
        ];
        faces.sort_by(|a, b| {
            let dep = |v: &Vec<V>| v.iter().map(|p| pr.depth(*p)).sum::<f32>() / 4.;
            dep(&a.0).total_cmp(&dep(&b.0))
        });
        for (v, c) in faces {
            let pts = poly(
                &painter,
                &pr,
                &v,
                if members.is_empty() {
                    rgb(22, 28, 35)
                } else {
                    c
                },
            );
            hits.push(Hit {
                points: pts,
                name: b.name.clone(),
                members: members.clone(),
                key: Some(b.key.clone()),
            });
            // 창문을 각 벽면 좌표에서 그려 회전할 때도 벽에 붙어 있도록 한다.
            let a = v[0];
            let end = v[1];
            let span = ((end[0] - a[0]).powi(2) + (end[2] - a[2]).powi(2)).sqrt();
            let ux = (end[0] - a[0]) / span;
            let uz = (end[2] - a[2]) / span;
            // 노란 빛 띠를 바닥에서 지붕으로 반복 이동시킨다.
            if !members.is_empty() && cpu > 0.1 {
                let y = ((now as f32 * 0.18 + b.old[0].pid as f32 * 0.013).fract()) * h;
                for (half, color) in [
                    (3., Color32::from_rgba_unmultiplied(255, 204, 90, 35)),
                    (0.65, rgb(255, 218, 124)),
                ] {
                    let low = (y - half).max(0.);
                    let high = (y + half).min(h);
                    let point = |t: f32, y: f32| [a[0] + ux * t, y, a[2] + uz * t];
                    poly(
                        &painter,
                        &pr,
                        &[
                            point(0., low),
                            point(span, low),
                            point(span, high),
                            point(0., high),
                        ],
                        color,
                    );
                }
            }
            for floor in 0..(h / 10.) as usize {
                for col in 0..3 {
                    let y = 5. + floor as f32 * 10.;
                    let t = 4. + col as f32 * 7.;
                    let lit = !members.is_empty()
                        && (floor * 7 + col * 11 + b.old[0].pid as usize) % 10
                            < (2. + cpu.min(100.) / 15.) as usize;
                    let wave = ((now * 0.4 - floor as f64 * 0.12).sin() * 12.) as u8;
                    let color = if lit {
                        rgb(226u8.saturating_add(wave), 190, 115)
                    } else {
                        rgb(49, 60, 68)
                    };
                    let point = |t: f32, y: f32| [a[0] + ux * t, y, a[2] + uz * t];
                    poly(
                        &painter,
                        &pr,
                        &[
                            point(t, y),
                            point(t + 3., y),
                            point(t + 3., y + 4.),
                            point(t, y + 4.),
                        ],
                        color,
                    );
                }
            }
        }
        let pts = poly(
            &painter,
            &pr,
            &[[x, h, z], [x + w, h, z], [x + w, h, z + d], [x, h, z + d]],
            if active {
                rgb(233, 189, 112)
            } else {
                rgb(67, 79, 90)
            },
        );
        hits.push(Hit {
            points: pts,
            name: b.name,
            members,
            key: Some(b.key),
        });
    }
    if let Some(renderer) = &camera.renderer {
        renderer.paint(&painter, canvas, pr.mesh.take());
    }
    for (point, name, old) in labels {
        let limit = ((120. * scale / 10.) as usize).clamp(3, 16);
        let title = if name.chars().count() > limit {
            format!("{}…", name.chars().take(limit - 1).collect::<String>())
        } else {
            name.clone()
        };
        let rect = painter.text(
            point,
            Align2::CENTER_TOP,
            title,
            FontId::proportional(10.),
            rgb(157, 176, 187),
        );
        hits.push(Hit {
            points: vec![
                rect.left_top(),
                rect.right_top(),
                rect.right_bottom(),
                rect.left_bottom(),
            ],
            name,
            members: live_members(&old, live),
            key: None,
        });
    }
    let hover = response
        .hover_pos()
        .and_then(|pos| hits.iter().rev().find(|h| contains(&h.points, pos)));
    if response.drag_started_by(PointerButton::Primary) {
        camera.dragging = ui
            .input(|i| i.pointer.press_origin())
            .and_then(|pos| hits.iter().rev().find(|h| contains(&h.points, pos)))
            .and_then(|h| h.key.clone());
    }
    if response.drag_stopped() {
        camera.dragging = None;
    }
    let clicked = if response.clicked() {
        hover.filter(|h| !h.members.is_empty()).map(|h| Selection {
            label: h.name.clone(),
            rows: h.members.clone(),
        })
    } else {
        None
    };
    if let Some(h) = hover {
        response.clone().on_hover_text(format!(
            "{}\n{}개 프로세스 · RSS {:.1} MiB · CPU {:.1}%\n{}",
            h.name,
            h.members.len(),
            h.members.iter().map(|r| r.memory).sum::<u64>() as f64 / 1048576.,
            h.members.iter().map(|r| r.cpu).sum::<f32>(),
            if h.members.is_empty() {
                "종료됨 또는 조회 불가"
            } else if camera.edit {
                "드래그로 위치 이동"
            } else {
                "클릭으로 세부 목록 보기"
            }
        ));
    }
    // 장시간 실행 시 수동 위치 기록의 메모리 사용량을 제한한다.
    if camera.offsets.len() > 4096 {
        camera.offsets.clear();
    }
    ui.small(format!(
        "높이 = RAM (로그 축) · 창문 = CPU 활동 · 배치 5분 고정 · 다음 배치 {}초",
        (300. - (now - captured)).max(0.).ceil() as u32
    ));
    if ui.input(|i| i.viewport().focused.unwrap_or(false)) {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(if response.dragged() {
                16
            } else {
                50
            }));
    }
    clicked
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn polygon_hit_and_projection_rotation() {
        let p = Projection {
            mesh: Default::default(),
            canvas: Rect::EVERYTHING,
            gpu: false,
            center: Pos2::ZERO,
            scale: 1.,
            angle: 0.,
            tilt: 0.5,
        };
        assert_eq!(p.point([10., 0., 0.]).x, 10.);
        // 가까운 물체는 크게, 먼 물체는 작게 보이며 좌우 수평은 유지된다.
        assert!(p.point([10., 0., 200.]).x > p.point([10., 0., -200.]).x);
        assert_eq!(p.point([10., 0., 0.]).y, p.point([-10., 0., 0.]).y);
        let rotated = Projection {
            angle: std::f32::consts::FRAC_PI_2,
            ..p
        };
        assert!(rotated.point([10., 0., 0.]).x.abs() < 0.001);
        let rect = vec![pos2(0., 0.), pos2(10., 0.), pos2(10., 10.), pos2(0., 10.)];
        assert!(contains(&rect, pos2(5., 5.)));
        assert!(!contains(&rect, pos2(12., 5.)));
    }
}
