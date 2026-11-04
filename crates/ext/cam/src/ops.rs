//! Opérations d'usinage 2.5D.
//!
//! Décalages d'outil avec la crate `geo` (MIT/Apache) : `Buffer` pour élargir un contour,
//! soustraction (`BooleanOps`) d'une bande autour du bord pour le rétrécir.

use geo::algorithm::buffer::Buffer;
use geo::{BooleanOps, Coord, LineString, MultiLineString, MultiPolygon, Polygon};
use makercraft_c2d::Contour;
use makercraft_geom::{Vec2, Vec3};

use crate::{Heights, Pen, Step, depth_levels};

/// Réglages communs d'une opération.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cut {
    /// Rayon de l'outil (mm).
    pub tool_radius: f64,
    /// Profondeurs (positives) : départ, fin, pas.
    pub start_depth: f64,
    pub end_depth: f64,
    pub stepdown: f64,
    /// Avance et plongée (mm/min).
    pub feed: f64,
    pub plunge: f64,
    /// Avalant (climb) au lieu de l'opposition.
    pub climb: bool,
}

/// Où passer par rapport au trait.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Outside,
    Inside,
    OnLine,
}

// ------------------------------------------------------------------ géométrie

fn ring_area(r: &[Vec2]) -> f64 {
    let n = r.len();
    if n < 3 {
        return 0.0;
    }
    (0..n).map(|i| r[i].cross(r[(i + 1) % n])).sum::<f64>() / 2.0
}

fn to_ls(r: &[Vec2]) -> LineString<f64> {
    let mut pts: Vec<Coord<f64>> = r.iter().map(|p| Coord { x: p.x, y: p.y }).collect();
    if let Some(&first) = pts.first() {
        pts.push(first);
    }
    LineString::new(pts)
}

fn from_ls(ls: &LineString<f64>) -> Vec<Vec2> {
    let mut v: Vec<Vec2> = ls.0.iter().map(|c| Vec2::new(c.x, c.y)).collect();
    if v.len() > 1 && v.first().zip(v.last()).is_some_and(|(a, b)| (*a - *b).length() < 1e-9) {
        v.pop();
    }
    v
}

/// Toutes les boucles (extérieurs et trous) d'une région.
fn rings(mp: &MultiPolygon<f64>) -> Vec<Vec<Vec2>> {
    let mut out = Vec::new();
    for p in &mp.0 {
        out.push(from_ls(p.exterior()));
        out.extend(p.interiors().iter().map(from_ls));
    }
    out.retain(|r| r.len() >= 3 && ring_area(r).abs() > 1e-6);
    out
}

/// Rétrécit une région de `d` (vide si l'outil n'y passe pas).
pub fn inset(region: &MultiPolygon<f64>, d: f64) -> MultiPolygon<f64> {
    if d <= 1e-9 {
        return region.clone();
    }
    let mut lines = Vec::new();
    for p in &region.0 {
        lines.push(p.exterior().clone());
        lines.extend(p.interiors().iter().cloned());
    }
    let band = MultiLineString::new(lines).buffer(d);
    region.difference(&band)
}

/// Élargit un contour fermé de `d`.
pub fn outset(ring: &[Vec2], d: f64) -> Vec<Vec<Vec2>> {
    let poly = Polygon::new(to_ls(ring), vec![]);
    let grown = poly.buffer(d);
    grown.0.iter().map(|p| from_ls(p.exterior())).filter(|r| r.len() >= 3).collect()
}

/// Regroupe des contours fermés en régions (règle pair-impair : un contour dans un autre est
/// un trou, un contour dans un trou est une île…).
pub fn regions(contours: &[&Contour]) -> Vec<MultiPolygon<f64>> {
    let closed: Vec<&Contour> = contours.iter().copied().filter(|c| c.closed && c.points.len() >= 3).collect();
    let depth_of = |i: usize| -> usize {
        let Some(&p) = closed[i].points.first() else { return 0 };
        closed.iter().enumerate().filter(|(j, o)| *j != i && o.signed_area().abs() > closed[i].signed_area().abs() && o.contains(p)).count()
    };
    let depths: Vec<usize> = (0..closed.len()).map(depth_of).collect();
    let mut polys: Vec<(usize, Polygon<f64>)> = Vec::new();
    for (i, c) in closed.iter().enumerate() {
        if depths[i].is_multiple_of(2) {
            polys.push((i, Polygon::new(to_ls(&c.points), vec![])));
        }
    }
    for (i, c) in closed.iter().enumerate() {
        if depths[i] % 2 == 1 {
            // le plus petit extérieur qui le contient
            let Some(&p) = c.points.first() else { continue };
            let owner = polys
                .iter_mut()
                .filter(|(j, _)| depths[*j] + 1 == depths[i] && closed[*j].contains(p))
                .min_by(|a, b| closed[a.0].signed_area().abs().total_cmp(&closed[b.0].signed_area().abs()));
            if let Some((_, poly)) = owner {
                poly.interiors_push(to_ls(&c.points));
            }
        }
    }
    polys.into_iter().map(|(_, p)| MultiPolygon::new(vec![p])).collect()
}

