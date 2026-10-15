use super::Row;
use eframe::egui::*;
use std::collections::BTreeMap;
pub struct Selection {
    pub label: String,
    pub rows: Vec<Row>,
}
#[derive(Default)]
pub struct LayoutCache {
    key: String,
    captured: f64,
    rows: Vec<Row>,
    camera: super::night::Camera,
}
impl LayoutCache {
    fn update(&mut self, key: &str, now: f64, rows: &[Row]) {
        if self.key != key
            || now - self.captured >= 300.
            || (self.rows.is_empty() && !rows.is_empty())
        {
            self.key = key.into();
            self.captured = now;
            self.rows = rows.to_vec();
        }
    }
}

fn fit(painter: &Painter, text: &str, size: f32, width: f32) -> String {
    let font = FontId::proportional(size);
    if painter
        .layout_no_wrap(text.into(), font.clone(), Color32::WHITE)
        .size()
        .x
        <= width
    {
        return text.into();
    }
    let mut short = text.to_string();
    while !short.is_empty() {
        short.pop();
        let candidate = format!("{short}…");
        if painter
            .layout_no_wrap(candidate.clone(), font.clone(), Color32::WHITE)
            .size()
            .x
            <= width
        {
            return candidate;
        }
    }
    String::new()
}

// 이진 면적 분할: 구역 면적을 RSS 가중치에 비례시킨다.
fn partition(rect: Rect, weights: &[f64]) -> Vec<Rect> {
    if weights.is_empty() {
        return vec![];
    }
    if weights.len() == 1 {
        return vec![rect];
    }
    let sum: f64 = weights.iter().sum();
    if sum <= 0. {
        return vec![];
    }
    let mut split = 1;
    let mut left = weights[0];
    while split < weights.len() - 1 && left + weights[split] / 2. < sum / 2. {
        left += weights[split];
        split += 1;
    }
    let ratio = (left / sum) as f32;
    let (a, b) = if rect.width() > rect.height() {
        let x = rect.left() + rect.width() * ratio;
        (
            Rect::from_min_max(rect.min, pos2(x, rect.bottom())),
            Rect::from_min_max(pos2(x, rect.top()), rect.max),
        )
    } else {
        let y = rect.top() + rect.height() * ratio;
        (
            Rect::from_min_max(rect.min, pos2(rect.right(), y)),
            Rect::from_min_max(pos2(rect.left(), y), rect.max),
        )
    };
    let mut result = partition(a, &weights[..split]);
    result.extend(partition(b, &weights[split..]));
    result
}

