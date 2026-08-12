//! 多模态 inline_data 工具 —— Gemini `parts[].inline_data = {mime_type, data}`
//! ↔ Anthropic `content[].image.source.{type, media_type, data}` ↔ OpenAI
//! `content[].image_url = {url, detail}`。
//!
//! CLIProxyAPI aligned: 934da237 - fix(openai): preserve structured and stringified
//!                        custom tool outputs during Responses conversion
//! Source: https://github.com/router-for-me/CLIProxyAPI/commit/934da2379d6272a704953a02322b666b2a2efa3e
//! Last verified: 2026-08-12

use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine;
use serde_json::{Map, Value};

/// Gemini `inline_data` 部件。
#[derive(Debug, Clone)]
pub struct InlineData {
    pub mime_type: String,
    /// base64 编码后的二进制内容。
    pub data: String,
}

impl InlineData {
    /// 从 base64 字符串构造。
    pub fn from_base64(mime_type: impl Into<String>, data_b64: impl Into<String>) -> Self {
        Self { mime_type: mime_type.into(), data: data_b64.into() }
    }

    /// 从原始字节构造（自动 base64 编码）。
    pub fn from_bytes(mime_type: impl Into<String>, bytes: &[u8]) -> Self {
        Self {
            mime_type: mime_type.into(),
            data: BASE64_STANDARD.encode(bytes),
        }
    }

    /// 渲染为 Gemini `parts[].inline_data` JSON。
    pub fn to_json(&self) -> Value {
        Value::Object(Map::from_iter([
            ("inline_data".into(), Value::Object(Map::from_iter([
                ("mime_type".into(), Value::String(self.mime_type.clone())),
                ("data".into(), Value::String(self.data.clone())),
            ]))),
        ]))
    }

    /// 渲染为 Anthropic `content[].image` block。
    pub fn to_anthropic_image(&self) -> Value {
        Value::Object(Map::from_iter([
            ("type".into(), Value::String("image".into())),
            (
                "source".into(),
                Value::Object(Map::from_iter([
                    ("type".into(), Value::String("base64".into())),
                    ("media_type".into(), Value::String(self.mime_type.clone())),
                    ("data".into(), Value::String(self.data.clone())),
                ])),
            ),
        ]))
    }

    /// 渲染为 OpenAI Chat `content[].image_url` block（data URL）。
    pub fn to_openai_image_url(&self, detail: Option<&str>) -> Value {
        let data_url = format!("data:{};base64,{}", self.mime_type, self.data);
        let mut url_obj = Map::new();
        url_obj.insert("url".into(), Value::String(data_url));
        if let Some(d) = detail {
            url_obj.insert("detail".into(), Value::String(d.to_string()));
        }
        Value::Object(Map::from_iter([
            ("type".into(), Value::String("image_url".into())),
            ("image_url".into(), Value::Object(url_obj)),
        ]))
    }

    /// 解码 base64 到原始字节。
    pub fn decode(&self) -> Result<Vec<u8>, base64::DecodeError> {
        BASE64_STANDARD.decode(&self.data)
    }
}

/// 判断 MIME 是否为图片类型。
pub fn is_image_mime(mime: &str) -> bool {
    mime.starts_with("image/")
}

/// 判断 MIME 是否为音频类型。
pub fn is_audio_mime(mime: &str) -> bool {
    mime.starts_with("audio/")
}

/// 判断 MIME 是否为视频类型。
pub fn is_video_mime(mime: &str) -> bool {
    mime.starts_with("video/")
}

/// 判断 MIME 是否为 PDF 文档。
pub fn is_pdf_mime(mime: &str) -> bool {
    mime == "application/pdf"
}

/// 判断 MIME 是否属于多模态可处理范围（图片/音频/视频/PDF）。
pub fn is_multimodal_mime(mime: &str) -> bool {
    is_image_mime(mime) || is_audio_mime(mime) || is_video_mime(mime) || is_pdf_mime(mime)
}

/// 解析 data URL：`data:<mime>;base64,<data>`。
///
/// 返回 `(mime, data)`，若 URL 不是 data URL 则返回 None。
pub fn parse_data_url(url: &str) -> Option<(&str, &str)> {
    let rest = url.strip_prefix("data:")?;
    let (mime, data) = rest.split_once(";base64,")?;
    Some((mime, data))
}

