use rusqlite::types::ValueRef;
use rusqlite::{Connection, Params};
use serde_json::{Map, Value};
use std::path::Path;
use std::time::Duration;

const SCHEMA: &str = "
    CREATE TABLE IF NOT EXISTS notifications (
        id TEXT PRIMARY KEY,
        service TEXT NOT NULL,
        title TEXT NOT NULL,
        body TEXT DEFAULT '',
        priority TEXT DEFAULT 'medium' CHECK(priority IN ('low','medium','high')),
        metadata TEXT DEFAULT '{}',
        created_at TEXT NOT NULL
    );
    CREATE INDEX IF NOT EXISTS idx_service ON notifications(service);
    CREATE INDEX IF NOT EXISTS idx_created_at ON notifications(created_at DESC);
    CREATE TABLE IF NOT EXISTS push_devices (
        installation_id TEXT PRIMARY KEY,
        token TEXT NOT NULL,
        topic TEXT NOT NULL,
        environment TEXT NOT NULL,
        version INTEGER NOT NULL DEFAULT 1,
        active INTEGER NOT NULL DEFAULT 1,
        updated_at INTEGER NOT NULL,
        UNIQUE(token, topic, environment)
    );
    CREATE TABLE IF NOT EXISTS push_jobs (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        notification_id TEXT NOT NULL,
        installation_id TEXT NOT NULL,
        device_version INTEGER NOT NULL,
        state TEXT NOT NULL DEFAULT 'pending',
        attempts INTEGER NOT NULL DEFAULT 0,
        next_attempt_at INTEGER NOT NULL,
        expires_at INTEGER NOT NULL,
        last_error TEXT,
        UNIQUE(notification_id, installation_id)
    );
    CREATE INDEX IF NOT EXISTS idx_push_due ON push_jobs(state, next_attempt_at);
";

pub fn open(path: &str) -> Result<Connection, Box<dyn std::error::Error + Send + Sync>> {
    if let Some(dir) = Path::new(path).parent() {
        if !dir.as_os_str().is_empty() && !dir.is_dir() {
            std::fs::create_dir_all(dir)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
            }
        }
    }
    let conn = Connection::open(path)?;
    conn.query_row("PRAGMA journal_mode=WAL", [], |_| Ok(()))?;
    conn.busy_timeout(Duration::from_secs(5))?;
    conn.execute_batch(SCHEMA)?;
    Ok(conn)
}

/// Runs a query and returns every row as a JSON object keyed by column name
/// (like PDO::FETCH_ASSOC with PHP 8.1 native int types).
pub fn query_json<P: Params>(conn: &Connection, sql: &str, params: P) -> rusqlite::Result<Vec<Map<String, Value>>> {
    let mut stmt = conn.prepare(sql)?;
    let names: Vec<String> = stmt.column_names().into_iter().map(String::from).collect();
    let mut rows = stmt.query(params)?;
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        let mut obj = Map::new();
        for (i, name) in names.iter().enumerate() {
            let v = match row.get_ref(i)? {
                ValueRef::Null => Value::Null,
                ValueRef::Integer(n) => Value::from(n),
                ValueRef::Real(f) => Value::from(f),
                ValueRef::Text(t) | ValueRef::Blob(t) => Value::from(String::from_utf8_lossy(t).into_owned()),
            };
            obj.insert(name.clone(), v);
        }
        out.push(obj);
    }
    Ok(out)
}