/// Oriente une boucle : sens trigonométrique si `ccw`, sinon horaire.
fn oriented(mut r: Vec<Vec2>, ccw: bool) -> Vec<Vec2> {
    if (ring_area(&r) > 0.0) != ccw {
        r.reverse();
    }
    r
}

/// Ordonne des éléments au plus proche voisin à partir de `start` (moins de déplacements à vide).
pub fn nearest_order<T>(items: Vec<T>, start: Vec2, key: impl Fn(&T) -> Vec2) -> Vec<T> {
    let mut rest: Vec<Option<T>> = items.into_iter().map(Some).collect();
    let mut out = Vec::with_capacity(rest.len());
    let mut here = start;
    while let Some((i, p)) = rest
        .iter()
        .enumerate()
        .filter_map(|(i, t)| t.as_ref().map(|t| (i, key(t))))
        .min_by(|a, b| (a.1 - here).length().total_cmp(&(b.1 - here).length()))
    {
        if let Some(t) = rest.get_mut(i).and_then(Option::take) {
            out.push(t);
        }
        here = p;
    }
    out
}

// ------------------------------------------------------------------ trajectoires

fn follow_ring(pen: &mut Pen, ring: &[Vec2], z: f64, cut: &Cut) {
    let Some(&start) = ring.first() else { return };
    pen.approach(start);
    pen.plunge(z, cut.plunge);
    for p in ring.iter().skip(1).chain(std::iter::once(&start)) {
        pen.feed(Vec3::new(p.x, p.y, z), cut.feed);
    }
}

fn follow_open(pen: &mut Pen, pts: &[Vec2], z: f64, cut: &Cut, reverse: bool) {
    let seq: Vec<Vec2> = if reverse { pts.iter().rev().copied().collect() } else { pts.to_vec() };
    let Some(&start) = seq.first() else { return };
    pen.approach(start);
    pen.plunge(z, cut.plunge);
    for p in seq.iter().skip(1) {
        pen.feed(Vec3::new(p.x, p.y, z), cut.feed);
    }
}

/// Découpe de contours. Chaque contour fermé est usiné du côté demandé (toutes les passes en
/// profondeur avant de passer au suivant) ; les contours ouverts sont suivis sur le trait.
/// En opposition, l'extérieur se parcourt dans le sens trigonométrique, l'intérieur en horaire.
pub fn profile(h: Heights, items: &[(&Contour, Side)], cut: &Cut, warnings: &mut Vec<String>) -> Vec<Step> {
    let mut pen = Pen::new(h);
    let levels = depth_levels(cut.start_depth, cut.end_depth, cut.stepdown);
    for (c, side) in items {
        if !c.closed {
            for (k, d) in levels.iter().enumerate() {
                follow_open(&mut pen, &c.points, -d, cut, k % 2 == 1);
            }
            continue;
        }
        let paths: Vec<Vec<Vec2>> = match side {
            Side::OnLine => vec![c.points.clone()],
            Side::Outside => outset(&c.points, cut.tool_radius),
            Side::Inside => {
                let region = MultiPolygon::new(vec![Polygon::new(to_ls(&c.points), vec![])]);
                rings(&inset(&region, cut.tool_radius))
            }
        };
        if paths.is_empty() {
            warnings.push(format!("contour de {:.1} mm trop petit pour l'outil : ignoré", c.length()));
            continue;
        }
        let ccw = matches!(side, Side::Outside | Side::OnLine) != cut.climb;
        for path in paths {
            let path = oriented(path, ccw);
            for d in &levels {
                follow_ring(&mut pen, &path, -d, cut);
            }
        }
    }
    pen.lift();
    pen.steps
}

