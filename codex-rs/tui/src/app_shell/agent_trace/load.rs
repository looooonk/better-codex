use super::Lane;
use super::Records;
use super::Trace;
use crate::app_server_session::read_thread_history;
use crate::app_shell::backend::app_shell_request_id;
use crate::app_shell::backend_background::descendants;
use codex_app_server_client::AppServerRequestHandle;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::SessionSource;
use codex_app_server_protocol::ThreadReadParams;
use codex_app_server_protocol::ThreadReadResponse;
use codex_protocol::ThreadId;
use codex_protocol::protocol::SubAgentSource;
use color_eyre::Result;
use std::collections::HashSet;
use std::collections::VecDeque;
use std::sync::Arc;

pub(in crate::app_shell) async fn load(
    client: AppServerRequestHandle,
    root: ThreadId,
    local: bool,
    previous: Arc<Trace>,
) -> Result<Trace> {
    let mut root = root;
    let mut root_thread = metadata(&client, root).await?;
    let mut ancestors = HashSet::from([root]);
    let mut partial = false;
    while let SessionSource::SubAgent(SubAgentSource::ThreadSpawn {
        parent_thread_id, ..
    }) = &root_thread.source
    {
        let parent = *parent_thread_id;
        if !ancestors.insert(parent) {
            partial = true;
            break;
        }
        match metadata(&client, parent).await {
            Ok(thread) => {
                root = parent;
                root_thread = thread;
            }
            Err(_) => {
                partial = true;
                break;
            }
        }
    }
    let root_id = root_thread.id.clone();
    let children = match descendants(client.clone(), root).await {
        Err(err) if previous.lanes.len() > 1 => return Err(err),
        result => result,
    };
    partial |= children.is_err();
    let mut pending = VecDeque::from([root_thread]);
    pending.extend(children.unwrap_or_default());
    let mut known: HashSet<_> = pending.iter().map(|thread| thread.id.clone()).collect();
    let mut checked = HashSet::new();
    let mut lanes = Vec::new();
    while let Some(thread) = pending.pop_front() {
        let is_root = thread.id == root_id;
        let (path, nickname) = match &thread.source {
            SessionSource::SubAgent(SubAgentSource::ThreadSpawn {
                agent_path,
                agent_nickname,
                ..
            }) => (
                agent_path.as_ref().map(ToString::to_string),
                agent_nickname.as_ref(),
            ),
            _ => (None, None),
        };
        let path = path.unwrap_or_else(|| {
            if is_root {
                "/root".into()
            } else {
                thread.id.clone()
            }
        });
        let name = if is_root {
            "Main".into()
        } else {
            path.rsplit('/')
                .next()
                .filter(|_| path != thread.id)
                .map(str::to_owned)
                .or_else(|| thread.agent_nickname.as_ref().or(nickname).cloned())
                .unwrap_or_else(|| path.clone())
        };
        let mut file = if local
            && let Some(path) = &thread.path
            && let Some(path) = codex_rollout::existing_rollout_path(path).await
            && let Ok(meta) = tokio::fs::metadata(&path).await
            && let Ok(modified) = meta.modified()
        {
            Some((path, meta.len(), modified))
        } else {
            None
        };
        let cached = previous
            .lanes
            .iter()
            .find(|lane| file.is_some() && lane.target.thread_id == thread.id && lane.file == file);
        let records = if let Some(cached) = cached {
            cached.records.clone()
        } else if let Some((file, _, _)) = &file
            && let Ok(records) = Records::read(file, &path, &thread.id).await
        {
            Arc::new(records)
        } else {
            let history =
                read_thread_history(client.clone(), ThreadId::from_string(&thread.id)?).await;
            // Projected history may omit recipient-side bodies and replies.
            partial = true;
            file = None;
            match history {
                Ok(thread) => Arc::new(Records::projected(&thread)),
                Err(_) => previous
                    .lanes
                    .iter()
                    .find(|lane| lane.target.thread_id == thread.id)
                    .map(|lane| lane.records.clone())
                    .unwrap_or_default(),
            }
        };
        // A legacy store can have the spawn in its rollout but no indexed edge.
        // Only follow IDs whose metadata independently identifies this parent.
        for send in records.sent.iter().chain(&records.messages) {
            if send.kind != "Task"
                || send.from != thread.id
                || known.contains(&send.to)
                || !checked.insert((thread.id.clone(), send.to.clone()))
            {
                continue;
            }
            if let Ok(id) = ThreadId::from_string(&send.to)
                && let Ok(child) = metadata(&client, id).await
                && matches!(&child.source, SessionSource::SubAgent(SubAgentSource::ThreadSpawn {parent_thread_id,..}) if parent_thread_id.to_string() == thread.id)
            {
                known.insert(child.id.clone());
                pending.push_back(child);
            }
        }
        let mut lane = Lane::new(&thread, path, name, records);
        lane.file = file;
        lanes.push(lane);
    }
    Ok(Trace::assemble(lanes, &root_id, partial))
}

async fn metadata(
    client: &AppServerRequestHandle,
    id: ThreadId,
) -> Result<codex_app_server_protocol::Thread> {
    let response: ThreadReadResponse = client
        .request_typed(ClientRequest::ThreadRead {
            request_id: app_shell_request_id("agent-trace-thread"),
            params: ThreadReadParams {
                thread_id: id.to_string(),
                include_turns: false,
            },
        })
        .await?;
    Ok(response.thread)
}
