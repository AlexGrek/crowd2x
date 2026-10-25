//! Reading `relationships.yaml` for a new game: the rules relationships are
//! run by ([`Rules`]). The simulation owns the rules and their defaults; this
//! only finds the file.

use crate::sim::relationships::Rules;

/// Read on a new game. `CROWD2X_RELATIONSHIPS` names the file, or
/// `relationships.yaml`. A missing file is the built-in defaults; a malformed
/// or unreadable one is too, with a line saying why it was ignored.
pub fn load_rules() -> (Rules, Option<String>) {
    let path = std::env::var_os("CROWD2X_RELATIONSHIPS")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| "relationships.yaml".into());
    let result = match std::fs::read_to_string(&path) {
        Ok(text) => Rules::from_yaml(&text),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return (Rules::default(), None),
        Err(error) => Err(error.to_string()),
    };
    match result {
        Ok(rules) => (rules, None),
        Err(error) => (Rules::default(), Some(format!("{}: {error}; using built-in relationship defaults", path.display()))),
    }
}
