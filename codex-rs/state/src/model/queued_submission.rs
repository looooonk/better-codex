use anyhow::Context;
use codex_protocol::ThreadId;
use sqlx::Row;
use sqlx::sqlite::SqliteRow;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QueuedSubmissionState {
    Pending,
    Starting,
    Inflight,
    Terminal,
}

impl QueuedSubmissionState {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Starting => "starting",
            Self::Inflight => "inflight",
            Self::Terminal => "terminal",
        }
    }

    fn parse(value: &str) -> anyhow::Result<Self> {
        match value {
            "pending" => Ok(Self::Pending),
            "starting" => Ok(Self::Starting),
            "inflight" => Ok(Self::Inflight),
            "terminal" => Ok(Self::Terminal),
            _ => anyhow::bail!("unknown queued submission state `{value}`"),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QueuedSubmissionTerminalStatus {
    Completed,
    Failed,
    Interrupted,
}

impl QueuedSubmissionTerminalStatus {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Interrupted => "interrupted",
        }
    }

    fn parse(value: &str) -> anyhow::Result<Self> {
        match value {
            "completed" => Ok(Self::Completed),
            "failed" => Ok(Self::Failed),
            "interrupted" => Ok(Self::Interrupted),
            _ => anyhow::bail!("unknown queued submission terminal status `{value}`"),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QueuedSubmissionRecord {
    pub id: String,
    pub thread_id: ThreadId,
    pub payload: String,
    pub payload_digest: String,
    pub client_user_message_id: String,
    pub state: QueuedSubmissionState,
    pub turn_id: Option<String>,
    pub terminal_status: Option<QueuedSubmissionTerminalStatus>,
}

impl QueuedSubmissionRecord {
    pub(crate) fn try_from_row(row: &SqliteRow) -> anyhow::Result<Self> {
        let state = QueuedSubmissionState::parse(row.try_get("state")?)?;
        let terminal_status = row
            .try_get::<Option<String>, _>("terminal_status")?
            .as_deref()
            .map(QueuedSubmissionTerminalStatus::parse)
            .transpose()?;
        Ok(Self {
            id: row.try_get("id")?,
            thread_id: ThreadId::try_from(row.try_get::<String, _>("thread_id")?)
                .context("queued submission has invalid thread id")?,
            payload: row.try_get("payload_json")?,
            payload_digest: row.try_get("payload_digest")?,
            client_user_message_id: row.try_get("client_user_message_id")?,
            state,
            turn_id: row.try_get("turn_id")?,
            terminal_status,
        })
    }
}
