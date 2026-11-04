//! Écriture du G-code pour Grbl 1.1 / grblHAL.
//!
//! **ASCII strict** : Grbl 1.1 traite tout octet 0x80-0xFF comme une commande temps réel, même
//! dans un commentaire (0x94 = avance -1 %, 0xA0 = arrosage…). Les accents des commentaires sont
//! donc translittérés, et les parenthèses internes remplacées (Grbl n'imbrique pas les
//! commentaires). Pas de G2/G3 pour l'instant : arcs en segments (comme Carbide Create).

use makercraft_geom::Vec3;

use crate::job::{Job, ToolGroup, tool_label};
use crate::{Move, Step};

/// Texte → ASCII imprimable sûr pour un commentaire Grbl.
pub fn ascii(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        let r: &str = match c {
            'à' | 'â' | 'ä' | 'á' | 'ã' | 'å' => "a",
            'À' | 'Â' | 'Ä' | 'Á' | 'Ã' | 'Å' => "A",
            'é' | 'è' | 'ê' | 'ë' => "e",
            'É' | 'È' | 'Ê' | 'Ë' => "E",
            'î' | 'ï' | 'í' | 'ì' => "i",
            'Î' | 'Ï' | 'Í' | 'Ì' => "I",
            'ô' | 'ö' | 'ó' | 'ò' | 'õ' => "o",
            'Ô' | 'Ö' | 'Ó' | 'Ò' | 'Õ' => "O",
            'ù' | 'û' | 'ü' | 'ú' => "u",
            'Ù' | 'Û' | 'Ü' | 'Ú' => "U",
            'ç' => "c",
            'Ç' => "C",
            'œ' => "oe",
            'Œ' => "OE",
            'æ' => "ae",
            'ø' | 'Ø' => "D",
            '°' => " deg",
            '«' | '»' | '“' | '”' => "\"",
            '‘' | '’' => "'",
            '–' | '—' => "-",
            '×' => "x",
            '(' | '[' => "[",
            ')' | ']' => "]",
            ';' => ",",
            c if c.is_ascii() && !c.is_ascii_control() => {
                out.push(c);
                continue;
            }
            c if c.is_whitespace() => " ",
            _ => "?",
        };
        out.push_str(r);
    }
    out
}

