//! Icônes vectorielles dessinées avec le Painter d'egui (aucun glyphe spécial, aucun emoji).
//! Les tracés reprennent ceux de la preview : coordonnées dans une boîte carrée de `box` unités.

use eframe::egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, pos2};

use crate::summary::{State, TileKind};

/// Crayon : convertit les coordonnées de la boîte de dessin vers l'écran.
pub struct Pen<'a> {
    p: &'a Painter,
    origin: Pos2,
    scale: f32,
    color: Color32,
}

impl<'a> Pen<'a> {
    /// `size` : taille de la boîte d'origine (48 pour les tuiles, 16 pour les pastilles…).
    pub fn new(p: &'a Painter, rect: Rect, size: f32, color: Color32) -> Self {
        let scale = rect.width().min(rect.height()) / size;
        let origin = rect.center() - eframe::egui::vec2(size, size) * scale / 2.0;
        Self { p, origin, scale, color }
    }

    fn pt(&self, (x, y): (f32, f32)) -> Pos2 {
        self.origin + eframe::egui::vec2(x, y) * self.scale
    }

    fn stroke(&self, w: f32) -> Stroke {
        Stroke::new(w * self.scale, self.color)
    }

    /// Ligne brisée ouverte, extrémités arrondies.
    pub fn line(&self, pts: &[(f32, f32)], w: f32) {
        let v: Vec<Pos2> = pts.iter().map(|&q| self.pt(q)).collect();
        if let (Some(a), Some(b)) = (v.first(), v.last()) {
            let r = w * self.scale / 2.0;
            self.p.circle_filled(*a, r, self.color);
            self.p.circle_filled(*b, r, self.color);
        }
        self.p.add(Shape::line(v, self.stroke(w)));
    }

    /// Polygone fermé.
    pub fn closed(&self, pts: &[(f32, f32)], w: f32) {
        let v: Vec<Pos2> = pts.iter().map(|&q| self.pt(q)).collect();
        self.p.add(Shape::closed_line(v, self.stroke(w)));
    }

    pub fn circle(&self, c: (f32, f32), r: f32, w: f32) {
        self.p.circle_stroke(self.pt(c), r * self.scale, self.stroke(w));
    }

    pub fn dot(&self, c: (f32, f32), r: f32) {
        self.p.circle_filled(self.pt(c), r * self.scale, self.color);
    }

    pub fn disc(&self, c: (f32, f32), r: f32, color: Color32) {
        self.p.circle_filled(self.pt(c), r * self.scale, color);
    }

    /// Arc de cercle (angles en degrés, 0 = droite, sens horaire à l'écran).
    pub fn arc(&self, c: (f32, f32), r: f32, from: f32, to: f32, w: f32) {
        let n = 16;
        let pts: Vec<(f32, f32)> = (0..=n)
            .map(|i| {
                let a = (from + (to - from) * i as f32 / n as f32).to_radians();
                (c.0 + r * a.cos(), c.1 + r * a.sin())
            })
            .collect();
        self.line(&pts, w);
    }

    /// Rectangle à coins arrondis, plein trait ou pointillé.
    pub fn rrect(&self, (x, y, w, h): (f32, f32, f32, f32), r: f32, sw: f32, dashed: bool) {
        let pts: Vec<Pos2> =
            rounded_path(Rect::from_min_size(pos2(x, y), eframe::egui::vec2(w, h)), r).into_iter().map(|q| self.pt((q.x, q.y))).collect();
        if dashed {
            let mut closed = pts.clone();
            if let Some(f) = pts.first() {
                closed.push(*f);
            }
            self.p.extend(Shape::dashed_line(&closed, self.stroke(sw), 3.0 * self.scale, 3.0 * self.scale));
        } else {
            self.p.add(Shape::closed_line(pts, self.stroke(sw)));
        }
    }
}

/// Contour d'un rectangle à coins arrondis (points dans le sens horaire).
pub fn rounded_path(r: Rect, radius: f32) -> Vec<Pos2> {
    let rad = radius.min(r.width() / 2.0).min(r.height() / 2.0).max(0.0);
    let corners = [
        (pos2(r.right() - rad, r.top() + rad), -90.0_f32),
        (pos2(r.right() - rad, r.bottom() - rad), 0.0),
        (pos2(r.left() + rad, r.bottom() - rad), 90.0),
        (pos2(r.left() + rad, r.top() + rad), 180.0),
    ];
    let mut v = Vec::with_capacity(40);
    for (c, start) in corners {
        for i in 0..=8 {
            let a = (start + 90.0 * i as f32 / 8.0).to_radians();
            v.push(pos2(c.x + rad * a.cos(), c.y + rad * a.sin()));
        }
    }
    v
}