pub fn show(
    ui: &mut Ui,
    rows: &[Row],
    selected: Option<&Row>,
    checked: &BTreeMap<u32, Row>,
    buildings: bool,
    cache: &mut LayoutCache,
    view_key: &str,
) -> Option<Selection> {
    let now = ui.input(|i| i.time);
    cache.update(view_key, now, rows);
    if buildings {
        return super::night::show(
            ui,
            &cache.rows,
            rows,
            selected,
            checked,
            &mut cache.camera,
            cache.captured,
        );
    }
    let live: BTreeMap<_, _> = rows.iter().map(|r| (r.pid, r)).collect();
    let current_members = |members: &[Row]| -> Vec<Row> {
        members
            .iter()
            .filter_map(|old| {
                live.get(&old.pid)
                    .filter(|r| super::same_identity(r, old))
                    .map(|r| (*r).clone())
            })
            .collect()
    };
    ui.add_space(6.);
    ui.horizontal(|ui| {
        ui.strong("RESOURCE CITY");
        ui.label(RichText::new("메모리 지형 · CPU 활동").color(Color32::from_rgb(103, 172, 189)));
    });
    ui.small("면적 = RSS 비율 · 구역/건물 클릭으로 목록 좁히기 · ⌘ 클릭으로 개별 선택");
    let height = (ui.available_height() * 0.46).clamp(170., 300.);
    let (canvas, _) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
    let painter = ui.painter_at(canvas);
    painter.rect_filled(canvas, 6., Color32::from_rgb(7, 15, 26));
    for x in (0..canvas.width() as i32).step_by(24) {
        painter.line_segment(
            [
                pos2(canvas.left() + x as f32, canvas.top()),
                pos2(canvas.left() + x as f32, canvas.bottom()),
            ],
            Stroke::new(0.5, Color32::from_rgb(17, 34, 49)),
        );
    }
    for y in (0..canvas.height() as i32).step_by(24) {
        painter.line_segment(
            [
                pos2(canvas.left(), canvas.top() + y as f32),
                pos2(canvas.right(), canvas.top() + y as f32),
            ],
            Stroke::new(0.5, Color32::from_rgb(17, 34, 49)),
        );
    }
    let mut groups: BTreeMap<String, Vec<Row>> = BTreeMap::new();
    for r in cache.rows.iter().filter(|r| r.memory > 0) {
        groups
            .entry(super::district(r))
            .or_default()
            .push(r.clone());
    }
    let mut districts: Vec<_> = groups.into_iter().collect();
    districts.sort_by(|(an, a), (bn, b)| {
        b.iter()
            .map(|r| r.memory)
            .sum::<u64>()
            .cmp(&a.iter().map(|r| r.memory).sum::<u64>())
            .then(an.cmp(bn))
    });
    if districts.len() > 24 {
        let rest = districts
            .split_off(23)
            .into_iter()
            .flat_map(|(_, r)| r)
            .collect();
        districts.push(("나머지 구역 · 클릭하여 펼치기".into(), rest));
    }
    let weights: Vec<f64> = districts
        .iter()
        .map(|(_, rs)| rs.iter().map(|r| r.memory as f64).sum())
        .collect();
    let total: f64 = weights.iter().sum();
    if total == 0. {
        painter.text(
            canvas.center(),
            Align2::CENTER_CENTER,
            "표시할 메모리 표본이 없어",
            FontId::proportional(14.),
            Color32::GRAY,
        );
        return None;
    }
    let rects = partition(canvas.shrink(8.), &weights);
    let time = ui.input(|i| i.time) as f32;
    let mut clicked = None;
    for (index, ((name, mut rs), district)) in districts.into_iter().zip(rects).enumerate() {
        let district = district.shrink(3.);
        if district.width() < 8. || district.height() < 8. {
            continue;
        }
        let region = ui.interact(district, ui.id().with(("district", &name)), Sense::click());
        if region.clicked() {
            clicked = Some(Selection {
                label: name.clone(),
                rows: current_members(&rs),
            });
        }
        region.on_hover_text(format!(
            "{name} · {}개 프로세스\n클릭하면 세부 목록을 볼 수 있어",
            rs.len()
        ));
        painter.rect_filled(district, 3., Color32::from_rgb(19, 38, 53));
        painter.rect_stroke(
            district,
            3.,
            Stroke::new(1., Color32::from_rgb(44, 85, 102)),
            StrokeKind::Inside,
        );
        let label_rect = Rect::from_min_size(
            district.min + vec2(5., 2.),
            vec2((district.width() - 10.).max(0.), 19.),
        );
        painter.with_clip_rect(label_rect).text(
            label_rect.min,
            Align2::LEFT_TOP,
            fit(&painter, &name, 11., label_rect.width()),
            FontId::proportional(11.),
            Color32::from_rgb(145, 182, 197),
        );
        let inner = Rect::from_min_max(district.min + vec2(4., 22.), district.max - vec2(4., 4.));
        if !inner.is_positive() {
            continue;
        }
        rs.sort_by_key(|r| (std::cmp::Reverse(r.memory), r.pid));
        let remainder = if rs.len() > 12 {
            Some(rs.split_off(11))
        } else {
            None
        };
        let mut tiles: Vec<(Vec<Row>, u64, f32, String)> = rs
            .into_iter()
            .map(|r| {
                let m = r.memory;
                let c = r.cpu;
                let n = r.name.clone();
                (vec![r], m, c, n)
            })
            .collect();
        if let Some(rest) = remainder {
            tiles.push((
                rest.clone(),
                rest.iter().map(|r| r.memory).sum(),
                rest.iter().map(|r| r.cpu).sum(),
                format!("{}개 더 보기", rest.len()),
            ));
        }
        let cell_weights: Vec<_> = tiles.iter().map(|t| t.1 as f64).collect();
        let cells = partition(inner, &cell_weights);
        for (tile_index, ((members, memory, cpu, label), cell)) in
            tiles.into_iter().zip(cells).enumerate()
        {
            let _layout_values = (memory, cpu);
            let members = current_members(&members);
            let memory: u64 = members.iter().map(|r| r.memory).sum();
            let cpu: f32 = members.iter().map(|r| r.cpu).sum();
            let row = if members.len() == 1 {
                members.first()
            } else {
                None
            };
            let base = cell.shrink(2.);
            if base.width() < 5. || base.height() < 5. {
                continue;
            }
            let lift = if buildings {
                28_f32.min(base.height() * 0.35)
            } else {
                0.
            };
            let side = if buildings {
                12_f32.min(base.width() * 0.18)
            } else {
                0.
            };
            let roof = Rect::from_min_max(base.min, base.max - vec2(side, lift));
            let active = row.as_ref().is_some_and(|r| {
                checked.contains_key(&r.pid)
                    || selected.is_some_and(|s| s.pid == r.pid && s.start == r.start)
            });
            let hue = (index % 3) as u8;
            painter.rect_filled(
                base.translate(vec2(2., 2.)),
                2.,
                Color32::from_rgb(3, 9, 17),
            );
            painter.rect_filled(
                base,
                2.,
                if buildings {
                    Color32::from_rgb(33, 35, 40)
                } else {
                    Color32::from_rgb(22, 53, 69)
                },
            );
            if buildings {
                painter.add(Shape::convex_polygon(
                    vec![
                        roof.right_top(),
                        base.right_top() + vec2(0., lift),
                        base.right_bottom(),
                        roof.right_bottom(),
                    ],
                    Color32::from_rgb(23, 24, 29),
                    Stroke::NONE,
                ));
                painter.add(Shape::convex_polygon(
                    vec![
                        roof.left_bottom(),
                        roof.right_bottom(),
                        base.right_bottom(),
                        base.left_bottom() + vec2(side, 0.),
                    ],
                    Color32::from_rgb(43, 44, 48),
                    Stroke::NONE,
                ));
                for floor in 1..4 {
                    let y = roof.bottom() + lift * floor as f32 / 4.;
                    painter.line_segment(
                        [
                            pos2(roof.left() + side, y),
                            pos2(roof.right() + side * floor as f32 / 4., y),
                        ],
                        Stroke::new(1., Color32::from_rgb(93, 85, 60)),
                    );
                }
            }
            painter.rect_filled(
                roof,
                2.,
                if active {
                    if buildings {
                        Color32::from_rgb(68, 64, 50)
                    } else {
                        Color32::from_rgb(35, 113, 131)
                    }
                } else if buildings {
                    Color32::from_rgb(46, 48, 54)
                } else {
                    Color32::from_rgb(25, 63 + hue * 7, 81 + hue * 7)
                },
            );
            painter.rect_stroke(
                roof,
                2.,
                Stroke::new(
                    if active { 2. } else { 0.8 },
                    if active {
                        Color32::from_rgb(255, 215, 112)
                    } else if buildings {
                        Color32::from_rgb(86, 87, 93)
                    } else {
                        Color32::from_rgb(56, 110, 130)
                    },
                ),
                StrokeKind::Inside,
            );
            let response = ui.interact(
                base,
                ui.id().with(("city", index, tile_index)),
                Sense::click(),
            );
            let local = painter.with_clip_rect(roof.shrink(1.));
            if buildings && roof.width() > 20. && roof.height() > 20. {
                // 고정된 창문 배열과 따뜻한 조명으로 야간 사무실을 표현한다.
                for floor in 0..((roof.height() / 12.) as usize).min(24) {
                    for col in 0..((roof.width() / 13.) as usize).min(40) {
                        let lit =
                            !members.is_empty() && (floor * 7 + col * 11 + tile_index) % 5 < 2;
                        local.rect_filled(
                            Rect::from_min_size(
                                roof.min + vec2(5. + col as f32 * 13., 5. + floor as f32 * 12.),
                                vec2(4., 5.),
                            ),
                            0.,
                            if lit {
                                Color32::from_rgba_unmultiplied(255, 207, 103, 90)
                            } else {
                                Color32::from_rgb(32, 34, 39)
                            },
                        );
                    }
                }
            }
            if cpu > 0.1 {
                let y =
                    roof.bottom() - ((time * (12. + cpu.min(100.) * 0.7)) % roof.height().max(1.));
                for (w, alpha) in [(12., 10), (6., 22), (1.5, 110)] {
                    local.line_segment(
                        [pos2(roof.left(), y), pos2(roof.right(), y)],
                        Stroke::new(w, Color32::from_rgba_unmultiplied(255, 211, 94, alpha)),
                    );
                }
            }
            if roof.width() > 65. && roof.height() > 35. {
                local.text(
                    roof.min + vec2(7., 5.),
                    Align2::LEFT_TOP,
                    fit(&local, &label, 12., roof.width() - 14.),
                    FontId::proportional(12.),
                    Color32::from_rgb(222, 244, 247),
                );
                local.text(
                    roof.min + vec2(7., 22.),
                    Align2::LEFT_TOP,
                    fit(
                        &local,
                        &format!(
                            "{:.1}% · {:.0} MiB",
                            memory as f64 / total * 100.,
                            memory as f64 / 1048576.
                        ),
                        10.,
                        roof.width() - 14.,
                    ),
                    FontId::proportional(10.),
                    Color32::from_rgb(146, 196, 208),
                );
            }
            response.clone().on_hover_text(format!(
                "프로젝트 / 구역: {name}\n{}\n{label}\nRSS {:.1} MiB · 배치 기준 RSS 대비 {:.2}%\nCPU {:.1}% (코어 합산){}",
                members.iter().map(super::district).collect::<std::collections::BTreeSet<_>>().into_iter().take(8).collect::<Vec<_>>().join("\n"),
                memory as f64 / 1048576.,
                memory as f64 / total * 100.,
                cpu,
                if members.is_empty() {
                    "\n종료됨 또는 현재 조회 불가 · 다음 배치에서 제거돼."
                } else if row.is_none() {
                    "\n클릭하면 이 건물에 포함된 프로세스 목록을 볼 수 있어."
                } else {
                    ""
                }
            ));
            if response.clicked() && !members.is_empty() {
                clicked = Some(Selection {
                    label: format!("{name} / {label}"),
                    rows: members,
                });
            }
        }
    }
    ui.small(format!(
        "배치 5분 고정 · 다음 배치 {}초 후 · 숫자·조명은 실시간 · 신규 프로세스는 목록에서 확인",
        (300. - (now - cache.captured)).max(0.).ceil() as u32
    ));
    if ui.input(|i| i.viewport().focused.unwrap_or(false)) {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(100));
    }
    clicked
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn district_click_opens_exact_members_in_both_views() {
        for buildings in [false, true] {
            let ctx = Context::default();
            let row = Row {
                pid: 777,
                parent: 1,
                start: 123,
                name: "demo-worker".into(),
                exe: "/bin/demo".into(),
                project: "demo-district".into(),
                role: "기타".into(),
                memory: 100,
                cpu: 4.,
                seconds: 1,
                owned: true,
            };
            let mut picked = None;
            let mut cache = LayoutCache::default();
            cache.update("test", 0., std::slice::from_ref(&row));
            let mut changed = row.clone();
            changed.memory = 900;
            cache.update("test", 299., std::slice::from_ref(&changed));
            assert_eq!(cache.rows[0].memory, 100);
            cache.update("test", 300., std::slice::from_ref(&changed));
            assert_eq!(cache.rows[0].memory, 900);
            cache.update("other", 301., std::slice::from_ref(&row));
            assert_eq!(cache.rows[0].memory, 100);
            let mut frame = |events: Vec<Event>| {
                ctx.run(
                    RawInput {
                        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800., 600.))),
                        events,
                        ..Default::default()
                    },
                    |ctx| {
                        CentralPanel::default().show(ctx, |ui| {
                            picked = show(
                                ui,
                                std::slice::from_ref(&row),
                                None,
                                &BTreeMap::new(),
                                buildings,
                                &mut cache,
                                "render",
                            );
                        });
                    },
                )
            };
            let output = frame(vec![]);
            let pos = output
                .shapes
                .iter()
                .find_map(|s| {
                    if let Shape::Text(t) = &s.shape {
                        if t.galley.job.text == "demo-district" {
                            Some(t.pos + vec2(3., 3.))
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                })
                .unwrap();
            frame(vec![
                Event::PointerMoved(pos),
                Event::PointerButton {
                    pos,
                    button: PointerButton::Primary,
                    pressed: true,
                    modifiers: Modifiers::default(),
                },
            ]);
            frame(vec![Event::PointerButton {
                pos,
                button: PointerButton::Primary,
                pressed: false,
                modifiers: Modifiers::default(),
            }]);
            let result = picked.unwrap();
            assert_eq!(result.label, "demo-district");
            assert_eq!(result.rows[0].pid, 777);
        }
    }
    #[test]
    fn areas_match_weights() {
        let rect = Rect::from_min_size(Pos2::ZERO, vec2(800., 300.));
        let cells = partition(rect, &[1., 3., 2., 4.]);
        for (cell, w) in cells.iter().zip([1., 3., 2., 4.]) {
            assert!((cell.area() / rect.area() - w / 10.).abs() < 0.0001);
            assert!(rect.contains_rect(*cell));
        }
        for (i, a) in cells.iter().enumerate() {
            for b in cells.iter().skip(i + 1) {
                assert!(a.intersect(*b).area() <= 0.);
            }
        }
        assert!(partition(rect, &[]).is_empty());
    }
}
