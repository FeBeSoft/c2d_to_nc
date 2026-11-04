//! Extension `cam` : commandes accessibles depuis la CLI, les scripts et plus tard l'interface.

use std::path::{Path, PathBuf};

use makercraft_c2d::Project;
use makercraft_ext_api::{CommandSpec, ExtError, Extension, Registry, Value, json};

use crate::job::{Job, job_from_c2d, tool_label};
use crate::{gcode, preview};

pub struct CamExtension;

impl Extension for CamExtension {
    fn id(&self) -> &'static str {
        "cam"
    }

    fn label(&self) -> &'static str {
        "FAO (parcours d'outil, G-code)"
    }

    fn register(&self, r: &mut Registry) -> Result<(), ExtError> {
        r.add_command(CommandSpec { id: "cam.c2d_info", label: "Infos d'un projet Carbide Create", menu: "FAO/Carbide Create", run: c2d_info })?;
        r.add_command(CommandSpec {
            id: "cam.c2d_gcode",
            label: "G-code depuis un projet Carbide Create",
            menu: "FAO/Carbide Create",
            run: c2d_gcode,
        })
    }
}

fn path_param(p: &Value) -> Result<PathBuf, ExtError> {
    p.get("path").and_then(Value::as_str).map(PathBuf::from).ok_or_else(|| ExtError::BadParams("attendu {\"path\": \"projet.c2d\"}".into()))
}

/// Ouvre un projet et calcule le travail.
pub fn load(path: &Path, only: Option<&str>) -> Result<(Project, Job), String> {
    let pr = Project::open(path).map_err(|e| e.to_string())?;
    let title = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let job = job_from_c2d(&pr, &title, only);
    Ok((pr, job))
}

/// Gravité d'une vérification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Ok,
    Info,
    Warn,
    /// Bloque l'export.
    Error,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Check {
    pub level: Level,
    pub text: String,
}

fn chk(level: Level, text: impl Into<String>) -> Check {
    Check { level, text: text.into() }
}

/// Vérifie que le projet contient ce qu'il faut pour produire un G-code utilisable.
pub fn check(pr: &Project, job: &Job) -> Vec<Check> {
    use makercraft_c2d::ToolpathKind;
    let mut v = Vec::new();
    let (w, h, t) = job.stock_size;
    if w > 0.0 && h > 0.0 && t > 0.0 {
        v.push(chk(
            Level::Ok,
            format!("Brut {w} x {h} x {t} mm{}", if job.machine.is_empty() { String::new() } else { format!(" ({})", job.machine) }),
        ));
    } else {
        v.push(chk(Level::Warn, "Dimensions du brut absentes ou nulles"));
    }
    if pr.has_encrypted_gcode() {
        v.push(chk(Level::Info, "G-code de Carbide Create chiffré : non utilisé, les parcours sont recalculés"));
    }
    if pr.toolpaths.is_empty() {
        v.push(chk(Level::Error, "Aucun parcours dans le projet : créer les parcours dans Carbide Create"));
    }
    for tp in &pr.toolpaths {
        let n = pr.elements_for(tp).len();
        let name = &tp.name;
        if !tp.enabled {
            v.push(chk(Level::Warn, format!("« {name} » désactivé : ignoré")));
            continue;
        }
        if let ToolpathKind::Other(kind) = &tp.kind {
            v.push(chk(Level::Warn, format!("« {name} » : type « {kind} » pas encore géré, ignoré")));
            continue;
        }
        let mut problems = Vec::new();
        if n == 0 {
            problems.push((Level::Warn, "aucune forme sur ses calques, ignoré".to_owned()));
        }
        if tp.tool.diameter <= 0.0 {
            problems.push((Level::Error, "diamètre d'outil nul".to_owned()));
        }
        if tp.speeds.feed <= 0.0 || tp.speeds.plunge <= 0.0 {
            problems.push((Level::Error, format!("vitesse nulle (avance {}, plongée {})", tp.speeds.feed, tp.speeds.plunge)));
        }
        if tp.speeds.rpm <= 0.0 {
            problems.push((Level::Warn, "rotation broche non renseignée (pas de M3)".to_owned()));
        }
        if tp.end_depth <= tp.start_depth {
            problems.push((Level::Error, format!("profondeur nulle ({} → {} mm)", tp.start_depth, tp.end_depth)));
        }
        if t > 0.0 && tp.end_depth > t + 1.0 {
            problems.push((Level::Warn, format!("descend {:.1} mm sous le brut (martyr)", tp.end_depth - t)));
        }
        if tp.stepdown <= 0.0 && tp.kind != ToolpathKind::Drill && tp.end_depth > tp.tool.diameter {
            problems.push((Level::Warn, format!("une seule passe de {} mm avec un outil de {} mm", tp.end_depth, tp.tool.diameter)));
        }
        if problems.is_empty() {
            v.push(chk(
                Level::Ok,
                format!("« {name} » : {} sur {n} forme(s), {} mm de profondeur, T{}", tp.kind.label(), tp.end_depth, tp.tool.number),
            ));
        }
        for (level, p) in problems {
            v.push(chk(level, format!("« {name} » : {p}")));
        }
    }
    for w in &job.warnings {
        if !v.iter().any(|c| c.text.contains(w.as_str())) {
            v.push(chk(Level::Warn, w.clone()));
        }
    }
    for g in &job.groups {
        for op in &g.ops {
            for w in &op.warnings {
                v.push(chk(Level::Warn, format!("« {} » : {w}", op.name)));
            }
        }
    }
    if job.groups.is_empty() && !pr.toolpaths.is_empty() {
        v.push(chk(Level::Error, "Aucun G-code à produire avec ce projet"));
    }
    v
}

