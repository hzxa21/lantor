use std::collections::HashMap;

use sqlx::{sqlite::SqliteRow, Row, SqlitePool};
use uuid::Uuid;

use crate::{
    models::{Artifact, Message, MessageAttachment, SavedMessage},
    to_string, CommandResult,
};

pub(crate) async fn load_messages(pool: &SqlitePool) -> CommandResult<Vec<Message>> {
    let rows = sqlx::query(
        r#"
        select
            m.id,
            m.channel_id,
            m.thread_root_id,
            m.sender_agent_id,
            m.sender_name,
            m.sender_role,
            m.body,
            m.is_task,
            m.thread_followed,
            m.delivery_state,
            m.stream_key,
            t.number as task_number,
            t.status as task_status,
            m.created_at,
            m.updated_at
        from messages m
        left join tasks t on t.message_id = m.id
        order by julianday(m.created_at) asc, m.created_at asc
        "#,
    )
    .fetch_all(pool)
    .await
    .map_err(to_string)?;

    let mut messages: Vec<Message> = rows
        .into_iter()
        .map(|row| Message {
            id: row.get("id"),
            channel_id: row.get("channel_id"),
            thread_root_id: row.get("thread_root_id"),
            sender_agent_id: row.get("sender_agent_id"),
            sender_name: row.get("sender_name"),
            sender_role: row.get("sender_role"),
            body: row.get("body"),
            is_task: row.get("is_task"),
            thread_followed: row.get("thread_followed"),
            delivery_state: row.get("delivery_state"),
            stream_key: row.get("stream_key"),
            task_number: row.get("task_number"),
            task_status: row.get("task_status"),
            attachments: Vec::new(),
            artifacts: Vec::new(),
            created_at: row.get("created_at"),
            updated_at: row.get("updated_at"),
        })
        .collect();
    attach_message_attachments(pool, &mut messages).await?;
    attach_message_artifacts(pool, &mut messages).await?;
    Ok(messages)
}

pub(crate) async fn load_saved_messages(pool: &SqlitePool) -> CommandResult<Vec<SavedMessage>> {
    let rows = sqlx::query(
        r#"
        select
            sm.id,
            sm.message_id,
            m.channel_id,
            c.name as channel_name,
            m.thread_root_id,
            m.sender_name,
            m.sender_role,
            m.body,
            m.created_at as message_created_at,
            sm.created_at
        from saved_messages sm
        join messages m on m.id = sm.message_id
        join channels c on c.id = m.channel_id
        order by sm.created_at desc
        "#,
    )
    .fetch_all(pool)
    .await
    .map_err(to_string)?;

    Ok(rows
        .into_iter()
        .map(|row| SavedMessage {
            id: row.get("id"),
            message_id: row.get("message_id"),
            channel_id: row.get("channel_id"),
            channel_name: row.get("channel_name"),
            thread_root_id: row.get("thread_root_id"),
            sender_name: row.get("sender_name"),
            sender_role: row.get("sender_role"),
            body: row.get("body"),
            message_created_at: row.get("message_created_at"),
            created_at: row.get("created_at"),
        })
        .collect())
}

pub(crate) async fn load_message(pool: &SqlitePool, message_id: Uuid) -> CommandResult<Message> {
    let row = sqlx::query(
        r#"
        select
            m.id,
            m.channel_id,
            m.thread_root_id,
            m.sender_agent_id,
            m.sender_name,
            m.sender_role,
            m.body,
            m.is_task,
            m.thread_followed,
            m.delivery_state,
            m.stream_key,
            t.number as task_number,
            t.status as task_status,
            m.created_at,
            m.updated_at
        from messages m
        left join tasks t on t.message_id = m.id
        where m.id = $1
        "#,
    )
    .bind(message_id)
    .fetch_one(pool)
    .await
    .map_err(to_string)?;

    let mut message = Message {
        id: row.get("id"),
        channel_id: row.get("channel_id"),
        thread_root_id: row.get("thread_root_id"),
        sender_agent_id: row.get("sender_agent_id"),
        sender_name: row.get("sender_name"),
        sender_role: row.get("sender_role"),
        body: row.get("body"),
        is_task: row.get("is_task"),
        thread_followed: row.get("thread_followed"),
        delivery_state: row.get("delivery_state"),
        stream_key: row.get("stream_key"),
        task_number: row.get("task_number"),
        task_status: row.get("task_status"),
        attachments: Vec::new(),
        artifacts: Vec::new(),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    };
    attach_message_attachments(pool, std::slice::from_mut(&mut message)).await?;
    attach_message_artifacts(pool, std::slice::from_mut(&mut message)).await?;
    Ok(message)
}