/// Nombre court : 3 décimales au plus, sans zéros inutiles ni « -0 ».
pub fn num(v: f64) -> String {
    let s = format!("{v:.3}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() || s == "-0" { "0".to_owned() } else { s.to_owned() }
}

struct Writer {
    out: String,
    /// Position connue axe par axe (inconnue au démarrage du fichier).
    axes: [Option<f64>; 3],
    motion: u8,
    feed: Option<f64>,
}

impl Writer {
    fn new() -> Self {
        Self { out: String::new(), axes: [None; 3], motion: 255, feed: None }
    }

    /// Monte en Z seul (aucun déplacement XY tant que l'outil peut être dans la matière).
    fn rapid_z(&mut self, z: f64) {
        if self.axes[2].is_some_and(|o| num(o) == num(z)) {
            return;
        }
        let g = if self.motion == 0 { "" } else { "G0 " };
        self.line(&format!("{g}Z{}", num(z)));
        self.motion = 0;
        self.axes[2] = Some(z);
    }

    fn line(&mut self, s: &str) {
        self.out.push_str(s);
        self.out.push('\n');
    }

    fn comment(&mut self, s: &str) {
        let c = ascii(s);
        self.line(&format!("({})", c.trim()));
    }

    fn mv(&mut self, m: &Move) {
        let (code, p, f) = match m {
            Move::Rapid(p) => (0u8, *p, None),
            Move::Feed(p, f) => (1u8, *p, Some(*f)),
        };
        let mut words = Vec::new();
        for (i, (axis, v)) in [('X', p.x), ('Y', p.y), ('Z', p.z)].into_iter().enumerate() {
            if self.axes[i].is_none_or(|o| num(o) != num(v)) {
                words.push(format!("{axis}{}", num(v)));
            }
            self.axes[i] = Some(v);
        }
        if words.is_empty() {
            return; // rien ne bouge : rien à écrire, le mode modal reste inchangé
        }
        if let Some(f) = f
            && self.feed.is_none_or(|o| (o - f).abs() > 1e-6)
        {
            words.push(format!("F{}", num(f.max(1.0))));
            self.feed = Some(f);
        }
        if self.motion != code {
            words.insert(0, format!("G{code}"));
            self.motion = code;
        }
        self.line(&words.join(" "));
    }
}

fn header(w: &mut Writer, job: &Job, what: &str) {
    let (sx, sy, sz) = job.stock_size;
    w.comment(&format!("MakerCraft - {}", job.title));
    w.comment(what);
    w.comment(&format!("Brut {} x {} x {} mm - X0 Y0 = coin avant gauche, Z0 = dessus du brut", num(sx), num(sy), num(sz)));
    if !job.machine.is_empty() {
        w.comment(&format!("Machine du projet : {}", job.machine));
    }
    w.line("G90 G94 G17");
    w.line("G21");
}

fn group_body(w: &mut Writer, job: &Job, g: &ToolGroup) {
    let safe = job.heights.safe_z;
    w.rapid_z(safe);
    let mut rpm = None;
    for op in &g.ops {
        w.comment(&format!("--- {} : {} ---", op.kind, op.name));
        for warn in &op.warnings {
            w.comment(&format!("ATTENTION : {warn}"));
        }
        if rpm != Some(op.rpm) && op.rpm > 0.0 {
            w.line(&format!("M3 S{}", num(op.rpm)));
            if rpm.is_none() {
                w.line("G4 P2");
            }
            rpm = Some(op.rpm);
        }
        for s in &op.steps {
            match s {
                Step::Comment(c) => w.comment(c),
                Step::Move(m) => w.mv(m),
            }
        }
    }
    w.rapid_z(safe);
    w.line("M5");
}

/// Un fichier pour un groupe d'outil (à lancer après avoir fait le zéro avec CET outil).
pub fn write_group(job: &Job, index: usize) -> Option<String> {
    let g = job.groups.get(index)?;
    let mut w = Writer::new();
    header(&mut w, job, &format!("Fichier {}/{} - outil {}", index + 1, job.groups.len(), tool_label(&g.tool)));
    w.comment("Monter cet outil et faire le zero X Y Z avant de lancer");
    group_body(&mut w, job, g);
    w.mv(&Move::Rapid(Vec3::new(0.0, 0.0, job.heights.safe_z)));
    w.line("M2");
    Some(w.out)
}

/// Un seul fichier : pause (M0) à chaque changement d'outil.
pub fn write_all(job: &Job) -> String {
    let mut w = Writer::new();
    header(&mut w, job, &format!("{} outil(s), pause M0 a chaque changement", job.groups.len()));
    for (i, g) in job.groups.iter().enumerate() {
        if i > 0 {
            w.rapid_z(job.heights.safe_z);
            w.mv(&Move::Rapid(Vec3::new(0.0, 0.0, job.heights.safe_z)));
            w.comment(&format!("CHANGEMENT D'OUTIL : monter {}, refaire le zero Z, puis reprendre", tool_label(&g.tool)));
            w.line("M0");
        } else {
            w.comment(&format!("Outil : {}", tool_label(&g.tool)));
        }
        group_body(&mut w, job, g);
    }
    w.mv(&Move::Rapid(Vec3::new(0.0, 0.0, job.heights.safe_z)));
    w.line("M2");
    w.out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Heights, Operation};
    use makercraft_c2d::{Tool, ToolKind};

    #[test]
    fn ascii_is_safe_for_grbl() {
        let s = ascii("Outil 2 - Fraise plate (Découpe + Perçage) — Ø3,175 à 30°");
        assert!(s.bytes().all(|b| b.is_ascii() && b >= 0x20), "{s}");
        assert!(!s.contains('(') && !s.contains(')'));
        assert_eq!(ascii("é—à"), "e-a");
    }

    #[test]
    fn numbers() {
        assert_eq!(num(10.0), "10");
        assert_eq!(num(-2.54), "-2.54");
        assert_eq!(num(-0.0001), "0");
        assert_eq!(num(1.23456), "1.235");
    }

    fn job() -> Job {
        let tool = Tool { number: 102, name: "Fraise (Découpe)".into(), kind: ToolKind::Flat, diameter: 3.175, angle: 0.0 };
        let steps = vec![
            Step::Move(Move::Rapid(Vec3::new(5.0, 5.0, 10.0))),
            Step::Move(Move::Rapid(Vec3::new(5.0, 5.0, 1.0))),
            Step::Move(Move::Feed(Vec3::new(5.0, 5.0, -2.0), 300.0)),
            Step::Move(Move::Feed(Vec3::new(15.0, 5.0, -2.0), 800.0)),
            Step::Move(Move::Feed(Vec3::new(15.0, 9.0, -2.0), 800.0)),
            Step::Move(Move::Rapid(Vec3::new(15.0, 9.0, 10.0))),
        ];
        let op = Operation { name: "Cutout 1".into(), kind: "Découpe".into(), rpm: 10000.0, steps, warnings: vec!["à vérifier".into()] };
        Job {
            title: "Planche — essai".into(),
            stock_size: (290.0, 170.0, 9.0),
            machine: "Shapeoko 3".into(),
            heights: Heights { safe_z: 10.0, clear_z: 1.0 },
            groups: vec![ToolGroup { tool: tool.clone(), ops: vec![op.clone()] }, ToolGroup { tool, ops: vec![op] }],
            design: vec![],
            warnings: vec![],
        }
    }

    #[test]
    fn group_file_is_minimal_and_ascii() {
        let Some(g) = write_group(&job(), 0) else { panic!("groupe absent") };
        assert!(g.bytes().all(|b| b.is_ascii()), "{g}");
        let lines: Vec<&str> = g.lines().filter(|l| !l.starts_with('(')).collect();
        assert_eq!(
            lines,
            ["G90 G94 G17", "G21", "G0 Z10", "M3 S10000", "G4 P2", "X5 Y5", "Z1", "G1 Z-2 F300", "X15 F800", "Y9", "G0 Z10", "M5", "X0 Y0", "M2"]
        );
    }

    #[test]
    fn combined_file_pauses_between_tools() {
        let all = write_all(&job());
        assert_eq!(all.matches("M0\n").count(), 1);
        assert!(all.contains("CHANGEMENT D'OUTIL"));
        assert!(all.bytes().all(|b| b.is_ascii()));
    }
}
