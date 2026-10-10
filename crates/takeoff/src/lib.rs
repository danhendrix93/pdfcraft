//! Quantity take-off. Count markers are small filled circle annotations placed on symbols
//! (gullies, manholes, lighting columns), each tagged with the item it counts and numbered
//! within that item. Saved measurements belong to the item their label names ("Kerb type B",
//! "150mm pipe"). [`summarise`] totals counts, lengths and areas per item and unit. Nothing
//! is stored beyond the annotations themselves, so the summary is always recalculated from
//! the document.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use pdfcraft_annot::{Meta, NewAnnotation, Shape, Style};
use pdfcraft_cos::{Document, Object, PdfString};
use pdfcraft_measure::{Kind, Measurement, Point};
use serde::Serialize;
use std::collections::BTreeMap;

pub mod viewport;

/// Item name for measurements saved without a label.
pub const UNTAGGED: &str = "Untagged";
/// Unit shown for counts ("number").
pub const COUNT_UNIT: &str = "nr";
/// Marker diameter in user-space points.
pub const MARKER_SIZE: f64 = 10.0;
const MAX_ITEM: usize = 512;
const MAX_ANNOTATIONS: usize = 100_000;
const MAX_COORD: f64 = 1e9;
/// Marks a count marker annotation; the value is the item it counts.
const ITEM_KEY: &[u8] = b"PCTakeoffItem";
const NUMBER_KEY: &[u8] = b"PCTakeoffNumber";

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum TakeoffError {
    #[error("{0}")]
    Invalid(String),
    #[error(transparent)]
    Annotation(#[from] pdfcraft_annot::AnnotError),
}
type Result<T> = std::result::Result<T, TakeoffError>;
fn invalid(s: &str) -> TakeoffError {
    TakeoffError::Invalid(s.into())
}

/// Lengths (distances and perimeters) total together; areas and counts total separately.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Quantity {
    Count,
    Length,
    Area,
    /// An area with a depth.
    Volume,
}
impl Quantity {
    pub fn of(kind: Kind) -> Self {
        match kind {
            Kind::Distance | Kind::Perimeter => Self::Length,
            Kind::Area => Self::Area,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Count => "count",
            Self::Length => "length",
            Self::Area => "area",
            Self::Volume => "volume",
        }
    }
}

/// The trimmed label, or [`UNTAGGED`] when it is blank.
pub fn item_name(label: &str) -> &str {
    let s = label.trim();
    if s.is_empty() { UNTAGGED } else { s }
}
fn same_item(a: &str, b: &str) -> bool {
    item_name(a).to_lowercase() == item_name(b).to_lowercase()
}

#[derive(Clone, Debug, PartialEq)]
pub struct NewCount {
    /// 0-based page index.
    pub page: usize,
    /// Marker centre in PDF user space.
    pub at: Point,
    pub item: String,
    pub color: pdfcraft_annot::Rgb,
    /// Marker diameter in user-space points (clamped to 4..=48; [`MARKER_SIZE`] by default).
    pub size: f64,
    pub author: String,
}

/// A saved count marker.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Count {
    /// 0-based page and annotation index.
    pub page: usize,
    pub index: usize,
    pub id: String,
    pub item: String,
    /// 1-based position within its item, in the order markers were placed.
    pub number: u32,
    /// Marker centre in PDF user space.
    pub at: Point,
}

/// Add a count marker and return its annotation index on the page. It is numbered one
/// past the highest number already used for the same item anywhere in the document.
pub fn add_count(doc: &mut Document, new: &NewCount, meta: &Meta) -> Result<(usize, u32)> {
    if new.at.iter().any(|v| !v.is_finite() || v.abs() > MAX_COORD) {
        return Err(invalid("count marker position must be finite and within 1e9 user units"));
    }
    if new.item.trim().is_empty() || new.item.len() > MAX_ITEM || new.item.chars().any(char::is_control) {
        return Err(invalid("count item must be 1 to 512 bytes of printable text"));
    }
    if new.author.len() > MAX_ITEM {
        return Err(invalid("author must be at most 512 bytes"));
    }
    let item = new.item.trim();
    let number = counts(doc).iter().filter(|c| same_item(&c.item, item)).map(|c| c.number).max().unwrap_or(0).saturating_add(1);
    let contents = format!("{item} #{number}");
    let size = if new.size.is_finite() { new.size.clamp(4.0, 48.0) } else { MARKER_SIZE };
    let r = size / 2.0;
    let [x, y] = new.at;
    let index = pdfcraft_annot::add_annotation(
        doc,
        &NewAnnotation {
            page: new.page,
            shape: Shape::Oval { rect: [x - r, y - r, x + r, y + r] },
            style: Style { color: new.color, opacity: 1.0, width: 1.0, fill: Some(new.color) },
            contents,
            author: new.author.clone(),
        },
        meta,
    )?;
    let (obj, mut d) = annotation(doc, new.page, index)?;
    d.set(b"Subj".to_vec(), PdfString::text("Count"));
    d.set(ITEM_KEY.to_vec(), PdfString::text(item));
    d.set(NUMBER_KEY.to_vec(), Object::Int(i64::from(number)));
    doc.update_dict(obj, |target| *target = d).map_err(|e| invalid(&e.to_string()))?;
    pdfcraft_annot::set_appearance(doc, obj)?;
    Ok((index, number))
}

