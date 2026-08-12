//! 路由聚合翻译器层 —— 多协议 ↔ 多协议 的请求/响应翻译。
//!
//! CLIProxyAPI aligned: c13dbcc2 - feat(translator): add test and logic to ensure
//!                        object schemas include properties field
//! Source: https://github.com/router-for-me/CLIProxyAPI/commit/c13dbcc24e1373e353338d90bdb38b8e4722e22b
//! Last verified: 2026-08-12
//!
//! 设计参考 CLIProxyAPI `internal/translator/`：
//! - `translator/translator.go` 定义 `Register(from, to, request, response)`
//!   + `Request/Response/ResponseNonStream` 三个 dispatcher
//! - `init.go` 注册所有有效 pair
//!
//! AgentBuddy 用 `TranslatorRegistry` (HashMap<(Format, Format), Arc<dyn Translatable>>)
//! 实现同款 dispatcher；`ProviderRouter` 在转发前查询 source/target 决定走 passthrough
//! 还是翻译。
//!
//! ## Phase 0（当前阶段）
//! 仅搭建骨架：注册表 trait、Format 枚举、StreamParams 流式状态机、跨 pair
//! 共享的 common 工具（tool_name sanitize / schema normalize）。**不实现任何具体
//! pair**，具体 pair 在 Phase 1~6 逐个落地。
//!
//! ## 文件头 CLIProxyAPI aligned 规范
//! 所有翻译器文件第 1-3 行必须包含：
//!   ```
//!   //! Translator: <source> → <target>
//!   //!
//!   //! CLIProxyAPI aligned: <short_sha> - <commit message>
//!   //! Source: https://github.com/router-for-me/CLIProxyAPI/commit/<full_sha>
//!   //! Last verified: <YYYY-MM-DD>
//!   ```
//!
//! ## 同步上游机制
//! - `scripts/check_upstream_sync.py` 每周一 9:00 UTC 跑一次；
//! - 距上次同步 >14 天 CI 报警；
//! - 同步操作手册 `docs/SYNC_PLAYBOOK.md`；
//! - 同步状态记录 `docs/cli_proxy_api_sync_state.json`。
//!
//! ## 用户拍板决策（2026-08-12）
//! 1. pair 命名约定：`<source>_<target>/` 平铺，OK
//! 2. MVP 范围：Phase 0~4，OK
//! 3. 同步检查脚本语言：Python，OK
//! 4. Antigravity 路径：**不实现**
//! 5. 同步 CI 频率：每周一 9:00 UTC + 14 天阈值，OK
//! 6. passthrough 优先级 vs 自建翻译：接受简化（Google OpenAI 兼容端点 passthrough）

// Phase 0 期间 trait / 公共类型尚未被任何 pair 接入，编译期会有大量 dead_code 警告；
// 临时压制整个 translator 子树。Phase 1 接入第一个 pair 后，逐步移除子模块 allow。
#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::Arc;

pub mod claude_gemini;
pub mod common;
pub mod formats;
pub mod gemini_openai_chat;
pub mod gemini_openai_responses;
pub mod openai_gemini;
pub mod openai_openai_responses;
pub mod params;
pub mod translatable;

pub use formats::Format;
#[allow(unused_imports)]
pub use params::StreamParams;
#[allow(unused_imports)]
pub use translatable::TranslateError;
pub use translatable::Translatable;

/// 翻译器注册表：`(source, target) → Translatable impl`。
///
/// 注册表是 `Send + Sync` 的，可直接以 `Arc<TranslatorRegistry>` 形式存放在
/// `RouteAggregationState` 中。`ProviderRouter::forward` 时按 `(Format, Format)`
/// 查询；未注册时回退到 passthrough（仅当上下游协议天然兼容，如 Anthropic ↔ Anthropic）。
///
/// 内部用 `Mutex` 保护 HashMap 以支持 `&self` 注册（setup 阶段一次性调用）。
pub struct TranslatorRegistry {
    inner: std::sync::Mutex<HashMap<(Format, Format), Arc<dyn Translatable>>>,
}

impl Default for TranslatorRegistry {
    fn default() -> Self {
        Self {
            inner: std::sync::Mutex::new(HashMap::new()),
        }
    }
}

