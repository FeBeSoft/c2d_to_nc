//! c2d_to_nc : glisser un projet Carbide Create (.c2d) dans la fenêtre, vérifier son contenu,
//! exporter le G-code Grbl (.nc) dans le même dossier que le projet.
//!
//! Le fichier peut aussi être déposé sur l'icône de l'exécutable (passé en argument).

// pas de console noire derrière la fenêtre (sauf en debug, pour voir les messages)
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod icons;
mod summary;
mod theme;
mod widgets;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use eframe::egui::{
    self, Align2, Color32, ColorImage, CornerRadius, FontData, FontDefinitions, FontFamily, Label, Margin, Rect, RichText, ScrollArea, Sense, Stroke,
    StrokeKind, TextureHandle, TextureOptions, Ui, UiBuilder, pos2, vec2,
};
use makercraft_c2d::Project;
use makercraft_cam::Job;
use makercraft_cam::ext::{Check, Level, check, load, output_names, write_outputs};
use makercraft_cam::job::tool_label;

use icons::{Glyph, Small};
use summary::{State, Summary, Tile, fmt_minutes, fmt_mm, plural, summarize};
use theme::Palette;
use widgets::{PillStyle, alpha, bold, mix, mono, regular};

const ICON_PNG: &[u8] = include_bytes!("../../../assets/c2d_to_nc-256.png");

struct Loaded {
    path: PathBuf,
    project: Project,
    job: Job,
    checks: Vec<Check>,
    summary: Summary,
}

impl Loaded {
    fn blocking(&self) -> bool {
        self.checks.iter().any(|c| c.level == Level::Error) || self.job.groups.is_empty()
    }

    fn dir(&self) -> PathBuf {
        self.path.parent().map(Path::to_path_buf).unwrap_or_default()
    }

    fn total_seconds(&self) -> f64 {
        self.job.groups.iter().map(|g| g.stats(self.job.heights).seconds).sum()
    }
}

/// Action demandée par l'interface, exécutée après le dessin de l'image.
enum Action {
    /// fichier déposé dans la fenêtre
    Open(PathBuf),
    Pick,
    Export,
    OpenFolder,
    ToggleTheme,
    ToggleSingle,
    TogglePreview,
}

#[derive(Default)]
struct App {
    loaded: Option<Loaded>,
    error: Option<String>,
    single: bool,
    preview: bool,
    exported: Option<Result<Vec<PathBuf>, String>>,
    light: bool,
    applied: Option<bool>,
    logo: Option<TextureHandle>,
    logo_tried: bool,
}

impl App {
    fn open(&mut self, path: PathBuf) {
        self.exported = None;
        self.error = None;
        self.loaded = None;
        if !path.extension().is_some_and(|e| e.eq_ignore_ascii_case("c2d")) {
            self.error = Some(format!("« {} » n'est pas un projet Carbide Create (.c2d)", file_name(&path)));
            return;
        }
        match load(&path, None) {
            Ok((project, job)) => {
                let checks = check(&project, &job);
                let summary = summarize(&project, &job, &checks);
                self.loaded = Some(Loaded { path, project, job, checks, summary });
            }
            Err(e) => self.error = Some(format!("{} : {e}", file_name(&path))),
        }
    }

    fn export(&mut self) {
        let Some(l) = &self.loaded else { return };
        self.exported = Some(write_outputs(&l.job, &l.dir(), self.single, self.preview));
    }

