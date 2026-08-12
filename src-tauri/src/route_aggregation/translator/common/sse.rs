//! SSE chunk 解析工具 —— 把上游分块到达的字节流拆成完整 SSE 行。
//!
//! CLIProxyAPI aligned: 150e7f0 - fix(auth): repair force-mapped Responses SSE
//!                        framing for WS forwarder
//! Source: https://github.com/router-for-me/CLIProxyAPI/commit/150e7f0dc50e3d3a0f7c4e552cc402ae105eb2a0
//! Last verified: 2026-08-12
//!
//! Gemini / Codex 的流式响应经常省略尾部换行；Antigravity 完全不依赖换行；
//! 必须在翻译器内部维持一个"未完成行 buffer"，跨多次 `raw_chunk` 累积。CLIProxyAPI
//! 的修复 `safeReplaceGlued` / `safeReplaceGluedComplete` 专门处理 chunk 边界
//! 拼接问题。

/// 维持一个 SSE 行缓冲；每次喂入新 chunk，返回完整的行（不含分隔符 `\n\n`）。
///
/// 行的语义是：以 `\n\n` 结束的事件（包含 `event:` / `data:` / 空行）。
#[derive(Debug, Default)]
pub struct SseLineBuffer {
    pending: Vec<u8>,
}

impl SseLineBuffer {
    pub fn new() -> Self {
        Self::default()
    }

    /// 喂入一段 chunk，返回所有完整的 SSE 行（不含末尾 `\n\n`）。
    pub fn feed(&mut self, chunk: &[u8]) -> Vec<Vec<u8>> {
        self.pending.extend_from_slice(chunk);

        let mut lines = Vec::new();
        while let Some(idx) = find_event_separator(&self.pending) {
            // drain(..idx) 返回 idx 之前的内容并 remove 之
            let line: Vec<u8> = self.pending.drain(..idx).collect();
            // 跳过 "\n\n"
            self.pending.drain(..2);
            lines.push(line);
        }
        lines
    }

    /// 当前未闭合的剩余字节（用于 stream 结束时 flush）。
    pub fn flush(&mut self) -> Option<Vec<u8>> {
        if self.pending.is_empty() {
            None
        } else {
            Some(std::mem::take(&mut self.pending))
        }
    }
}

fn find_event_separator(buf: &[u8]) -> Option<usize> {
    if buf.len() < 2 {
        return None;
    }
    for i in 0..buf.len().saturating_sub(1) {
        if buf[i] == b'\n' && buf[i + 1] == b'\n' {
            return Some(i);
        }
    }
    None
}

/// 从一行里抽出 `data: ` 之后的 JSON 内容（不含 data 前缀）。
///
/// 兼容 `data:`（无空格）和 `data: `（有空格）两种格式。
pub fn extract_data(line: &[u8]) -> Option<&[u8]> {
    if line.starts_with(b"data:") {
        let rest = &line[5..];
        let rest = rest.strip_prefix(b" ").unwrap_or(rest);
        Some(rest)
    } else {
        None
    }
}

/// 提取 `event: xxx` 的事件名。
pub fn extract_event(line: &[u8]) -> Option<&[u8]> {
    if line.starts_with(b"event:") {
        let rest = &line[6..];
        let rest = rest.strip_prefix(b" ").unwrap_or(rest);
        Some(rest)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn feed_single_complete_event() {
        let mut buf = SseLineBuffer::new();
        let chunk = b"data: {\"a\":1}\n\n";
        let lines = buf.feed(chunk);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0], b"data: {\"a\":1}");
    }

    #[test]
    fn feed_splits_two_events() {
        let mut buf = SseLineBuffer::new();
        let chunk = b"data: {\"a\":1}\n\ndata: {\"b\":2}\n\n";
        let lines = buf.feed(chunk);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], b"data: {\"a\":1}");
        assert_eq!(lines[1], b"data: {\"b\":2}");
    }

    #[test]
    fn feed_splits_event_across_chunks() {
        let mut buf = SseLineBuffer::new();
        let first = buf.feed(b"data: {\"a\"");
        assert_eq!(first.len(), 0);
        let second = buf.feed(b":1}\n\n");
        assert_eq!(second.len(), 1);
        assert_eq!(second[0], b"data: {\"a\":1}");
    }

    #[test]
    fn feed_handles_no_trailing_newline() {
        let mut buf = SseLineBuffer::new();
        let lines = buf.feed(b"data: {\"a\":1}");
        assert_eq!(lines.len(), 0);
        let tail = buf.flush().unwrap();
        assert_eq!(tail, b"data: {\"a\":1}");
    }

    #[test]
    fn extract_data_with_and_without_space() {
        assert_eq!(extract_data(b"data: {\"a\":1}"), Some(b"{\"a\":1}".as_slice()));
        assert_eq!(extract_data(b"data:{\"a\":1}"), Some(b"{\"a\":1}".as_slice()));
        assert_eq!(extract_data(b"event: ping"), None);
    }

    #[test]
    fn extract_event_basic() {
        assert_eq!(extract_event(b"event: ping"), Some(b"ping".as_slice()));
        assert_eq!(extract_event(b"data: ping"), None);
    }
}