//! Measure objects: page-space clicks, live readings and persistent drawing scales.
use crate::{LeftPanel, PdfCraftApp, QuickTool, canvas::DocView, icons, theme::Tokens};
use egui::{Color32, Pos2, Stroke};
use pdfcraft_engine::{
    Document, Edit, Style,
    measure::{self, Kind, NewMeasurement, Point, Reading, Scale},
    takeoff,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Distance,
    Perimeter,
    Area,
    Calibrate,
    /// Take-off count markers, tagged with the label as their item.
    Count,
}
impl Tool {
    pub fn name(self) -> &'static str {
        match self {
            Self::Distance => "distance",
            Self::Perimeter => "perimeter",
            Self::Area => "area",
            Self::Calibrate => "calibrate",
            Self::Count => "count",
        }
    }
    pub fn from_name(s: &str) -> Option<Self> {
        match s {
            "distance" => Some(Self::Distance),
            "perimeter" => Some(Self::Perimeter),
            "area" => Some(Self::Area),
            "calibrate" => Some(Self::Calibrate),
            "count" => Some(Self::Count),
            _ => None,
        }
    }
    fn kind(self) -> Kind {
        match self {
            Self::Area => Kind::Area,
            Self::Perimeter => Kind::Perimeter,
            _ => Kind::Distance,
        }
    }
}
/// What the Measure panel shows when no measuring tool is active.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Section {
    #[default]
    Takeoff,
    Scale,
}
pub struct MeasureView {
    pub section: Section,
    /// The viewport (drawing scale) being edited on the current page, by /VP index.
    pub editing: Option<usize>,
    pub line_color: [f64; 3],
    pub line_width: f64,
    pub opacity: f64,
    pub dashed: bool,
    pub caption: takeoff::Caption,
    pub caption_size: f64,
    pub count_color: [f64; 3],
    pub count_size: f64,
    /// Area tool: give the area a depth to take off a volume.
    pub depth_on: bool,
    /// Depth differs between the start and end (a fall) rather than being fixed.
    pub depth_varies: bool,
    pub depth_start: f64,
    pub depth_end: f64,
    pub points: Vec<Point>,
    pub page: Option<usize>,
    pub tool: Option<Tool>,
    pub reading: Option<Reading>,
    pub scale: Option<Scale>,
    pub error: Option<String>,
    pub snaps: measure::snap::SnapOptions,
    pub snap_enabled: bool,
    pub sensitivity: f64,
    pub drawing_points: f64,
    pub real_distance: f64,
    pub unit: String,
    pub precision: u8,
    pub name: String,
    pub whole_page: bool,
    pub rect: [f64; 4],
    pub label: String,
    seeded_page: Option<usize>,
    /// Snapping geometry (or why it failed) per (page, edit generation): extraction is not
    /// repeated on every hover frame.
    paths: Option<(usize, u64, Result<measure::snap::Geometry, String>)>,
    /// Saved measurements per edit generation, for the panel.
    listing: Option<(u64, Result<measure::Listing, String>)>,
    /// Take-off totals per edit generation, for the panel.
    takeoff: Option<(u64, Result<Vec<takeoff::Line>, String>)>,
}
impl Default for MeasureView {
    fn default() -> Self {
        Self {
            section: Section::Takeoff,
            editing: None,
            line_color: [0.0, 0.47, 0.84],
            line_width: 1.0,
            opacity: 1.0,
            dashed: false,
            caption: takeoff::Caption::Value,
            caption_size: 10.0,
            count_color: [0.89, 0.13, 0.13],
            count_size: takeoff::MARKER_SIZE,
            depth_on: false,
            depth_varies: false,
            depth_start: 0.15,
            depth_end: 0.15,
            points: Vec::new(),
            page: None,
            tool: None,
            reading: None,
            scale: None,
            error: None,
            snaps: Default::default(),
            snap_enabled: true,
            sensitivity: 6.0,
            drawing_points: 72.0,
            real_distance: 1.0,
            unit: "in".into(),
            precision: 2,
            name: "Drawing scale".into(),
            whole_page: true,
            rect: [0.0, 0.0, 100.0, 100.0],
            label: String::new(),
            seeded_page: None,
            paths: None,
            listing: None,
            takeoff: None,
        }
    }
}
impl MeasureView {
    pub fn cancel(&mut self) {
        self.points.clear();
        self.page = None;
        self.reading = None;
        self.scale = None;
    }
}
pub(crate) fn page_input(ui: &egui::Ui, resp: &egui::Response, doc: &Document, view: &mut DocView, cx: &crate::comments::PageCx<'_>, tool: Tool) {
    let page = cx.page;
    let xf = cx.xf;
    let author = &cx.prefs.author;
    if !doc.allows_annotation() {
        return;
    }
    let Some(info) = doc.info.pages.get(page) else { return };
    let state = &mut view.measure;
    if state.tool != Some(tool) {
        state.cancel();
        state.tool = Some(tool);
    }
    // Keys belong to a focused text field (the label, unit or viewport name) when there is one.
    let typing = ui.ctx().egui_wants_keyboard_input();
    if !typing && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        state.cancel();
    }
    let screen = |p: Point| {
        let v = info.user_to_view(p[0] as f32, p[1] as f32);
        xf.norm_to_screen(v[0] / xf.pw, v[1] / xf.ph)
    };
    let pos = ui.input(|i| i.pointer.hover_pos()).filter(|p| xf.rect.contains(*p));
    let mut hover = None;
    let mut snap = None;
    if let Some(pos) = pos {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
        let (vx, vy) = xf.screen_to_view(pos);
        let [x, y] = info.view_to_user(vx, vy);
        let mut point = [f64::from(x), f64::from(y)];
        if state.snap_enabled {
            if state.paths.as_ref().is_none_or(|(p, g, _)| *p != page || *g != doc.edit_generation()) {
                state.paths = Some((page, doc.edit_generation(), doc.measurement_paths(page)));
            }
            let pixels_per_unit = (xf.rect.width() / xf.pw).max(1e-6);
            if let Some((_, _, Err(e))) = &state.paths {
                state.error = Some(e.clone());
            }
            if let Some((_, _, Ok(paths))) = &state.paths {
                if paths.truncated {
                    state.error = Some(tl!("This drawing exceeds the snapping limit; only the first paths are used.").into());
                } else if paths.unreadable > 0 {
                    state.error = Some(tl!("Some page content couldn't be read; snapping uses the rest.").into());
                }
                let tolerance = doc
                    .measurement_to_user(page, [f64::from(vx) + state.sensitivity / f64::from(pixels_per_unit), f64::from(vy)])
                    .map(|p| measure::distance(point, p))
                    .unwrap_or(state.sensitivity / f64::from(pixels_per_unit));
                if state.snaps.intersections && paths.intersection_limited(point, tolerance) {
                    state.error = Some(tl!("This area has too many overlapping paths; intersection snapping is limited.").into());
                }
                if let Ok(Some(hit)) = paths.snap(point, tolerance, state.snaps) {
                    point = hit.point;
                    snap = Some(hit.kind);
                }
            }
        }
        if ui.input(|i| i.modifiers.shift)
            && let Some(last) = state.points.last()
        {
            point = measure::snap::constrain(*last, point);
        }
        hover = Some(point);
        if tool == Tool::Count {
            if resp.clicked() {
                if state.label.trim().is_empty() {
                    state.error = Some(tl!("Type the item to count in the Item box first, for example Gully.").into());
                } else {
                    state.error = None;
                    view.pending_edit = Some(Edit::AddCount(takeoff::NewCount {
                        page,
                        at: point,
                        item: state.label.clone(),
                        color: state.count_color,
                        size: state.count_size,
                        author: author.clone(),
                    }));
                }
            }
            if snap.is_some() {
                ui.painter().circle_stroke(screen(point), 6.0, Stroke::new(1.0, Color32::from_rgb(46, 158, 92)));
            }
            return;
        }
        if resp.clicked() {
            if state.page != Some(page) {
                state.cancel();
                state.page = Some(page);
            }
            if state.points.is_empty() {
                match doc.measurement_scale(page, point) {
                    Ok(scale) => {
                        state.scale = Some(scale);
                        state.error = None;
                    }
                    Err(e) => {
                        state.error = Some(e);
                        return;
                    }
                }
            }
            let closes =
                tool == Tool::Area && state.points.len() >= 3 && state.points.first().is_some_and(|first| screen(*first).distance(pos) <= 6.0);
            let repeats = state.points.last().is_some_and(|last| measure::distance(*last, point) < 1e-6);
            if !closes && !repeats && state.points.len() < measure::MAX_POINTS {
                state.points.push(point);
            }
            let finishes = closes || matches!(tool, Tool::Distance | Tool::Calibrate) && state.points.len() == 2;
            if finishes {
                finish(state, &mut view.pending_edit, doc, page, tool, author);
            }
        }
    }
    if state.page == Some(page) && !state.points.is_empty() {
        if (resp.double_clicked() || !typing && ui.input(|i| i.key_pressed(egui::Key::Enter))) && matches!(tool, Tool::Perimeter | Tool::Area) {
            finish(state, &mut view.pending_edit, doc, page, tool, author);
        }
        if !typing && ui.input(|i| i.key_pressed(egui::Key::Backspace)) {
            state.points.pop();
        }
        let mut points = state.points.clone();
        if let Some(p) = hover
            && points.last().is_none_or(|last| measure::distance(*last, p) > 1e-6)
        {
            points.push(p);
        }
        if let Some(scale) = &state.scale {
            match measure::reading(tool.kind(), &points, scale) {
                Ok(reading) => state.reading = Some(reading),
                Err(e) => state.error = Some(e.to_string()),
            }
        }
        let positions: Vec<Pos2> = points.iter().map(|&p| screen(p)).collect();
        let [r, g, b] = state.line_color.map(|c| (c.clamp(0.0, 1.0) * 255.0) as u8);
        let stroke = Stroke::new(1.5, Color32::from_rgb(r, g, b));
        for pair in positions.windows(2) {
            if let [a, b] = pair {
                ui.painter().line_segment([*a, *b], stroke);
            }
        }
        if tool == Tool::Area
            && let (Some(a), Some(b)) = (positions.first(), positions.last())
        {
            ui.painter().line_segment([*a, *b], stroke);
        }
        for &p in &positions {
            ui.painter().circle_filled(p, 3.0, stroke.color);
        }
        if let (Some(p), Some(kind)) = (hover, snap) {
            ui.painter().circle_stroke(screen(p), 6.0, Stroke::new(1.0, Color32::from_rgb(46, 158, 92)));
            ui.painter().text(
                screen(p) + egui::vec2(10.0, -10.0),
                egui::Align2::LEFT_BOTTOM,
                tl!(match kind {
                    measure::snap::SnapKind::Endpoint => "Endpoint",
                    measure::snap::SnapKind::Midpoint => "Midpoint",
                    measure::snap::SnapKind::Intersection => "Intersection",
                    measure::snap::SnapKind::Path => "Path",
                }),
                crate::theme::regular(11.0),
                stroke.color,
            );
        }
        if let (Some(p), Some(reading)) = (hover, &state.reading) {
            ui.painter().text(screen(p) + egui::vec2(12.0, 12.0), egui::Align2::LEFT_TOP, &reading.label, crate::theme::semibold(12.0), stroke.color);
        }
    }
}
fn finish(state: &mut MeasureView, pending: &mut Option<Edit>, doc: &Document, page: usize, tool: Tool, author: &str) {
    if tool == Tool::Calibrate {
        if let [a, b] = state.points.as_slice() {
            let a = doc.measurement_to_view(page, *a);
            let b = doc.measurement_to_view(page, *b);
            if let (Ok(a), Ok(b)) = (a, b) {
                state.drawing_points = measure::distance(a, b);
            }
        }
        state.cancel();
        return;
    }
    let minimum = if tool == Tool::Area { 3 } else { 2 };
    if state.points.len() < minimum {
        state.error = Some(crate::i18n::fmt(tl!("This measurement needs at least {n} points."), &[("n", &minimum.to_string())]));
        return;
    }
    if let Some(scale) = state.scale.clone() {
        let measurement = NewMeasurement {
            page,
            kind: tool.kind(),
            points: state.points.clone(),
            scale,
            style: Style { color: state.line_color, width: state.line_width, opacity: state.opacity, ..Style::default() },
            label: state.label.clone(),
            author: author.into(),
        };
        let depth = (tool == Tool::Area && state.depth_on)
            .then(|| takeoff::Depth { start: state.depth_start, end: if state.depth_varies { state.depth_end } else { state.depth_start } });
        let look = takeoff::Look { dashed: state.dashed, caption: state.caption, caption_size: state.caption_size, depth };
        *pending = Some(if look == takeoff::Look::default() {
            Edit::AddMeasurement(measurement)
        } else {
            Edit::AddTakeoffMeasurement { measurement, look }
        });
        state.cancel();
    }
}
/// Measure and take-off tools on the toolbar: (icon, tooltip, tool).
const TOOLBAR: [(&str, &str, Tool); 4] = [
    ("ruler", "Distance", Tool::Distance),
    ("spline", "Perimeter", Tool::Perimeter),
    ("pentagon", "Area", Tool::Area),
    ("hash", "Count", Tool::Count),
];
/// Swatches offered for lines and count markers.
const COLOURS: [[f64; 3]; 8] = [
    [0.0, 0.47, 0.84],
    [0.89, 0.13, 0.13],
    [0.18, 0.62, 0.36],
    [1.0, 0.54, 0.0],
    [0.54, 0.25, 0.82],
    [1.0, 0.37, 0.64],
    [0.95, 0.8, 0.0],
    [0.1, 0.1, 0.1],
];