    fn pal(&self) -> &'static Palette {
        if self.light { &theme::LIGHT } else { &theme::DARK }
    }

    fn apply_theme(&mut self, ctx: &egui::Context) {
        if self.applied == Some(self.light) {
            return;
        }
        self.applied = Some(self.light);
        let pal = self.pal();
        let (theme, mut v) = if self.light { (egui::Theme::Light, egui::Visuals::light()) } else { (egui::Theme::Dark, egui::Visuals::dark()) };
        v.panel_fill = pal.window;
        v.window_fill = pal.tooltip_bg;
        v.window_stroke = Stroke::new(1.0, pal.tooltip_line);
        v.window_corner_radius = CornerRadius::same(10);
        v.override_text_color = Some(pal.text);
        v.selection.bg_fill = alpha(pal.accent, 0.35);
        ctx.set_theme(theme);
        ctx.set_visuals_of(theme, v);
    }

    fn run(&mut self, action: Action) {
        match action {
            Action::Open(p) => self.open(p),
            Action::Pick => {
                if let Some(p) = rfd::FileDialog::new().add_filter("Projet Carbide Create", &["c2d"]).pick_file() {
                    self.open(p);
                }
            }
            Action::Export => self.export(),
            Action::OpenFolder => {
                if let Some(l) = &self.loaded {
                    open_folder(&l.dir());
                }
            }
            Action::ToggleTheme => self.light = !self.light,
            Action::ToggleSingle => self.single = !self.single,
            Action::TogglePreview => self.preview = !self.preview,
        }
    }
}

fn file_name(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

fn open_folder(dir: &Path) {
    #[cfg(target_os = "windows")]
    let _ = std::process::Command::new("explorer").arg(dir).spawn();
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(dir).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let _ = std::process::Command::new("xdg-open").arg(dir).spawn();
}

/// Polices : Segoe UI et Consolas de Windows si présentes, sinon celles d'egui.
/// La famille « bold » existe toujours (sinon egui s'arrêterait au premier texte en gras).
fn setup_fonts(ctx: &egui::Context) {
    let mut defs = FontDefinitions::default();
    let fonts_dir = std::env::var_os("WINDIR").map_or_else(|| PathBuf::from(r"C:\Windows"), PathBuf::from).join("Fonts");
    let add = |defs: &mut FontDefinitions, key: &str, file: &str| -> bool {
        match std::fs::read(fonts_dir.join(file)) {
            Ok(bytes) => {
                defs.font_data.insert(key.to_owned(), Arc::new(FontData::from_owned(bytes)));
                true
            }
            Err(_) => false,
        }
    };
    let prop = FontFamily::Proportional;
    let has_ui = add(&mut defs, "ui", "segoeui.ttf");
    let has_bold = add(&mut defs, "ui-bold", "segoeuib.ttf");
    let has_mono = add(&mut defs, "mono", "consola.ttf");
    if has_ui {
        defs.families.entry(prop.clone()).or_default().insert(0, "ui".into());
    }
    let mut bold_list = defs.families.get(&prop).cloned().unwrap_or_default();
    if has_bold {
        bold_list.insert(0, "ui-bold".into());
    }
    defs.families.insert(FontFamily::Name("bold".into()), bold_list);
    if has_mono {
        defs.families.entry(FontFamily::Monospace).or_default().insert(0, "mono".into());
    }
    ctx.set_fonts(defs);
}

fn load_logo(ctx: &egui::Context) -> Option<TextureHandle> {
    let icon = eframe::icon_data::from_png_bytes(ICON_PNG).ok()?;
    let img = ColorImage::from_rgba_unmultiplied([icon.width as usize, icon.height as usize], &icon.rgba);
    Some(ctx.load_texture("logo", img, TextureOptions::LINEAR))
}

fn level_color(pal: &Palette, l: Level) -> Color32 {
    match l {
        Level::Ok => pal.ok.line,
        Level::Info => pal.accent,
        Level::Warn => pal.warn.line,
        Level::Error => pal.bad.line,
    }
}

/// Carte arrondie (fond légèrement relevé).
fn card<R>(ui: &mut Ui, pal: &Palette, add: impl FnOnce(&mut Ui) -> R) -> R {
    egui::Frame::new()
        .fill(pal.card)
        .stroke(Stroke::new(1.0, pal.card_line))
        .corner_radius(CornerRadius::same(14))
        .inner_margin(Margin::same(16))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing = vec2(8.0, 14.0);
            add(ui)
        })
        .inner
}

