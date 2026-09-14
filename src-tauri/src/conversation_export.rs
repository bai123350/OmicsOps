use base64::Engine;
use omicsops_dto::{ConversationExportFormat, ConversationExportRequest};
use std::{io::Write, path::Path};
use tauri_plugin_dialog::DialogExt;

const MAX_BYTES: usize = 24 * 1024 * 1024;

fn extension(format: ConversationExportFormat) -> &'static str {
    match format {
        ConversationExportFormat::Html => "html",
        ConversationExportFormat::Png => "png",
    }
}

fn decode_export(request: &ConversationExportRequest) -> Result<Vec<u8>, String> {
    if request.content_base64.len() > MAX_BYTES * 4 / 3 + 4 {
        return Err("Export exceeds 24 MiB".into());
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(&request.content_base64)
        .map_err(|_| "Invalid export encoding")?;
    if bytes.is_empty() || bytes.len() > MAX_BYTES {
        return Err("Invalid export size".into());
    }
    match request.format {
        ConversationExportFormat::Png => {
            if bytes.len() < 45
                || !bytes.starts_with(b"\x89PNG\r\n\x1a\n")
                || &bytes[12..16] != b"IHDR"
                || !bytes.ends_with(b"\0\0\0\0IEND\xaeB`\x82")
            {
                return Err("Invalid PNG signature".into());
            }
            let width = u32::from_be_bytes(bytes[16..20].try_into().unwrap());
            let height = u32::from_be_bytes(bytes[20..24].try_into().unwrap());
            if !(480..=1600).contains(&width)
                || height == 0
                || height > 16000
                || u64::from(width) * u64::from(height) > 16_000_000
            {
                return Err("PNG dimensions exceed export limits".into());
            }
        }
        ConversationExportFormat::Html => {
            let html = std::str::from_utf8(&bytes).map_err(|_| "Invalid HTML encoding")?;
            if !html.starts_with("<!DOCTYPE html><html><head><meta charset=\"utf-8\"><meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; style-src 'unsafe-inline'; base-uri 'none'; form-action 'none'\">") || !html.ends_with("</body></html>") { return Err("Invalid standalone HTML signature".into()); }
        }
    }
    Ok(bytes)
}

fn write_export(path: &Path, format: ConversationExportFormat, bytes: &[u8]) -> Result<(), String> {
    if !path
        .extension()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.eq_ignore_ascii_case(extension(format)))
    {
        return Err("Save path extension does not match export format".into());
    }
    let parent = path.parent().ok_or("Invalid save directory")?;
    let mut file = tempfile::NamedTempFile::new_in(parent).map_err(|error| error.to_string())?;
    file.write_all(bytes).map_err(|error| error.to_string())?;
    file.as_file()
        .sync_all()
        .map_err(|error| error.to_string())?;
    file.persist(path)
        .map_err(|error| error.error.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn save_conversation_export(
    app: tauri::AppHandle,
    request: ConversationExportRequest,
) -> Result<Option<String>, String> {
    let bytes = decode_export(&request)?;
    tauri::async_runtime::spawn_blocking(move || {
        let ext = extension(request.format);
        let Some(selected) = app
            .dialog()
            .file()
            .add_filter("Conversation", &[ext])
            .set_file_name(format!("omicsops-conversation.{ext}"))
            .blocking_save_file()
        else {
            return Ok(None);
        };
        let mut path = selected.into_path().map_err(|error| error.to_string())?;
        if path.extension().is_none() {
            path.set_extension(ext);
        }
        write_export(&path, request.format, &bytes)?;
        Ok(Some(path.to_string_lossy().into_owned()))
    })
    .await
    .map_err(|error| error.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;
    fn request(format: ConversationExportFormat, bytes: &[u8]) -> ConversationExportRequest {
        ConversationExportRequest {
            format,
            content_base64: base64::engine::general_purpose::STANDARD.encode(bytes),
        }
    }
    #[test]
    fn rejects_mismatched_signatures_and_oversized_payloads() {
        assert!(decode_export(&request(ConversationExportFormat::Png, b"html")).is_err());
        assert!(
            decode_export(&request(
                ConversationExportFormat::Html,
                b"<script>x</script>"
            ))
            .is_err()
        );
        assert!(
            decode_export(&ConversationExportRequest {
                format: ConversationExportFormat::Png,
                content_base64: "a".repeat(MAX_BYTES * 4 / 3 + 8)
            })
            .is_err()
        );
    }
    #[test]
    fn saves_exact_bytes_and_rejects_wrong_extension() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("conversation.html");
        write_export(&path, ConversationExportFormat::Html, b"sample").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"sample");
        assert!(
            write_export(
                &directory.path().join("bad.exe"),
                ConversationExportFormat::Html,
                b"sample"
            )
            .is_err()
        );
    }
}