fn open_tool(app: &mut PdfCraftApp, tool: Option<Tool>, section: Section) {
    app.left = LeftPanel::Tool("measure");
    app.left_open = true;
    if let Some((index, _)) = app.active_ids() {
        let state = &mut app.views[index].measure;
        state.cancel();
        state.section = section;
        state.error = None;
    }
    app.quick_tool = match tool {
        Some(tool) => QuickTool::Measure(tool),
        None => QuickTool::Select,
    };
}

/// The floating measure and take-off toolbar at the top of the document area.
pub(crate) fn toolbar(app: &mut PdfCraftApp, area: egui::Rect, ui: &mut egui::Ui) {
    let Some((index, _)) = app.active_ids() else { return };
    let t = Tokens::get(ui.ctx());
    let panel_open = app.left_open && app.left == LeftPanel::Tool("measure");
    let active = match app.quick_tool {
        QuickTool::Measure(tool) if tool != Tool::Calibrate => Some(tool),
        _ => None,
    };
    let section = app.views[index].measure.section;
    let mut pick: Option<(Option<Tool>, Section)> = None;
    let pos = egui::pos2(area.center().x, area.top() + 14.0);
    egui::Area::new(egui::Id::new("measure-bar")).order(egui::Order::Middle).pivot(egui::Align2::CENTER_TOP).fixed_pos(pos).show(ui.ctx(), |ui| {
        egui::Frame::NONE
            .fill(t.card)
            .stroke(Stroke::new(1.0, t.border))
            .corner_radius(egui::CornerRadius::same(10))
            .shadow(egui::Shadow { offset: [0, 2], blur: 10, spread: 0, color: Color32::from_black_alpha(if t.dark() { 80 } else { 22 }) })
            .inner_margin(egui::Margin::same(4))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    for (icon, label, tool) in TOOLBAR {
                        let on = active == Some(tool);
                        if icons::button(ui, icon, 32.0, on, tl!(label)).clicked() {
                            // A second click on the active tool puts it down.
                            pick = Some(if on { (None, Section::Takeoff) } else { (Some(tool), section) });
                        }
                    }
                    ui.separator();
                    let scale_on = panel_open && active.is_none() && section == Section::Scale;
                    if icons::button(ui, "sliders-horizontal", 32.0, scale_on, tl!("Drawing scale")).clicked() {
                        pick = Some((None, Section::Scale));
                    }
                    let takeoff_on = panel_open && active.is_none() && section == Section::Takeoff;
                    if icons::button(ui, "file-spreadsheet", 32.0, takeoff_on, tl!("Take-off summary")).clicked() {
                        pick = Some((None, Section::Takeoff));
                    }
                });
            });
    });
    if let Some((tool, section)) = pick {
        open_tool(app, tool, section);
    }
}