/// Ligne « pastille de couleur + texte » (info-bulles, vérifications générales).
fn check_line(ui: &mut Ui, pal: &Palette, c: &Check) {
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let (r, _) = ui.allocate_exact_size(vec2(10.0, 18.0), Sense::hover());
        widgets::status_dot(ui.painter(), pos2(r.center().x, r.top() + 9.0), level_color(pal, c.level));
        ui.add(Label::new(RichText::new(&c.text).size(13.0).color(pal.text)).wrap());
    });
}

fn tile_tooltip(ui: &mut Ui, pal: &Palette, t: &Tile) {
    ui.set_max_width(290.0);
    ui.spacing_mut().item_spacing.y = 3.0;
    let sc = pal.state(t.state);
    let title_color = if t.state == State::Unused { pal.text } else { sc.icon };
    ui.label(RichText::new(&t.headline).font(bold(14.0)).color(title_color));
    for c in &t.details {
        check_line(ui, pal, c);
    }
}

fn paint_tile(ui: &Ui, rect: Rect, t: &Tile, pal: &Palette, hovered: bool) {
    let p = ui.painter();
    let sc = pal.state(t.state);
    if pal.halo > 0.0 && t.state != State::Unused {
        widgets::glow(p, rect, 12, alpha(sc.line, pal.halo * if hovered { 1.6 } else { 1.0 }), 16);
    }
    p.rect_filled(rect, CornerRadius::same(12), sc.bg);
    let line = if hovered { mix(sc.line, pal.text, 0.25) } else { sc.line };
    p.rect_stroke(rect, CornerRadius::same(12), Stroke::new(2.0, line), StrokeKind::Inside);
    // pastille d'état
    let bc = pos2(rect.right() - 19.0, rect.top() + 19.0);
    p.circle_filled(bc, 11.0, sc.badge);
    icons::badge_mark(p, Rect::from_center_size(bc, vec2(12.0, 12.0)), t.state, sc.badge_fg);
    // grosse icône
    let ic = Rect::from_center_size(pos2(rect.center().x, rect.top() + 40.0), vec2(46.0, 46.0));
    icons::tile_icon(p, ic, t.kind, t.state, sc.icon);
    // titre et chiffre clé
    let title_color = if t.state == State::Unused { pal.weak } else { pal.text };
    p.text(pos2(rect.center().x, rect.bottom() - 30.0), Align2::CENTER_BOTTOM, t.kind.title(), bold(15.0), title_color);
    let font = regular(12.5);
    let key = widgets::elide_start(&t.key, rect.width() - 12.0, |s| widgets::text_width(p, s, &font));
    p.text(pos2(rect.center().x, rect.bottom() - 11.0), Align2::CENTER_BOTTOM, key, font, sc.key);
}

/// Badge « T102 » d'un outil ; renvoie sa largeur.
fn tool_badge(ui: &Ui, pos: egui::Pos2, number: i64, pal: &Palette) -> f32 {
    let p = ui.painter();
    let text = format!("T{number}");
    let font = mono(13.0);
    let w = widgets::text_width(p, &text, &font) + 16.0;
    let r = Rect::from_min_size(pos2(pos.x, pos.y - 11.0), vec2(w, 22.0));
    if pal.dark {
        p.rect_filled(r, CornerRadius::same(6), pal.accent);
        p.text(r.center(), Align2::CENTER_CENTER, text, font, pal.on_accent);
    } else {
        p.rect_filled(r, CornerRadius::same(6), pal.accent_pill_bg);
        p.rect_stroke(r, CornerRadius::same(6), Stroke::new(1.0, pal.accent_pill_line), StrokeKind::Inside);
        p.text(r.center(), Align2::CENTER_CENTER, text, font, pal.accent_text);
    }
    w
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.apply_theme(&ctx);
        if !self.logo_tried {
            self.logo_tried = true;
            self.logo = load_logo(&ctx);
        }
        // fichiers déposés
        let dropped: Vec<PathBuf> =
            ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).filter(|p| !p.as_os_str().is_empty()).collect());
        let hovering = ctx.input(|i| !i.raw.hovered_files.is_empty());

        let pal = self.pal();
        let mut actions: Vec<Action> = dropped.into_iter().take(1).map(Action::Open).collect();
        egui::CentralPanel::default().frame(egui::Frame::new().fill(pal.window)).show(ui, |ui| self.page(ui, hovering, &mut actions));
        // egui peut rejouer une passe (première apparition d'une info-bulle…) : les clics seraient
        // alors vus deux fois ; on n'agit que sur la passe qui sera affichée.
        if !actions.is_empty() && !ctx.will_discard() {
            for a in actions {
                self.run(a);
            }
            // l'action change l'affichage (thème, projet, résultat d'export) : nouvelle image
            ctx.request_repaint();
        }
    }
}

