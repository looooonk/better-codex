//! Compatibility with Desktop hosts that still keep local chat placement in global settings.
//! Browse through a read-only connection. Explicit changes mirror only the selected chat,
//! inside the same SQLite transaction as the native assignment.
use std::collections::HashMap;

use sqlx::Connection;
use sqlx::Row;
use sqlx::SqliteConnection;
use sqlx::sqlite::SqliteConnectOptions;

use super::StateRuntime;

const DATABASE: &str = ".codex-global-state.sqlite";
const ASSIGNMENTS: &str = "thread-project-assignments";
const PROJECT_MAP: &str = "app-server-project-id-by-legacy-project-id-by-host";
const PROJECTLESS: &str = "projectless-thread-ids";

impl StateRuntime {
    pub(super) async fn desktop_memberships(&self) -> anyhow::Result<HashMap<String, String>> {
        let path = self.sqlite.home().join(DATABASE);
        if !tokio::fs::try_exists(&path).await? {
            return Ok(HashMap::new());
        }
        let options = SqliteConnectOptions::new().filename(path).read_only(true);
        let mut connection = SqliteConnection::connect_with(&options).await?;
        let host = format!("local:{}", self.sqlite.home().display());
        let rows = sqlx::query(
            "SELECT key,value FROM settings WHERE scope='global'
             AND key IN (?,?)",
        )
        .bind(ASSIGNMENTS)
        .bind(PROJECT_MAP)
        .fetch_all(&mut connection)
        .await?;
        let mut records = serde_json::Value::Null;
        let mut ids = serde_json::Value::Null;
        for row in rows {
            let key: String = row.try_get("key")?;
            let mut value: serde_json::Value = serde_json::from_str(row.try_get("value")?)?;
            match key.as_str() {
                ASSIGNMENTS => records = value,
                PROJECT_MAP => {
                    ids = value
                        .get_mut(&host)
                        .map(serde_json::Value::take)
                        .unwrap_or_default()
                }
                _ => {}
            }
        }
        let Some(records) = records.as_object() else {
            return Ok(HashMap::new());
        };
        let projects: std::collections::HashSet<String> =
            sqlx::query_scalar("SELECT id FROM projects")
                .fetch_all(self.pool.as_ref())
                .await?
                .into_iter()
                .collect();
        let mut assignments = HashMap::new();
        for (thread, assignment) in records {
            if assignment["projectKind"] == "local"
                && assignment["hostId"].as_str().is_none_or(|id| id == "local")
                && let Some(old) = assignment["projectId"].as_str()
                && let Some(id) = ids.get(old).and_then(serde_json::Value::as_str)
                && projects.contains(id)
            {
                assignments.insert(thread.clone(), id.to_owned());
            }
        }
        Ok(assignments)
    }

    /// A dedicated connection keeps the attached Desktop database out of the shared pool.
    pub(super) async fn project_write_connection(
        &self,
    ) -> anyhow::Result<Option<SqliteConnection>> {
        let path = self.sqlite.home().join(DATABASE);
        if !tokio::fs::try_exists(&path).await? {
            return Ok(None);
        }
        let mut conn = self.pool.acquire().await?.detach();
        sqlx::query("ATTACH DATABASE ? AS desktop")
            .bind(path.to_string_lossy().into_owned())
            .execute(&mut conn)
            .await?;
        Ok(Some(conn))
    }

    pub(super) async fn mirror_desktop_membership(
        &self,
        conn: &mut SqliteConnection,
        thread: &str,
        project: Option<&str>,
    ) -> anyhow::Result<()> {
        let host = format!("local:{}", self.sqlite.home().display());
        let legacy: Option<String> = sqlx::query_scalar(
            "SELECT ids.key FROM desktop.settings, json_each(desktop.settings.value) AS h, json_each(h.value) AS ids
             WHERE desktop.settings.scope='global' AND desktop.settings.key=? AND h.key=? AND ids.value=?",
        ).bind(PROJECT_MAP).bind(host).bind(project).fetch_optional(&mut *conn).await?;
        // Server-only projects have no corresponding Desktop identity yet.
        if project.is_some() && legacy.is_none() {
            return Ok(());
        }
        let path = format!("$.\"{thread}\"");
        let current: Option<String> = sqlx::query_scalar(
            "SELECT json_extract(value,?) FROM desktop.settings WHERE scope='global' AND key=?",
        )
        .bind(&path)
        .bind(ASSIGNMENTS)
        .fetch_optional(&mut *conn)
        .await?
        .flatten();
        if let Some(current) = current {
            let assignment: serde_json::Value = serde_json::from_str(&current)?;
            anyhow::ensure!(
                assignment["projectKind"] == "local"
                    && assignment["hostId"].as_str().is_none_or(|id| id == "local"),
                "this chat belongs to another Desktop host"
            );
        }
        sqlx::query(
            "INSERT INTO desktop.settings(scope,key,value) VALUES ('global',?,'{}')
             ON CONFLICT(scope,key) DO NOTHING",
        )
        .bind(ASSIGNMENTS)
        .execute(&mut *conn)
        .await?;
        sqlx::query(
            "UPDATE desktop.settings SET value=CASE WHEN ? IS NULL THEN json_remove(value,?)
             ELSE json_set(value,?,json_object('projectKind','local','projectId',?)) END
             WHERE scope='global' AND key=?",
        )
        .bind(&legacy)
        .bind(&path)
        .bind(&path)
        .bind(&legacy)
        .bind(ASSIGNMENTS)
        .execute(&mut *conn)
        .await?;
        sqlx::query(
            "INSERT INTO desktop.settings(scope,key,value) VALUES ('global',?,'[]')
             ON CONFLICT(scope,key) DO NOTHING",
        )
        .bind(PROJECTLESS)
        .execute(&mut *conn)
        .await?;
        sqlx::query(
            "UPDATE desktop.settings SET value=(SELECT json_group_array(id) FROM
               (SELECT j.value AS id FROM json_each(desktop.settings.value) j WHERE j.value<>?
                UNION ALL SELECT ? WHERE ? IS NULL))
             WHERE scope='global' AND key=?",
        )
        .bind(thread)
        .bind(thread)
        .bind(project)
        .bind(PROJECTLESS)
        .execute(&mut *conn)
        .await?;
        Ok(())
    }
}

#[cfg(test)]
#[path = "desktop_projects_tests.rs"]
mod tests;