/// Grosse icône d'une tuile (boîte 48).
pub fn tile_icon(p: &Painter, rect: Rect, kind: TileKind, state: State, c: Color32) {
    let pen = Pen::new(p, rect, 48.0, c);
    match kind {
        TileKind::Stock => {
            pen.closed(&[(4.0, 28.0), (30.0, 16.0), (44.0, 22.0), (18.0, 34.0)], 2.6);
            pen.line(&[(4.0, 28.0), (4.0, 34.0), (18.0, 40.0), (44.0, 28.0), (44.0, 22.0)], 2.6);
            pen.line(&[(18.0, 34.0), (18.0, 40.0)], 2.6);
            pen.line(&[(12.0, 24.0), (32.0, 15.0)], 1.6);
            pen.line(&[(16.0, 27.0), (34.0, 19.0)], 1.6);
        }
        TileKind::Design => {
            pen.closed(&[(10.0, 38.0), (32.0, 16.0), (38.0, 22.0), (16.0, 44.0), (10.0, 44.0)], 2.6);
            pen.line(&[(30.0, 12.0), (34.0, 8.0), (40.0, 14.0), (36.0, 18.0)], 2.6);
            pen.line(&[(27.0, 21.0), (33.0, 27.0)], 2.6);
        }
        TileKind::VCarve => {
            pen.line(&[(8.0, 10.0), (24.0, 38.0), (40.0, 10.0)], 3.0);
            pen.line(&[(4.0, 42.0), (44.0, 42.0)], 1.6);
        }
        TileKind::Drill => {
            pen.closed(&[(18.0, 4.0), (30.0, 4.0), (30.0, 12.0), (18.0, 12.0)], 2.6);
            pen.closed(&[(20.0, 12.0), (28.0, 12.0), (28.0, 32.0), (24.0, 40.0), (20.0, 32.0)], 2.6);
            pen.line(&[(20.0, 18.0), (28.0, 22.0)], 2.0);
            pen.line(&[(20.0, 25.0), (28.0, 29.0)], 2.0);
        }
        TileKind::Pocket => {
            pen.rrect((8.0, 8.0, 32.0, 32.0), 9.0, 2.8, false);
            pen.rrect((16.0, 16.0, 16.0, 16.0), 4.0, 1.8, true);
        }
        TileKind::Cutout => {
            pen.circle((13.0, 35.0), 5.5, 2.6);
            pen.circle((35.0, 35.0), 5.5, 2.6);
            pen.line(&[(17.0, 31.0), (36.0, 8.0)], 2.6);
            pen.line(&[(31.0, 31.0), (12.0, 8.0)], 2.6);
        }
        TileKind::Tabs => {
            pen.line(
                &[
                    (4.0, 32.0),
                    (13.0, 32.0),
                    (13.0, 24.0),
                    (21.0, 24.0),
                    (21.0, 32.0),
                    (27.0, 32.0),
                    (27.0, 24.0),
                    (35.0, 24.0),
                    (35.0, 32.0),
                    (44.0, 32.0),
                ],
                2.8,
            );
            if state == State::Missing {
                pen.line(&[(10.0, 12.0), (38.0, 36.0)], 2.0);
            }
        }
        TileKind::Tools => {
            pen.rrect((18.0, 4.0, 12.0, 18.0), 2.0, 2.6, false);
            pen.line(&[(20.0, 22.0), (20.0, 38.0), (24.0, 44.0), (28.0, 38.0), (28.0, 22.0)], 2.6);
            pen.line(&[(20.0, 28.0), (28.0, 25.0)], 2.0);
            pen.line(&[(20.0, 34.0), (28.0, 31.0)], 2.0);
        }
    }
}

/// Signe dans la pastille d'état (boîte 12).
pub fn badge_mark(p: &Painter, rect: Rect, state: State, c: Color32) {
    let pen = Pen::new(p, rect, 12.0, c);
    match state {
        State::Ok => pen.line(&[(2.5, 6.2), (5.0, 8.5), (9.5, 3.5)], 2.0),
        State::Warn => {
            pen.line(&[(6.0, 2.5), (6.0, 6.7)], 2.0);
            pen.dot((6.0, 9.4), 1.15);
        }
        State::Missing => {
            pen.line(&[(3.0, 3.0), (9.0, 9.0)], 2.0);
            pen.line(&[(9.0, 3.0), (3.0, 9.0)], 2.0);
        }
        State::Unused => pen.line(&[(3.5, 6.0), (8.5, 6.0)], 2.0),
    }
}

