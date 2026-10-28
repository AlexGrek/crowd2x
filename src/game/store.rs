//! Reading `store.yaml` for a new game: the rules the store's superfridge is
//! run by ([`StoreRules`]). The simulation owns the rules and their defaults;
//! this only finds the file.

use crate::sim::superfridge::StoreRules;

/// Read on a new game. `CROWD2X_STORE` names the file, or `store.yaml`. A
/// missing file is the built-in defaults; a malformed or unreadable one is
/// too, with a line saying why it was ignored.
pub fn load_rules() -> (StoreRules, Option<String>) {
    let path = std::env::var_os("CROWD2X_STORE").map(std::path::PathBuf::from).unwrap_or_else(|| "store.yaml".into());
    let result = match std::fs::read_to_string(&path) {
        Ok(text) => StoreRules::from_yaml(&text),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return (StoreRules::default(), None),
        Err(error) => Err(error.to_string()),
    };
    match result {
        Ok(rules) => (rules, None),
        Err(error) => (StoreRules::default(), Some(format!("{}: {error}; using built-in store defaults", path.display()))),
    }
}
