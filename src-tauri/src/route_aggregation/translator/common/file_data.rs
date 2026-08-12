//! OpenAI file content normalization → Gemini inline data part。
//!
//! CLIProxyAPI aligned: e47ffda - feat(translator): normalize and extract
//!                         OpenAI file content with MIME type
//! Source: https://github.com/router-for-me/CLIProxyAPI/commit/e47ffda75b6d55ce88462ca6e76f8ffed1c0e88a
//! Last verified: 2026-08-12
//!
//! 解析 OpenAI Chat `type: file` / OpenAI Responses `type: input_file` 内容：
//! - `file_data` 可能是 data URL（`data:<mime>;base64,<payload>`）或 raw base64
//! - 优先用 data URL 内嵌的 MIME；否则从 filename 扩展名推断
//! - 返回 `(mime_type, base64_payload, ok)`；失败时返回 `("", "", false)`

/// 规范化 OpenAI file content 为 Gemini inline data `(mime, base64)`。
///
/// 解析规则（对齐 CLIProxyAPI `NormalizeOpenAIFileData`）：
/// 1. `file_data` 为空 → 失败
/// 2. 若 `file_data` 以 `data:` 开头（大小写不敏感）：
///    - 切 `data:` 与 `,` 之间为 `metadata` 和 `payload`
///    - `metadata` 按 `;` 切分成 mime + 字段；要求存在 `base64` 标志
///    - mime 取第一个字段（trim 后）；若为空 → 失败
/// 3. 若 `file_data` 是 raw base64：
///    - 用 `fallback_mime_type` 推断；为空时也用 `filename` 扩展名推断
///    - 推断不出 → 失败
/// 4. 返回 `(mime, base64_payload, true)`
pub fn normalize_openai_file_data(
    filename: &str,
    fallback_mime_type: &str,
    file_data: &str,
) -> (String, String, bool) {
    if file_data.is_empty() {
        return (String::new(), String::new(), false);
    }

    let fallback = if fallback_mime_type.is_empty() {
        mime_from_filename(filename)
    } else {
        fallback_mime_type.to_string()
    };

    const DATA_URL_PREFIX: &str = "data:";
    if file_data.len() < DATA_URL_PREFIX.len()
        || !file_data[..DATA_URL_PREFIX.len()].eq_ignore_ascii_case(DATA_URL_PREFIX)
    {
        // raw base64
        if fallback.is_empty() {
            return (String::new(), String::new(), false);
        }
        return (fallback, file_data.to_string(), true);
    }

    let body = &file_data[DATA_URL_PREFIX.len()..];
    let (metadata, payload) = match body.split_once(',') {
        Some(pair) => pair,
        None => return (String::new(), String::new(), false),
    };
    if payload.is_empty() {
        return (String::new(), String::new(), false);
    }

    let mut fields = metadata.split(';');
    let mime = fields.next().map(str::trim).unwrap_or("");
    if mime.is_empty() {
        return (String::new(), String::new(), false);
    }

    let mut has_base64 = false;
    for field in fields {
        if field.trim().eq_ignore_ascii_case("base64") {
            has_base64 = true;
            break;
        }
    }
    if !has_base64 {
        return (String::new(), String::new(), false);
    }

    (mime.to_string(), payload.to_string(), true)
}

/// 从 filename 扩展名推断 MIME。覆盖 OpenAI 常见附件类型。
fn mime_from_filename(filename: &str) -> String {
    let ext = filename
        .rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .unwrap_or_default();
    mime_for_ext(&ext)
}

fn mime_for_ext(ext: &str) -> String {
    match ext {
        "pdf" => "application/pdf",
        "txt" => "text/plain",
        "csv" => "text/csv",
        "json" => "application/json",
        // images
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        // audio
        "wav" => "audio/wav",
        "mp3" => "audio/mpeg",
        "ogg" => "audio/ogg",
        // common documents
        "md" => "text/markdown",
        "html" | "htm" => "text/html",
        "xml" => "application/xml",
        "zip" => "application/zip",
        _ => "",
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_url_form_pdf() {
        let (mime, data, ok) =
            normalize_openai_file_data("test.pdf", "", "data:application/pdf;base64,JVBERi0");
        assert!(ok);
        assert_eq!(mime, "application/pdf");
        assert_eq!(data, "JVBERi0");
    }

    #[test]
    fn data_url_with_charset_and_case_insensitive_base64() {
        let (mime, data, ok) = normalize_openai_file_data(
            "test.txt",
            "",
            "data:application/pdf;charset=binary;BASE64,JVBERi0",
        );
        assert!(ok);
        assert_eq!(mime, "application/pdf");
        assert_eq!(data, "JVBERi0");
    }

    #[test]
    fn data_url_with_uppercase_data_scheme() {
        let (mime, data, ok) =
            normalize_openai_file_data("test.pdf", "", "DATA:application/pdf;base64,JVBERi0");
        assert!(ok);
        assert_eq!(mime, "application/pdf");
        assert_eq!(data, "JVBERi0");
    }

    #[test]
    fn raw_base64_uses_filename_extension() {
        let (mime, data, ok) =
            normalize_openai_file_data("TEST.PDF", "", "JVBERi0");
        assert!(ok);
        assert_eq!(mime, "application/pdf");
        assert_eq!(data, "JVBERi0");
    }

    #[test]
    fn raw_base64_with_explicit_fallback_mime() {
        let (mime, data, ok) = normalize_openai_file_data("", "application/pdf", "JVBERi0");
        assert!(ok);
        assert_eq!(mime, "application/pdf");
        assert_eq!(data, "JVBERi0");
    }

    #[test]
    fn empty_data_returns_error() {
        let (mime, data, ok) = normalize_openai_file_data("test.pdf", "", "");
        assert!(!ok);
        assert!(mime.is_empty());
        assert!(data.is_empty());
    }

    #[test]
    fn raw_base64_without_any_mime_hint_is_error() {
        let (mime, data, ok) = normalize_openai_file_data("test", "", "JVBERi0");
        assert!(!ok);
        assert!(mime.is_empty());
        assert!(data.is_empty());
    }

    #[test]
    fn data_url_without_base64_marker_is_error() {
        let (mime, data, ok) = normalize_openai_file_data(
            "test.pdf",
            "",
            "data:application/pdf,JVBERi0",
        );
        assert!(!ok);
        assert!(mime.is_empty());
        assert!(data.is_empty());
    }

    #[test]
    fn data_url_without_mime_is_error() {
        let (mime, data, ok) =
            normalize_openai_file_data("test.pdf", "", "data:;base64,JVBERi0");
        assert!(!ok);
        assert!(mime.is_empty());
        assert!(data.is_empty());
    }

    #[test]
    fn data_url_without_payload_is_error() {
        let (mime, data, ok) = normalize_openai_file_data(
            "test.pdf",
            "",
            "data:application/pdf;base64,",
        );
        assert!(!ok);
        assert!(mime.is_empty());
        assert!(data.is_empty());
    }

    #[test]
    fn mime_from_filename_known_extensions() {
        assert_eq!(mime_from_filename("a.pdf"), "application/pdf");
        assert_eq!(mime_from_filename("a.PNG"), "image/png");
        assert_eq!(mime_from_filename("a.json"), "application/json");
        assert_eq!(mime_from_filename("a.txt"), "text/plain");
        assert_eq!(mime_from_filename("a.mp3"), "audio/mpeg");
    }

    #[test]
    fn mime_from_filename_unknown_extension() {
        assert!(mime_from_filename("a.unknownext").is_empty());
        assert!(mime_from_filename("noext").is_empty());
    }
}