/// Petites icônes (boîte 16).
#[derive(Clone, Copy)]
pub enum Small {
    Ruler,
    Machine,
    Clock,
    Lock,
}

pub fn small(p: &Painter, rect: Rect, which: Small, c: Color32) {
    let pen = Pen::new(p, rect, 16.0, c);
    match which {
        Small::Ruler => {
            pen.rrect((1.5, 5.0, 13.0, 6.0), 1.0, 1.4, false);
            pen.line(&[(4.5, 5.0), (4.5, 7.5)], 1.2);
            pen.line(&[(7.5, 5.0), (7.5, 8.5)], 1.2);
            pen.line(&[(10.5, 5.0), (10.5, 7.5)], 1.2);
        }
        Small::Machine => {
            pen.line(&[(2.0, 2.0), (14.0, 2.0)], 1.4);
            pen.line(&[(8.0, 2.0), (8.0, 6.0)], 1.4);
            pen.closed(&[(5.0, 6.0), (11.0, 6.0), (11.0, 10.0), (5.0, 10.0)], 1.4);
            pen.line(&[(8.0, 10.0), (8.0, 14.0)], 1.4);
        }
        Small::Clock => {
            pen.circle((8.0, 8.0), 6.0, 1.4);
            pen.line(&[(8.0, 4.5), (8.0, 8.0), (10.5, 9.5)], 1.4);
        }
        Small::Lock => {
            pen.rrect((3.0, 7.0, 10.0, 7.0), 1.5, 1.4, false);
            pen.arc((8.0, 5.0), 2.5, 180.0, 360.0, 1.4);
            pen.line(&[(5.5, 5.0), (5.5, 7.0)], 1.4);
            pen.line(&[(10.5, 5.0), (10.5, 7.0)], 1.4);
        }
    }
}

/// Icônes moyennes (boîte 24 sauf mention).
#[derive(Clone, Copy)]
pub enum Glyph {
    File,
    Export,
    Folder,
    /// zone de dépôt (boîte 48)
    Drop,
    Sun,
    Moon,
}

pub fn glyph(p: &Painter, rect: Rect, which: Glyph, c: Color32, bg: Color32) {
    if let Glyph::Drop = which {
        let pen = Pen::new(p, rect, 48.0, c);
        pen.line(&[(8.0, 30.0), (8.0, 40.0), (40.0, 40.0), (40.0, 30.0)], 3.0);
        pen.line(&[(24.0, 32.0), (24.0, 8.0)], 3.0);
        pen.line(&[(16.0, 16.0), (24.0, 8.0), (32.0, 16.0)], 3.0);
        return;
    }
    let pen = Pen::new(p, rect, 24.0, c);
    match which {
        Glyph::File => {
            pen.closed(&[(6.0, 2.0), (14.0, 2.0), (19.0, 7.0), (19.0, 22.0), (6.0, 22.0)], 2.0);
            pen.line(&[(14.0, 2.0), (14.0, 7.0), (19.0, 7.0)], 2.0);
            pen.line(&[(9.0, 12.0), (16.0, 12.0)], 1.6);
            pen.line(&[(9.0, 16.0), (16.0, 16.0)], 1.6);
        }
        Glyph::Export => {
            pen.line(&[(12.0, 3.0), (12.0, 15.0)], 2.4);
            pen.line(&[(7.0, 10.0), (12.0, 15.0), (17.0, 10.0)], 2.4);
            pen.line(&[(4.0, 20.0), (20.0, 20.0)], 2.4);
        }
        Glyph::Folder => {
            pen.closed(&[(3.0, 5.0), (9.0, 5.0), (11.0, 7.0), (21.0, 7.0), (21.0, 19.5), (3.0, 19.5)], 1.8);
        }
        Glyph::Sun => {
            pen.circle((12.0, 12.0), 4.0, 1.8);
            for i in 0..8 {
                let a = (i as f32 * 45.0).to_radians();
                let (s, co) = a.sin_cos();
                pen.line(&[(12.0 + 7.0 * co, 12.0 + 7.0 * s), (12.0 + 9.5 * co, 12.0 + 9.5 * s)], 1.8);
            }
        }
        Glyph::Moon => {
            pen.dot((12.0, 12.0), 8.0);
            pen.disc((16.0, 8.5), 7.0, bg);
        }
        Glyph::Drop => {}
    }
}
