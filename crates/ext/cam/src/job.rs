//! Travail complet : les parcours d'un projet regroupés par outil, prêts à écrire en G-code.

use makercraft_c2d::{Contour, Project, Tool, ToolKind, ToolpathKind};
use makercraft_geom::{Vec2, Vec3};

use crate::ops::{self, Cut, Side};
use crate::{Heights, Operation, Stats, stats};

/// Opérations consécutives faites avec le même outil (un fichier G-code, un zéro Z).
#[derive(Debug, Clone, PartialEq)]
pub struct ToolGroup {
    pub tool: Tool,
    pub ops: Vec<Operation>,
}

impl ToolGroup {
    pub fn stats(&self, h: Heights) -> Stats {
        stats(self.ops.iter().flat_map(Operation::moves), Vec3::new(0.0, 0.0, h.safe_z))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Job {
    pub title: String,
    pub stock_size: (f64, f64, f64),
    pub machine: String,
    pub heights: Heights,
    pub groups: Vec<ToolGroup>,
    /// Formes du dessin (pour l'aperçu).
    pub design: Vec<Contour>,
    pub warnings: Vec<String>,
}

/// Description française d'un outil.
pub fn tool_label(t: &Tool) -> String {
    let kind = match t.kind {
        ToolKind::Flat => "fraise droite".to_owned(),
        ToolKind::Ball => "fraise boule".to_owned(),
        ToolKind::VBit => format!("fraise en V {:.0} deg", t.angle),
        ToolKind::Other(k) => format!("outil type {k}"),
    };
    format!("T{} {} - {} D{:.3} mm", t.number, t.name, kind, t.diameter)
}

/// Construit le travail à partir d'un projet Carbide Create. `only` : ne garder que les parcours
/// dont le nom contient ce texte (sans tenir compte de la casse).
pub fn job_from_c2d(pr: &Project, title: &str, only: Option<&str>) -> Job {
    let h = Heights { safe_z: pr.stock.retract.clamp(2.0, 50.0), clear_z: 1.0 };
    let mut warnings = Vec::new();
    if pr.stock.zero_z.abs() > 1e-9 {
        warnings.push(format!("zéro Z du projet = {} : le G-code suppose le zéro sur le DESSUS du brut", pr.stock.zero_z));
    }
    let mut groups: Vec<ToolGroup> = Vec::new();
    for tp in &pr.toolpaths {
        if let Some(f) = only
            && !tp.name.to_lowercase().contains(&f.to_lowercase())
        {
            continue;
        }
        if !tp.enabled {
            warnings.push(format!("« {} » désactivé dans Carbide Create : ignoré", tp.name));
            continue;
        }
        let elements = pr.elements_for(tp);
        if elements.is_empty() {
            warnings.push(format!("« {} » : aucune forme à usiner", tp.name));
            continue;
        }
        let r = tp.tool.diameter / 2.0;
        if r <= 1e-6 {
            warnings.push(format!("« {} » : diamètre d'outil nul, ignoré", tp.name));
            continue;
        }
        let cut = Cut {
            tool_radius: r,
            start_depth: tp.start_depth,
            end_depth: tp.end_depth,
            stepdown: tp.stepdown,
            feed: tp.speeds.feed,
            plunge: tp.speeds.plunge,
            climb: tp.climb,
        };
        let mut op = Operation { name: tp.name.clone(), kind: tp.kind.label(), rpm: tp.speeds.rpm, ..Operation::default() };
        let contours: Vec<&Contour> = elements.iter().flat_map(|e| e.contours.iter()).collect();
        op.steps = match &tp.kind {
            ToolpathKind::Cutout => {
                if elements.iter().any(|e| e.has_tabs) && !tp.ignore_tabs {
                    op.warnings.push("attaches dessinées non reproduites : bien brider les pièces".into());
                }
                let closed: Vec<&Contour> = contours.iter().copied().filter(|c| c.closed).collect();
                let items: Vec<(&Contour, Side)> = contours
                    .iter()
                    .map(|c| {
                        if !c.closed {
                            return (*c, Side::OnLine);
                        }
                        let p = c.points.first().copied().unwrap_or(Vec2::ZERO);
                        let depth = closed.iter().filter(|o| o.signed_area().abs() > c.signed_area().abs() && o.contains(p)).count();
                        let outside = depth.is_multiple_of(2) != tp.flip_inside_outside;
                        (*c, if outside { Side::Outside } else { Side::Inside })
                    })
                    .collect();
                // les trous d'une pièce avant son contour extérieur, puis au plus proche voisin
                let (inner, outer): (Vec<_>, Vec<_>) = items.into_iter().partition(|(_, s)| *s == Side::Inside);
                let first = |it: &(&Contour, Side)| it.0.points.first().copied().unwrap_or(Vec2::ZERO);
                let mut items = ops::nearest_order(inner, Vec2::ZERO, first);
                let from = items.last().map(first).unwrap_or(Vec2::ZERO);
                items.extend(ops::nearest_order(outer, from, first));
                if tp.end_depth >= pr.stock.thickness - 1e-6 && !elements.iter().any(|e| e.has_tabs) {
                    op.warnings.push("aucune attache : les pièces seront libres à la dernière passe (bien les maintenir)".into());
                }
                ops::profile(h, &items, &cut, &mut op.warnings)
            }
            ToolpathKind::Pocket => {
                let centre = |r: &geo::MultiPolygon<f64>| {
                    let pts: Vec<Vec2> = r.0.iter().flat_map(|p| p.exterior().0.iter().map(|c| Vec2::new(c.x, c.y))).collect();
                    let b: makercraft_geom::Bounds2 = pts.into_iter().collect();
                    (b.min + b.max) * 0.5
                };
                let regs = ops::nearest_order(ops::regions(&contours), Vec2::ZERO, centre);
                ops::pocket(h, &regs, &cut, tp.stepover, tp.stock_to_leave, &mut op.warnings)
            }
            ToolpathKind::Drill => {
                let mut pts = Vec::new();
                let mut wide = 0usize;
                for e in &elements {
                    let center = e.center.or_else(|| {
                        e.contours.first().map(|c| {
                            let b = c.bounds();
                            (b.min + b.max) * 0.5
                        })
                    });
                    if let Some(c) = center {
                        pts.push(c);
                    }
                    if e.radius.is_some_and(|rad| rad * 2.0 > tp.tool.diameter * 1.05) {
                        wide += 1;
                    }
                }
                if wide > 0 {
                    op.warnings.push(format!("{wide} trou(s) plus large(s) que l'outil : percé(s) au centre seulement"));
                }
                let pts = ops::nearest_order(pts, Vec2::ZERO, |p| *p);
                ops::drill(h, &pts, tp.end_depth, tp.peck, tp.speeds.plunge)
            }
            ToolpathKind::VCarve => {
                op.kind = "Gravure (V-carve approché)".into();
                op.warnings.push(format!("V-carve approché : gravure SUR le trait à {:.2} mm (pas de V à profondeur variable)", tp.end_depth));
                let contours = ops::nearest_order(contours, Vec2::ZERO, |c| c.points.first().copied().unwrap_or(Vec2::ZERO));
                ops::engrave(h, &contours, &cut)
            }
            ToolpathKind::Other(t) => {
                warnings.push(format!("« {} » : type de parcours « {t} » pas encore géré, ignoré", tp.name));
                continue;
            }
        };
        if op.steps.is_empty() {
            warnings.push(format!("« {} » : aucune trajectoire produite", tp.name));
            continue;
        }
        let same_tool = groups.last().is_some_and(|g| g.tool.number == tp.tool.number && (g.tool.diameter - tp.tool.diameter).abs() < 1e-6);
        match groups.last_mut() {
            Some(g) if same_tool => g.ops.push(op),
            _ => groups.push(ToolGroup { tool: tp.tool.clone(), ops: vec![op] }),
        }
    }
    // formes utilisées par au moins un parcours (les calques de construction sont ignorés)
    let used: std::collections::HashSet<&str> = pr.toolpaths.iter().flat_map(|tp| pr.elements_for(tp)).map(|e| e.id.as_str()).collect();
    let design = pr.elements.iter().filter(|e| used.contains(e.id.as_str())).flat_map(|e| e.contours.iter().cloned()).collect();
    Job {
        title: title.to_owned(),
        stock_size: (pr.stock.width, pr.stock.height, pr.stock.thickness),
        machine: pr.machine.clone(),
        heights: h,
        groups,
        design,
        warnings,
    }
}
