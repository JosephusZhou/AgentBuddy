//! 通用模型配置入口：按 agent 标识分派到各自后端。
//!
//! - `opencode` → `opencode_config`（`~/.config/opencode/opencode.json(c)`）
//! - `pi` → `pi_model_config`（`~/.pi/agent/models.json`）
//!
//! DTO 与 models.dev 目录能力统一放在 `opencode_config`，供各后端共享。

/// 支持可视化模型配置的 agent。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelConfigAgent {
    Opencode,
    Pi,
}

impl ModelConfigAgent {
    /// 解析前端传入的 agent 标识。
    pub fn parse(s: &str) -> Result<Self, String> {
        match s.trim().to_ascii_lowercase().as_str() {
            "opencode" => Ok(Self::Opencode),
            "pi" => Ok(Self::Pi),
            other => Err(format!("未知的模型配置 Agent: {other}")),
        }
    }

    /// agents 注册表中的 name（用于 sniff 等）。
    pub fn id(&self) -> &'static str {
        match self {
            Self::Opencode => "opencode",
            Self::Pi => "pi",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_agent_ids() {
        assert_eq!(
            ModelConfigAgent::parse("opencode").unwrap(),
            ModelConfigAgent::Opencode
        );
        assert_eq!(ModelConfigAgent::parse("Pi").unwrap(), ModelConfigAgent::Pi);
        assert!(ModelConfigAgent::parse("codex").is_err());
        assert!(ModelConfigAgent::parse("omp").is_err());
    }
}