impl TranslatorRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// 注册一个 pair。同一对 (source, target) 重复注册后者覆盖前者（用于热重载）。
    pub fn register(
        &self,
        source: Format,
        target: Format,
        translator: Arc<dyn Translatable>,
    ) {
        let mut inner = self.inner.lock().unwrap();
        inner.insert((source, target), translator);
    }

    /// 移除一个 pair（一般用于测试或热重载回滚）。
    pub fn unregister(&self, source: Format, target: Format) -> Option<Arc<dyn Translatable>> {
        self.inner.lock().unwrap().remove(&(source, target))
    }

    /// 查询一个 pair。
    pub fn get(&self, source: Format, target: Format) -> Option<Arc<dyn Translatable>> {
        self.inner.lock().unwrap().get(&(source, target)).cloned()
    }

    /// 是否注册了某个 pair。
    pub fn supports(&self, source: Format, target: Format) -> bool {
        self.inner.lock().unwrap().contains_key(&(source, target))
    }

    /// 已注册 pair 数量（用于 metrics）。
    pub fn len(&self) -> usize {
        self.inner.lock().unwrap().len()
    }

    /// 是否为空注册表（Phase 0 启动时为 true）。
    pub fn is_empty(&self) -> bool {
        self.inner.lock().unwrap().is_empty()
    }
}

impl std::fmt::Debug for TranslatorRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let inner = self.inner.lock().unwrap();
        let mut keys: Vec<_> = inner.keys().collect();
        keys.sort();
        f.debug_struct("TranslatorRegistry")
            .field("pairs", &keys.len())
            .field("keys", &keys)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use translatable::Translatable;

    struct StubTranslator(&'static str);
    impl Translatable for StubTranslator {
        fn translate_request(
            &self,
            _model: &str,
            _raw: &serde_json::Value,
            _stream: bool,
            _params: &mut StreamParams,
        ) -> Result<serde_json::Value, TranslateError> {
            Ok(serde_json::json!({"stub": self.0}))
        }
        fn translate_response_stream(
            &self,
            _model: &str,
            _or: &serde_json::Value,
            _tr: &serde_json::Value,
            _c: &[u8],
            _p: &mut StreamParams,
        ) -> Result<Vec<Vec<u8>>, TranslateError> {
            Ok(vec![])
        }
        fn translate_response_non_stream(
            &self,
            _model: &str,
            _or: &serde_json::Value,
            _tr: &serde_json::Value,
            _r: &[u8],
            _p: &mut StreamParams,
        ) -> Result<Vec<u8>, TranslateError> {
            Ok(vec![])
        }
        fn name(&self) -> &'static str {
            self.0
        }
    }

    #[test]
    fn empty_registry_has_no_pairs() {
        let reg = TranslatorRegistry::new();
        assert!(reg.is_empty());
        assert_eq!(reg.len(), 0);
        assert!(!reg.supports(Format::Anthropic, Format::Gemini));
    }

    #[test]
    fn register_and_lookup() {
        let mut reg = TranslatorRegistry::new();
        reg.register(
            Format::Anthropic,
            Format::Gemini,
            Arc::new(StubTranslator("claude→gemini")),
        );
        assert_eq!(reg.len(), 1);
        assert!(reg.supports(Format::Anthropic, Format::Gemini));

        let t = reg.get(Format::Anthropic, Format::Gemini).unwrap();
        assert_eq!(t.name(), "claude→gemini");
        let mut p = StreamParams::default();
        let v = t.translate_request("m", &serde_json::json!({}), false, &mut p).unwrap();
        assert_eq!(v, serde_json::json!({"stub": "claude→gemini"}));
    }

    #[test]
    fn unregister_removes_pair() {
        let mut reg = TranslatorRegistry::new();
        reg.register(Format::Gemini, Format::Anthropic, Arc::new(StubTranslator("gemini→claude")));
        assert!(reg.supports(Format::Gemini, Format::Anthropic));

        let removed = reg.unregister(Format::Gemini, Format::Anthropic).unwrap();
        assert_eq!(removed.name(), "gemini→claude");
        assert!(!reg.supports(Format::Gemini, Format::Anthropic));
    }

    #[test]
    fn reregister_overrides_previous() {
        let mut reg = TranslatorRegistry::new();
        reg.register(Format::Anthropic, Format::Gemini, Arc::new(StubTranslator("v1")));
        reg.register(Format::Anthropic, Format::Gemini, Arc::new(StubTranslator("v2")));
        let t = reg.get(Format::Anthropic, Format::Gemini).unwrap();
        assert_eq!(t.name(), "v2");
        assert_eq!(reg.len(), 1);
    }

    #[test]
    fn debug_representation_doesnt_leak_translator_state() {
        let mut reg = TranslatorRegistry::new();
        reg.register(Format::Anthropic, Format::Gemini, Arc::new(StubTranslator("claude→gemini")));
        reg.register(Format::Gemini, Format::Anthropic, Arc::new(StubTranslator("gemini→claude")));
        let s = format!("{:?}", reg);
        assert!(s.contains("pairs: 2"));
        // Format 是 derive(Debug)，输出 enum variant 名字（首字母大写）
        assert!(s.contains("Anthropic"));
        assert!(s.contains("Gemini"));
    }
}