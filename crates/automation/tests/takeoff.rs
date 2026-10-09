//! Quantity take-off tools: count markers, the per-item summary and its CSV export.
use pdfcraft_automation::{Automation, Content};
use serde_json::{Value, json};
use std::path::PathBuf;

/// Two blank 200×300 pt pages.
fn fixture() -> Vec<u8> {
    let objs = [
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 /MediaBox [0 0 200 300] >>",
        "<< /Type /Page /Parent 2 0 R >>",
        "<< /Type /Page /Parent 2 0 R >>",
    ];
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, o) in objs.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{o}\nendobj\n", i + 1).as_bytes());
    }
    let xref = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).as_bytes());
    for o in offsets {
        out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objs.len() + 1).as_bytes());
    out
}
fn workdir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pdfcraft-automation-takeoff-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("a.pdf"), fixture()).unwrap();
    dir
}
fn ok(a: &mut Automation, tool: &str, args: Value) -> Value {
    match a.call(tool, &args) {
        Ok(mut c) => match c.remove(0) {
            Content::Json(v) => v,
            other => panic!("{tool}: expected JSON, got {other:?}"),
        },
        Err(e) => panic!("{tool} {args}: {e}"),
    }
}

#[test]
fn counts_and_measurements_total_per_item_save_reopen_and_export() {
    let dir = workdir();
    let mut a = Automation::new().with_root(&dir).unwrap().with_clock(|| 1_700_000_000);
    let doc = ok(&mut a, "doc_open", json!({"path":"a.pdf"}))["doc"].as_u64().unwrap();
    ok(&mut a, "measure_scale", json!({"doc":doc,"page":1,"points":[[10,10],[70,10]],"distance":6,"unit":"m"}));
    ok(&mut a, "measure_distance", json!({"doc":doc,"page":1,"points":[[10,20],[70,20]],"label":"Kerb"}));
    ok(&mut a, "measure_distance", json!({"doc":doc,"page":1,"points":[[10,40],[40,40]],"label":"kerb"}));
    for (page, at) in [(1, [20, 200]), (1, [60, 200]), (2, [30, 30])] {
        ok(&mut a, "takeoff_count", json!({"doc":doc,"page":page,"at":at,"item":"Gully"}));
    }
    ok(&mut a, "takeoff_count", json!({"doc":doc,"page":2,"at":[100,100],"item":"Manhole","color":"blue"}));
    assert!(a.call("takeoff_count", &json!({"doc":doc,"page":1,"at":[10,10],"item":" "})).is_err());
    assert!(a.call("takeoff_count", &json!({"doc":doc,"page":9,"at":[10,10],"item":"Gully"})).is_err());

    let counts = ok(&mut a, "takeoff_counts", json!({"doc":doc}));
    assert_eq!(counts["count"], 4);
    let first = &counts["counts"][0];
    assert_eq!((first["page"].as_u64(), first["item"].as_str(), first["number"].as_u64()), (Some(1), Some("Gully"), Some(1)));
    let at: Vec<f64> = first["at"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
    assert!((at[0] - 20.0).abs() < 1e-6 && (at[1] - 200.0).abs() < 1e-6, "{at:?}");
    assert_eq!(counts["counts"][2]["number"], 3);

    let summary = ok(&mut a, "takeoff_summary", json!({"doc":doc}));
    let rows: Vec<(String, String, f64, String)> = summary["lines"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| {
            (
                l["item"].as_str().unwrap().into(),
                l["quantity"].as_str().unwrap().into(),
                l["total"].as_f64().unwrap(),
                l["unit"].as_str().unwrap().into(),
            )
        })
        .collect();
    assert_eq!(rows.len(), 3, "{rows:?}");
    assert_eq!((rows[0].0.as_str(), rows[0].1.as_str(), rows[0].2, rows[0].3.as_str()), ("Gully", "count", 3.0, "nr"));
    assert_eq!((rows[1].0.as_str(), rows[1].1.as_str(), rows[1].3.as_str()), ("Kerb", "length", "m"));
    assert!((rows[1].2 - 9.0).abs() < 1e-6, "{rows:?}");
    assert_eq!((rows[2].0.as_str(), rows[2].2), ("Manhole", 1.0));

    ok(&mut a, "edit_undo", json!({"doc":doc}));
    assert_eq!(ok(&mut a, "takeoff_counts", json!({"doc":doc}))["count"], 3);
    ok(&mut a, "edit_redo", json!({"doc":doc}));
    ok(&mut a, "doc_save", json!({"doc":doc,"path":"counted.pdf"}));
    let reopened = ok(&mut a, "doc_open", json!({"path":"counted.pdf"}))["doc"].as_u64().unwrap();
    assert_eq!(ok(&mut a, "takeoff_summary", json!({"doc":reopened}))["lines"], summary["lines"]);
    ok(&mut a, "takeoff_export", json!({"doc":reopened,"out":"takeoff.csv","precision":1}));
    let csv = std::fs::read_to_string(dir.join("takeoff.csv")).unwrap();
    assert_eq!(
        csv,
        "Item,Quantity,Total,Unit,Count,Pages\r\n\"Gully\",count,3,\"nr\",3,\"1 2\"\r\n\"Kerb\",length,9.0,\"m\",2,\"1\"\r\n\"Manhole\",count,1,\"nr\",1,\"2\"\r\n"
    );
    assert!(a.call("takeoff_export", &json!({"doc":doc,"out":"../outside.csv"})).is_err());
    assert!(a.call("takeoff_export", &json!({"doc":doc,"out":"x.csv","precision":7})).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}
