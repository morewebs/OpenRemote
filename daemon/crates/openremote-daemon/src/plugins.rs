//! Plugin catalog. The curated marketplace was a prototype list of launch
//! commands that were never installed for real - it is empty until a catalog
//! is facts. A plugin is written by hand; a key never crosses this API.

use serde::Serialize;

#[derive(Clone, Serialize, Debug)]
pub struct CatalogEntry {
    pub id: String,
    pub name: String,
    pub detail: String,
    pub needs_key: bool,
    /// The launch command, as that machine would start it.
    pub command: String,
}

/// No curated catalog. Plugins are written by hand.
pub fn marketplace() -> Vec<CatalogEntry> {
    Vec::new()
}

pub fn catalog_entry(id: &str) -> Option<CatalogEntry> {
    marketplace().into_iter().find(|e| e.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_catalog_is_empty() {
        assert!(marketplace().is_empty());
        assert!(catalog_entry("github").is_none());
    }
}
