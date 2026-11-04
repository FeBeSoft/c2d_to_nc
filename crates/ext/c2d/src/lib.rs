//! Lecture des projets Carbide Create (`.c2d`, versions 7 et 8).
//!
//! Format observé sur des fichiers de projet (lecture seule, à des fins d'interopérabilité) :
//! base SQLite avec
//! - `params (key, value)` : brut, machine, zéro, hauteur de retrait… ;
//! - `items (id, uuid, name, type, version, sz, data)` : un objet JSON par ligne, compressé zlib
//!   quand `length(data) != sz` (types `layer`, `element`, `toolpath`, `toolpath_group`, `model`) ;
//! - `sqlar` : archive SQLite de fichiers annexes (aperçus SVG, fond…).
//!
//! Le G-code que Carbide Create range dans `sqlar/gcode.egc` est **chiffré** (en-tête `CCV1`) :
//! il n'est jamais lu ici. On lit le projet (formes, outils, paramètres des parcours) et
//! MakerCraft recalcule lui-même les parcours (crate `makercraft-cam`).
//!
//! Unités : millimètres. Origine : coin avant gauche du brut, Y vers le fond (comme la machine).
//! Le fichier est ouvert en lecture seule et n'est jamais modifié.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::Path;

use makercraft_geom::{Bounds2, Vec2};
use rusqlite::{Connection, OpenFlags};
use serde_json::Value;

/// Tolérance par défaut pour transformer les courbes de Bézier en segments (mm).
pub const DEFAULT_TOLERANCE: f64 = 0.01;
/// Taille maximale d'un objet décompressé (protection contre un fichier abîmé).
const MAX_ITEM_BYTES: u64 = 64 * 1024 * 1024;
/// Nombre maximal de segments pour une courbe de Bézier.
const MAX_CURVE_STEPS: usize = 256;

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum C2dError {
    #[error("impossible d'ouvrir {path} : {msg}")]
    Open { path: String, msg: String },
    #[error("ce n'est pas un projet Carbide Create lisible ({0})")]
    NotC2d(String),
    #[error("lecture du projet : {0}")]
    Sql(String),
}

impl From<rusqlite::Error> for C2dError {
    fn from(e: rusqlite::Error) -> Self {
        Self::Sql(e.to_string())
    }
}

/// Un contour en segments droits, en mm, coordonnées du brut.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Contour {
    pub points: Vec<Vec2>,
    pub closed: bool,
}

impl Contour {
    /// Aire signée (> 0 : sens trigonométrique). Nulle pour un contour ouvert.
    pub fn signed_area(&self) -> f64 {
        if !self.closed || self.points.len() < 3 {
            return 0.0;
        }
        let n = self.points.len();
        (0..n).map(|i| self.points[i].cross(self.points[(i + 1) % n])).sum::<f64>() / 2.0
    }

