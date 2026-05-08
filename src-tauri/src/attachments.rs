use std::{collections::HashMap, env, fs, path::PathBuf};

use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::CommandResult;

pub(crate) const ATTACHMENT_SIZE_LIMIT: usize = 25 * 1024 * 1024;

fn attachment_root_dir() -> CommandResult<PathBuf> {
    if let Ok(path) = env::var("LOCAL_SLOCK_ATTACHMENT_DIR") {
        return Ok(PathBuf::from(path));
    }
    let home = env::var("HOME").map_err(|_| "HOME is not set".to_owned())?;
    Ok(PathBuf::from(home)
        .join("Library")
        .join("Application Support")
        .join("LocalSlock")
        .join("attachments"))
}

fn attachment_extension(original_name: &str) -> String {
    let path = PathBuf::from(original_name);
    let Some(extension) = path.extension().and_then(|value| value.to_str()) else {
        return String::new();
    };
    let sanitized: String = extension
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .take(12)
        .collect();
    if sanitized.is_empty() {
        String::new()
    } else {
        format!(".{}", sanitized.to_ascii_lowercase())
    }
}

pub(crate) fn write_attachment_file(
    message_id: Uuid,
    attachment_id: Uuid,
    original_name: &str,
    bytes: &[u8],
) -> CommandResult<String> {
    let root = attachment_root_dir()?;
    let message_dir = root.join(message_id.to_string());
    fs::create_dir_all(&message_dir).map_err(|err| err.to_string())?;
    let path = message_dir.join(format!(
        "{}{}",
        attachment_id,
        attachment_extension(original_name)
    ));
    fs::write(&path, bytes).map_err(|err| err.to_string())?;
    Ok(path.to_string_lossy().to_string())
}

pub(crate) fn format_attachment_size(size_bytes: i64) -> String {
    if size_bytes >= 1_000_000 {
        format!("{:.1}MB", size_bytes as f64 / 1_000_000.0)
    } else if size_bytes >= 1_000 {
        format!("{:.1}KB", size_bytes as f64 / 1_000.0)
    } else {
        format!("{size_bytes}B")
    }
}

pub(crate) async fn load_message_attachment_lines(
    pool: &PgPool,
    message_ids: &[Uuid],
) -> CommandResult<HashMap<Uuid, Vec<String>>> {
    if message_ids.is_empty() {
        return Ok(HashMap::new());
    }
    let rows = sqlx::query(
        r#"
        select id, message_id, original_name, mime_type, size_bytes, storage_path
        from message_attachments
        where message_id = any($1)
        order by created_at asc
        "#,
    )
    .bind(message_ids)
    .fetch_all(pool)
    .await
    .map_err(|err| err.to_string())?;
    let mut attachments_by_message: HashMap<Uuid, Vec<String>> = HashMap::new();
    for row in rows {
        let id: Uuid = row.get("id");
        let message_id: Uuid = row.get("message_id");
        let original_name: String = row.get("original_name");
        let mime_type: String = row.get("mime_type");
        let size_bytes: i64 = row.get("size_bytes");
        let storage_path: String = row.get("storage_path");
        attachments_by_message
            .entry(message_id)
            .or_default()
            .push(format!(
                "- attachment_id={} name=\"{}\" mime={} size={} local_path=\"{}\"",
                id,
                original_name.replace('"', "\\\""),
                mime_type,
                format_attachment_size(size_bytes),
                storage_path.replace('"', "\\\"")
            ));
    }
    Ok(attachments_by_message)
}

pub(crate) fn attachment_summary_sql() -> &'static str {
    r#"
    coalesce((
        select string_agg(
            'attachment_id=' || ma.id::text ||
            ' name=' || quote_literal(ma.original_name) ||
            ' mime=' || ma.mime_type ||
            ' size=' || ma.size_bytes::text ||
            ' local_path=' || quote_literal(ma.storage_path),
            E'\n'
            order by ma.created_at asc
        )
        from message_attachments ma
        where ma.message_id = m.id
    ), '') as attachment_summary
    "#
}