fn annotation(doc: &Document, page: usize, index: usize) -> Result<(pdfcraft_cos::ObjRef, pdfcraft_cos::Dict)> {
    let p = pdfcraft_model::pages(doc).get(page).cloned().ok_or_else(|| invalid("page does not exist"))?;
    let list = doc.resolve(p.dict.get(b"Annots").ok_or_else(|| invalid("page has no annotations"))?);
    let o = list.as_array().and_then(|a| a.get(index)).ok_or_else(|| invalid("count marker annotation does not exist"))?;
    let r = o.as_ref().ok_or_else(|| invalid("inline annotations cannot be edited"))?;
    let d = doc.get(r).as_dict().cloned().ok_or_else(|| invalid("invalid annotation dictionary"))?;
    Ok((r, d))
}

/// Every count marker in page order. Annotations tagged as markers but with unreadable
/// geometry are skipped; enumeration stops after 100,000 annotations.
pub fn counts(doc: &Document) -> Vec<Count> {
    let mut out = Vec::new();
    let mut seen = 0usize;
    for (page, p) in pdfcraft_model::pages(doc).iter().enumerate() {
        let Some(o) = p.dict.get(b"Annots") else { continue };
        let o = doc.resolve(o);
        let Some(arr) = o.as_array() else { continue };
        for (index, a) in arr.iter().enumerate() {
            seen = seen.saturating_add(1);
            if seen > MAX_ANNOTATIONS {
                return out;
            }
            let o = doc.resolve(a);
            let Some(d) = o.as_dict() else { continue };
            let Some(item) = d.get(ITEM_KEY).and_then(Object::as_string).map(PdfString::to_text) else { continue };
            let rect: Option<Vec<f64>> =
                d.get(b"Rect").map(|r| doc.resolve(r)).and_then(|r| r.as_array().and_then(|a| a.iter().map(Object::as_f64).collect()));
            let Some(&[x0, y0, x1, y1]) = rect.as_deref() else { continue };
            let at = [(x0 + x1) / 2.0, (y0 + y1) / 2.0];
            if at.iter().any(|v| !v.is_finite()) {
                continue;
            }
            let number = d.int(NUMBER_KEY).and_then(|n| u32::try_from(n).ok()).unwrap_or(0);
            let id = d.get(b"NM").and_then(Object::as_string).map(PdfString::to_text).unwrap_or_default();
            out.push(Count { page, index, id, item, number, at });
        }
    }
    out
}

/// What a measurement's caption shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Caption {
    /// "12.30 m".
    #[default]
    Value,
    /// "Kerb: 12.30 m" (the value alone when there is no item).
    ItemAndValue,
    /// No caption on the page; the value is still listed and totalled.
    Hidden,
}

/// Line and label styling applied to a saved measurement beyond its colour and width.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Look {
    pub dashed: bool,
    pub caption: Caption,
    /// Caption text size in points (clamped to 4..=72).
    pub caption_size: f64,
    /// Areas only: a depth that turns the area into a volume.
    pub depth: Option<Depth>,
}
impl Default for Look {
    fn default() -> Self {
        Self { dashed: false, caption: Caption::Value, caption_size: 10.0, depth: None }
    }
}

/// Depth of an area in the scale's length unit. The volume uses the average of the two,
/// so a fixed depth has `start == end` and a fall (e.g. 150 mm to 300 mm) differs.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Depth {
    pub start: f64,
    pub end: f64,
}
impl Depth {
    pub fn average(self) -> Option<f64> {
        let d = (self.start + self.end) / 2.0;
        (self.start.is_finite() && self.end.is_finite() && self.start >= 0.0 && self.end >= 0.0 && d > 0.0 && d <= 1e9).then_some(d)
    }
}
const DEPTH_KEY: &[u8] = b"PCTakeoffDepth";

