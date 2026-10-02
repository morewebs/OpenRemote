//! The plugin marketplace: curated MCP servers, each one a plain launch
//! command (the server's own, verified on npm/PyPI — facts, not code). A
//! key, where one is needed, never crosses this API — it stays on the
//! machine the plugin runs on.

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

/// The catalog the prototype drew: the generic servers a session can call.
pub fn marketplace() -> Vec<CatalogEntry> {
    [
        (
            "github",
            "GitHub",
            "Issues, pull requests, and checks.",
            false,
            "npx -y @modelcontextprotocol/server-github",
        ),
        (
            "gitlab",
            "GitLab",
            "Merge requests and pipelines.",
            true,
            "npx -y @modelcontextprotocol/server-gitlab",
        ),
        (
            "linear",
            "Linear",
            "Issues and project status.",
            true,
            "npx -y mcp-server-linear",
        ),
        (
            "sentry",
            "Sentry",
            "Production errors.",
            true,
            "npx -y @sentry/mcp-server",
        ),
        (
            "postgres",
            "Postgres",
            "Read-only SQL against the app database.",
            true,
            "npx -y @modelcontextprotocol/server-postgres",
        ),
        (
            "sqlite",
            "SQLite",
            "Query a database file on that machine.",
            false,
            "uvx mcp-server-sqlite",
        ),
        (
            "browser",
            "Browser",
            "Open local pages from a session.",
            false,
            "npx -y @playwright/mcp",
        ),
    ]
    .iter()
    .map(|(id, name, detail, needs_key, command)| CatalogEntry {
        id: id.to_string(),
        name: name.to_string(),
        detail: detail.to_string(),
        needs_key: *needs_key,
        command: command.to_string(),
    })
    .collect()
}

pub fn catalog_entry(id: &str) -> Option<CatalogEntry> {
    marketplace().into_iter().find(|e| e.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_catalog_ids_are_unique() {
        let entries = marketplace();
        let mut ids: Vec<&str> = entries.iter().map(|e| e.id.as_str()).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), entries.len());
    }

    #[test]
    fn every_launch_command_has_a_program() {
        for entry in marketplace() {
            assert!(
                entry.command.split_whitespace().next().is_some(),
                "{} has no command",
                entry.name
            );
        }
    }
}