/// Poche : vide l'intérieur des régions par anneaux concentriques espacés de `stepover`, du
/// centre vers le bord, en laissant `stock_to_leave` contre le bord.
pub fn pocket(h: Heights, regions: &[MultiPolygon<f64>], cut: &Cut, stepover: f64, stock_to_leave: f64, warnings: &mut Vec<String>) -> Vec<Step> {
    let mut pen = Pen::new(h);
    let levels = depth_levels(cut.start_depth, cut.end_depth, cut.stepdown);
    let step = if stepover > 1e-3 { stepover.min(cut.tool_radius * 2.0) } else { cut.tool_radius };
    let mut too_small = 0usize;
    for region in regions {
        let mut layers: Vec<Vec<Vec<Vec2>>> = Vec::new();
        let mut k = 0usize;
        loop {
            let r = rings(&inset(region, cut.tool_radius + stock_to_leave + k as f64 * step));
            if r.is_empty() || k > 2000 {
                break;
            }
            layers.push(r);
            k += 1;
        }
        if layers.is_empty() {
            too_small += 1;
            continue;
        }
        for d in &levels {
            for ring_set in layers.iter().rev() {
                for r in ring_set {
                    // intérieur d'une poche : horaire en opposition
                    let r = oriented(r.clone(), cut.climb);
                    follow_ring(&mut pen, &r, -d, cut);
                }
            }
        }
        pen.lift();
    }
    if too_small > 0 {
        warnings.push(format!("{too_small} poche(s) plus étroite(s) que l'outil (avec la surépaisseur) : ignorée(s)"));
    }
    pen.lift();
    pen.steps
}

/// Perçage en plongée à chaque point, avec débourrage tous les `peck` mm (0 = d'une traite).
pub fn drill(h: Heights, points: &[Vec2], depth: f64, peck: f64, plunge: f64) -> Vec<Step> {
    let mut pen = Pen::new(h);
    for &p in points {
        pen.approach(p);
        if peck > 1e-3 {
            let mut done = 0.0f64;
            while done < depth - 1e-6 {
                let next = (done + peck).min(depth);
                if done > 0.0 {
                    // redescend vite jusqu'à 0,5 mm au-dessus du fond déjà percé
                    pen.rapid_down_to(-(done - 0.5).max(0.0));
                }
                pen.plunge(-next, plunge);
                pen.up_to(h.clear_z);
                done = next;
            }
        } else {
            pen.plunge(-depth, plunge);
            pen.up_to(h.clear_z);
        }
    }
    pen.lift();
    pen.steps
}

