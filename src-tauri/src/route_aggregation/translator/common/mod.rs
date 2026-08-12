//! 跨 pair 共享工具。Phase 0 落地基础工具；多模态 / thinking 等大块逻辑在后续
//! Phase 实现。
//!
//! CLIProxyAPI aligned: ac8fb97 - feat(thinking): remove thinkingConfig for ModeNone
//!                        with zero budget and no level
//! Source: https://github.com/router-for-me/CLIProxyAPI/commit/ac8fb9706fb84bedfbd1f813738680fdc6767115
//! Last verified: 2026-08-12

pub mod claude_messages;
pub mod http;
pub mod id_map;
pub mod multimodal;
pub mod schema;
pub mod sse;
pub mod thinking;
pub mod tool_name;