//! Listing, editing and removing a page's measurement viewports (ISO 32000-2 §12.9),
//! the rectangles that carry a drawing scale. `pdfcraft_measure::set_scale` adds them.
use crate::{Result, invalid};
use pdfcraft_cos::{Document, Object, PdfString};
use pdfcraft_measure::Scale;
use serde::Serialize;

const MAX_VIEWPORTS: usize = 1024;

/// Common drawing units and their length in metres.
pub const UNITS: [(&str, f64); 7] = [("mm", 0.001), ("cm", 0.01), ("m", 1.0), ("km", 1000.0), ("in", 0.0254), ("ft", 0.3048), ("yd", 0.9144)];

/// Metres per `unit`, for the units in [`UNITS`].
pub fn metres_per(unit: &str) -> Option<f64> {
    UNITS.iter().find(|(u, _)| u.eq_ignore_ascii_case(unit.trim())).map(|(_, m)| *m)
}
/// `value` in `from` expressed in `to`, when both are in [`UNITS`].
pub fn convert(value: f64, from: &str, to: &str) -> Option<f64> {
    let out = value * metres_per(from)? / metres_per(to)?;
    out.is_finite().then_some(out)
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Viewport {
    /// 0-based position in the page's /VP array (later viewports win where they overlap).
    pub index: usize,
    pub name: String,
    /// PDF user space.
    pub bbox: [f64; 4],
    /// The scale, or why it can't be read (compound or non-rectilinear imports).
    pub scale: std::result::Result<Scale, String>,
}

fn page(doc: &Document, page: usize) -> Result<pdfcraft_model::Page> {
    pdfcraft_model::pages(doc).get(page).cloned().ok_or_else(|| invalid("page does not exist"))
}
fn list(doc: &Document, p: &pdfcraft_model::Page) -> Result<Vec<Object>> {
    match p.dict.get(b"VP") {
        Some(o) => doc.resolve(o).as_array().cloned().ok_or_else(|| invalid("invalid page viewports")),
        None => Ok(Vec::new()),
    }
}
fn store(doc: &mut Document, p: &pdfcraft_model::Page, list: Vec<Object>) -> Result<()> {
    // An indirect /VP array stays indirect (it may be shared); otherwise the page holds it.
    match p.dict.get(b"VP").and_then(Object::as_ref) {
        Some(r) => doc.set(r, Object::Array(list)),
        None if list.is_empty() => doc
            .update_dict(p.obj, |d| {
                d.remove(b"VP");
            })
            .map_err(|e| invalid(&e.to_string()))?,
        None => doc.update_dict(p.obj, |d| d.set(b"VP".to_vec(), Object::Array(list))).map_err(|e| invalid(&e.to_string()))?,
    }
    Ok(())
}

/// The page's viewports in drawing order.
pub fn viewports(doc: &Document, page_index: usize) -> Result<Vec<Viewport>> {
    let p = page(doc, page_index)?;
    let mut out = Vec::new();
    for (index, o) in list(doc, &p)?.iter().enumerate().take(MAX_VIEWPORTS) {
        let o = doc.resolve(o);
        let Some(d) = o.as_dict() else { continue };
        let bbox: Option<Vec<f64>> =
            d.get(b"BBox").map(|b| doc.resolve(b)).and_then(|b| b.as_array().and_then(|a| a.iter().map(|v| doc.resolve(v).as_f64()).collect()));
        let Some(&[x0, y0, x1, y1]) = bbox.as_deref() else { continue };
        let name = d.get(b"Name").and_then(Object::as_string).map(PdfString::to_text).unwrap_or_default();
        let scale = match d.get(b"Measure") {
            Some(m) => Scale::read(doc, m).map_err(|e| e.to_string()),
            None => Err("viewport has no measurement scale".into()),
        };
        out.push(Viewport { index, name, bbox: [x0.min(x1), y0.min(y1), x0.max(x1), y0.max(y1)], scale });
    }
    Ok(out)
}

/// Replace viewport `index` with a new rectangle, name and scale. Saved measurements keep
/// the scale they were drawn with; only new ones use the change.
pub fn replace_viewport(doc: &mut Document, page_index: usize, index: usize, bbox: [f64; 4], name: &str, scale: &Scale) -> Result<()> {
    if bbox.iter().any(|v| !v.is_finite() || v.abs() > 1e9) || bbox[2] <= bbox[0] || bbox[3] <= bbox[1] || name.len() > 256 {
        return Err(invalid("viewport needs a positive rectangle and a name of at most 256 bytes"));
    }
    let p = page(doc, page_index)?;
    let mut items = list(doc, &p)?;
    let slot = items.get_mut(index).ok_or_else(|| invalid("viewport does not exist"))?;
    let mut d = doc.resolve(slot).as_dict().cloned().unwrap_or_default();
    d.set(b"Type".to_vec(), Object::name("Viewport"));
    d.set(b"BBox".to_vec(), Object::Array(bbox.into_iter().map(Object::Real).collect()));
    d.set(b"Name".to_vec(), PdfString::text(name));
    d.set(b"Measure".to_vec(), Object::Dict(scale.dictionary().map_err(|e| invalid(&e.to_string()))?));
    *slot = Object::Dict(d);
    store(doc, &p, items)
}

/// Remove viewport `index`. Saved measurements keep their own scales.
pub fn remove_viewport(doc: &mut Document, page_index: usize, index: usize) -> Result<()> {
    let p = page(doc, page_index)?;
    let mut items = list(doc, &p)?;
    if index >= items.len() {
        return Err(invalid("viewport does not exist"));
    }
    items.remove(index);
    store(doc, &p, items)
}