/// Gravure sur le trait (passes successives en profondeur).
pub fn engrave(h: Heights, contours: &[&Contour], cut: &Cut) -> Vec<Step> {
    let mut pen = Pen::new(h);
    let levels = depth_levels(cut.start_depth, cut.end_depth, cut.stepdown);
    for c in contours {
        for (k, d) in levels.iter().enumerate() {
            if c.closed {
                follow_ring(&mut pen, &c.points, -d, cut);
            } else {
                follow_open(&mut pen, &c.points, -d, cut, k % 2 == 1);
            }
        }
    }
    pen.lift();
    pen.steps
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Move;

    const H: Heights = Heights { safe_z: 10.0, clear_z: 1.0 };

    fn square(x0: f64, y0: f64, s: f64) -> Contour {
        Contour { points: vec![Vec2::new(x0, y0), Vec2::new(x0 + s, y0), Vec2::new(x0 + s, y0 + s), Vec2::new(x0, y0 + s)], closed: true }
    }

    fn circle(c: Vec2, r: f64) -> Contour {
        Contour {
            points: (0..72).map(|i| c + Vec2::new((i as f64 * 5f64.to_radians()).cos(), (i as f64 * 5f64.to_radians()).sin()) * r).collect(),
            closed: true,
        }
    }

    fn cut(r: f64, end: f64, step: f64) -> Cut {
        Cut { tool_radius: r, start_depth: 0.0, end_depth: end, stepdown: step, feed: 800.0, plunge: 300.0, climb: false }
    }

    fn feeds(steps: &[Step]) -> Vec<Vec3> {
        steps.iter().filter_map(|s| if let Step::Move(Move::Feed(p, _)) = s { Some(*p) } else { None }).collect()
    }

    #[test]
    fn outside_profile_keeps_tool_radius_from_the_part() {
        let sq = square(0.0, 0.0, 20.0);
        let mut w = Vec::new();
        let steps = profile(H, &[(&sq, Side::Outside)], &cut(1.5, 6.0, 3.0), &mut w);
        let f = feeds(&steps);
        assert!(w.is_empty());
        // aucun point à moins du rayon de l'outil du carré, tous à ~1,5 mm
        for p in f.iter().filter(|p| p.z < 0.0) {
            let dx = (-p.x).max(p.x - 20.0).max(0.0);
            let dy = (-p.y).max(p.y - 20.0).max(0.0);
            let dist = (dx * dx + dy * dy).sqrt();
            assert!((dist - 1.5).abs() < 0.05, "point {p:?} à {dist} mm");
        }
        assert!(f.iter().any(|p| p.z == -3.0) && f.iter().any(|p| p.z == -6.0));
        assert!(matches!(steps.last(), Some(Step::Move(Move::Rapid(p))) if p.z == 10.0));
    }

    #[test]
    fn inside_profile_and_orientation() {
        let sq = square(0.0, 0.0, 20.0);
        let mut w = Vec::new();
        let f = feeds(&profile(H, &[(&sq, Side::Inside)], &cut(1.5, 1.0, 1.0), &mut w));
        assert!(f.iter().all(|p| p.x >= 1.49 && p.x <= 18.51 && p.y >= 1.49 && p.y <= 18.51));
        let ring: Vec<Vec2> = f.iter().filter(|p| p.z < 0.0).map(|p| p.xy()).collect();
        assert!(ring_area(&ring) < 0.0, "intérieur en opposition = horaire");
    }

    #[test]
    fn pocket_clears_a_counterbore() {
        // lamage du projet OSB : cercle R3,25, fraise Ø3,175, surépaisseur 0,5
        let c = circle(Vec2::new(16.0, 18.0), 3.25);
        let regs = regions(&[&c]);
        let mut w = Vec::new();
        let f = feeds(&pocket(H, &regs, &cut(1.5875, 3.5, 2.54), 1.587, 0.5, &mut w));
        assert!(w.is_empty(), "{w:?}");
        let max_r = f.iter().map(|p| (p.xy() - Vec2::new(16.0, 18.0)).length()).fold(0.0, f64::max);
        assert!((max_r - (3.25 - 1.5875 - 0.5)).abs() < 0.05, "anneau à {max_r}");
        assert!(f.iter().any(|p| p.z == -2.54) && f.iter().any(|p| p.z == -3.5));
        // trop petit
        let tiny = circle(Vec2::new(0.0, 0.0), 1.0);
        let mut w = Vec::new();
        assert!(feeds(&pocket(H, &regions(&[&tiny]), &cut(1.5875, 1.0, 1.0), 1.0, 0.0, &mut w)).is_empty());
        assert_eq!(w.len(), 1);
    }

    #[test]
    fn regions_make_holes() {
        let outer = square(0.0, 0.0, 30.0);
        let hole = square(10.0, 10.0, 10.0);
        let r = regions(&[&outer, &hole]);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].0[0].interiors().len(), 1);
    }

    #[test]
    fn nearest_neighbour_order() {
        let pts = vec![Vec2::new(10.0, 0.0), Vec2::new(1.0, 0.0), Vec2::new(5.0, 0.0)];
        assert_eq!(nearest_order(pts, Vec2::ZERO, |p| *p), vec![Vec2::new(1.0, 0.0), Vec2::new(5.0, 0.0), Vec2::new(10.0, 0.0)]);
        assert!(nearest_order(Vec::<Vec2>::new(), Vec2::ZERO, |p| *p).is_empty());
    }

    #[test]
    fn drilling_with_pecks() {
        let f = feeds(&drill(H, &[Vec2::new(5.0, 5.0)], 9.1, 3.0, 300.0));
        let bottoms: Vec<f64> = f.iter().map(|p| p.z).filter(|z| *z < -0.6).collect();
        assert!(bottoms.contains(&-3.0) && bottoms.contains(&-6.0) && bottoms.contains(&-9.1), "{bottoms:?}");
        let one = feeds(&drill(H, &[Vec2::new(5.0, 5.0)], 9.1, 0.0, 300.0));
        assert_eq!(one.len(), 1);
    }
}
