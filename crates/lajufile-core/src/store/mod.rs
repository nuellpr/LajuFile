//! SQLite persistence for downloads and their segments.

pub mod model;

pub use model::*;

use rusqlite::{Connection, OptionalExtension, Row, params};
use std::path::Path;
use std::time::Duration;

const SCHEMA_V1: &str = include_str!("schema_v1.sql");

const DOWNLOAD_COLUMNS: &str = "id, url, final_url, file_name, save_path, temp_path, size, \
downloaded, status, category, kind, video_format, connections, ranged, etag, last_modified, \
cookie_enc, referrer, user_agent, error, queue_order, created_at, completed_at";

const SCHEMA_V2: &str = "ALTER TABLE downloads ADD COLUMN kind TEXT NOT NULL DEFAULT 'http';
ALTER TABLE downloads ADD COLUMN video_format TEXT;";

pub struct Store {
    conn: Connection,
}

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("I/O: {0}")]
    Io(#[from] std::io::Error),
    #[error("download {0} not found")]
    NotFound(i64),
    #[error("unsupported schema version {0}")]
    UnsupportedSchema(i32),
    #[error("invalid status in DB: {0}")]
    InvalidStatus(String),
    #[error("invalid category in DB: {0}")]
    InvalidCategory(String),
}

impl Store {
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut conn = Connection::open(path)?;
        conn.query_row("PRAGMA journal_mode=WAL", [], |row| row.get::<_, String>(0))?;
        Self::configure(&conn)?;
        Self::migrate(&mut conn)?;
        Ok(Self { conn })
    }

    pub fn open_in_memory() -> Result<Self, StoreError> {
        let mut conn = Connection::open_in_memory()?;
        Self::configure(&conn)?;
        Self::migrate(&mut conn)?;
        Ok(Self { conn })
    }

    fn configure(conn: &Connection) -> Result<(), StoreError> {
        conn.pragma_update(None, "foreign_keys", true)?;
        conn.busy_timeout(Duration::from_secs(5))?;
        Ok(())
    }

    fn migrate(conn: &mut Connection) -> Result<(), StoreError> {
        let version: i32 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        match version {
            0 => {
                let tx = conn.transaction()?;
                tx.execute_batch(SCHEMA_V1)?;
                tx.execute_batch(SCHEMA_V2)?;
                tx.pragma_update(None, "user_version", 2)?;
                tx.commit()?;
                Ok(())
            }
            1 => {
                let tx = conn.transaction()?;
                tx.execute_batch(SCHEMA_V2)?;
                tx.pragma_update(None, "user_version", 2)?;
                tx.commit()?;
                Ok(())
            }
            2 => Ok(()),
            other => Err(StoreError::UnsupportedSchema(other)),
        }
    }

    pub fn insert_download(&mut self, new: &NewDownload) -> Result<i64, StoreError> {
        let tx = self.conn.transaction()?;
        tx.execute(
            "INSERT INTO downloads (
                url, final_url, file_name, save_path, temp_path, size, status, category,
                kind, video_format, connections, ranged, etag, last_modified, cookie_enc,
                referrer, user_agent, queue_order, created_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19)",
            params![
                new.url,
                new.final_url,
                new.file_name,
                new.save_path.to_string_lossy(),
                new.temp_path.to_string_lossy(),
                new.size.map(|v| v as i64),
                new.status.as_str(),
                new.category.as_str(),
                new.kind,
                new.video_format,
                new.connections as i64,
                new.ranged as i64,
                new.etag,
                new.last_modified,
                new.cookie_enc,
                new.referrer,
                new.user_agent,
                new.queue_order,
                new.created_at,
            ],
        )?;
        let id = tx.last_insert_rowid();

        for segment in &new.segments {
            tx.execute(
                "INSERT INTO segments (download_id, idx, start, end, done)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    id,
                    segment.idx as i64,
                    segment.start as i64,
                    segment.end as i64,
                    segment.done as i64,
                ],
            )?;
        }

        tx.commit()?;
        Ok(id)
    }

    pub fn get_download(&self, id: i64) -> Result<Download, StoreError> {
        let sql = format!("SELECT {DOWNLOAD_COLUMNS} FROM downloads WHERE id = ?1");
        match self
            .conn
            .query_row(&sql, [id], |row| Ok(row_to_download(row)))
        {
            Ok(Ok(download)) => Ok(download),
            Ok(Err(error)) => Err(error),
            Err(rusqlite::Error::QueryReturnedNoRows) => Err(StoreError::NotFound(id)),
            Err(error) => Err(error.into()),
        }
    }

    pub fn list_downloads(&self) -> Result<Vec<Download>, StoreError> {
        let sql = format!("SELECT {DOWNLOAD_COLUMNS} FROM downloads ORDER BY queue_order, id");
        let mut stmt = self.conn.prepare(&sql)?;
        let mut rows = stmt.query([])?;
        let mut downloads = Vec::new();
        while let Some(row) = rows.next()? {
            downloads.push(row_to_download(row)?);
        }
        Ok(downloads)
    }

    pub fn active_by_url(&self, url: &str) -> Result<Option<Download>, StoreError> {
        let sql = format!(
            "SELECT {DOWNLOAD_COLUMNS} FROM downloads
             WHERE url = ?1 AND status IN ('queued', 'downloading', 'paused')
             ORDER BY id DESC LIMIT 1"
        );
        let result = self
            .conn
            .query_row(&sql, [url], |row| Ok(row_to_download(row)))
            .optional()?;
        match result {
            Some(Ok(download)) => Ok(Some(download)),
            Some(Err(error)) => Err(error),
            None => Ok(None),
        }
    }

    pub fn next_queue_order(&self) -> Result<i64, StoreError> {
        Ok(self.conn.query_row(
            "SELECT COALESCE(MAX(queue_order), 0) + 1 FROM downloads",
            [],
            |row| row.get(0),
        )?)
    }

    pub fn update_status(
        &mut self,
        id: i64,
        status: DownloadStatus,
        error: Option<&str>,
    ) -> Result<(), StoreError> {
        let changed = self.conn.execute(
            "UPDATE downloads SET status = ?1, error = ?2 WHERE id = ?3",
            params![status.as_str(), error, id],
        )?;
        if changed == 0 {
            return Err(StoreError::NotFound(id));
        }
        Ok(())
    }

    pub fn update_progress(
        &mut self,
        id: i64,
        segment_idx: u32,
        done: u64,
    ) -> Result<u64, StoreError> {
        let tx = self.conn.transaction()?;
        let changed = tx.execute(
            "UPDATE segments SET done = ?1 WHERE download_id = ?2 AND idx = ?3",
            params![done as i64, id, segment_idx as i64],
        )?;
        if changed == 0 {
            return Err(StoreError::NotFound(id));
        }
        let total: i64 = tx.query_row(
            "SELECT COALESCE(SUM(done), 0) FROM segments WHERE download_id = ?1",
            [id],
            |row| row.get(0),
        )?;
        let changed = tx.execute(
            "UPDATE downloads SET downloaded = ?1 WHERE id = ?2",
            params![total, id],
        )?;
        if changed == 0 {
            return Err(StoreError::NotFound(id));
        }
        tx.commit()?;
        Ok(total as u64)
    }

    pub fn update_progress_batch(
        &mut self,
        id: i64,
        updates: &[(u32, u64)],
    ) -> Result<u64, StoreError> {
        let tx = self.conn.transaction()?;
        for (segment_idx, done) in updates {
            let changed = tx.execute(
                "UPDATE segments SET done = ?1 WHERE download_id = ?2 AND idx = ?3",
                params![*done as i64, id, *segment_idx as i64],
            )?;
            if changed == 0 {
                return Err(StoreError::NotFound(id));
            }
        }
        let total: i64 = tx.query_row(
            "SELECT COALESCE(SUM(done), 0) FROM segments WHERE download_id = ?1",
            [id],
            |row| row.get(0),
        )?;
        let changed = tx.execute(
            "UPDATE downloads SET downloaded = ?1 WHERE id = ?2",
            params![total, id],
        )?;
        if changed == 0 {
            return Err(StoreError::NotFound(id));
        }
        tx.commit()?;
        Ok(total as u64)
    }

    pub fn replace_segments(
        &mut self,
        id: i64,
        segments: &[Segment],
        connections: u32,
        ranged: bool,
    ) -> Result<u64, StoreError> {
        let tx = self.conn.transaction()?;
        let exists: i64 = tx.query_row(
            "SELECT COUNT(*) FROM downloads WHERE id = ?1",
            [id],
            |row| row.get(0),
        )?;
        if exists == 0 {
            return Err(StoreError::NotFound(id));
        }
        tx.execute("DELETE FROM segments WHERE download_id = ?1", [id])?;
        let mut total = 0u64;
        {
            let mut insert = tx.prepare(
                "INSERT INTO segments (download_id, idx, start, end, done)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
            )?;
            for segment in segments {
                insert.execute(params![
                    id,
                    segment.idx as i64,
                    segment.start as i64,
                    segment.end as i64,
                    segment.done as i64
                ])?;
                total = total.saturating_add(segment.done);
            }
        }
        let changed = tx.execute(
            "UPDATE downloads SET connections = ?1, ranged = ?2, downloaded = ?3
             WHERE id = ?4",
            params![connections as i64, ranged as i64, total as i64, id],
        )?;
        if changed == 0 {
            return Err(StoreError::NotFound(id));
        }
        tx.commit()?;
        Ok(total)
    }

    pub fn mark_done(&mut self, id: i64, completed_at: i64) -> Result<(), StoreError> {
        let changed = self.conn.execute(
            "UPDATE downloads
             SET status = 'done', completed_at = ?1,
                 downloaded = COALESCE(size, downloaded), error = NULL
             WHERE id = ?2",
            params![completed_at, id],
        )?;
        if changed == 0 {
            return Err(StoreError::NotFound(id));
        }
        Ok(())
    }

    pub fn delete_download(&mut self, id: i64) -> Result<(), StoreError> {
        let changed = self
            .conn
            .execute("DELETE FROM downloads WHERE id = ?1", [id])?;
        if changed == 0 {
            return Err(StoreError::NotFound(id));
        }
        Ok(())
    }

    pub fn segments(&self, download_id: i64) -> Result<Vec<Segment>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT idx, start, end, done FROM segments
             WHERE download_id = ?1 ORDER BY idx",
        )?;
        let mut rows = stmt.query([download_id])?;
        let mut segments = Vec::new();
        while let Some(row) = rows.next()? {
            segments.push(Segment {
                idx: row.get::<_, i64>(0)? as u32,
                start: row.get::<_, i64>(1)? as u64,
                end: row.get::<_, i64>(2)? as u64,
                done: row.get::<_, i64>(3)? as u64,
            });
        }
        Ok(segments)
    }
}

fn row_to_download(row: &Row<'_>) -> Result<Download, StoreError> {
    Ok(Download {
        id: row.get(0)?,
        url: row.get(1)?,
        final_url: row.get(2)?,
        file_name: row.get(3)?,
        save_path: Path::new(&row.get::<_, String>(4)?).to_path_buf(),
        temp_path: Path::new(&row.get::<_, String>(5)?).to_path_buf(),
        size: row.get::<_, Option<i64>>(6)?.map(|value| value as u64),
        downloaded: row.get::<_, i64>(7)? as u64,
        status: DownloadStatus::from_db(&row.get::<_, String>(8)?)?,
        category: Category::from_db(&row.get::<_, String>(9)?)?,
        kind: row.get(10)?,
        video_format: row.get(11)?,
        connections: row.get::<_, i64>(12)? as u32,
        ranged: row.get::<_, i64>(13)? != 0,
        etag: row.get(14)?,
        last_modified: row.get(15)?,
        cookie_enc: row.get(16)?,
        referrer: row.get(17)?,
        user_agent: row.get(18)?,
        error: row.get(19)?,
        queue_order: row.get(20)?,
        created_at: row.get(21)?,
        completed_at: row.get(22)?,
    })
}
