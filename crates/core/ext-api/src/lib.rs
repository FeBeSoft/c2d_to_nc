//! Contrat d'extension de MakerCraft.
//!
//! Le noyau ne connaît aucun métier. Chaque métier (dao, cao, cam, machine…) est une
//! [`Extension`] qui enregistre ses commandes dans le [`Registry`]. Toute action visible par
//! l'utilisateur est une commande identifiée par `extension.action` (ex. `machine.jog`) et
//! appelée avec des paramètres JSON : menus, ligne de commande, scripts, CLI et MCP passent
//! tous par là (même principe que CadCraft et que les plugins WASM prévus par SolidCraft).
//!
//! Étapes suivantes prévues : contexte d'exécution (document actif, undo), registres de types
//! de documents, d'importeurs/exporteurs, de panneaux et de fournisseurs de géométrie.

use std::collections::BTreeMap;

pub use serde_json::{Value, json};

/// Erreurs du système d'extensions.
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum ExtError {
    #[error("extension déjà chargée : {0}")]
    DuplicateExtension(String),
    #[error("commande déjà enregistrée : {0}")]
    DuplicateCommand(String),
    #[error("identifiant de commande invalide « {id} » : il doit commencer par « {ext}. »")]
    BadCommandId { ext: String, id: String },
    #[error("l'extension {ext} requiert {missing}, absente ou en dépendance circulaire")]
    MissingDependency { ext: String, missing: String },
    #[error("commande inconnue : {0}")]
    UnknownCommand(String),
    #[error("paramètre invalide : {0}")]
    BadParams(String),
    #[error("{0}")]
    Failed(String),
}

/// Fonction exécutée par une commande : paramètres JSON en entrée, résultat JSON en sortie.
pub type CommandFn = fn(&Value) -> Result<Value, ExtError>;

/// Description d'une commande.
#[derive(Clone)]
pub struct CommandSpec {
    /// Identifiant unique `extension.action`.
    pub id: &'static str,
    /// Libellé affiché dans les menus.
    pub label: &'static str,
    /// Chemin de menu, ex. `Machine/Connexion`.
    pub menu: &'static str,
    pub run: CommandFn,
}

impl std::fmt::Debug for CommandSpec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CommandSpec").field("id", &self.id).field("label", &self.label).field("menu", &self.menu).finish()
    }
}

/// Registre des commandes, trié par identifiant.
#[derive(Default, Debug)]
pub struct CommandRegistry {
    map: BTreeMap<&'static str, CommandSpec>,
}

impl CommandRegistry {
    pub fn get(&self, id: &str) -> Option<&CommandSpec> {
        self.map.get(id)
    }

    pub fn iter(&self) -> impl Iterator<Item = &CommandSpec> {
        self.map.values()
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// Exécute une commande. Une commande ne doit jamais paniquer ; ses erreurs remontent en `Err`.
    pub fn run(&self, id: &str, params: &Value) -> Result<Value, ExtError> {
        let spec = self.get(id).ok_or_else(|| ExtError::UnknownCommand(id.to_owned()))?;
        (spec.run)(params)
    }
}

/// Ce qu'une extension peut enrichir. Une extension n'y écrit que pendant son `register`.
#[derive(Default, Debug)]
pub struct Registry {
    commands: CommandRegistry,
    current_ext: Option<&'static str>,
}

impl Registry {
    /// Ajoute une commande de l'extension en cours d'enregistrement.
    pub fn add_command(&mut self, spec: CommandSpec) -> Result<(), ExtError> {
        if let Some(ext) = self.current_ext {
            let prefix_ok = spec.id.strip_prefix(ext).is_some_and(|rest| rest.starts_with('.') && rest.len() > 1);
            if !prefix_ok {
                return Err(ExtError::BadCommandId { ext: ext.to_owned(), id: spec.id.to_owned() });
            }
        }
        if self.commands.map.contains_key(spec.id) {
            return Err(ExtError::DuplicateCommand(spec.id.to_owned()));
        }
        self.commands.map.insert(spec.id, spec);
        Ok(())
    }

    pub fn commands(&self) -> &CommandRegistry {
        &self.commands
    }
}

/// Une extension métier.
pub trait Extension: Send + Sync {
    /// Identifiant court et stable : `dao`, `cao`, `cam`, `machine`…
    fn id(&self) -> &'static str;
    /// Nom affiché.
    fn label(&self) -> &'static str;
    /// Extensions qui doivent être chargées avant celle-ci.
    fn depends_on(&self) -> &'static [&'static str] {
        &[]
    }
    /// Enregistre commandes, panneaux, formats… dans le registre.
    fn register(&self, registry: &mut Registry) -> Result<(), ExtError>;
}