impl App {
    fn page(&mut self, ui: &mut Ui, hovering: bool, actions: &mut Vec<Action>) {
        let pal = self.pal();
        let full = ui.max_rect();
        let bar = Rect::from_min_size(full.min, vec2(full.width(), 52.0));
        let status = Rect::from_min_max(pos2(full.left(), full.bottom() - 34.0), full.max);
        self.top_bar(ui, bar, pal, actions);
        self.status_bar(ui, status, pal);
        let body = Rect::from_min_max(pos2(full.left(), bar.bottom()), pos2(full.right(), status.top()));
        let mut child = ui.new_child(UiBuilder::new().max_rect(body));
        ScrollArea::vertical().id_salt("page").auto_shrink([false, false]).show(&mut child, |ui| {
            egui::Frame::new().inner_margin(Margin { left: 20, right: 20, top: 18, bottom: 16 }).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.spacing_mut().item_spacing = vec2(8.0, 14.0);
                self.body(ui, hovering, pal, actions);
            });
        });
    }

    fn top_bar(&self, ui: &Ui, bar: Rect, pal: &Palette, actions: &mut Vec<Action>) {
        let p = ui.painter();
        p.rect_filled(bar, CornerRadius::ZERO, pal.bar);
        p.hline(bar.x_range(), bar.bottom() - 0.5, Stroke::new(1.0, pal.bar_line));
        let logo = Rect::from_min_size(pos2(bar.left() + 18.0, bar.center().y - 15.0), vec2(30.0, 30.0));
        if let Some(t) = &self.logo {
            p.image(t.id(), logo, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
        }
        let title = p.text(pos2(logo.right() + 12.0, bar.center().y), Align2::LEFT_CENTER, "c2d_to_nc", bold(19.0), pal.text);
        p.text(
            pos2(title.right() + 10.0, bar.center().y + 2.0),
            Align2::LEFT_CENTER,
            concat!("MakerCraft · v", env!("CARGO_PKG_VERSION")),
            regular(12.0),
            pal.weak,
        );
        // bascule de thème
        let r = Rect::from_min_size(pos2(bar.right() - 8.0 - 44.0, bar.center().y - 18.0), vec2(44.0, 36.0));
        let label = if self.light { "Thème sombre" } else { "Thème clair" };
        let resp = widgets::hit(ui, r, label, true);
        let bg = if resp.hovered() { pal.pill } else { pal.bar };
        p.rect_filled(r, CornerRadius::same(6), bg);
        let g = if self.light { Glyph::Moon } else { Glyph::Sun };
        icons::glyph(p, Rect::from_center_size(r.center(), vec2(20.0, 20.0)), g, if resp.hovered() { pal.text } else { pal.weak }, bg);
        if resp.clicked() {
            actions.push(Action::ToggleTheme);
        }
        resp.on_hover_text(label);
    }

    fn status_bar(&self, ui: &Ui, r: Rect, pal: &Palette) {
        let p = ui.painter();
        p.rect_filled(r, CornerRadius::ZERO, pal.bar);
        p.hline(r.x_range(), r.top() + 0.5, Stroke::new(1.0, pal.bar_line));
        let (color, text) = match (&self.exported, &self.loaded, &self.error) {
            (Some(Ok(files)), _, _) => {
                (pal.ok.line, format!("{} dans le dossier du projet", plural(files.len(), "fichier écrit", "fichiers écrits")))
            }
            (Some(Err(e)), _, _) => (pal.bad.line, format!("Échec de l'export : {e}")),
            (None, _, Some(e)) => (pal.bad.line, e.clone()),
            (None, Some(l), None) => {
                let s = &l.summary;
                let (ok, warn, miss) = (s.count(State::Ok), s.count(State::Warn), s.count(State::Missing));
                if l.blocking() {
                    let first = l.checks.iter().find(|c| c.level == Level::Error).map_or("rien à produire", |c| c.text.as_str());
                    (pal.bad.line, format!("Export impossible — {first}"))
                } else {
                    let mut parts = vec![plural(ok, "vérification OK", "vérifications OK")];
                    if warn > 0 {
                        parts.push(plural(warn, "avertissement", "avertissements"));
                    }
                    if miss > 0 {
                        parts.push(format!("{} (non bloquant)", plural(miss, "manquant", "manquants")));
                    }
                    (if warn + miss > 0 { pal.warn.line } else { pal.ok.line }, format!("Prêt — {}", parts.join(" · ")))
                }
            }
            (None, None, None) => (pal.weak, "En attente d'un projet Carbide Create (.c2d)".to_owned()),
        };
        widgets::status_dot(p, pos2(r.left() + 22.0, r.center().y), color);
        let right = p.text(pos2(r.right() - 18.0, r.center().y), Align2::RIGHT_CENTER, "ASCII · Grbl 1.1", mono(12.0), pal.weak2);
        let font = regular(12.0);
        let max = right.left() - 16.0 - (r.left() + 34.0);
        let shown = widgets::elide_start(&text, max, |s| widgets::text_width(p, s, &font));
        p.text(pos2(r.left() + 34.0, r.center().y), Align2::LEFT_CENTER, shown, font, pal.weak2);
        if let Some(Ok(files)) = &self.exported {
            let resp = ui.interact(r, ui.id().with("etat"), Sense::hover());
            resp.on_hover_ui(|ui| {
                for f in files {
                    ui.label(RichText::new(file_name(f)).font(mono(12.0)));
                }
            });
        }
    }

    fn body(&mut self, ui: &mut Ui, hovering: bool, pal: &Palette, actions: &mut Vec<Action>) {
        self.drop_zone(ui, hovering, pal, actions);
        let Some(l) = &self.loaded else {
            ui.label(
                RichText::new("Le G-code produit est du Grbl standard en ASCII strict (Candle, UGS, LaserGRBL, Carbide Motion…).")
                    .size(12.5)
                    .color(pal.weak),
            );
            return;
        };
        project_card(ui, l, pal);
        output_card(ui, l, pal, self.single, self.preview, actions);

        // actions
        let can_export = !l.blocking();
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 12.0;
            let ew = (ui.available_width() - 12.0 - 56.0).max(120.0);
            let export = widgets::primary_button(ui, vec2(ew, 56.0), "Exporter les .nc", can_export, pal, |p, r, c| {
                icons::glyph(p, r, Glyph::Export, c, c);
            });
            if export.clicked() {
                actions.push(Action::Export);
            }
            if !can_export {
                export.on_hover_text("Export impossible : corriger les éléments en rouge");
            }
            let (fr, _) = ui.allocate_exact_size(vec2(56.0, 56.0), Sense::hover());
            let folder = widgets::secondary_button(ui, fr, "Ouvrir le dossier", None, true, pal);
            icons::glyph(ui.painter(), Rect::from_center_size(fr.center(), vec2(24.0, 24.0)), Glyph::Folder, pal.secondary_fg, pal.secondary_bg);
            if folder.clicked() {
                actions.push(Action::OpenFolder);
            }
            folder.on_hover_text("Ouvrir le dossier du projet (où sont écrits les .nc)");
        });
    }

    fn drop_zone(&self, ui: &mut Ui, hovering: bool, pal: &Palette, actions: &mut Vec<Action>) {
        let w = ui.available_width();
        let compact = self.loaded.is_some();
        let (rect, _) = ui.allocate_exact_size(vec2(w, if compact { 84.0 } else { 190.0 }), Sense::hover());
        let p = ui.painter();
        let line = if hovering { pal.accent } else { pal.drop_line };
        widgets::glow(p, rect, 14, if hovering { alpha(pal.accent, 0.35) } else { pal.drop_glow }, 18);
        p.rect_filled(rect, CornerRadius::same(14), if hovering { alpha(pal.accent, 0.12) } else { pal.drop_bg });
        widgets::dashed_rect(p, rect.shrink(1.0), 13.0, Stroke::new(2.0, line), 7.0, 5.0);

        if compact {
            let ic = Rect::from_min_size(pos2(rect.left() + 18.0, rect.center().y - 22.0), vec2(44.0, 44.0));
            icons::glyph(p, ic, Glyph::Drop, pal.accent, pal.drop_bg);
            let label = "Ouvrir…";
            let bw = widgets::text_width(p, label, &bold(14.0)) + 36.0;
            let br = Rect::from_min_size(pos2(rect.right() - 18.0 - bw, rect.center().y - 22.0), vec2(bw, 44.0));
            if widgets::secondary_button(ui, br, label, Some(label), true, pal).clicked() {
                actions.push(Action::Pick);
            }
            let x = ic.right() + 16.0;
            let max = br.left() - 16.0 - x;
            let (name, dir) = self.loaded.as_ref().map(|l| (file_name(&l.path), l.dir().display().to_string())).unwrap_or_default();
            let (sub, sub_color) = match &self.error {
                Some(e) => (e.clone(), pal.bad.line),
                None => (dir, pal.weak),
            };
            let top = if hovering { "Relâchez pour ouvrir le projet" } else { "Déposer un fichier .c2d ici" };
            p.text(pos2(x, rect.center().y - 20.0), Align2::LEFT_CENTER, top, regular(12.0), if hovering { pal.accent_text } else { pal.weak });
            let nf = mono(14.0);
            let name = widgets::elide_start(&name, max, |s| widgets::text_width(p, s, &nf));
            p.text(pos2(x, rect.center().y), Align2::LEFT_CENTER, name, nf, pal.text);
            let sf = regular(12.0);
            let sub = widgets::elide_start(&sub, max, |s| widgets::text_width(p, s, &sf));
            p.text(pos2(x, rect.center().y + 20.0), Align2::LEFT_CENTER, sub, sf, sub_color);
        } else {
            let ic = Rect::from_center_size(pos2(rect.center().x, rect.top() + 46.0), vec2(56.0, 56.0));
            icons::glyph(p, ic, Glyph::Drop, pal.accent, pal.drop_bg);
            let msg = if hovering { "Relâchez pour ouvrir le projet" } else { "Glissez un projet Carbide Create (.c2d) ici" };
            p.text(pos2(rect.center().x, rect.top() + 96.0), Align2::CENTER_CENTER, msg, bold(17.0), pal.text);
            if let Some(e) = &self.error {
                let f = regular(12.5);
                let e = widgets::elide_start(e, rect.width() - 40.0, |s| widgets::text_width(p, s, &f));
                p.text(pos2(rect.center().x, rect.top() + 120.0), Align2::CENTER_CENTER, e, f, pal.bad.line);
            }
            let label = "Ouvrir un fichier…";
            let bw = widgets::text_width(p, label, &bold(14.0)) + 40.0;
            let br = Rect::from_center_size(pos2(rect.center().x, rect.bottom() - 36.0), vec2(bw, 44.0));
            if widgets::secondary_button(ui, br, label, Some(label), true, pal).clicked() {
                actions.push(Action::Pick);
            }
        }
    }
}