async fn attach_message_attachments(
    pool: &SqlitePool,
    messages: &mut [Message],
) -> CommandResult<()> {
    if messages.is_empty() {
        return Ok(());
    }
    let ids: Vec<Uuid> = messages.iter().map(|message| message.id).collect();
    let placeholders = (0..ids.len())
        .map(|index| format!("${}", index + 1))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        r#"
        select id, message_id, original_name, mime_type, size_bytes, storage_path, created_at
        from message_attachments
        where message_id in ({placeholders})
        order by created_at asc
        "#,
    );
    let mut query = sqlx::query(&sql);
    for id in &ids {
        query = query.bind(*id);
    }
    let rows = query.fetch_all(pool).await.map_err(to_string)?;
    let mut attachments_by_message: HashMap<Uuid, Vec<MessageAttachment>> = HashMap::new();
    for row in rows {
        let attachment = MessageAttachment {
            id: row.get("id"),
            message_id: row.get("message_id"),
            original_name: row.get("original_name"),
            mime_type: row.get("mime_type"),
            size_bytes: row.get("size_bytes"),
            storage_path: row.get("storage_path"),
            created_at: row.get("created_at"),
        };
        attachments_by_message
            .entry(attachment.message_id)
            .or_default()
            .push(attachment);
    }
    for message in messages {
        message.attachments = attachments_by_message
            .remove(&message.id)
            .unwrap_or_default();
    }
    Ok(())
}

fn artifact_from_row(row: &SqliteRow) -> Artifact {
    Artifact {
        id: row.get("id"),
        message_id: row.get("message_id"),
        channel_id: row.get("channel_id"),
        thread_root_id: row.get("thread_root_id"),
        creator_agent_id: row.get("creator_agent_id"),
        creator_agent_handle: row.get("creator_agent_handle"),
        kind: row.get("kind"),
        title: row.get("title"),
        summary: row.get("summary"),
        content: row.get("content"),
        metadata: row.get("metadata"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    }
}

pub(crate) async fn load_artifacts(pool: &SqlitePool) -> CommandResult<Vec<Artifact>> {
    let rows = sqlx::query(
        r#"
        select
            ar.id,
            ar.message_id,
            ar.channel_id,
            ar.thread_root_id,
            ar.creator_agent_id,
            a.handle as creator_agent_handle,
            ar.kind,
            ar.title,
            ar.summary,
            ar.content,
            ar.metadata,
            ar.created_at,
            ar.updated_at
        from artifacts ar
        left join agents a on a.id = ar.creator_agent_id
        order by ar.created_at asc
        "#,
    )
    .fetch_all(pool)
    .await
    .map_err(to_string)?;
    Ok(rows.iter().map(artifact_from_row).collect())
}

pub(crate) async fn load_artifact(pool: &SqlitePool, artifact_id: Uuid) -> CommandResult<Artifact> {
    let row = sqlx::query(
        r#"
        select
            ar.id,
            ar.message_id,
            ar.channel_id,
            ar.thread_root_id,
            ar.creator_agent_id,
            a.handle as creator_agent_handle,
            ar.kind,
            ar.title,
            ar.summary,
            ar.content,
            ar.metadata,
            ar.created_at,
            ar.updated_at
        from artifacts ar
        left join agents a on a.id = ar.creator_agent_id
        where ar.id = $1
        "#,
    )
    .bind(artifact_id)
    .fetch_one(pool)
    .await
    .map_err(to_string)?;
    Ok(artifact_from_row(&row))
}

async fn attach_message_artifacts(
    pool: &SqlitePool,
    messages: &mut [Message],
) -> CommandResult<()> {
    if messages.is_empty() {
        return Ok(());
    }
    let ids: Vec<Uuid> = messages.iter().map(|message| message.id).collect();
    let placeholders = (0..ids.len())
        .map(|index| format!("${}", index + 1))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        r#"
        select
            ar.id,
            ar.message_id,
            ar.channel_id,
            ar.thread_root_id,
            ar.creator_agent_id,
            a.handle as creator_agent_handle,
            ar.kind,
            ar.title,
            ar.summary,
            ar.content,
            ar.metadata,
            ar.created_at,
            ar.updated_at
        from artifacts ar
        left join agents a on a.id = ar.creator_agent_id
        where ar.message_id in ({placeholders})
        order by ar.created_at asc
        "#,
    );
    let mut query = sqlx::query(&sql);
    for id in &ids {
        query = query.bind(*id);
    }
    let rows = query.fetch_all(pool).await.map_err(to_string)?;
    let mut artifacts_by_message: HashMap<Uuid, Vec<Artifact>> = HashMap::new();
    for row in rows {
        let artifact = artifact_from_row(&row);
        artifacts_by_message
            .entry(artifact.message_id)
            .or_default()
            .push(artifact);
    }
    for message in messages {
        message.artifacts = artifacts_by_message.remove(&message.id).unwrap_or_default();
    }
    Ok(())
}