/// 构造 data URL。
pub fn build_data_url(mime: &str, data_b64: &str) -> String {
    format!("data:{};base64,{}", mime, data_b64)
}

/// 从 MIME 前缀嗅探（用于 response 方向已知是 base64 但 MIME 未知）。
pub fn sniff_image_mime(b64: &str) -> Option<&'static str> {
    // PNG magic: iVBORw0KGgo (base64 of \x89PNG\r\n\x1a\n)
    if b64.starts_with("iVBORw0KGgo") {
        Some("image/png")
    } else if b64.starts_with("/9j/") {
        Some("image/jpeg")
    } else if b64.starts_with("R0lGOD") {
        Some("image/gif")
    } else if b64.starts_with("UklGR") {
        Some("image/webp")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inline_data_roundtrip_base64() {
        let bytes = b"hello world";
        let d = InlineData::from_bytes("text/plain", bytes);
        assert_eq!(d.mime_type, "text/plain");
        assert_eq!(d.decode().unwrap(), bytes);
    }

    #[test]
    fn inline_data_from_base64_keeps_string() {
        let b64 = BASE64_STANDARD.encode(b"hi");
        let d = InlineData::from_base64("text/plain", b64.clone());
        assert_eq!(d.data, b64);
        assert_eq!(d.decode().unwrap(), b"hi");
    }

    #[test]
    fn to_json_matches_gemini_shape() {
        let d = InlineData::from_base64("image/png", "iVBOR");
        let v = d.to_json();
        assert_eq!(
            v,
            serde_json::json!({
                "inline_data": {"mime_type": "image/png", "data": "iVBOR"}
            })
        );
    }

    #[test]
    fn to_anthropic_image_shape() {
        let d = InlineData::from_base64("image/png", "iVBOR");
        let v = d.to_anthropic_image();
        assert_eq!(
            v,
            serde_json::json!({
                "type": "image",
                "source": {
                    "type": "base64",
                    "media_type": "image/png",
                    "data": "iVBOR"
                }
            })
        );
    }

    #[test]
    fn to_openai_image_url_with_detail() {
        let d = InlineData::from_base64("image/png", "iVBOR");
        let v = d.to_openai_image_url(Some("high"));
        assert_eq!(
            v,
            serde_json::json!({
                "type": "image_url",
                "image_url": {
                    "url": "data:image/png;base64,iVBOR",
                    "detail": "high"
                }
            })
        );
    }

    #[test]
    fn mime_classifiers() {
        assert!(is_image_mime("image/png"));
        assert!(is_image_mime("image/jpeg"));
        assert!(!is_image_mime("text/plain"));

        assert!(is_audio_mime("audio/mp3"));
        assert!(is_audio_mime("audio/wav"));
        assert!(!is_audio_mime("video/mp4"));

        assert!(is_video_mime("video/mp4"));
        assert!(!is_video_mime("image/gif"));

        assert!(is_pdf_mime("application/pdf"));
        assert!(!is_pdf_mime("text/plain"));

        assert!(is_multimodal_mime("image/png"));
        assert!(is_multimodal_mime("audio/wav"));
        assert!(is_multimodal_mime("video/mp4"));
        assert!(is_multimodal_mime("application/pdf"));
        assert!(!is_multimodal_mime("text/plain"));
    }

    #[test]
    fn parse_data_url_extracts_mime_and_b64() {
        assert_eq!(
            parse_data_url("data:image/png;base64,iVBOR"),
            Some(("image/png", "iVBOR"))
        );
        assert_eq!(parse_data_url("https://example.com/x.png"), None);
        assert_eq!(parse_data_url("data:text/plain;base64,aGk="), Some(("text/plain", "aGk=")));
    }

    #[test]
    fn build_data_url_roundtrip() {
        let url = build_data_url("image/jpeg", "AAAA");
        assert_eq!(parse_data_url(&url), Some(("image/jpeg", "AAAA")));
    }

    #[test]
    fn sniff_image_mime_recognizes_magic_bytes() {
        assert_eq!(sniff_image_mime("iVBORw0KGgoAAAA"), Some("image/png"));
        assert_eq!(sniff_image_mime("/9j/4AAQ"), Some("image/jpeg"));
        assert_eq!(sniff_image_mime("R0lGODlh"), Some("image/gif"));
        assert_eq!(sniff_image_mime("UklGRi"), Some("image/webp"));
        assert_eq!(sniff_image_mime("aGk="), None);
    }
}