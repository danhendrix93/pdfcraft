use super::*;
use pdfcraft_cos::{SaveOptions, write_full};
use pdfcraft_measure::{Scale, reading};
use std::sync::Arc;

fn blank(pages: usize) -> Document {
    let kids: Vec<String> = (0..pages).map(|i| format!("{} 0 R", i + 3)).collect();
    let mut objects = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        format!("<< /Type /Pages /Kids [{}] /Count {pages} /MediaBox [0 0 612 792] >>", kids.join(" ")),
    ];
    objects.extend((0..pages).map(|_| "<< /Type /Page /Parent 2 0 R >>".to_string()));
    let mut bytes = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, o) in objects.iter().enumerate() {
        offsets.push(bytes.len());
        bytes.extend(format!("{} 0 obj\n{o}\nendobj\n", i + 1).as_bytes());
    }
    let xref = bytes.len();
    bytes.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes());
    for o in offsets {
        bytes.extend(format!("{o:010} 00000 n \n").as_bytes());
    }
    bytes.extend(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objects.len() + 1).as_bytes());
    Document::open(Arc::new(bytes)).unwrap()
}
fn count(page: usize, at: Point, item: &str) -> NewCount {
    NewCount { page, at, item: item.into(), color: [0.9, 0.1, 0.1], size: MARKER_SIZE, author: "A".into() }
}
fn meta() -> Meta {
    Meta { date: None, id: "count".into() }
}

#[test]
fn count_markers_number_per_item_and_survive_save() {
    let mut d = blank(2);
    assert_eq!(add_count(&mut d, &count(0, [100.0, 200.0], "Gully"), &meta()).unwrap(), (0, 1));
    assert_eq!(add_count(&mut d, &count(1, [50.0, 60.0], " gully "), &meta()).unwrap(), (0, 2));
    assert_eq!(add_count(&mut d, &count(0, [10.0, 20.0], "Manhole"), &meta()).unwrap(), (1, 1));
    assert_eq!(add_count(&mut d, &count(1, [70.0, 80.0], "Gully"), &meta()).unwrap(), (1, 3));
    let bytes = write_full(&d, &SaveOptions::default()).unwrap();
    let d = Document::open(Arc::new(bytes)).unwrap();
    let found = counts(&d);
    let rows: Vec<_> = found.iter().map(|c| (c.page, c.item.as_str(), c.number, c.at)).collect();
    assert_eq!(
        rows,
        [(0, "Gully", 1, [100.0, 200.0]), (0, "Manhole", 1, [10.0, 20.0]), (1, "gully", 2, [50.0, 60.0]), (1, "Gully", 3, [70.0, 80.0])]
    );
    let lines = summarise(&[], &found);
    let rows: Vec<_> = lines.iter().map(|l| (l.item.as_str(), l.quantity, l.unit.as_str(), l.total, l.pages.clone())).collect();
    assert_eq!(rows, [("Gully", Quantity::Count, COUNT_UNIT, 3.0, vec![1, 2]), ("Manhole", Quantity::Count, COUNT_UNIT, 1.0, vec![1])]);
}

#[test]
fn count_markers_reject_bad_input() {
    let mut d = blank(1);
    for bad in [
        count(0, [f64::NAN, 0.0], "Gully"),
        count(0, [2e9, 0.0], "Gully"),
        count(0, [1.0, 1.0], "  "),
        count(0, [1.0, 1.0], "a\nb"),
        count(3, [1.0, 1.0], "Gully"),
    ] {
        assert!(add_count(&mut d, &bad, &meta()).is_err(), "{bad:?}");
    }
    assert!(add_count(&mut d, &count(0, [1.0, 1.0], &"x".repeat(513)), &meta()).is_err());
    assert!(counts(&d).is_empty());
}

fn metres() -> Scale {
    Scale::new(0.1, "m", 2).unwrap()
}
fn m(page: usize, label: &str, kind: Kind, points: &[Point], scale: &Scale) -> Measurement {
    let reading = reading(kind, points, scale).unwrap();
    Measurement {
        page,
        index: 0,
        id: String::new(),
        author: String::new(),
        label: label.into(),
        points: points.to_vec(),
        scale: scale.clone(),
        reading,
    }
}

#[test]
fn lengths_and_areas_total_per_item_and_unit() {
    let s = metres();
    let feet = Scale::new(1.0, "ft", 2).unwrap();
    let list = [
        m(0, "Kerb", Kind::Distance, &[[0.0, 0.0], [100.0, 0.0]], &s),
        m(2, " kerb ", Kind::Perimeter, &[[0.0, 0.0], [30.0, 0.0], [30.0, 40.0]], &s),
        m(1, "Kerb", Kind::Distance, &[[0.0, 0.0], [10.0, 0.0]], &feet),
        m(0, "Kerb", Kind::Area, &[[0.0, 0.0], [10.0, 0.0], [10.0, 10.0]], &s),
        m(1, "", Kind::Distance, &[[0.0, 0.0], [0.0, 50.0]], &s),
        m(0, "Asphalt", Kind::Area, &[[0.0, 0.0], [100.0, 0.0], [100.0, 100.0], [0.0, 100.0]], &s),
    ];
    let lines = summarise(&list, &[]);
    let rows: Vec<_> = lines.iter().map(|l| (l.item.as_str(), l.quantity, l.unit.as_str(), l.count, l.pages.clone())).collect();
    assert_eq!(
        rows,
        [
            ("Asphalt", Quantity::Area, "m^2", 1, vec![1]),
            ("Kerb", Quantity::Length, "ft", 1, vec![2]),
            ("Kerb", Quantity::Length, "m", 2, vec![1, 3]),
            ("Kerb", Quantity::Area, "m^2", 1, vec![1]),
            (UNTAGGED, Quantity::Length, "m", 1, vec![2]),
        ]
    );
    let total = |i: usize| lines[i].total;
    assert!((total(0) - 100.0).abs() < 1e-9);
    assert!((total(1) - 10.0).abs() < 1e-9);
    assert!((total(2) - 17.0).abs() < 1e-9); // 10 m + (3 m + 4 m)
    assert!((total(3) - 0.5).abs() < 1e-9);
    assert!((total(4) - 5.0).abs() < 1e-9);
}

#[test]
fn empty_input_and_csv_is_spreadsheet_safe() {
    assert!(summarise(&[], &[]).is_empty());
    let s = metres();
    let mut lines = summarise(&[m(0, "=SUM(A1) \"x\"", Kind::Distance, &[[0.0, 0.0], [12.346, 0.0]], &s)], &[]);
    lines.extend(summarise(&[], &[Count { page: 1, index: 0, id: String::new(), item: "Gully".into(), number: 1, at: [0.0, 0.0] }]));
    let csv = csv(&lines, 3);
    assert_eq!(
        csv,
        "Item,Quantity,Total,Unit,Count,Pages\r\n\"'=SUM(A1) \"\"x\"\"\",length,1.235,\"m\",1,\"1\"\r\n\"Gully\",count,1,\"nr\",1,\"2\"\r\n"
    );
}