fn heading(ui: &mut egui::Ui, t: &Tokens, text: &str) {
    ui.add_space(6.0);
    ui.label(egui::RichText::new(text).font(crate::theme::semibold(13.0)).color(t.text));
}
fn swatches(ui: &mut egui::Ui, t: &Tokens, colour: &mut [f64; 3]) {
    ui.horizontal_wrapped(|ui| {
        for c in COLOURS {
            let (rect, resp) = ui.allocate_exact_size(egui::vec2(20.0, 20.0), egui::Sense::click());
            let fill = Color32::from_rgb((c[0] * 255.0) as u8, (c[1] * 255.0) as u8, (c[2] * 255.0) as u8);
            ui.painter().circle_filled(rect.center(), 8.0, fill);
            if *colour == c {
                ui.painter().circle_stroke(rect.center(), 10.0, Stroke::new(2.0, t.accent));
            }
            if resp.clicked() {
                *colour = c;
            }
        }
    });
}
fn item_box(ui: &mut egui::Ui, t: &Tokens, state: &mut MeasureView, items: &[String]) {
    heading(ui, t, tl!("Item"));
    egui::Frame::NONE.fill(t.field).stroke(Stroke::new(1.0, t.border)).corner_radius(4).inner_margin(egui::Margin::symmetric(6, 4)).show(ui, |ui| {
        ui.add(
            egui::TextEdit::singleline(&mut state.label)
                .hint_text(tl!("e.g. Gully, Kerb, 150mm pipe"))
                .frame(egui::Frame::NONE)
                .desired_width(f32::INFINITY)
                .char_limit(128),
        );
    });
    if !items.is_empty() {
        ui.label(egui::RichText::new(tl!("Items in this document")).color(t.text_muted));
        ui.horizontal_wrapped(|ui| {
            for item in items {
                let on = state.label.trim().eq_ignore_ascii_case(item);
                if ui.selectable_label(on, item.as_str()).clicked() {
                    state.label = item.clone();
                }
            }
        });
    }
}
fn line_style(ui: &mut egui::Ui, t: &Tokens, state: &mut MeasureView) {
    heading(ui, t, tl!("Line style"));
    swatches(ui, t, &mut state.line_color);
    ui.add(egui::Slider::new(&mut state.line_width, 0.5..=6.0).step_by(0.5).text(tl!("Width")));
    ui.add(egui::Slider::new(&mut state.opacity, 0.1..=1.0).text(tl!("Opacity")));
    ui.horizontal(|ui| {
        ui.selectable_value(&mut state.dashed, false, tl!("Solid"));
        ui.selectable_value(&mut state.dashed, true, tl!("Dashed"));
    });
}
fn label_style(ui: &mut egui::Ui, t: &Tokens, state: &mut MeasureView) {
    heading(ui, t, tl!("Label style"));
    ui.horizontal_wrapped(|ui| {
        ui.selectable_value(&mut state.caption, takeoff::Caption::Value, tl!("Value"));
        ui.selectable_value(&mut state.caption, takeoff::Caption::ItemAndValue, tl!("Item and value"));
        ui.selectable_value(&mut state.caption, takeoff::Caption::Hidden, tl!("None"));
    });
    if state.caption != takeoff::Caption::Hidden {
        ui.add(egui::Slider::new(&mut state.caption_size, 6.0..=36.0).step_by(1.0).text(tl!("Text size")));
    }
}
fn count_style(ui: &mut egui::Ui, t: &Tokens, state: &mut MeasureView) {
    heading(ui, t, tl!("Marker style"));
    swatches(ui, t, &mut state.count_color);
    ui.add(egui::Slider::new(&mut state.count_size, 4.0..=48.0).step_by(1.0).text(tl!("Size")));
}
fn volume_style(ui: &mut egui::Ui, t: &Tokens, state: &mut MeasureView, unit: &str) {
    heading(ui, t, tl!("Volume"));
    ui.checkbox(&mut state.depth_on, tl!("Give the area a depth"));
    if state.depth_on {
        ui.horizontal(|ui| {
            ui.selectable_value(&mut state.depth_varies, false, tl!("Fixed depth"));
            ui.selectable_value(&mut state.depth_varies, true, tl!("Start and end"));
        });
        ui.horizontal(|ui| {
            ui.label(if state.depth_varies { tl!("Start") } else { tl!("Depth") });
            ui.add(egui::DragValue::new(&mut state.depth_start).range(0.0..=1e6).speed(0.01).suffix(format!(" {unit}")));
        });
        if state.depth_varies {
            ui.horizontal(|ui| {
                ui.label(tl!("End"));
                ui.add(egui::DragValue::new(&mut state.depth_end).range(0.0..=1e6).speed(0.01).suffix(format!(" {unit}")));
            });
        }
        ui.label(egui::RichText::new(tl!("Volume = area × average depth. Use the same unit as the drawing scale.")).color(t.text_muted));
    }
}
fn snapping(ui: &mut egui::Ui, state: &mut MeasureView) {
    egui::CollapsingHeader::new(tl!("Snapping")).id_salt("measure-snapping").show(ui, |ui| {
        ui.checkbox(&mut state.snap_enabled, tl!("Snap to drawing"));
        if state.snap_enabled {
            ui.horizontal_wrapped(|ui| {
                ui.checkbox(&mut state.snaps.endpoints, tl!("Endpoints"));
                ui.checkbox(&mut state.snaps.midpoints, tl!("Midpoints"));
                ui.checkbox(&mut state.snaps.intersections, tl!("Intersections"));
                ui.checkbox(&mut state.snaps.paths, tl!("Paths"));
            });
            ui.add(egui::Slider::new(&mut state.sensitivity, 1.0..=20.0).text(tl!("Sensitivity")));
        }
    });
}

