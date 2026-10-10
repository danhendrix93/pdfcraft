//! Quantity take-off tools: count markers, and counts and measurements totalled per item.
use crate::{Args, Automation, Result, failed};
use pdfcraft_engine::{Edit, takeoff};
use serde_json::{Value, json};

impl Automation {
    pub(crate) fn takeoff_count(&mut self, a: &Args) -> Result<Value> {
        let page = self.page(a)?;
        let at = a.nums::<2>("at")?.ok_or_else(|| failed("a count marker needs at"))?;
        let at = self.doc(a)?.measurement_to_user(page, at).map_err(failed)?;
        let color = super::comments::parse_color(a.opt_str("color")?.unwrap_or("red"))?;
        let edit = Edit::AddCount(takeoff::NewCount {
            page,
            at,
            item: a.str("item")?.into(),
            color,
            size: a.opt_num("size")?.unwrap_or(takeoff::MARKER_SIZE),
            author: a.opt_str("author")?.unwrap_or(super::comments::DEFAULT_AUTHOR).into(),
        });
        self.apply(a, edit)
    }
    pub(crate) fn takeoff_counts(&self, a: &Args) -> Result<Value> {
        let doc = self.doc(a)?;
        let counts: Vec<Value> = doc
            .counts()
            .map_err(failed)?
            .iter()
            .map(|c| -> Result<Value> {
                let at = doc.measurement_to_view(c.page, c.at).map_err(failed)?;
                Ok(json!({"page":c.page + 1,"index":c.index + 1,"id":c.id,"item":c.item,"number":c.number,"at":at}))
            })
            .collect::<Result<_>>()?;
        Ok(json!({"count":counts.len(),"counts":counts}))
    }
    fn takeoff_lines(&self, a: &Args) -> Result<(Vec<takeoff::Line>, usize, bool)> {
        let doc = self.doc(a)?;
        let all = doc.measurements().map_err(failed)?;
        let counts = doc.counts().map_err(failed)?;
        Ok((takeoff::summarise(&all.measurements, &counts), all.unsupported.len(), all.truncated))
    }
    pub(crate) fn takeoff_summary(&self, a: &Args) -> Result<Value> {
        let (lines, unsupported, truncated) = self.takeoff_lines(a)?;
        Ok(json!({"count":lines.len(),"lines":lines,"unsupported":unsupported,"truncated":truncated}))
    }
    pub(crate) fn takeoff_export(&self, a: &Args) -> Result<Value> {
        let precision =
            u8::try_from(a.opt_int("precision")?.unwrap_or(2)).ok().filter(|p| *p <= 6).ok_or_else(|| failed("precision must be between 0 and 6"))?;
        let (lines, unsupported, truncated) = self.takeoff_lines(a)?;
        let target = self.resolve(a.str("out")?, true)?;
        crate::write_atomic(&target, takeoff::csv(&lines, precision).as_bytes())?;
        Ok(json!({"path":target,"count":lines.len(),"unsupported":unsupported,"truncated":truncated}))
    }
}
