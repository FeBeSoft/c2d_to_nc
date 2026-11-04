//! Petits éléments d'interface dessinés à la main : boutons, interrupteur, pastilles, cadres.
//! Chaque élément cliquable déclare son nom à l'accessibilité (UI Automation sous Windows).

use eframe::egui::{
    self, Align2, Color32, CornerRadius, CursorIcon, FontFamily, FontId, Painter, Pos2, Rect, Response, Sense, Shape, Stroke, StrokeKind, Ui,
    WidgetInfo, WidgetType, epaint::Shadow, pos2, vec2,
};

use crate::icons::rounded_path;
use crate::theme::Palette;

pub fn regular(size: f32) -> FontId {
    FontId::proportional(size)
}

pub fn bold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("bold".into()))
}

pub fn mono(size: f32) -> FontId {
    FontId::monospace(size)
}

/// Mélange de deux couleurs (t = 0 → a, t = 1 → b).
pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let f = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t).round() as u8;
    Color32::from_rgba_premultiplied(f(a.r(), b.r()), f(a.g(), b.g()), f(a.b(), b.b()), f(a.a(), b.a()))
}

/// Même teinte, opacité multipliée.
pub fn alpha(c: Color32, a: f32) -> Color32 {
    c.gamma_multiply(a.clamp(0.0, 1.0))
}

pub fn text_width(p: &Painter, s: &str, font: &FontId) -> f32 {
    p.layout_no_wrap(s.to_owned(), font.clone(), Color32::WHITE).size().x
}

/// Raccourcit un texte par le début (« … fin du nom.nc ») pour garder la partie distinctive.
pub fn elide_start(s: &str, max: f32, measure: impl Fn(&str) -> f32) -> String {
    if measure(s) <= max {
        return s.to_owned();
    }
    let chars: Vec<char> = s.chars().collect();
    // recherche dichotomique du plus long suffixe qui tient
    let (mut lo, mut hi) = (1usize, chars.len());
    let cut = |i: usize| -> String { std::iter::once('…').chain(chars[i..].iter().copied()).collect() };
    while lo < hi {
        let mid = (lo + hi) / 2;
        if measure(&cut(mid)) <= max { hi = mid } else { lo = mid + 1 }
    }
    cut(lo.min(chars.len()))
}

/// Halo diffus autour d'un rectangle.
pub fn glow(p: &Painter, rect: Rect, radius: u8, color: Color32, blur: u8) {
    if color.a() == 0 {
        return;
    }
    p.add(Shadow { offset: [0, 0], blur, spread: 0, color }.as_shape(rect, CornerRadius::same(radius)));
}

/// Ombre portée (boutons, fenêtres).
pub fn drop_shadow(p: &Painter, rect: Rect, radius: u8, color: Color32) {
    p.add(Shadow { offset: [0, 8], blur: 22, spread: 0, color }.as_shape(rect, CornerRadius::same(radius)));
}

/// Bordure en pointillés d'un rectangle arrondi.
pub fn dashed_rect(p: &Painter, rect: Rect, radius: f32, stroke: Stroke, dash: f32, gap: f32) {
    let mut pts = rounded_path(rect, radius);
    if let Some(f) = pts.first().copied() {
        pts.push(f);
    }
    p.extend(Shape::dashed_line(&pts, stroke, dash, gap));
}

/// Zone cliquable avec nom accessible ; renvoie la réponse (survol, clic).
pub fn hit(ui: &Ui, rect: Rect, label: &str, enabled: bool) -> Response {
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let resp = ui.interact(rect, ui.id().with(("bouton", label)), sense);
    let l = label.to_owned();
    resp.widget_info(|| WidgetInfo::labeled(WidgetType::Button, enabled, &l));
    if enabled { resp.on_hover_cursor(CursorIcon::PointingHand) } else { resp }
}

/// Bouton plein (couleur d'accent), icône à gauche du texte.
pub fn primary_button(ui: &mut Ui, size: egui::Vec2, label: &str, enabled: bool, pal: &Palette, icon: impl Fn(&Painter, Rect, Color32)) -> Response {
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let resp = hit(ui, rect, label, enabled);
    let p = ui.painter();
    let (fill, fg) = if enabled {
        let f = if resp.is_pointer_button_down_on() {
            mix(pal.accent, Color32::BLACK, 0.12)
        } else if resp.hovered() {
            mix(pal.accent, Color32::WHITE, 0.10)
        } else {
            pal.accent
        };
        (f, pal.on_accent)
    } else {
        (pal.pill, pal.weak)
    };
    if enabled {
        drop_shadow(p, rect, 12, alpha(pal.accent, 0.35));
    }
    p.rect_filled(rect, CornerRadius::same(12), fill);
    let font = bold(18.0);
    let tw = text_width(p, label, &font);
    let total = 22.0 + 10.0 + tw;
    let x0 = rect.center().x - total / 2.0;
    icon(p, Rect::from_min_size(pos2(x0, rect.center().y - 11.0), vec2(22.0, 22.0)), fg);
    p.text(pos2(x0 + 32.0, rect.center().y), Align2::LEFT_CENTER, label, font, fg);
    resp
}