    /// Point à l'intérieur (règle pair-impair) ; faux pour un contour ouvert.
    pub fn contains(&self, p: Vec2) -> bool {
        if !self.closed || self.points.len() < 3 {
            return false;
        }
        let mut inside = false;
        let n = self.points.len();
        let mut j = n - 1;
        for i in 0..n {
            let (a, b) = (self.points[i], self.points[j]);
            if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
                inside = !inside;
            }
            j = i;
        }
        inside
    }

    pub fn bounds(&self) -> Bounds2 {
        self.points.iter().copied().collect()
    }

    /// Longueur totale (avec la fermeture pour un contour fermé).
    pub fn length(&self) -> f64 {
        let mut l: f64 = self.points.windows(2).map(|w| (w[1] - w[0]).length()).sum();
        if self.closed
            && let (Some(a), Some(b)) = (self.points.first(), self.points.last())
        {
            l += (*a - *b).length();
        }
        l
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Layer {
    pub uuid: String,
    pub name: String,
    pub visible: bool,
}

/// Une forme du dessin.
#[derive(Debug, Clone, PartialEq)]
pub struct Element {
    pub id: String,
    /// `path`, `rectangle`, `circle`, `text`…
    pub kind: String,
    pub layer_uuid: String,
    pub layer_name: String,
    pub text: Option<String>,
    /// Centre et rayon pour les cercles (utile au perçage).
    pub center: Option<Vec2>,
    pub radius: Option<f64>,
    /// Des attaches sont dessinées sur la forme.
    pub has_tabs: bool,
    pub contours: Vec<Contour>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ToolKind {
    Flat,
    Ball,
    VBit,
    Other(i64),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Tool {
    pub number: i64,
    pub name: String,
    pub kind: ToolKind,
    /// mm
    pub diameter: f64,
    /// degrés (fraise en V)
    pub angle: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Speeds {
    /// avance (mm/min)
    pub feed: f64,
    /// plongée (mm/min)
    pub plunge: f64,
    pub rpm: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ToolpathKind {
    /// Découpe de contour (`cutout`)
    Cutout,
    /// Poche (`pocket_toolpath`)
    Pocket,
    /// Perçage (`drilling_toolpath`)
    Drill,
    /// V-carve (`vcarve_toolpath`, `advanced_vcarve_toolpath`)
    VCarve,
    /// Type non reconnu (nom d'origine)
    Other(String),
}

impl ToolpathKind {
    fn from_type(t: &str) -> Self {
        match t {
            "cutout" => Self::Cutout,
            "pocket_toolpath" => Self::Pocket,
            "drilling_toolpath" => Self::Drill,
            "vcarve_toolpath" | "advanced_vcarve_toolpath" => Self::VCarve,
            other => Self::Other(other.to_owned()),
        }
    }

    pub fn label(&self) -> String {
        match self {
            Self::Cutout => "Découpe".into(),
            Self::Pocket => "Poche".into(),
            Self::Drill => "Perçage".into(),
            Self::VCarve => "V-carve".into(),
            Self::Other(t) => format!("non géré ({t})"),
        }
    }
}

/// Un parcours tel que défini dans Carbide Create (paramètres, pas de trajectoire).
#[derive(Debug, Clone, PartialEq)]
pub struct Toolpath {
    pub name: String,
    pub kind: ToolpathKind,
    pub enabled: bool,
    pub tool: Tool,
    pub speeds: Speeds,
    /// Calques dont les formes sont usinées (uuid).
    pub layers: Vec<String>,
    /// Formes sélectionnées une à une (id).
    pub element_ids: Vec<String>,
    /// Profondeurs positives sous le zéro (mm).
    pub start_depth: f64,
    pub end_depth: f64,
    pub stepdown: f64,
    pub stepover: f64,
    pub stock_to_leave: f64,
    /// Perçage : distance de débourrage (0 = descente directe).
    pub peck: f64,
    /// Découpe : inverser intérieur/extérieur.
    pub flip_inside_outside: bool,
    /// Découpe : ignorer les attaches.
    pub ignore_tabs: bool,
    /// Poche : avalant (climb).
    pub climb: bool,
    /// JSON d'origine, pour les paramètres non exploités.
    pub raw: Value,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Stock {
    pub width: f64,
    pub height: f64,
    pub thickness: f64,
    /// Paramètre `zero_z` (0 = zéro Z sur le dessus du brut).
    pub zero_z: f64,
    /// Hauteur de dégagement (mm au-dessus du zéro).
    pub retract: f64,
}

/// Un fichier de l'archive interne (`sqlar`).
#[derive(Debug, Clone, PartialEq)]
pub struct Attachment {
    pub name: String,
    pub size: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Project {
    pub params: BTreeMap<String, String>,
    pub stock: Stock,
    pub machine: String,
    pub layers: Vec<Layer>,
    pub elements: Vec<Element>,
    pub toolpaths: Vec<Toolpath>,
    pub attachments: Vec<Attachment>,
    /// Remarques de lecture (objets illisibles, etc.).
    pub warnings: Vec<String>,
}

// ---------------------------------------------------------------- lecture JSON tolérante

/// Nombre écrit en nombre ou en texte (`"0.500"`) ; rejette NaN, infini et valeurs absurdes.
fn num(v: &Value, key: &str) -> Option<f64> {
    let x = match v.get(key)? {
        Value::Number(n) => n.as_f64()?,
        Value::String(s) => s.trim().parse().ok()?,
        Value::Bool(b) => f64::from(u8::from(*b)),
        _ => return None,
    };
    (x.is_finite() && x.abs() < 1.0e6).then_some(x)
}

fn text(v: &Value, key: &str) -> String {
    v.get(key).and_then(Value::as_str).unwrap_or("").to_owned()
}

fn flag(v: &Value, key: &str) -> bool {
    v.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn vec2(v: &Value) -> Option<Vec2> {
    let a = v.as_array()?;
    let x = a.first()?.as_f64()?;
    let y = a.get(1)?.as_f64()?;
    (x.is_finite() && y.is_finite()).then_some(Vec2::new(x, y))
}

fn vec2_list(v: Option<&Value>) -> Vec<Vec2> {
    v.and_then(Value::as_array).map(|a| a.iter().filter_map(vec2).collect()).unwrap_or_default()
}

/// Transformation de l'objet (matrice 3×3 de Qt, `m11 m12 m13 m21 m22 m23 dx dy m33`) puis
/// décalage `position`.
#[derive(Debug, Clone, Copy)]
struct Placement {
    m: [f64; 6],
    position: Vec2,
}

impl Placement {
    fn of(e: &Value) -> Self {
        let mut m = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
        if let Some(a) = e.get("transform").and_then(Value::as_array)
            && a.len() == 9
        {
            let g = |i: usize| a.get(i).and_then(Value::as_f64).filter(|x| x.is_finite());
            if let (Some(m11), Some(m12), Some(m21), Some(m22), Some(dx), Some(dy)) = (g(0), g(1), g(3), g(4), g(6), g(7)) {
                m = [m11, m12, m21, m22, dx, dy];
            }
        }
        let position = e.get("position").and_then(vec2).unwrap_or(Vec2::ZERO);
        Self { m, position }
    }

    fn apply(&self, p: Vec2) -> Vec2 {
        let [m11, m12, m21, m22, dx, dy] = self.m;
        Vec2::new(m11 * p.x + m21 * p.y + dx, m12 * p.x + m22 * p.y + dy) + self.position
    }
}

fn cubic(p0: Vec2, p1: Vec2, p2: Vec2, p3: Vec2, tol: f64, out: &mut Vec<Vec2>) {
    let dd = (p0 - p1 * 2.0 + p2).length().max((p1 - p2 * 2.0 + p3).length());
    let n = if dd <= 1e-12 { 1 } else { ((0.75 * dd / tol.max(1e-6)).sqrt().ceil() as usize).clamp(1, MAX_CURVE_STEPS) };
    for k in 1..=n {
        let t = k as f64 / n as f64;
        let u = 1.0 - t;
        out.push(p0 * (u * u * u) + p1 * (3.0 * u * u * t) + p2 * (3.0 * u * t * t) + p3 * (t * t * t));
    }
}

fn finish(cur: &mut Contour, out: &mut Vec<Contour>) {
    let mut c = std::mem::take(cur);
    if c.closed
        && c.points.len() > 1
        && let (Some(a), Some(b)) = (c.points.first(), c.points.last())
        && (*a - *b).length() < 1e-9
    {
        c.points.pop();
    }
    if c.points.len() >= 2 {
        out.push(c);
    }
}

/// Contours d'un objet décrit par `points` / `cp1` / `cp2` / `point_type`
/// (0 = début, 1 = ligne, 3 = Bézier cubique dont `cp1[i]`, `cp2[i]` sont les points de
/// contrôle du segment qui arrive en `points[i]`, 4 = fermeture).
fn bezier_contours(e: &Value, place: &Placement, tol: f64) -> Vec<Contour> {
    let pts = vec2_list(e.get("points"));
    let cp1 = vec2_list(e.get("cp1"));
    let cp2 = vec2_list(e.get("cp2"));
    let types: Vec<i64> =
        e.get("point_type").and_then(Value::as_array).map(|a| a.iter().map(|t| t.as_i64().unwrap_or(1)).collect()).unwrap_or_default();
    let mut out = Vec::new();
    let mut cur = Contour::default();
    let mut last: Option<Vec2> = None;
    for (i, &p) in pts.iter().enumerate() {
        let t = types.get(i).copied().unwrap_or(1);
        match (t, last) {
            (4, _) => {
                cur.closed = true;
                finish(&mut cur, &mut out);
                last = None;
                continue;
            }
            (0, _) | (_, None) => {
                finish(&mut cur, &mut out);
                cur.points.push(place.apply(p));
            }
            (3, Some(prev)) => match (cp1.get(i), cp2.get(i)) {
                (Some(&c1), Some(&c2)) => {
                    let mut local = Vec::new();
                    cubic(prev, c1, c2, p, tol, &mut local);
                    cur.points.extend(local.into_iter().map(|q| place.apply(q)));
                }
                _ => cur.points.push(place.apply(p)),
            },
            _ => cur.points.push(place.apply(p)),
        }
        last = Some(p);
    }
    finish(&mut cur, &mut out);
    out
}

/// Contours d'un texte : Carbide Create stocke les glyphes déjà convertis (`rendered`).
fn text_contours(e: &Value, place: &Placement) -> Vec<Contour> {
    let mut out = Vec::new();
    for poly in e.get("rendered").and_then(Value::as_array).into_iter().flatten() {
        let pts: Vec<Vec2> = vec2_list(Some(poly)).into_iter().map(|p| place.apply(p)).collect();
        let closed = pts.len() > 2 && pts.first().zip(pts.last()).is_some_and(|(a, b)| (*a - *b).length() < 1e-6);
        let mut c = Contour { points: pts, closed };
        finish(&mut c, &mut out);
    }
    out
}

fn element_from_json(e: &Value, tol: f64) -> Element {
    let kind = text(e, "geometryType");
    let place = Placement::of(e);
    let contours = if kind == "text" { text_contours(e, &place) } else { bezier_contours(e, &place, tol) };
    let layer = e.get("layer").cloned().unwrap_or(Value::Null);
    let radius = (kind == "circle").then(|| num(e, "radius")).flatten();
    let center = (kind == "circle").then(|| e.get("center").and_then(vec2)).flatten();
    Element {
        id: text(e, "id"),
        kind,
        layer_uuid: text(&layer, "uuid"),
        layer_name: text(&layer, "name"),
        text: e.get("text").and_then(Value::as_str).map(str::to_owned),
        center,
        radius,
        has_tabs: e.get("tabs").and_then(Value::as_array).is_some_and(|t| !t.is_empty()),
        contours,
    }
}

fn tool_from_json(t: &Value) -> Tool {
    let kind = match t.get("type").and_then(Value::as_i64).unwrap_or(0) {
        0 => ToolKind::Flat,
        1 => ToolKind::Ball,
        2 => ToolKind::VBit,
        other => ToolKind::Other(other),
    };
    Tool {
        number: t.get("number").and_then(Value::as_i64).unwrap_or(0),
        name: text(t, "name"),
        kind,
        diameter: num(t, "diameter").unwrap_or(0.0),
        angle: num(t, "angle").unwrap_or(0.0),
    }
}

fn toolpath_from_json(v: &Value) -> Toolpath {
    let type_name = text(v, "type");
    let kind = ToolpathKind::from_type(&type_name);
    let s = v.get("speeds").cloned().unwrap_or(Value::Null);
    let speeds =
        Speeds { feed: num(&s, "feedrate").unwrap_or(0.0), plunge: num(&s, "plungerate").unwrap_or(0.0), rpm: num(&s, "rpm").unwrap_or(0.0) };
    let strings = |key: &str| -> Vec<String> {
        v.get(key).and_then(Value::as_array).map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_owned)).collect()).unwrap_or_default()
    };
    let start_depth = num(v, "start_depth").unwrap_or(0.0);
    let (end_depth, stepdown) = if kind == ToolpathKind::Cutout {
        // cut_depth = épaisseur à traverser ; break_through = profondeur finale (sous le brut)
        let cut = num(v, "cut_depth").unwrap_or(0.0);
        let through = num(v, "break_through").unwrap_or(0.0);
        (cut.max(through), num(v, "depth_per_pass").unwrap_or(0.0))
    } else {
        (num(v, "end_depth").unwrap_or(0.0), num(v, "stepdown").unwrap_or(0.0))
    };
    let peck = if v.get("drill_type").and_then(Value::as_i64).unwrap_or(0) != 0 { num(v, "peck_distance").unwrap_or(0.0) } else { 0.0 };
    Toolpath {
        name: text(v, "name"),
        kind,
        enabled: v.get("enabled").and_then(Value::as_bool).unwrap_or(true),
        tool: tool_from_json(v.get("tool").unwrap_or(&Value::Null)),
        speeds,
        layers: strings("toolpath_layers"),
        element_ids: strings("elements"),
        start_depth,
        end_depth,
        stepdown,
        stepover: num(v, "stepover").unwrap_or(0.0),
        stock_to_leave: num(v, "stock_to_leave").unwrap_or(0.0),
        peck,
        flip_inside_outside: flag(v, "flip_inside_outside"),
        ignore_tabs: flag(v, "ignore_tabs"),
        climb: flag(v, "climb"),
        raw: v.clone(),
    }
}

fn inflate(data: &[u8], size: i64) -> Result<Vec<u8>, String> {
    if i64::try_from(data.len()).is_ok_and(|l| l == size) {
        return Ok(data.to_vec());
    }
    let limit = u64::try_from(size).unwrap_or(0).min(MAX_ITEM_BYTES);
    let mut out = Vec::new();
    flate2::read::ZlibDecoder::new(data).take(limit + 1).read_to_end(&mut out).map_err(|e| e.to_string())?;
    if out.len() as u64 > limit {
        return Err("objet plus grand qu'annoncé".into());
    }
    Ok(out)
}

impl Project {
    /// Ouvre un `.c2d` en lecture seule.
    pub fn open(path: &Path) -> Result<Self, C2dError> {
        Self::open_with_tolerance(path, DEFAULT_TOLERANCE)
    }

    pub fn open_with_tolerance(path: &Path, tol: f64) -> Result<Self, C2dError> {
        let shown = path.display().to_string();
        let mut head = [0u8; 16];
        std::fs::File::open(path)
            .and_then(|mut f| f.read_exact(&mut head))
            .map_err(|e| C2dError::Open { path: shown.clone(), msg: e.to_string() })?;
        if &head != b"SQLite format 3\0" {
            return Err(C2dError::NotC2d("fichier chiffré, abîmé, ou ancien format JSON (avant la V7)".into()));
        }
        let db = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX)
            .map_err(|e| C2dError::Open { path: shown, msg: e.to_string() })?;
        Self::read(&db, tol)
    }

    fn read(db: &Connection, tol: f64) -> Result<Self, C2dError> {
        let has_table = |name: &str| -> bool {
            db.query_row("SELECT count(*) FROM sqlite_master WHERE type='table' AND name=?1", [name], |r| r.get::<_, i64>(0)).unwrap_or(0) > 0
        };
        if !has_table("params") || !has_table("items") {
            return Err(C2dError::NotC2d("tables params/items absentes".into()));
        }
        let mut params = BTreeMap::new();
        let mut st = db.prepare("SELECT key, value FROM params")?;
        let rows = st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?.unwrap_or_default())))?;
        for row in rows {
            let (k, v) = row?;
            params.insert(k, v);
        }
        let p = |k: &str, d: f64| params.get(k).and_then(|v| v.trim().parse::<f64>().ok()).filter(|x| x.is_finite()).unwrap_or(d);
        let stock = Stock {
            width: p("width", 0.0),
            height: p("height", 0.0),
            thickness: p("thickness", 0.0),
            zero_z: p("zero_z", 0.0),
            retract: p("retract", 5.0),
        };
        let machine = params.get("machine").cloned().unwrap_or_default();

        let mut project = Project {
            params,
            stock,
            machine,
            layers: Vec::new(),
            elements: Vec::new(),
            toolpaths: Vec::new(),
            attachments: Vec::new(),
            warnings: Vec::new(),
        };
        let mut st = db.prepare("SELECT id, type, sz, data FROM items ORDER BY id")?;
        let rows = st.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, Option<String>>(1)?.unwrap_or_default(),
                r.get::<_, Option<i64>>(2)?.unwrap_or(0),
                r.get::<_, Option<Vec<u8>>>(3)?.unwrap_or_default(),
            ))
        })?;
        for row in rows {
            let (id, kind, size, data) = row?;
            if !matches!(kind.as_str(), "layer" | "element" | "toolpath") {
                continue;
            }
            let json = inflate(&data, size).and_then(|b| serde_json::from_slice::<Value>(&b).map_err(|e| e.to_string()));
            let v = match json {
                Ok(v) => v,
                Err(e) => {
                    project.warnings.push(format!("objet {id} ({kind}) illisible : {e}"));
                    continue;
                }
            };
            match kind.as_str() {
                "layer" => project.layers.push(Layer { uuid: text(&v, "uuid"), name: text(&v, "name"), visible: flag(&v, "visible") }),
                "element" => project.elements.push(element_from_json(&v, tol)),
                _ => project.toolpaths.push(toolpath_from_json(&v)),
            }
        }
        if has_table("sqlar") {
            let mut st = db.prepare("SELECT name, sz FROM sqlar ORDER BY name")?;
            let rows = st.query_map([], |r| Ok(Attachment { name: r.get(0)?, size: r.get::<_, Option<i64>>(1)?.unwrap_or(0) }))?;
            for row in rows {
                project.attachments.push(row?);
            }
        }
        Ok(project)
    }

    /// Le fichier contient un G-code chiffré par Carbide Create (non utilisable).
    pub fn has_encrypted_gcode(&self) -> bool {
        self.attachments.iter().any(|a| a.name.ends_with(".egc"))
    }

    pub fn layer_name(&self, uuid: &str) -> String {
        self.layers.iter().find(|l| l.uuid == uuid).map(|l| l.name.clone()).unwrap_or_else(|| uuid.to_owned())
    }

    /// Formes usinées par un parcours : celles de ses calques et celles sélectionnées une à une.
    pub fn elements_for<'a>(&'a self, tp: &Toolpath) -> Vec<&'a Element> {
        self.elements.iter().filter(|e| tp.element_ids.contains(&e.id) || (!e.layer_uuid.is_empty() && tp.layers.contains(&e.layer_uuid))).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn zlib(bytes: &[u8]) -> Vec<u8> {
        let mut enc = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        let _ = enc.write_all(bytes);
        enc.finish().unwrap_or_default()
    }

    /// Projet synthétique au format observé (aucun fichier Carbide 3D dans le dépôt).
    fn sample(path: &Path) {
        let Ok(db) = Connection::open(path) else { panic!("création impossible") };
        let sql = "CREATE TABLE params(key TEXT PRIMARY KEY, value TEXT);
            CREATE TABLE items(id INTEGER PRIMARY KEY AUTOINCREMENT, uuid TEXT UNIQUE, name TEXT, type TEXT, version TEXT, sz INT, data BLOB);
            CREATE TABLE sqlar(name TEXT PRIMARY KEY, mode INT, mtime INT, sz INT, data BLOB);";
        if let Err(e) = db.execute_batch(sql) {
            panic!("{e}");
        }
        for (k, v) in
            [("width", "100.0"), ("height", "50.0"), ("thickness", "9.0"), ("zero_z", "0.0"), ("retract", "10.0"), ("machine", "Shapeoko 3")]
        {
            let _ = db.execute("INSERT INTO params VALUES (?1, ?2)", [k, v]);
        }
        let layer = serde_json::json!({"name": "Découpe", "uuid": "{L1}", "visible": true});
        let rect = serde_json::json!({
            "geometryType": "rectangle", "id": "{R1}", "layer": layer, "position": [20, 10],
            "points": [[-5, 5], [5, 5], [5, -5], [-5, -5], [-5, 5], [-5, 5]],
            "cp1": [[-5, 5], [-5, 5], [5, 5], [5, -5], [-5, -5], [-5, 5]],
            "cp2": [[-5, 5], [5, 5], [5, -5], [-5, -5], [-5, 5], [-5, 5]],
            "point_type": [0, 1, 1, 1, 1, 4], "tabs": []
        });
        let k = 0.5522847498 * 2.0;
        let circle = serde_json::json!({
            "geometryType": "circle", "id": "{C1}", "layer": layer, "position": [50, 25], "center": [50, 25], "radius": 2.0,
            "points": [[-2, 0], [0, 2], [2, 0], [0, -2], [-2, 0], [0, 0]],
            "cp1": [[0, 0], [-2, k], [k, 2], [2, -k], [-k, -2], [0, 0]],
            "cp2": [[0, 0], [-k, 2], [2, k], [k, -2], [-2, -k], [0, 0]],
            "point_type": [0, 3, 3, 3, 3, 4]
        });
        let txt = serde_json::json!({
            "geometryType": "text", "id": "{T1}", "layer": {"name": "Gravure", "uuid": "{L2}"}, "text": "A", "position": [0, 0],
            "transform": [1, 0, 0, 0, 1, 0, 70, 20, 1], "rendered": [[[0, 0], [2, 0], [1, 3], [0, 0]]]
        });
        let tp = serde_json::json!({
            "type": "cutout", "name": "Cutout 1", "enabled": true, "cut_depth": 9, "break_through": "9.500", "depth_per_pass": 2.54,
            "speeds": {"feedrate": 800, "plungerate": 300, "rpm": 10000}, "toolpath_layers": ["{L1}"], "elements": [],
            "tool": {"type": 0, "number": 102, "name": "Fraise 1/8", "diameter": 3.175, "angle": 0}, "slot_depth_per_pass": 6.9e252
        });
        let items = [("layer", layer), ("element", rect), ("element", circle), ("element", txt), ("toolpath", tp)];
        for (i, (kind, v)) in items.iter().enumerate() {
            let raw = v.to_string().into_bytes();
            let data = if i % 2 == 0 { zlib(&raw) } else { raw.clone() };
            let _ = db.execute(
                "INSERT INTO items (uuid, name, type, version, sz, data) VALUES (?1, '', ?2, 'J1', ?3, ?4)",
                rusqlite::params![format!("u{i}"), kind, raw.len() as i64, data],
            );
        }
        let _ = db.execute("INSERT INTO sqlar VALUES ('gcode.egc', 0, 0, 4, X'4343563100')", []);
    }

    fn temp(name: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("makercraft-c2d-{}-{name}.c2d", std::process::id()));
        let _ = std::fs::remove_file(&p);
        p
    }

    #[test]
    fn reads_synthetic_project() {
        let path = temp("ok");
        sample(&path);
        let pr = match Project::open(&path) {
            Ok(p) => p,
            Err(e) => panic!("{e}"),
        };
        let _ = std::fs::remove_file(&path);
        assert_eq!(pr.stock, Stock { width: 100.0, height: 50.0, thickness: 9.0, zero_z: 0.0, retract: 10.0 });
        assert_eq!(pr.machine, "Shapeoko 3");
        assert_eq!(pr.elements.len(), 3);
        assert!(pr.has_encrypted_gcode());
        assert!(pr.warnings.is_empty(), "{:?}", pr.warnings);

        let rect = &pr.elements[0];
        assert_eq!(rect.contours.len(), 1);
        let c = &rect.contours[0];
        assert!(c.closed);
        assert_eq!(c.points.len(), 4, "{:?}", c.points);
        let b = c.bounds();
        assert_eq!((b.min, b.max), (Vec2::new(15.0, 5.0), Vec2::new(25.0, 15.0)));

        let circle = &pr.elements[1];
        assert_eq!((circle.center, circle.radius), (Some(Vec2::new(50.0, 25.0)), Some(2.0)));
        let c = &circle.contours[0];
        assert!(c.closed && c.points.len() > 8);
        assert!(c.points.iter().all(|p| ((*p - Vec2::new(50.0, 25.0)).length() - 2.0).abs() < 0.01), "cercle mal placé");
        // polygone inscrit (flèche ≤ 0,01 mm) : un peu moins que πr², à moins de 1 %
        let area = c.signed_area().abs();
        assert!(area < std::f64::consts::PI * 4.0 && area > std::f64::consts::PI * 4.0 * 0.99, "aire {area}");

        let t = &pr.elements[2];
        assert_eq!(t.contours[0].points, vec![Vec2::new(70.0, 20.0), Vec2::new(72.0, 20.0), Vec2::new(71.0, 23.0)]);

        let tp = &pr.toolpaths[0];
        assert_eq!(tp.kind, ToolpathKind::Cutout);
        assert_eq!((tp.end_depth, tp.stepdown), (9.5, 2.54));
        assert_eq!(tp.tool.kind, ToolKind::Flat);
        assert_eq!(pr.elements_for(tp).len(), 2);
    }

    #[test]
    fn rejects_non_sqlite_and_encrypted() {
        let path = temp("enc");
        let _ = std::fs::write(&path, b"CCV1\x00\x01random bytes here");
        assert!(matches!(Project::open(&path), Err(C2dError::NotC2d(_))));
        let _ = std::fs::remove_file(&path);
        assert!(matches!(Project::open(Path::new("nexiste/pas.c2d")), Err(C2dError::Open { .. })));
    }

    #[test]
    fn contour_helpers() {
        let sq = Contour { points: vec![Vec2::new(0.0, 0.0), Vec2::new(2.0, 0.0), Vec2::new(2.0, 2.0), Vec2::new(0.0, 2.0)], closed: true };
        assert_eq!(sq.signed_area(), 4.0);
        assert!(sq.contains(Vec2::new(1.0, 1.0)));
        assert!(!sq.contains(Vec2::new(3.0, 1.0)));
        assert_eq!(sq.length(), 8.0);
    }
}
