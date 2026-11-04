//! Synthèse du projet pour l'affichage : 8 tuiles (Brut, Dessin, Gravure V, Perçage, Poche,
//! Découpe, Attaches, Outils), chacune avec un état, un chiffre clé et le détail des vérifications.
//! Aucune dépendance à egui : testable seul.

use makercraft_c2d::{Project, Toolpath, ToolpathKind};
use makercraft_cam::Job;
use makercraft_cam::ext::{Check, Level};
use makercraft_cam::job::tool_label;

/// État visuel d'une tuile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// vert ✓
    Ok,
    /// orange !
    Warn,
    /// rouge ✕
    Missing,
    /// gris : rien de ce type dans le projet
    Unused,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TileKind {
    Stock,
    Design,
    VCarve,
    Drill,
    Pocket,
    Cutout,
    Tabs,
    Tools,
}

impl TileKind {
    pub const ALL: [Self; 8] = [Self::Stock, Self::Design, Self::VCarve, Self::Drill, Self::Pocket, Self::Cutout, Self::Tabs, Self::Tools];

    pub fn title(self) -> &'static str {
        match self {
            Self::Stock => "Brut",
            Self::Design => "Dessin",
            Self::VCarve => "Gravure V",
            Self::Drill => "Perçage",
            Self::Pocket => "Poche",
            Self::Cutout => "Découpe",
            Self::Tabs => "Attaches",
            Self::Tools => "Outils",
        }
    }

    fn of(kind: &ToolpathKind) -> Option<Self> {
        match kind {
            ToolpathKind::Cutout => Some(Self::Cutout),
            ToolpathKind::Pocket => Some(Self::Pocket),
            ToolpathKind::Drill => Some(Self::Drill),
            ToolpathKind::VCarve => Some(Self::VCarve),
            ToolpathKind::Other(_) => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Tile {
    pub kind: TileKind,
    pub state: State,
    /// chiffre clé affiché sous le titre
    pub key: String,
    /// titre de l'info-bulle
    pub headline: String,
    /// lignes de l'info-bulle
    pub details: Vec<Check>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Summary {
    pub tiles: Vec<Tile>,
    /// vérifications non rattachées à une tuile (avertissements et erreurs seulement)
    pub other: Vec<Check>,
}

impl Summary {
    pub fn count(&self, s: State) -> usize {
        self.tiles.iter().filter(|t| t.state == s).count()
    }
}

/// Nombre en millimètres à la française : 9.5 → « 9,5 », 290.0 → « 290 ».
pub fn fmt_mm(v: f64) -> String {
    let s = format!("{v:.2}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".to_owned() } else { s.replace('.', ",") }
}

/// « 1 trou », « 64 trous ».
pub fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n > 1 { many } else { one })
}

/// Durée lisible : « ≈ 27 min », « < 1 min ».
pub fn fmt_minutes(seconds: f64) -> String {
    let m = (seconds / 60.0).round();
    if m < 1.0 { "< 1 min".to_owned() } else { format!("≈ {m:.0} min") }
}

fn ok(text: impl Into<String>) -> Check {
    Check { level: Level::Ok, text: text.into() }
}

fn warn(text: impl Into<String>) -> Check {
    Check { level: Level::Warn, text: text.into() }
}

/// Tuile qui doit recevoir une vérification. `None` : vérification générale.
fn route(c: &Check, pr: &Project) -> Vec<TileKind> {
    let low = c.text.to_lowercase();
    if low.contains("attache") {
        return vec![TileKind::Tabs];
    }
    // le nom de parcours le plus long d'abord (« Pocket 10 » avant « Pocket 1 »)
    let mut tps: Vec<&Toolpath> = pr.toolpaths.iter().collect();
    tps.sort_by_key(|t| std::cmp::Reverse(t.name.chars().count()));
    if let Some(tp) = tps.into_iter().find(|t| !t.name.is_empty() && c.text.contains(t.name.as_str())) {
        let mut v: Vec<TileKind> = TileKind::of(&tp.kind).into_iter().collect();
        if low.contains("diamètre d'outil") || low.contains("vitesse") || low.contains("broche") {
            v.push(TileKind::Tools);
        }
        return v;
    }
    if low.contains("brut") || low.contains("zéro") {
        return vec![TileKind::Stock];
    }
    Vec::new()
}

fn state_of(details: &[Check]) -> State {
    match details.iter().map(|c| c.level).max() {
        Some(Level::Error) => State::Missing,
        Some(Level::Warn) => State::Warn,
        _ => State::Ok,
    }
}

/// Construit les 8 tuiles à partir du projet, du travail calculé et des vérifications.
pub fn summarize(pr: &Project, job: &Job, checks: &[Check]) -> Summary {
    // vérifications rattachées (hors « OK » de parcours, remplacées par nos propres lignes)
    let mut routed: Vec<(TileKind, Check)> = Vec::new();
    let mut other = Vec::new();
    for c in checks {
        let targets = route(c, pr);
        if targets.is_empty() {
            if c.level >= Level::Warn {
                other.push(c.clone());
            }
            continue;
        }
        let is_tp_ok = c.level == Level::Ok && c.text.starts_with('«');
        for t in targets {
            if !is_tp_ok {
                routed.push((t, c.clone()));
            }
        }
    }
    let checks_for = |k: TileKind| -> Vec<Check> { routed.iter().filter(|(t, _)| *t == k).map(|(_, c)| c.clone()).collect() };
    let enabled = |k: TileKind| -> Vec<&Toolpath> { pr.toolpaths.iter().filter(|t| t.enabled && TileKind::of(&t.kind) == Some(k)).collect() };

    let tiles = TileKind::ALL
        .iter()
        .map(|&kind| match kind {
            TileKind::Stock => stock_tile(job, checks_for(kind)),
            TileKind::Design => design_tile(pr),
            TileKind::VCarve | TileKind::Drill | TileKind::Pocket | TileKind::Cutout => op_tile(pr, kind, &enabled(kind), checks_for(kind)),
            TileKind::Tabs => tabs_tile(pr, &enabled(TileKind::Cutout), checks_for(kind)),
            TileKind::Tools => tools_tile(pr, checks_for(kind)),
        })
        .collect();
    Summary { tiles, other }
}

fn stock_tile(job: &Job, mut details: Vec<Check>) -> Tile {
    let (w, h, t) = job.stock_size;
    let key = if w > 0.0 && h > 0.0 && t > 0.0 { format!("{}×{}×{}", fmt_mm(w), fmt_mm(h), fmt_mm(t)) } else { "absent".to_owned() };
    details.push(ok("Zéro : X0 Y0 coin avant gauche, Z0 dessus du brut"));
    let state = if key == "absent" { State::Missing } else { state_of(&details) };
    Tile { kind: TileKind::Stock, state, headline: format!("Brut : {key} mm"), key, details }
}

fn design_tile(pr: &Project) -> Tile {
    use std::collections::{BTreeSet, HashSet};
    let n = pr.elements.len();
    // formes réellement utilisées par un parcours actif
    let mut used: HashSet<&str> = HashSet::new();
    let mut pieces = 0;
    for tp in pr.toolpaths.iter().filter(|t| t.enabled && TileKind::of(&t.kind).is_some()) {
        let els = pr.elements_for(tp);
        if tp.kind == ToolpathKind::Cutout {
            pieces += els.len();
        }
        used.extend(els.iter().map(|e| e.id.as_str()));
    }
    let texts = pr.elements.iter().filter(|e| e.text.is_some() && used.contains(e.id.as_str())).count();
    let mut details = Vec::new();
    if n == 0 {
        details.push(warn("Aucune forme dans le dessin"));
    } else if used.is_empty() {
        details.push(warn(format!("{} mais aucune sur les calques des parcours", plural(n, "forme", "formes"))));
    } else {
        let mut line = format!("{} par les parcours", plural(used.len(), "forme usinée", "formes usinées"));
        if texts > 0 {
            line.push_str(&format!(", dont {}", plural(texts, "texte", "textes")));
        }
        details.push(ok(line));
        let unused: Vec<_> = pr.elements.iter().filter(|e| !used.contains(e.id.as_str())).collect();
        if !unused.is_empty() {
            let layers: BTreeSet<&str> = unused.iter().map(|e| e.layer_name.as_str()).collect();
            let layers: Vec<&str> = layers.into_iter().collect();
            details.push(Check {
                level: Level::Info,
                text: format!("{} hors parcours (calques {}) : non usinées", plural(unused.len(), "forme", "formes"), layers.join(", ")),
            });
        }
    }
    details.extend(pr.warnings.iter().map(warn));
    let state = if n == 0 || used.is_empty() { State::Missing } else { state_of(&details) };
    let key = if pieces > 0 { plural(pieces, "pièce", "pièces") } else { plural(used.len(), "forme", "formes") };
    Tile { kind: TileKind::Design, state, headline: format!("Dessin : {}", plural(used.len(), "forme usinée", "formes usinées")), key, details }
}

fn noun(kind: TileKind, all_texts: bool) -> (&'static str, &'static str) {
    match kind {
        TileKind::VCarve if all_texts => ("texte", "textes"),
        TileKind::Drill => ("trou", "trous"),
        TileKind::Pocket => ("poche", "poches"),
        TileKind::Cutout => ("pièce", "pièces"),
        _ => ("forme", "formes"),
    }
}

fn op_tile(pr: &Project, kind: TileKind, tps: &[&Toolpath], mut routed: Vec<Check>) -> Tile {
    let disabled_only = tps.is_empty() && pr.toolpaths.iter().any(|t| TileKind::of(&t.kind) == Some(kind));
    if tps.is_empty() {
        let state = if routed.is_empty() { State::Unused } else { state_of(&routed) };
        let key = if disabled_only { "désactivé".to_owned() } else { "aucun".to_owned() };
        return Tile { kind, state, headline: format!("{} : pas de parcours actif", kind.title()), key, details: routed };
    }
    let mut details = Vec::new();
    let mut count = 0;
    let mut all_texts = true;
    let mut depth: f64 = 0.0;
    for tp in tps {
        let els = pr.elements_for(tp);
        count += els.len();
        all_texts &= !els.is_empty() && els.iter().all(|e| e.text.is_some());
        depth = depth.max(tp.end_depth);
        let (one, many) = noun(kind, els.iter().all(|e| e.text.is_some()) && !els.is_empty());
        let passes = if tp.stepdown > 0.0 && kind != TileKind::Drill {
            let n = ((tp.end_depth - tp.start_depth) / tp.stepdown).ceil().max(1.0);
            format!(" en {n:.0} passe(s)")
        } else {
            String::new()
        };
        details.push(ok(format!(
            "« {} » : {}, prof. {} mm{passes}, {}",
            tp.name,
            plural(els.len(), one, many),
            fmt_mm(tp.end_depth),
            tool_label(&tp.tool)
        )));
    }
    details.append(&mut routed);
    let (one, many) = noun(kind, all_texts);
    let mut key = plural(count, one, many);
    if matches!(kind, TileKind::VCarve | TileKind::Cutout) {
        key.push_str(&format!(" · {}", fmt_mm(depth)));
    }
    let headline = format!("{} : {}", kind.title(), plural(count, one, many));
    Tile { kind, state: state_of(&details), key, headline, details }
}

fn tabs_tile(pr: &Project, cutouts: &[&Toolpath], routed: Vec<Check>) -> Tile {
    let kind = TileKind::Tabs;
    if cutouts.is_empty() {
        return Tile { kind, state: State::Unused, key: "sans objet".into(), headline: "Attaches : pas de découpe".into(), details: routed };
    }
    let with_tabs: usize = cutouts.iter().map(|tp| if tp.ignore_tabs { 0 } else { pr.elements_for(tp).iter().filter(|e| e.has_tabs).count() }).sum();
    let through = cutouts.iter().any(|tp| tp.end_depth >= pr.stock.thickness - 1e-6);
    if with_tabs > 0 {
        let mut details = vec![ok(format!("{} avec attaches dans Carbide Create", plural(with_tabs, "pièce", "pièces")))];
        details.extend(routed);
        let key = plural(with_tabs, "pièce", "pièces");
        return Tile { kind, state: state_of(&details), headline: format!("Attaches : {key}"), key, details };
    }
    if through {
        let mut details = vec![warn("Les pièces seront libérées en fin de découpe : prévoir un bridage ou des attaches dans Carbide Create.")];
        details.extend(routed.into_iter().filter(|c| c.level > Level::Warn));
        return Tile { kind, state: State::Missing, key: "aucune".into(), headline: "Aucune attache".into(), details };
    }
    let mut details = vec![ok("La découpe ne traverse pas le brut : attaches inutiles")];
    details.extend(routed);
    Tile { kind, state: state_of(&details), key: "inutiles".into(), headline: "Attaches".into(), details }
}

fn tools_tile(pr: &Project, routed: Vec<Check>) -> Tile {
    let mut tools: Vec<&makercraft_c2d::Tool> = Vec::new();
    for tp in pr.toolpaths.iter().filter(|t| t.enabled && TileKind::of(&t.kind).is_some()) {
        if !tools.iter().any(|t| t.number == tp.tool.number) {
            tools.push(&tp.tool);
        }
    }
    tools.sort_by_key(|t| t.number);
    let kind = TileKind::Tools;
    if tools.is_empty() {
        return Tile { kind, state: State::Unused, key: "aucun".into(), headline: "Outils : aucun".into(), details: routed };
    }
    let names: Vec<String> = tools.iter().map(|t| format!("T{}", t.number)).collect();
    let key = if names.len() > 3 { format!("{} +{}", names[..2].join(" · "), names.len() - 2) } else { names.join(" · ") };
    let mut details: Vec<Check> = tools.iter().map(|t| ok(tool_label(t))).collect();
    details.extend(routed);
    Tile { kind, state: state_of(&details), headline: format!("Outils : {}", plural(tools.len(), "outil", "outils")), key, details }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn french_numbers() {
        assert_eq!(fmt_mm(290.0), "290");
        assert_eq!(fmt_mm(9.5), "9,5");
        assert_eq!(fmt_mm(0.5), "0,5");
        assert_eq!(fmt_mm(3.175), "3,17");
        assert_eq!(fmt_mm(-0.001), "0");
        assert_eq!(plural(1, "trou", "trous"), "1 trou");
        assert_eq!(plural(64, "trou", "trous"), "64 trous");
        assert_eq!(fmt_minutes(20.0), "< 1 min");
        assert_eq!(fmt_minutes(1620.0), "≈ 27 min");
    }

    /// Projet réel de démonstration (présent dans le dépôt de travail, pas dans le dépôt public).
    #[test]
    fn sample_project_tiles() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../_essais/Mise en planche - OSB 9mm.c2d");
        if !path.exists() {
            return;
        }
        let (pr, job) = match makercraft_cam::ext::load(&path, None) {
            Ok(v) => v,
            Err(e) => panic!("{e}"),
        };
        let checks = makercraft_cam::ext::check(&pr, &job);
        let s = summarize(&pr, &job, &checks);
        let tile = |k: TileKind| match s.tiles.iter().find(|t| t.kind == k) {
            Some(t) => t.clone(),
            None => panic!("tuile {k:?} absente"),
        };
        assert_eq!(s.tiles.len(), 8);
        assert_eq!(tile(TileKind::Stock).key, "290×170×9");
        assert_eq!(tile(TileKind::Stock).state, State::Ok);
        // 3 178 formes du dessin ne sont sur aucun calque de parcours : seules les usinées comptent
        assert_eq!(tile(TileKind::Design).key, "16 pièces");
        assert_eq!(tile(TileKind::Design).state, State::Ok);
        assert_eq!(tile(TileKind::Design).headline, "Dessin : 160 formes usinées");
        assert_eq!(tile(TileKind::Drill).key, "64 trous");
        assert_eq!(tile(TileKind::Drill).state, State::Ok);
        assert_eq!(tile(TileKind::Pocket).key, "64 poches");
        assert_eq!(tile(TileKind::VCarve).state, State::Warn);
        assert!(tile(TileKind::VCarve).key.starts_with("16 textes"), "{}", tile(TileKind::VCarve).key);
        assert!(tile(TileKind::Cutout).key.starts_with("16 pièces"), "{}", tile(TileKind::Cutout).key);
        assert_eq!(tile(TileKind::Cutout).state, State::Ok, "{:?}", tile(TileKind::Cutout).details);
        assert_eq!(tile(TileKind::Tabs).state, State::Missing);
        assert_eq!(tile(TileKind::Tabs).key, "aucune");
        assert_eq!(tile(TileKind::Tools).key, "T102 · T302");
        assert!(s.other.is_empty(), "{:?}", s.other);
    }
}