/// Bouton secondaire (contour), texte ou icône seule.
pub fn secondary_button(ui: &Ui, rect: Rect, label: &str, text: Option<&str>, enabled: bool, pal: &Palette) -> Response {
    let resp = hit(ui, rect, label, enabled);
    let p = ui.painter();
    let bg = if resp.hovered() && enabled { mix(pal.secondary_bg, pal.secondary_line, 0.12) } else { pal.secondary_bg };
    let line = if enabled { pal.secondary_line } else { pal.pill_line };
    p.rect_filled(rect, CornerRadius::same(10), bg);
    p.rect_stroke(rect, CornerRadius::same(10), Stroke::new(1.5, line), StrokeKind::Inside);
    if let Some(t) = text {
        let fg = if enabled { pal.secondary_fg } else { pal.weak };
        p.text(rect.center(), Align2::CENTER_CENTER, t, bold(14.0), fg);
    }
    resp
}

/// Interrupteur à bascule avec son libellé (vu comme une case à cocher par l'accessibilité).
/// Ne modifie rien : l'appelant inverse la valeur si `clicked()` (une seule fois par image affichée).
pub fn toggle(ui: &mut Ui, on: bool, label: &str, pal: &Palette) -> Response {
    let font = bold(14.0);
    let tw = text_width(ui.painter(), label, &font);
    let (rect, resp) = ui.allocate_exact_size(vec2(44.0 + 10.0 + tw, 44.0), Sense::click());
    let state = on;
    let l = label.to_owned();
    let enabled = ui.is_enabled();
    resp.widget_info(|| WidgetInfo::selected(WidgetType::Checkbox, enabled, state, &l));
    let resp = resp.on_hover_cursor(CursorIcon::PointingHand);
    let t = ui.ctx().animate_bool_responsive(resp.id, state);
    let p = ui.painter();
    let track = Rect::from_min_size(pos2(rect.left(), rect.center().y - 12.0), vec2(44.0, 24.0));
    p.rect_filled(track, CornerRadius::same(12), mix(pal.toggle_off, pal.accent, t));
    let knob = pos2(egui::lerp(track.left() + 12.0..=track.right() - 12.0, t), track.center().y);
    p.circle_filled(knob, 9.0, mix(pal.knob_off, Color32::WHITE, t));
    p.text(pos2(track.right() + 10.0, rect.center().y), Align2::LEFT_CENTER, label, font, pal.text);
    resp
}

/// Style d'une pastille d'information.
#[derive(Clone, Copy)]
pub struct PillStyle {
    pub bg: Color32,
    pub line: Color32,
    pub fg: Color32,
    pub icon: Color32,
    pub bold: bool,
}

/// Pastille « icône + texte » dans le flux (renvoie la réponse pour une info-bulle).
pub fn pill(ui: &mut Ui, text: &str, style: PillStyle, icon: impl Fn(&Painter, Rect, Color32)) -> Response {
    let font = if style.bold { bold(13.0) } else { regular(13.0) };
    let tw = text_width(ui.painter(), text, &font);
    let (rect, resp) = ui.allocate_exact_size(vec2(10.0 + 14.0 + 6.0 + tw + 10.0, 28.0), Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::same(8), style.bg);
    p.rect_stroke(rect, CornerRadius::same(8), Stroke::new(1.0, style.line), StrokeKind::Inside);
    icon(p, Rect::from_min_size(pos2(rect.left() + 10.0, rect.center().y - 7.0), vec2(14.0, 14.0)), style.icon);
    p.text(pos2(rect.left() + 30.0, rect.center().y), Align2::LEFT_CENTER, text, font, style.fg);
    resp
}

/// Petit rond de couleur (barre d'état, lignes d'info-bulle).
pub fn status_dot(p: &Painter, center: Pos2, color: Color32) {
    p.circle_filled(center, 4.0, color);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn elides_from_the_start() {
        // 1 unité par caractère
        let m = |s: &str| s.chars().count() as f32;
        assert_eq!(elide_start("court.nc", 20.0, m), "court.nc");
        let e = elide_start("Mise en planche - OSB 9mm - 1 - T302 - VCarve 1.nc", 20.0, m);
        assert_eq!(e.chars().count(), 20);
        assert!(e.starts_with('…') && e.ends_with("VCarve 1.nc"), "{e}");
        assert_eq!(elide_start("abc", 0.5, m), "…");
    }

    #[test]
    fn mixes_colors() {
        assert_eq!(mix(Color32::BLACK, Color32::WHITE, 0.0), Color32::BLACK);
        assert_eq!(mix(Color32::BLACK, Color32::WHITE, 1.0), Color32::WHITE);
    }
}
