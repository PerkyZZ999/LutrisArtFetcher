/// Lutris `SQLite` database reader.
///
/// Reads the `games` table from Lutris' `pga.db` to discover installed games.
/// All database work is synchronous — we read everything into memory and drop
/// the connection before any async work begins (rusqlite `Connection` is not `Send`).
use std::path::Path;

use color_eyre::eyre::{Context, Result, eyre};
use rusqlite::Connection;

/// A game entry read from the Lutris database.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct Game {
    pub id: i64,
    pub name: String,
    pub slug: String,
    pub runner: Option<String>,
    pub platform: Option<String>,
    pub service: Option<String>,
    pub service_id: Option<String>,
    pub has_custom_banner: bool,
    pub has_custom_coverart: bool,
}

/// Validate that the Lutris database file exists and is readable.
///
/// # Errors
///
/// Returns an error with a user-friendly message if the file is missing or inaccessible.
pub fn validate_db(path: &Path) -> Result<()> {
    if !path.exists() {
        return Err(eyre!(
            "Lutris database not found at {}\nIs Lutris installed?",
            path.display()
        ));
    }

    if !path.is_file() {
        return Err(eyre!(
            "{} exists but is not a regular file",
            path.display()
        ));
    }

    // Verify we can actually read it
    std::fs::metadata(path)
        .wrap_err_with(|| format!("Cannot read metadata for {}", path.display()))?;

    Ok(())
}

/// Read all installed games from the Lutris database, sorted alphabetically by name.
///
/// # Errors
///
/// Returns an error if the database cannot be opened or the query fails.
pub fn get_installed_games(path: &Path) -> Result<Vec<Game>> {
    let conn = Connection::open(path)
        .wrap_err_with(|| format!("Failed to open Lutris database at {}", path.display()))?;

    // Discover available columns to handle schema variations gracefully
    let has_coverart_big = table_has_column(&conn, "games", "has_custom_coverart_big");

    let coverart_col = if has_coverart_big {
        "has_custom_coverart_big"
    } else {
        "0" // default to false if column doesn't exist
    };

    let query = format!(
        "SELECT id, name, slug, runner, platform, service, service_id, \
         COALESCE(has_custom_banner, 0), COALESCE({coverart_col}, 0) \
         FROM games \
         WHERE installed = 1 \
         ORDER BY name COLLATE NOCASE"
    );

    let mut stmt = conn.prepare(&query)
        .wrap_err("Failed to prepare games query")?;

    let games = stmt
        .query_map([], |row| {
            Ok(Game {
                id: row.get(0)?,
                name: row.get(1)?,
                slug: row.get(2)?,
                runner: row.get(3)?,
                platform: row.get(4)?,
                service: row.get(5)?,
                service_id: row.get(6)?,
                has_custom_banner: row.get::<_, i64>(7)? != 0,
                has_custom_coverart: row.get::<_, i64>(8)? != 0,
            })
        })
        .wrap_err("Failed to query installed games")?
        .collect::<Result<Vec<_>, _>>()
        .wrap_err("Failed to read game row")?;

    Ok(games)
}

/// Check whether a table has a specific column (for schema compatibility).
fn table_has_column(conn: &Connection, table: &str, column: &str) -> bool {
    let query = format!("PRAGMA table_info({table})");
    let Ok(mut stmt) = conn.prepare(&query) else {
        return false;
    };

    let Ok(rows) = stmt.query_map([], |row| row.get::<_, String>(1)) else {
        return false;
    };

    let columns: Vec<String> = rows.filter_map(Result::ok).collect();
    columns.iter().any(|col_name| col_name == column)
}
