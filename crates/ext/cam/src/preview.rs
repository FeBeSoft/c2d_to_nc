//! Aperçu SVG d'un travail : brut, formes du dessin, trajectoires d'usinage par opération et
//! déplacements rapides. Échelle 1 mm = 4 px, Y vers le haut comme sur la machine.

use std::fmt::Write as _;

use makercraft_geom::{Vec2, Vec3};

use crate::job::Job;
use crate::{Move, Step};

const SCALE: f64 = 4.0;
const MARGIN: f64 = 10.0;
const COLORS: [&str; 8] = ["#1f6feb", "#d73a49", "#2da44e", "#bf8700", "#8250df", "#0598bc", "#cf222e", "#6e7781"];

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

pub fn svg(job: &Job) -> String {
    let (w, h, _) = job.stock_size;
    let px = |p: Vec2| (MARGIN + p.x * SCALE, MARGIN + (h - p.y) * SCALE);
    let width = w * SCALE + 2.0 * MARGIN;
    let height = h * SCALE + 2.0 * MARGIN + 30.0 + 18.0 * job.groups.iter().map(|g| g.ops.len()).sum::<usize>() as f64;
    let mut s = String::new();
    let _ = writeln!(
        s,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{width:.0}" height="{height:.0}" viewBox="0 0 {width:.0} {height:.0}" font-family="sans-serif" font-size="13">"#
    );
    let _ = writeln!(s, r##"<rect x="0" y="0" width="{width:.0}" height="{height:.0}" fill="#ffffff"/>"##);
    let _ = writeln!(s, r##"<rect x="{MARGIN}" y="{MARGIN}" width="{:.1}" height="{:.1}" fill="#f3e9d2" stroke="#8a7350"/>"##, w * SCALE, h * SCALE);
    // dessin
    for c in &job.design {
        let pts: Vec<String> = c.points.iter().map(|p| px(*p)).map(|(x, y)| format!("{x:.2},{y:.2}")).collect();
        let tag = if c.closed { "polygon" } else { "polyline" };
        let _ = writeln!(s, r##"<{tag} points="{}" fill="none" stroke="#9a8f80" stroke-width="0.8"/>"##, pts.join(" "));
    }
    // trajectoires
    let mut legend = Vec::new();
    let mut k = 0usize;
    let mut pos = Vec3::new(0.0, 0.0, job.heights.safe_z);
    for g in &job.groups {
        for op in &g.ops {
            let color = COLORS[k % COLORS.len()];
            k += 1;
            legend.push((color, format!("T{} {} : {}", g.tool.number, op.kind, op.name)));
            let mut cut = String::new();
            let mut rapid = String::new();
            for st in &op.steps {
                let Step::Move(m) = st else { continue };
                let (a, b) = (px(pos.xy()), px(m.target().xy()));
                let seg = format!("M{:.2} {:.2}L{:.2} {:.2}", a.0, a.1, b.0, b.1);
                match m {
                    Move::Feed(..) if m.target().z < 0.0 => cut.push_str(&seg),
                    Move::Rapid(_) if (m.target().xy() - pos.xy()).length() > 1e-6 => rapid.push_str(&seg),
                    _ => {}
                }
                pos = m.target();
            }
            let _ = writeln!(s, r##"<path d="{rapid}" fill="none" stroke="#c0c0c0" stroke-width="0.6" stroke-dasharray="3 3"/>"##);
            let _ = writeln!(s, r#"<path d="{cut}" fill="none" stroke="{color}" stroke-width="1.2" stroke-linejoin="round"/>"#);
        }
    }
    // légende
    let mut y = h * SCALE + 2.0 * MARGIN + 18.0;
    let _ = writeln!(
        s,
        r#"<text x="{MARGIN}" y="{y:.0}">{}</text>"#,
        esc(&format!("{} - brut {} x {} mm - gris : dessin, pointillés : rapides", job.title, w, h))
    );
    for (color, label) in legend {
        y += 18.0;
        let _ = writeln!(
            s,
            r#"<rect x="{MARGIN}" y="{:.0}" width="14" height="10" fill="{color}"/><text x="{:.0}" y="{y:.0}">{}</text>"#,
            y - 10.0,
            MARGIN + 20.0,
            esc(&label)
        );
    }
    s.push_str("</svg>\n");
    s
}