/// An area measurement's volume.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Volume {
    pub page: usize,
    pub index: usize,
    pub item: String,
    pub depth: Depth,
    pub value: f64,
    pub unit: String,
}
/// Volumes of the area measurements that have a depth.
pub fn volumes(doc: &Document, measurements: &[Measurement]) -> Vec<Volume> {
    let pages = pdfcraft_model::pages(doc);
    let mut out = Vec::new();
    for m in measurements.iter().filter(|m| m.reading.kind == Kind::Area) {
        let Some(p) = pages.get(m.page) else { continue };
        let Some(list) = p.dict.get(b"Annots").map(|o| doc.resolve(o)) else { continue };
        let Some(a) = list.as_array().and_then(|a| a.get(m.index)).map(|o| doc.resolve(o)) else { continue };
        let Some(d) = a.as_dict() else { continue };
        let depth: Option<Vec<f64>> = d.get(DEPTH_KEY).and_then(Object::as_array).and_then(|v| v.iter().map(Object::as_f64).collect());
        let Some(&[start, end]) = depth.as_deref() else { continue };
        let depth = Depth { start, end };
        let Some(avg) = depth.average() else { continue };
        let value = m.reading.value * avg;
        if value.is_finite() {
            out.push(Volume { page: m.page, index: m.index, item: m.label.clone(), depth, value, unit: format!("{}^3", m.scale.unit) });
        }
    }
    out
}

/// Apply `look` to the measurement annotation at `index` on `page` (just added by
/// `pdfcraft_measure::add`) and regenerate its appearance.
pub fn apply_look(doc: &mut Document, page: usize, index: usize, look: &Look, measurement: &pdfcraft_measure::NewMeasurement) -> Result<()> {
    if *look == Look::default() {
        return Ok(());
    }
    let (obj, mut d) = annotation(doc, page, index)?;
    let mut volume = None;
    if let Some(depth) = look.depth.filter(|_| measurement.kind == Kind::Area) {
        let avg = depth.average().ok_or_else(|| invalid("depth must be positive and finite"))?;
        let area = pdfcraft_measure::reading(Kind::Area, &measurement.points, &measurement.scale).map_err(|e| invalid(&e.to_string()))?;
        let p = usize::from(measurement.scale.precision);
        let unit = &measurement.scale.unit;
        let depth_text = if (depth.start - depth.end).abs() < 1e-12 {
            format!("{:.p$} {unit}", depth.start)
        } else {
            format!("{:.p$}-{:.p$} {unit}", depth.start, depth.end)
        };
        volume = Some(format!("{} x {depth_text} = {:.p$} {unit}^3", area.label, area.value * avg));
        d.set(DEPTH_KEY.to_vec(), Object::Array(vec![Object::Real(depth.start), Object::Real(depth.end)]));
    }
    if look.dashed {
        let mut bs = d.get(b"BS").and_then(Object::as_dict).cloned().unwrap_or_default();
        if !bs.contains(b"W") {
            bs.set(b"W".to_vec(), Object::Real(1.0));
        }
        bs.set(b"S".to_vec(), Object::name("D"));
        bs.set(b"D".to_vec(), Object::Array(vec![Object::Real(4.0), Object::Real(3.0)]));
        d.set(b"BS".to_vec(), Object::Dict(bs));
    }
    let value = volume.unwrap_or_else(|| d.get(b"PCMeasureValue").and_then(Object::as_string).map(PdfString::to_text).unwrap_or_default());
    let item = d.get(b"PCMeasureLabel").and_then(Object::as_string).map(PdfString::to_text).unwrap_or_default();
    let caption = match look.caption {
        Caption::Value => value,
        Caption::ItemAndValue if item.trim().is_empty() => value,
        Caption::ItemAndValue => format!("{}: {value}", item.trim()),
        Caption::Hidden => String::new(),
    };
    d.set(b"PCMeasureValue".to_vec(), PdfString::text(&caption));
    let size = if look.caption_size.is_finite() { look.caption_size.clamp(4.0, 72.0) } else { 10.0 };
    d.set(b"PCMeasureCaptionSize".to_vec(), Object::Real(size));
    // Room for a wider or taller caption above the geometry (add() reserved 16 pt for 10 pt text).
    if let Some(v) = d.get(b"Rect").and_then(Object::as_array).and_then(|a| a.iter().map(Object::as_f64).collect::<Option<Vec<f64>>>())
        && let [x0, y0, x1, y1] = v.as_slice()
    {
        let width = (caption.chars().count() as f64 * size * 0.6).max(40.0);
        let cx = (x0 + x1) * 0.5;
        let rect = [x0.min(cx - width / 2.0), *y0, x1.max(cx + width / 2.0), y1 + (size - 10.0).max(0.0)];
        d.set(b"Rect".to_vec(), Object::Array(rect.into_iter().map(Object::Real).collect()));
    }
    doc.update_dict(obj, |target| *target = d).map_err(|e| invalid(&e.to_string()))?;
    pdfcraft_annot::set_appearance(doc, obj)?;
    Ok(())
}

