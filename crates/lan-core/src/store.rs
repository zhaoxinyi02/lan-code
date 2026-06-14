use std::{
    path::Path,
    sync::{Arc, Mutex},
};

use anyhow::{Context, Result};
use lan_protocol::{CoreEvent, Session, SessionId};
use rusqlite::{Connection, params};

use crate::ModelMessage;

#[derive(Clone)]
pub struct SqliteStore {
    connection: Arc<Mutex<Connection>>,
}

impl SqliteStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let connection = Connection::open(path)?;
        connection.execute_batch(
            "
            PRAGMA journal_mode = WAL;
            PRAGMA foreign_keys = ON;
            CREATE TABLE IF NOT EXISTS sessions (
                id TEXT PRIMARY KEY,
                data TEXT NOT NULL,
                messages TEXT NOT NULL,
                updated_at INTEGER NOT NULL DEFAULT (unixepoch())
            );
            CREATE TABLE IF NOT EXISTS events (
                sequence INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL,
                data TEXT NOT NULL,
                created_at INTEGER NOT NULL DEFAULT (unixepoch())
            );
            CREATE INDEX IF NOT EXISTS events_session_sequence
                ON events(session_id, sequence);
            ",
        )?;
        Ok(Self {
            connection: Arc::new(Mutex::new(connection)),
        })
    }

    pub fn save_session(&self, session: &Session, messages: &[ModelMessage]) -> Result<()> {
        self.connection
            .lock()
            .expect("sqlite lock poisoned")
            .execute(
                "INSERT INTO sessions(id, data, messages) VALUES (?1, ?2, ?3)
                 ON CONFLICT(id) DO UPDATE SET
                   data = excluded.data,
                   messages = excluded.messages,
                   updated_at = unixepoch()",
                params![
                    session.id.to_string(),
                    serde_json::to_string(session)?,
                    serde_json::to_string(messages)?
                ],
            )?;
        Ok(())
    }

    pub fn load_sessions(&self) -> Result<Vec<(Session, Vec<ModelMessage>)>> {
        let connection = self.connection.lock().expect("sqlite lock poisoned");
        let mut statement = connection.prepare("SELECT data, messages FROM sessions")?;
        let rows = statement.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        rows.map(|row| {
            let (session, messages) = row?;
            Ok((
                serde_json::from_str(&session).context("invalid stored session")?,
                serde_json::from_str(&messages).context("invalid stored messages")?,
            ))
        })
        .collect()
    }

    pub fn delete_session(&self, session_id: SessionId) -> Result<()> {
        let mut connection = self.connection.lock().expect("sqlite lock poisoned");
        let transaction = connection.transaction()?;
        transaction.execute(
            "DELETE FROM events WHERE session_id = ?1",
            params![session_id.to_string()],
        )?;
        transaction.execute(
            "DELETE FROM sessions WHERE id = ?1",
            params![session_id.to_string()],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn append_event(&self, session_id: SessionId, event: &CoreEvent) -> Result<()> {
        self.connection
            .lock()
            .expect("sqlite lock poisoned")
            .execute(
                "INSERT INTO events(session_id, data) VALUES (?1, ?2)",
                params![session_id.to_string(), serde_json::to_string(event)?],
            )?;
        Ok(())
    }

    pub fn load_events(&self, session_id: SessionId) -> Result<Vec<CoreEvent>> {
        let connection = self.connection.lock().expect("sqlite lock poisoned");
        let mut statement = connection
            .prepare("SELECT data FROM events WHERE session_id = ?1 ORDER BY sequence")?;
        let rows = statement.query_map(params![session_id.to_string()], |row| {
            row.get::<_, String>(0)
        })?;
        rows.map(|row| serde_json::from_str(&row?).context("invalid stored event"))
            .collect()
    }
}