fn project_card(ui: &mut Ui, l: &Loaded, pal: &Palette) {
    card(ui, pal, |ui| {
        // en-tête
        let w = ui.available_width();
        let (hr, _) = ui.allocate_exact_size(vec2(w, 40.0), Sense::hover());
        let p = ui.painter();
        let sq = Rect::from_min_size(hr.min, vec2(40.0, 40.0));
        if pal.dark {
            p.rect_filled(sq, CornerRadius::same(10), pal.accent);
            icons::glyph(p, sq.shrink(9.0), Glyph::File, pal.card, pal.accent);
        } else {
            p.rect_filled(sq, CornerRadius::same(10), pal.accent_pill_bg);
            p.rect_stroke(sq, CornerRadius::same(10), Stroke::new(1.0, pal.accent_pill_line), StrokeKind::Inside);
            icons::glyph(p, sq.shrink(9.0), Glyph::File, pal.accent, pal.accent_pill_bg);
        }
        let tf = bold(18.0);
        let title = widgets::elide_start(&l.job.title, w - 56.0, |s| widgets::text_width(p, s, &tf));
        p.text(pos2(sq.right() + 12.0, hr.center().y), Align2::LEFT_CENTER, title, tf, pal.text);

        // pastilles
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
            let plain = PillStyle { bg: pal.pill, line: pal.pill_line, fg: pal.text, icon: pal.icon, bold: true };
            let (sw, sh, st) = l.job.stock_size;
            if sw > 0.0 {
                let t = format!("{} × {} × {} mm", fmt_mm(sw), fmt_mm(sh), fmt_mm(st));
                widgets::pill(ui, &t, plain, |p, r, c| icons::small(p, r, Small::Ruler, c));
            }
            if !l.job.machine.is_empty() {
                widgets::pill(ui, &l.job.machine, plain, |p, r, c| icons::small(p, r, Small::Machine, c));
            }
            if !l.job.groups.is_empty() {
                let accent = PillStyle { bg: pal.accent_pill_bg, line: pal.accent_pill_line, fg: pal.accent_text, icon: pal.accent_text, bold: true };
                widgets::pill(ui, &fmt_minutes(l.total_seconds()), accent, |p, r, c| icons::small(p, r, Small::Clock, c))
                    .on_hover_text("Durée estimée hors changements d'outil");
            }
            if l.project.has_encrypted_gcode() {
                let st = PillStyle { bg: pal.violet_pill_bg, line: pal.violet_pill_line, fg: pal.violet_text, icon: pal.violet, bold: false };
                widgets::pill(ui, "G-code CC ignoré", st, |p, r, c| icons::small(p, r, Small::Lock, c))
                    .on_hover_text("Le G-code embarqué de Carbide Create est chiffré : il est ignoré, les trajectoires sont recalculées");
            }
        });

        // tuiles 4 × 2
        let w = ui.available_width();
        let tw = ((w - 36.0) / 4.0).floor();
        for row in l.summary.tiles.chunks(4) {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 12.0;
                for t in row {
                    let (rect, resp) = ui.allocate_exact_size(vec2(tw, 128.0), Sense::hover());
                    paint_tile(ui, rect, t, pal, resp.hovered());
                    resp.on_hover_ui(|ui| tile_tooltip(ui, pal, t));
                }
            });
        }

        // vérifications générales (non rattachées à une tuile)
        if !l.summary.other.is_empty() {
            ui.spacing_mut().item_spacing.y = 4.0;
            for c in &l.summary.other {
                check_line(ui, pal, c);
            }
        }
    });
}