/// Item names already used in the document, first spelling of each, sorted case-insensitively.
pub fn items(lines: &[Line]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for l in lines {
        if l.item != UNTAGGED && !out.iter().any(|i| i.eq_ignore_ascii_case(&l.item)) {
            out.push(l.item.clone());
        }
    }
    out
}

/// One row of the take-off: an item's total in one unit.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Line {
    pub item: String,
    pub quantity: Quantity,
    pub unit: String,
    pub total: f64,
    /// How many markers or measurements make up the total.
    pub count: usize,
    /// 1-based pages they are on, ascending.
    pub pages: Vec<usize>,
}

/// Totals per item, quantity and unit, sorted by item (case-insensitively), then counts,
/// lengths and areas, then unit. Items differing only in case or surrounding spaces are
/// the same item; the first spelling seen is shown. Measurements of one item in different
/// units (a sheet calibrated in metres and another in feet) stay on separate lines rather
/// than being converted.
pub fn summarise(measurements: &[Measurement], counts: &[Count]) -> Vec<Line> {
    summarise_all(measurements, counts, &[])
}
/// [`summarise`] plus a volume line per item and unit for areas with a depth.
pub fn summarise_all(measurements: &[Measurement], counts: &[Count], volumes: &[Volume]) -> Vec<Line> {
    let mut lines: BTreeMap<(String, Quantity, String), Line> = BTreeMap::new();
    let mut add = |label: &str, quantity: Quantity, unit: &str, value: f64, page: usize| {
        let name = item_name(label);
        let line = lines.entry((name.to_lowercase(), quantity, unit.to_string())).or_insert_with(|| Line {
            item: name.to_string(),
            quantity,
            unit: unit.to_string(),
            total: 0.0,
            count: 0,
            pages: Vec::new(),
        });
        if value.is_finite() {
            line.total += value;
        }
        line.count = line.count.saturating_add(1);
        let page = page.saturating_add(1);
        if let Err(at) = line.pages.binary_search(&page) {
            line.pages.insert(at, page);
        }
    };
    for c in counts {
        add(&c.item, Quantity::Count, COUNT_UNIT, 1.0, c.page);
    }
    for m in measurements {
        add(&m.label, Quantity::of(m.reading.kind), &m.reading.unit, m.reading.value, m.page);
    }
    for v in volumes {
        add(&v.item, Quantity::Volume, &v.unit, v.value, v.page);
    }
    lines.into_values().collect()
}

/// Spreadsheet-safe CSV of a summary. Totals are rounded to `precision` decimal places
/// (at most 6); counts are whole numbers.
pub fn csv(lines: &[Line], precision: u8) -> String {
    let precision = usize::from(precision.min(6));
    let mut out = "Item,Quantity,Total,Unit,Count,Pages\r\n".to_string();
    for l in lines {
        let pages = l.pages.iter().map(usize::to_string).collect::<Vec<_>>().join(" ");
        let p = if l.quantity == Quantity::Count { 0 } else { precision };
        out.push_str(&format!("{},{},{:.p$},{},{},{}\r\n", field(&l.item), l.quantity.name(), l.total, field(&l.unit), l.count, field(&pages)));
    }
    out
}

// Prefix spreadsheet formula triggers; quoting alone does not prevent evaluation.
fn field(s: &str) -> String {
    let prefix = if s.trim_start().starts_with(['=', '+', '-', '@']) || s.starts_with(['\t', '\r']) { "'" } else { "" };
    format!("\"{prefix}{}\"", s.replace('"', "\"\""))
}

#[cfg(test)]
mod tests;
