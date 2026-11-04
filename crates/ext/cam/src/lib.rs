//! FAO 2.5D de MakerCraft : transforme des formes et des paramètres de parcours en
//! trajectoires d'outil, puis en G-code pour Grbl.
//!
//! - [`ops`] : découpe de contour (extérieur/intérieur), poche, perçage, gravure sur le trait ;
//! - [`job`] : construction d'un travail complet depuis un projet Carbide Create (`.c2d`) ;
//! - [`gcode`] : écriture du G-code Grbl (ASCII strict, un fichier par outil ou un seul fichier) ;
//! - [`preview`] : aperçu SVG des trajectoires.
//!
//! Conventions : mm, Z0 = dessus du brut, profondeurs positives vers le bas (Z négatif),
//! X0 Y0 = coin avant gauche du brut. Fraisage en opposition par défaut (broche horaire).

pub mod ext;
pub mod gcode;
pub mod job;
pub mod ops;
pub mod preview;

use makercraft_geom::{Vec2, Vec3};

pub use ext::CamExtension;
pub use job::{Job, ToolGroup, job_from_c2d};

/// Un déplacement.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Move {
    /// Déplacement rapide (G0).
    Rapid(Vec3),
    /// Déplacement en usinage (G1) à l'avance donnée (mm/min).
    Feed(Vec3, f64),
}

impl Move {
    pub fn target(&self) -> Vec3 {
        match self {
            Self::Rapid(p) | Self::Feed(p, _) => *p,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    Comment(String),
    Move(Move),
}

/// Une opération (un parcours) : sa trajectoire et ses remarques.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Operation {
    pub name: String,
    /// Découpe, Poche, Perçage, Gravure…
    pub kind: String,
    pub rpm: f64,
    pub steps: Vec<Step>,
    pub warnings: Vec<String>,
}

impl Operation {
    pub fn moves(&self) -> impl Iterator<Item = &Move> {
        self.steps.iter().filter_map(|s| match s {
            Step::Move(m) => Some(m),
            Step::Comment(_) => None,
        })
    }
}

/// Hauteurs de sécurité.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Heights {
    /// Déplacements rapides entre les zones (au-dessus des brides).
    pub safe_z: f64,
    /// Approche rapide juste au-dessus du brut avant de plonger.
    pub clear_z: f64,
}

/// Construit une trajectoire en évitant les mouvements inutiles.
pub(crate) struct Pen {
    pub steps: Vec<Step>,
    pos: Option<Vec3>,
    h: Heights,
}

const EPS: f64 = 1e-6;

impl Pen {
    pub fn new(h: Heights) -> Self {
        Self { steps: Vec::new(), pos: None, h }
    }

    fn rapid(&mut self, p: Vec3) {
        if self.pos != Some(p) {
            self.steps.push(Step::Move(Move::Rapid(p)));
            self.pos = Some(p);
        }
    }

    pub fn feed(&mut self, p: Vec3, f: f64) {
        if self.pos.is_some_and(|q| (q - p).length() < EPS) {
            return;
        }
        self.steps.push(Step::Move(Move::Feed(p, f)));
        self.pos = Some(p);
    }

    /// Remonte à la hauteur de sécurité.
    pub fn lift(&mut self) {
        if let Some(p) = self.pos
            && p.z < self.h.safe_z - EPS
        {
            self.rapid(Vec3::new(p.x, p.y, self.h.safe_z));
        }
    }

    /// Amène l'outil au-dessus de `xy`, juste au-dessus du brut (ou reste en place s'il y est déjà,
    /// plus bas, par exemple entre deux passes du même contour).
    pub fn approach(&mut self, xy: Vec2) {
        if let Some(p) = self.pos
            && (p.xy() - xy).length() < EPS
            && p.z <= self.h.clear_z + EPS
        {
            return;
        }
        self.lift();
        self.rapid(Vec3::new(xy.x, xy.y, self.h.safe_z));
        self.rapid(Vec3::new(xy.x, xy.y, self.h.clear_z));
    }

    /// Plonge (ou remonte en rapide) à `z` sur place.
    pub fn plunge(&mut self, z: f64, f: f64) {
        let Some(p) = self.pos else { return };
        if z < p.z {
            self.feed(Vec3::new(p.x, p.y, z), f);
        } else {
            self.rapid(Vec3::new(p.x, p.y, z));
        }
    }

    /// Descend en rapide à `z` sur place (uniquement dans une zone déjà usinée).
    pub fn rapid_down_to(&mut self, z: f64) {
        if let Some(p) = self.pos
            && z < p.z
        {
            self.rapid(Vec3::new(p.x, p.y, z));
        }
    }

    /// Remonte rapidement à `z` sur place.
    pub fn up_to(&mut self, z: f64) {
        if let Some(p) = self.pos
            && z > p.z
        {
            self.rapid(Vec3::new(p.x, p.y, z));
        }
    }
}

/// Profondeurs successives (positives) de `start` à `end` par pas de `step`.
pub fn depth_levels(start: f64, end: f64, step: f64) -> Vec<f64> {
    if end <= start + EPS {
        return Vec::new();
    }
    if step <= EPS {
        return vec![end];
    }
    let mut v = Vec::new();
    let mut d = start + step;
    while d < end - 1e-3 && v.len() < 10_000 {
        v.push(d);
        d += step;
    }
    v.push(end);
    v
}

/// Statistiques d'une trajectoire.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Stats {
    pub cut_length: f64,
    pub rapid_length: f64,
    /// Durée estimée (s), rapides à `rapid_rate` mm/min.
    pub seconds: f64,
}

/// Vitesse des rapides supposée pour l'estimation (mm/min), prudente pour une Shapeoko.
pub const RAPID_RATE: f64 = 3000.0;

pub fn stats<'a>(moves: impl IntoIterator<Item = &'a Move>, start: Vec3) -> Stats {
    let mut s = Stats::default();
    let mut pos = start;
    for m in moves {
        let d = (m.target() - pos).length();
        match m {
            Move::Rapid(_) => {
                s.rapid_length += d;
                s.seconds += d / RAPID_RATE * 60.0;
            }
            Move::Feed(_, f) => {
                s.cut_length += d;
                if *f > EPS {
                    s.seconds += d / f * 60.0;
                }
            }
        }
        pos = m.target();
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels() {
        assert_eq!(depth_levels(0.0, 9.5, 2.54), vec![2.54, 5.08, 7.62, 9.5]);
        assert_eq!(depth_levels(0.0, 0.5, 0.5), vec![0.5]);
        assert_eq!(depth_levels(0.0, 3.0, 0.0), vec![3.0]);
        assert!(depth_levels(1.0, 1.0, 0.5).is_empty());
    }

    #[test]
    fn pen_avoids_useless_moves() {
        let mut p = Pen::new(Heights { safe_z: 10.0, clear_z: 1.0 });
        p.approach(Vec2::new(5.0, 5.0));
        p.plunge(-2.0, 300.0);
        p.feed(Vec3::new(5.0, 5.0, -2.0), 800.0);
        p.approach(Vec2::new(5.0, 5.0));
        p.plunge(-4.0, 300.0);
        // rapide au-dessus, descente rapide à Z1, plongée à -2, plongée à -4 (rien d'inutile)
        assert_eq!(p.steps.len(), 4, "{:?}", p.steps);
        let s = stats(p.steps.iter().filter_map(|s| if let Step::Move(m) = s { Some(m) } else { None }), Vec3::new(5.0, 5.0, 10.0));
        assert!((s.cut_length - 5.0).abs() < 1e-9, "{s:?}");
    }
}