fn output_card(ui: &mut Ui, l: &Loaded, pal: &Palette, single: bool, preview: bool, actions: &mut Vec<Action>) {
    card(ui, pal, |ui| {
        ui.spacing_mut().item_spacing.y = 8.0;
        let w = ui.available_width();
        let names = output_names(&l.job, single);
        let (hr, _) = ui.allocate_exact_size(vec2(w, 24.0), Sense::hover());
        let p = ui.painter();
        p.text(pos2(hr.left(), hr.center().y), Align2::LEFT_CENTER, "Sortie", bold(17.0), pal.text);
        if !l.job.groups.is_empty() {
            let t = format!("{} · {}", plural(names.len(), "fichier", "fichiers"), fmt_minutes(l.total_seconds()));
            p.text(pos2(hr.right(), hr.center().y), Align2::RIGHT_CENTER, t, regular(12.5), pal.weak2);
        }

        let row = |ui: &mut Ui, tools: &[i64], name: &str, seconds: f64, detail: String| {
            let (r, resp) = ui.allocate_exact_size(vec2(w, 44.0), Sense::hover());
            let p = ui.painter();
            p.rect_filled(r, CornerRadius::same(9), pal.pill);
            if !pal.dark {
                p.rect_stroke(r, CornerRadius::same(9), Stroke::new(1.0, pal.card_line), StrokeKind::Inside);
            }
            let mut x = r.left() + 12.0;
            for &t in tools {
                x += tool_badge(ui, pos2(x, r.center().y), t, pal) + 6.0;
            }
            let df = regular(13.0);
            let dr = p.text(pos2(r.right() - 12.0, r.center().y), Align2::RIGHT_CENTER, fmt_minutes(seconds), df, pal.weak2);
            icons::small(p, Rect::from_min_size(pos2(dr.left() - 19.0, r.center().y - 7.0), vec2(14.0, 14.0)), Small::Clock, pal.weak2);
            let nf = mono(13.0);
            let max = dr.left() - 19.0 - 12.0 - (x + 4.0);
            let shown = widgets::elide_start(name, max, |s| widgets::text_width(p, s, &nf));
            p.text(pos2(x + 4.0, r.center().y), Align2::LEFT_CENTER, shown, nf, pal.text);
            resp.on_hover_ui(|ui| {
                ui.label(RichText::new(name).font(mono(12.5)));
                ui.label(RichText::new(detail).size(13.0).color(pal.weak2));
            });
        };

        if l.job.groups.is_empty() {
            ui.label(RichText::new("Rien à exporter avec ce projet.").size(13.0).color(pal.weak));
        } else if single || l.job.groups.len() <= 1 {
            let tools: Vec<i64> = l.job.groups.iter().map(|g| g.tool.number).collect();
            let ops: Vec<String> = l.job.groups.iter().flat_map(|g| g.ops.iter().map(|o| o.name.clone())).collect();
            let detail = if tools.len() > 1 { format!("{} — pause M0 à chaque changement d'outil", ops.join(" + ")) } else { ops.join(" + ") };
            row(ui, &tools, names.first().map_or("", String::as_str), l.total_seconds(), detail);
        } else {
            for (g, name) in l.job.groups.iter().zip(&names) {
                let ops: Vec<&str> = g.ops.iter().map(|o| o.name.as_str()).collect();
                let detail = format!("{} — {}", tool_label(&g.tool), ops.join(" + "));
                row(ui, &[g.tool.number], name, g.stats(l.job.heights).seconds, detail);
            }
        }

        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 22.0;
            let s = widgets::toggle(ui, single, "Fichier unique (M0)", pal);
            if s.clicked() {
                actions.push(Action::ToggleSingle);
            }
            s.on_hover_text("Un seul .nc, avec une pause M0 à chaque changement d'outil");
            let p = widgets::toggle(ui, preview, "Aperçu SVG", pal);
            if p.clicked() {
                actions.push(Action::TogglePreview);
            }
            p.on_hover_text("Écrire aussi un aperçu SVG des trajectoires");
        });
    });
}

fn main() -> eframe::Result {
    let mut app = App::default();
    if let Some(arg) = std::env::args_os().nth(1) {
        app.open(PathBuf::from(arg));
    }
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("c2d_to_nc — MakerCraft")
        .with_inner_size([720.0, 940.0])
        .with_min_inner_size([600.0, 560.0])
        .with_drag_and_drop(true);
    if let Ok(icon) = eframe::icon_data::from_png_bytes(ICON_PNG) {
        viewport = viewport.with_icon(icon);
    }
    let options = eframe::NativeOptions { viewport, ..Default::default() };
    eframe::run_native(
        "c2d_to_nc",
        options,
        Box::new(move |cc| {
            setup_fonts(&cc.egui_ctx);
            Ok(Box::new(app))
        }),
    )
}