/// Nom de base d'un fichier de sortie, sans caractère interdit par Windows.
pub fn file_stem_safe(s: &str) -> String {
    s.chars().map(|c| if c.is_alphanumeric() || " -_.+".contains(c) { c } else { '_' }).collect::<String>().trim().to_owned()
}

/// Noms des fichiers G-code qui seront écrits.
pub fn output_names(job: &Job, single: bool) -> Vec<String> {
    let base = file_stem_safe(&job.title);
    if single || job.groups.len() <= 1 {
        return vec![format!("{base}.nc")];
    }
    job.groups
        .iter()
        .enumerate()
        .map(|(i, g)| {
            let ops: Vec<&str> = g.ops.iter().map(|o| o.name.as_str()).collect();
            format!("{base} - {} - T{} - {}.nc", i + 1, g.tool.number, file_stem_safe(&ops.join(" + ")))
        })
        .collect()
}

/// Écrit les fichiers G-code (un par outil, ou un seul) dans `dir`, et l'aperçu SVG si demandé.
pub fn write_outputs(job: &Job, dir: &Path, single: bool, with_preview: bool) -> Result<Vec<PathBuf>, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{} : {e}", dir.display()))?;
    let mut files = Vec::new();
    let mut write = |name: String, content: &str| -> Result<(), String> {
        let p = dir.join(name);
        std::fs::write(&p, content).map_err(|e| format!("{} : {e}", p.display()))?;
        files.push(p);
        Ok(())
    };
    let names = output_names(job, single);
    if single || job.groups.len() <= 1 {
        if let Some(name) = names.into_iter().next() {
            write(name, &gcode::write_all(job))?;
        }
    } else {
        for (i, name) in names.into_iter().enumerate() {
            if let Some(text) = gcode::write_group(job, i) {
                write(name, &text)?;
            }
        }
    }
    if with_preview {
        write(format!("{} - apercu.svg", file_stem_safe(&job.title)), &preview::svg(job))?;
    }
    Ok(files)
}

fn job_json(job: &Job) -> Value {
    let groups: Vec<Value> = job
        .groups
        .iter()
        .map(|g| {
            let st = g.stats(job.heights);
            json!({
                "tool": tool_label(&g.tool),
                "operations": g.ops.iter().map(|o| json!({ "name": o.name, "kind": o.kind, "warnings": o.warnings })).collect::<Vec<_>>(),
                "cut_length_mm": st.cut_length.round(),
                "minutes": (st.seconds / 60.0 * 10.0).round() / 10.0,
            })
        })
        .collect();
    json!({ "title": job.title, "stock": [job.stock_size.0, job.stock_size.1, job.stock_size.2], "machine": job.machine, "groups": groups, "warnings": job.warnings })
}

fn c2d_info(p: &Value) -> Result<Value, ExtError> {
    let path = path_param(p)?;
    let (pr, job) = load(&path, None).map_err(ExtError::Failed)?;
    let mut v = job_json(&job);
    v["elements"] = json!(pr.elements.len());
    v["layers"] = json!(pr.layers.iter().map(|l| l.name.clone()).collect::<Vec<_>>());
    v["encrypted_gcode_ignored"] = json!(pr.has_encrypted_gcode());
    Ok(v)
}

fn c2d_gcode(p: &Value) -> Result<Value, ExtError> {
    let path = path_param(p)?;
    let only = p.get("only").and_then(Value::as_str);
    let single = p.get("single").and_then(Value::as_bool).unwrap_or(false);
    let (_, job) = load(&path, only).map_err(ExtError::Failed)?;
    let dir = p.get("out").and_then(Value::as_str).map(PathBuf::from).unwrap_or_else(|| path.parent().map(Path::to_path_buf).unwrap_or_default());
    let preview = p.get("preview").and_then(Value::as_bool).unwrap_or(true);
    let files = write_outputs(&job, &dir, single, preview).map_err(ExtError::Failed)?;
    let mut v = job_json(&job);
    v["files"] = json!(files.iter().map(|f| f.display().to_string()).collect::<Vec<_>>());
    Ok(v)
}