/// A dropdown of common units. Returns the previous unit when the choice changed.
fn unit_combo(ui: &mut egui::Ui, id: &str, unit: &mut String) -> Option<String> {
    let before = unit.clone();
    egui::ComboBox::from_id_salt(id).selected_text(unit.as_str()).width(70.0).show_ui(ui, |ui| {
        for (u, _) in takeoff::viewport::UNITS {
            ui.selectable_value(unit, u.to_string(), u);
        }
    });
    (*unit != before).then_some(before)
}
/// Keep the scale's meaning when only its unit changes (1 in becomes 25.4 mm).
fn change_unit(state: &mut MeasureView, previous: &str) {
    if let Some(v) = takeoff::viewport::convert(state.real_distance, previous, &state.unit) {
        state.real_distance = v;
    }
}
/// The whole page in PDF user space.
fn page_bbox(info: &pdfcraft_render::PageInfo) -> [f64; 4] {
    let a = info.view_to_user(0.0, 0.0);
    let b = info.view_to_user(info.width, info.height);
    [f64::from(a[0].min(b[0])), f64::from(a[1].min(b[1])), f64::from(a[0].max(b[0])), f64::from(a[1].max(b[1]))]
}
fn same_box(a: [f64; 4], b: [f64; 4]) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() < 0.5)
}