/// Hôte : charge les extensions dans l'ordre de leurs dépendances.
#[derive(Default)]
pub struct Host {
    registry: Registry,
    loaded: Vec<(&'static str, &'static str)>,
}

impl Host {
    pub fn new() -> Self {
        Self::default()
    }

    /// Charge un lot d'extensions. Leur ordre dans le lot n'a pas d'importance.
    /// En cas d'échec d'une extension, celles déjà chargées restent disponibles.
    pub fn load(&mut self, mut pending: Vec<Box<dyn Extension>>) -> Result<(), ExtError> {
        while !pending.is_empty() {
            let ready = pending.iter().position(|e| e.depends_on().iter().all(|d| self.is_loaded(d)));
            let Some(index) = ready else {
                // Aucune extension prête : dépendance absente ou circulaire.
                let ext = &pending[0];
                let missing = ext.depends_on().iter().find(|d| !self.is_loaded(d)).copied().unwrap_or("?");
                return Err(ExtError::MissingDependency { ext: ext.id().to_owned(), missing: missing.to_owned() });
            };
            let ext = pending.remove(index);
            if self.is_loaded(ext.id()) {
                return Err(ExtError::DuplicateExtension(ext.id().to_owned()));
            }
            self.registry.current_ext = Some(ext.id());
            let result = ext.register(&mut self.registry);
            self.registry.current_ext = None;
            result?;
            self.loaded.push((ext.id(), ext.label()));
        }
        Ok(())
    }

    pub fn is_loaded(&self, id: &str) -> bool {
        self.loaded.iter().any(|(i, _)| *i == id)
    }

    /// Extensions chargées, dans l'ordre de chargement : (identifiant, libellé).
    pub fn extensions(&self) -> &[(&'static str, &'static str)] {
        &self.loaded
    }

    pub fn commands(&self) -> &CommandRegistry {
        self.registry.commands()
    }

    pub fn run(&self, id: &str, params: &Value) -> Result<Value, ExtError> {
        self.registry.commands().run(id, params)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Ext {
        id: &'static str,
        deps: &'static [&'static str],
        cmd: &'static str,
    }

    impl Extension for Ext {
        fn id(&self) -> &'static str {
            self.id
        }
        fn label(&self) -> &'static str {
            "test"
        }
        fn depends_on(&self) -> &'static [&'static str] {
            self.deps
        }
        fn register(&self, r: &mut Registry) -> Result<(), ExtError> {
            r.add_command(CommandSpec { id: self.cmd, label: "t", menu: "Test", run: |p| Ok(json!({ "echo": p })) })
        }
    }

    fn ext(id: &'static str, deps: &'static [&'static str], cmd: &'static str) -> Box<dyn Extension> {
        Box::new(Ext { id, deps, cmd })
    }

    #[test]
    fn loads_in_dependency_order() {
        let mut host = Host::new();
        let r = host.load(vec![ext("cam", &["dao"], "cam.run"), ext("dao", &[], "dao.line")]);
        assert_eq!(r, Ok(()));
        assert_eq!(host.extensions().iter().map(|e| e.0).collect::<Vec<_>>(), ["dao", "cam"]);
        assert_eq!(host.run("cam.run", &json!(1)), Ok(json!({ "echo": 1 })));
    }

    #[test]
    fn reports_missing_and_circular_dependencies() {
        let mut host = Host::new();
        assert!(matches!(host.load(vec![ext("cam", &["dao"], "cam.run")]), Err(ExtError::MissingDependency { .. })));
        let mut host = Host::new();
        let r = host.load(vec![ext("a", &["b"], "a.x"), ext("b", &["a"], "b.x")]);
        assert!(matches!(r, Err(ExtError::MissingDependency { .. })));
    }

    #[test]
    fn rejects_duplicates_and_foreign_ids() {
        let mut host = Host::new();
        assert!(matches!(host.load(vec![ext("dao", &[], "dao.a"), ext("dao", &[], "dao.b")]), Err(ExtError::DuplicateExtension(_))));
        let mut host = Host::new();
        assert!(matches!(host.load(vec![ext("dao", &[], "cam.run")]), Err(ExtError::BadCommandId { .. })));
        let mut host = Host::new();
        assert!(matches!(host.load(vec![ext("dao", &[], "dao.")]), Err(ExtError::BadCommandId { .. })));
        assert_eq!(host.run("nope", &Value::Null), Err(ExtError::UnknownCommand("nope".into())));
    }
}