pub(crate) fn panel(app: &mut PdfCraftApp, ui: &mut egui::Ui, t: &Tokens) {
    egui::ScrollArea::vertical().id_salt("measure-panel").auto_shrink([false, false]).show(ui, |ui| panel_body(app, ui, t));
}
fn panel_body(app: &mut PdfCraftApp, ui: &mut egui::Ui, t: &Tokens) {
    let Some((index, id)) = app.active_ids() else {
        ui.label(tl!("Open a PDF to measure it."));
        return;
    };
    let active = match app.quick_tool {
        QuickTool::Measure(tool) if tool != Tool::Calibrate => Some(tool),
        _ => None,
    };
    let calibrating = app.quick_tool == QuickTool::Measure(Tool::Calibrate);
    let Some(doc) = app.session.get(id) else { return };
    let page = app.views[index].current;
    let info = doc.info.pages.get(page).cloned();
    let state = &mut app.views[index].measure;
    if state.listing.as_ref().is_none_or(|(g, _)| *g != doc.edit_generation()) {
        state.listing = Some((doc.edit_generation(), doc.measurements()));
    }
    if state.takeoff.as_ref().is_none_or(|(g, _)| *g != doc.edit_generation()) {
        state.takeoff = Some((doc.edit_generation(), takeoff_lines(doc)));
    }
    if state.seeded_page != Some(page) {
        state.seeded_page = Some(page);
        if let (Ok(a), Ok(b)) = (doc.measurement_to_user(page, [0.0, 0.0]), doc.measurement_to_user(page, [72.0, 0.0]))
            && let Ok(scale) = doc.measurement_scale(page, a)
            && let Ok(reading) = measure::reading(Kind::Distance, &[a, b], &scale)
        {
            state.drawing_points = 72.0;
            state.real_distance = reading.value;
            state.unit = scale.unit;
            state.precision = scale.precision;
        }
    }
    let viewports = doc.measurement_viewports(page).unwrap_or_default();
    if state.editing.is_some_and(|i| i >= viewports.len()) {
        state.editing = None;
    }
    let mut remove = None;
    let items = match state.takeoff.as_ref().map(|(_, l)| l) {
        Some(Ok(lines)) => takeoff::items(lines),
        _ => Vec::new(),
    };
    let mut tool = None;
    let mut section = None;
    let mut apply = false;
    let mut export = false;
    let mut export_takeoff = false;
    let mut select = None;
    match active {
        Some(current) => {
            let (icon, title) = TOOLBAR.iter().find(|(_, _, t)| *t == current).map_or(("ruler", "Measure"), |(i, l, _)| (*i, *l));
            ui.horizontal(|ui| {
                crate::icons::paint(ui, egui::Rect::from_min_size(ui.cursor().min, egui::vec2(20.0, 20.0)), icon, 18.0, t.accent);
                ui.add_space(24.0);
                ui.label(egui::RichText::new(tl!(title)).font(crate::theme::semibold(15.0)).color(t.text));
            });
            ui.label(
                egui::RichText::new(match current {
                    Tool::Distance => tl!("Click two points on the drawing."),
                    Tool::Perimeter => tl!("Click each vertex. Enter or double-click finishes."),
                    Tool::Area => tl!("Click each corner. Click the first point or press Enter to close."),
                    Tool::Count | Tool::Calibrate => tl!("Click each symbol to place a numbered marker."),
                })
                .color(t.text_muted),
            );
            item_box(ui, t, state, &items);
            if current == Tool::Count {
                count_style(ui, t, state);
            } else {
                line_style(ui, t, state);
                label_style(ui, t, state);
                if current == Tool::Area {
                    let at = doc.measurement_to_user(page, [0.0, 0.0]).unwrap_or([0.0, 0.0]);
                    let unit = doc.measurement_scale(page, at).map(|s| s.unit).unwrap_or_else(|_| state.unit.clone());
                    volume_style(ui, t, state, &unit);
                }
                heading(ui, t, tl!("Measurement"));
                match &state.reading {
                    Some(reading) => {
                        ui.label(egui::RichText::new(&reading.label).strong());
                        if current == Tool::Area && state.depth_on {
                            let end = if state.depth_varies { state.depth_end } else { state.depth_start };
                            let precision = state.scale.as_ref().map_or(2, |s| usize::from(s.precision));
                            let unit = state.scale.as_ref().map_or(String::new(), |s| s.unit.clone());
                            ui.label(format!("{} {:.precision$} {unit}^3", tl!("Volume"), reading.value * (state.depth_start + end) / 2.0));
                        }
                        ui.label(format!("X: {:.3}   Y: {:.3}   {}: {:.2}°", reading.delta_x, reading.delta_y, tl!("Angle"), reading.angle));
                    }
                    None => {
                        ui.label(tl!("Click the first point to begin."));
                    }
                }
                ui.label(egui::RichText::new(tl!("Shift constrains to 45 degrees. Backspace removes a vertex. Escape cancels.")).color(t.text_muted));
            }
            snapping(ui, state);
            ui.separator();
            let at = doc.measurement_to_user(page, [0.0, 0.0]).unwrap_or([0.0, 0.0]);
            if let Ok(scale) = doc.measurement_scale(page, at) {
                ui.label(egui::RichText::new(format!("{}: {}", tl!("Scale"), scale.ratio)).color(t.text_muted));
            }
            ui.horizontal(|ui| {
                ui.label(tl!("Unit"));
                if let Some(previous) = unit_combo(ui, "measure-tool-unit", &mut state.unit) {
                    change_unit(state, &previous);
                    state.editing = None;
                    apply = true;
                }
            });
            if ui.button(tl!("Change drawing scale")).clicked() {
                section = Some(Section::Scale);
            }
        }
        None if state.section == Section::Scale || calibrating => {
            ui.label(egui::RichText::new(tl!("Drawing scale")).font(crate::theme::semibold(15.0)).color(t.text));
            ui.label(
                egui::RichText::new(tl!("Set the scale before measuring. Saved measurements keep the scale they were drawn with."))
                    .color(t.text_muted),
            );
            if !viewports.is_empty() {
                heading(ui, t, tl!("Scales on this page"));
                for v in &viewports {
                    ui.horizontal_wrapped(|ui| {
                        let ratio = match &v.scale {
                            Ok(scale) => scale.ratio.clone(),
                            Err(e) => e.clone(),
                        };
                        let name = if v.name.is_empty() { tl!("Drawing scale").to_string() } else { v.name.clone() };
                        ui.label(egui::RichText::new(name).strong());
                        ui.label(egui::RichText::new(ratio).color(t.text_muted));
                        if let Ok(scale) = &v.scale
                            && ui.selectable_label(state.editing == Some(v.index), tl!("Edit")).clicked()
                        {
                            state.editing = Some(v.index);
                            state.name = v.name.clone();
                            state.unit = scale.unit.clone();
                            state.precision = scale.precision;
                            state.drawing_points = 72.0;
                            state.real_distance = 72.0 * scale.x * scale.distance_factor;
                            match &info {
                                Some(info) if !same_box(v.bbox, page_bbox(info)) => {
                                    state.whole_page = false;
                                    let a = info.user_to_view(v.bbox[0] as f32, v.bbox[1] as f32);
                                    let b = info.user_to_view(v.bbox[2] as f32, v.bbox[3] as f32);
                                    let (x, y) = (f64::from(a[0].min(b[0])), f64::from(a[1].min(b[1])));
                                    state.rect = [x, y, f64::from(a[0].max(b[0])) - x, f64::from(a[1].max(b[1])) - y];
                                }
                                _ => state.whole_page = true,
                            }
                        }
                        if ui.add_enabled(doc.allows_annotation(), egui::Button::new(tl!("Remove"))).clicked() {
                            remove = Some(v.index);
                        }
                    });
                }
                ui.separator();
            }
            heading(ui, t, if state.editing.is_some() { tl!("Edit scale") } else { tl!("New scale") });
            ui.horizontal(|ui| {
                ui.add(egui::DragValue::new(&mut state.drawing_points).range(0.000001..=1e9).prefix("pt "));
                ui.label("=");
                ui.add(egui::DragValue::new(&mut state.real_distance).range(0.000001..=1e12));
                if let Some(previous) = unit_combo(ui, "measure-scale-unit", &mut state.unit) {
                    change_unit(state, &previous);
                }
            });
            ui.add(egui::DragValue::new(&mut state.precision).range(0..=6).prefix(format!("{} ", tl!("Decimals"))));
            if ui.selectable_label(calibrating, tl!("Calibrate from two points")).clicked() {
                tool = Some(Tool::Calibrate);
            }
            if calibrating {
                ui.label(egui::RichText::new(tl!("Click both ends of a known length, then type its real length above.")).color(t.text_muted));
            }
            ui.checkbox(&mut state.whole_page, tl!("Entire page"));
            if !state.whole_page {
                ui.label(tl!("Viewport rectangle in page points (x, y, width, height)"));
                for (i, name) in ["X", "Y", "Width", "Height"].iter().enumerate() {
                    if let Some(v) = state.rect.get_mut(i) {
                        ui.add(egui::DragValue::new(v).range(0.0..=1e9).prefix(format!("{}: ", tl!(name))));
                    }
                }
            }
            ui.horizontal(|ui| {
                ui.label(tl!("Viewport"));
                ui.add(egui::TextEdit::singleline(&mut state.name).desired_width(130.0).char_limit(128));
            });
            ui.horizontal(|ui| {
                let label = if state.editing.is_some() { tl!("Update scale") } else { tl!("Apply scale") };
                apply = ui.add_enabled(doc.allows_annotation(), egui::Button::new(label)).clicked();
                if state.editing.is_some() && ui.button(tl!("Cancel")).clicked() {
                    state.editing = None;
                }
            });
        }
        None => {
            ui.label(egui::RichText::new(tl!("Take-off")).font(crate::theme::semibold(15.0)).color(t.text));
            ui.label(egui::RichText::new(tl!("Pick a tool from the toolbar above the page to measure or count.")).color(t.text_muted));
            match state.takeoff.as_ref().map(|(_, l)| l) {
                None => {}
                Some(Ok(lines)) if lines.is_empty() => {
                    ui.label(tl!("Label measurements or place counts to build a take-off."));
                }
                Some(Ok(lines)) => {
                    egui::Grid::new("takeoff-grid").striped(true).show(ui, |ui| {
                        for l in lines {
                            ui.label(&l.item);
                            let precision = if l.quantity == takeoff::Quantity::Count { 0 } else { usize::from(state.precision.min(6)) };
                            ui.label(format!("{:.precision$} {}", l.total, l.unit));
                            ui.end_row();
                        }
                    });
                }
                Some(Err(e)) => {
                    ui.colored_label(Color32::from_rgb(190, 50, 50), e);
                }
            }
            export_takeoff = ui.button(tl!("Export take-off as CSV")).clicked();
            ui.separator();
            match state.listing.as_ref().map(|(_, l)| l) {
                None => {}
                Some(Ok(listing)) => {
                    ui.label(format!("{}: {}", tl!("Saved measurements"), listing.measurements.len()));
                    if !listing.unsupported.is_empty() {
                        let reasons: Vec<String> =
                            listing.unsupported.iter().map(|u| format!("{} {}: {}", tl!("Page"), u.page.saturating_add(1), u.reason)).collect();
                        ui.colored_label(
                            Color32::from_rgb(190, 120, 30),
                            format!("{}: {}", tl!("Unsupported measurements"), listing.unsupported.len()),
                        )
                        .on_hover_text(reasons.join("\n"));
                    }
                    egui::ScrollArea::vertical().id_salt("measurement-list").max_height(160.0).show(ui, |ui| {
                        for m in &listing.measurements {
                            if ui.button(format!("{} {}: {} {}", tl!("Page"), m.page.saturating_add(1), m.reading.label, m.label)).clicked() {
                                select = Some((m.page, m.index, m.reading.clone(), m.scale.clone()));
                            }
                        }
                    });
                }
                Some(Err(e)) => {
                    ui.colored_label(Color32::from_rgb(190, 50, 50), e);
                }
            }
            export = ui.button(tl!("Export measurements as CSV")).clicked();
        }
    }
    if let Some(error) = &state.error {
        ui.colored_label(Color32::from_rgb(190, 50, 50), error);
    }
    if let Some(section) = section {
        state.section = section;
    }
    if apply && let Some(info) = info {
        let result =
            doc.measurement_to_user(page, [0.0, 0.0]).and_then(|a| doc.measurement_to_user(page, [1.0, 0.0]).map(|b| (a, b))).and_then(|(a, b)| {
                Scale::calibrate(a, b, state.real_distance / state.drawing_points, &state.unit, state.precision).map_err(|e| e.to_string())
            });
        match result {
            Ok(mut scale) => {
                scale.ratio = format!("{} pt = {} {}", trim_number(state.drawing_points), trim_number(state.real_distance), state.unit);
                let [x, y, w, h] = if state.whole_page { [0.0, 0.0, f64::from(info.width), f64::from(info.height)] } else { state.rect };
                let a = info.view_to_user(x as f32, y as f32);
                let b = info.view_to_user((x + w) as f32, (y + h) as f32);
                let bbox = [f64::from(a[0].min(b[0])), f64::from(a[1].min(b[1])), f64::from(a[0].max(b[0])), f64::from(a[1].max(b[1]))];
                // Editing replaces that viewport; a whole-page scale replaces the page's
                // existing whole-page one rather than stacking another on top.
                let target = state.editing.or_else(|| viewports.iter().rev().find(|v| same_box(v.bbox, bbox)).map(|v| v.index));
                let name = state.name.clone();
                state.editing = None;
                app.views[index].pending_edit = Some(match target {
                    Some(i) => Edit::ReplaceMeasurementScale { page, index: i, bbox, name, scale },
                    None => Edit::SetMeasurementScale { page, bbox, name, scale },
                });
            }
            Err(e) => state.error = Some(e.to_string()),
        }
    }
    if let Some(i) = remove {
        app.views[index].measure.editing = None;
        app.views[index].pending_edit = Some(Edit::RemoveMeasurementScale { page, index: i });
    }
    if section.is_some() {
        app.quick_tool = QuickTool::Select;
    }
    if let Some((page, annotation, reading, scale)) = select {
        app.views[index].current = page;
        app.views[index].comments.selected = Some((page, annotation));
        app.views[index].measure.reading = Some(reading);
        app.views[index].measure.scale = Some(scale);
        app.quick_tool = QuickTool::Select;
    }
    if let Some(tool) = tool {
        app.views[index].measure.cancel();
        app.quick_tool = QuickTool::Measure(tool);
    }
    if export {
        export_csv(app);
    }
    if export_takeoff {
        export_takeoff_csv(app);
    }
}
/// A number without trailing zeros ("72", "0.5").
fn trim_number(v: f64) -> String {
    let s = format!("{v:.6}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}
fn takeoff_lines(doc: &Document) -> Result<Vec<takeoff::Line>, String> {
    Ok(takeoff::summarise_all(&doc.measurements()?.measurements, &doc.counts()?, &doc.volumes()?))
}
pub(crate) fn command(app: &mut PdfCraftApp, id: &str) {
    app.left = LeftPanel::Tool("measure");
    app.left_open = true;
    if let Some((index, _)) = app.active_ids()
        && let Some(tool) = id.strip_prefix("measure.").and_then(Tool::from_name)
    {
        app.views[index].measure.cancel();
        app.quick_tool = QuickTool::Measure(tool);
    }
    if id == "measure.export" {
        export_csv(app);
    }
}
fn export_takeoff_csv(app: &mut PdfCraftApp) {
    let Some((index, id)) = app.active_ids() else { return };
    let precision = app.views[index].measure.precision;
    match app.session.get(id).ok_or_else(|| "no such document".to_string()).and_then(takeoff_lines) {
        Err(e) => app.notify(&e),
        Ok(lines) => save_csv(app, "takeoff.csv", takeoff::csv(&lines, precision), tl!("Saved take-off.")),
    }
}
fn save_csv(app: &mut PdfCraftApp, name: &'static str, csv: String, done: &'static str) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let write =
            move |app: &mut PdfCraftApp, path: std::path::PathBuf| match crate::editing::write_atomically(&path.to_string_lossy(), csv.as_bytes()) {
                Ok(()) => app.notify(done),
                Err(e) => app.notify(e.to_string()),
            };
        match app.export_dir_override.as_ref().map(|d| std::path::PathBuf::from(d).join(name)) {
            Some(path) => write(app, path),
            None => {
                let dialog = rfd::AsyncFileDialog::new().set_file_name(name).add_filter("CSV", &["csv"]);
                app.ask_one(crate::pickers::Ask::Save(dialog), None, write);
            }
        }
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = done;
        if let Err(e) = crate::editing::download(name, csv.as_bytes()) {
            app.notify(&e);
        }
    }
}
fn export_csv(app: &mut PdfCraftApp) {
    let Some((_, id)) = app.active_ids() else { return };
    let result = app.session.get(id).ok_or_else(|| "no such document".to_string()).and_then(|d| d.measurements());
    match result {
        Err(e) => app.notify(&e),
        Ok(listing) => {
            let csv = measure::csv(&listing.measurements);
            if !listing.unsupported.is_empty() {
                app.notify(crate::i18n::fmt(
                    tl!("{n} measurements use an unsupported format and are left out."),
                    &[("n", &listing.unsupported.len().to_string())],
                ));
            }
            #[cfg(not(target_arch = "wasm32"))]
            {
                let write = move |app: &mut PdfCraftApp, path: std::path::PathBuf| match crate::editing::write_atomically(
                    &path.to_string_lossy(),
                    csv.as_bytes(),
                ) {
                    Ok(()) => app.notify(tl!("Saved measurements.")),
                    Err(e) => app.notify(e.to_string()),
                };
                match app.export_dir_override.as_ref().map(|d| std::path::PathBuf::from(d).join("measurements.csv")) {
                    Some(path) => write(app, path),
                    None => {
                        let dialog = rfd::AsyncFileDialog::new().set_file_name("measurements.csv").add_filter("CSV", &["csv"]);
                        app.ask_one(crate::pickers::Ask::Save(dialog), None, write);
                    }
                }
            }
            #[cfg(target_arch = "wasm32")]
            if let Err(e) = crate::editing::download("measurements.csv", csv.as_bytes()) {
                app.notify(&e);
            }
        }
    }
}
