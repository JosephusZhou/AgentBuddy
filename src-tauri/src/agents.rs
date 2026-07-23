//! 集中式 Agent 注册表：agent 身份 + 发现路径 + MCP 方言/路径 + Skills 根，
//! 作为 `sniff` / `mcp_config` / `skills` 三处的单一数据源。
//!
//! 新增一个 agent 只需在 `AGENTS` 里加一条 `AgentSpec`（外加 `mcp_config` 中
//! 若有全新的路径解析方式才需扩展 `McpPath`）。`name` 是跨模块隐式主键，务必唯一稳定。

/// MCP 配置文件的写入方言。变体名与 `mcp_config` 内的分派保持一致。
#[derive(Clone, Copy)]
pub enum McpDialect {
    /// Codex: `config.toml` → `[mcp_servers.*]`
    TomlMcpServers,
    /// Claude Desktop / Kiro / CodeBuddy* / WorkBuddy: 顶层 `mcpServers`
    JsonMcpServers,
    /// OpenCode / DevEco: 顶层 `mcp`（JSON/JSONC）
    JsonMcp,
    /// Antigravity / Gemini: `mcpServers`，远程用 `httpUrl`
    JsonGeminiMixed,
    /// Claude Code: `~/.claude.json` 顶层 `mcpServers`
    ClaudeJsonUser,
}

/// MCP 配置文件路径的解析方式（含需运行时探测的特殊场景）。
#[derive(Clone, Copy)]
pub enum McpPath {
    /// 相对用户主目录的固定路径，如 `.codex/config.toml`。
    Fixed(&'static str),
    /// OpenCode: `~/.config/opencode/opencode.{jsonc,json}`（存在优先，默认 `.json`）。
    OpencodeConfig,
    /// CodeBuddy: `~/.codebuddy/{.mcp.json,mcp.json}`（存在优先，默认 `.mcp.json`）。
    CodebuddyMcp,
    /// Claude Desktop: 在 `~/Library/Application Support` 下扫描。
    ClaudeDesktopScan,
}

/// 单个 agent 的 MCP 映射。
pub struct McpSpec {
    pub dialect: McpDialect,
    pub path: McpPath,
    /// 是否按 JSONC（json5）解析写入。`OpencodeConfig` 由运行时依扩展名判定，此处填 `false`。
    pub jsonc: bool,
}

/// 一个 agent 的完整规格：身份 + 发现 + MCP + Skills。
pub struct AgentSpec {
    pub name: &'static str,
    pub display_name: &'static str,
    pub icon: &'static str,

    // ---- 发现（sniff）----
    /// 静态安装路径（App 和/或 CLI 的固定位置）。
    pub bin_paths: &'static [&'static str],
    /// 在 `PATH` 中搜索的 CLI 可执行名（按顺序，命中即止）。
    pub search_names: &'static [&'static str],
    /// 配置目录候选。
    pub config_paths: &'static [&'static str],
    /// 是否扫描 `~/Library/Application Support` 找 Claude Desktop 配置目录。
    pub scan_app_support: bool,

    // ---- MCP ----
    pub mcp: McpSpec,

    // ---- Skills ----
    /// Skills 目录候选（相对 `~`）；扫描按序、写入取第一个。
    pub skills_roots: &'static [&'static str],
    pub skills_supported: bool,

    /// 共享物理配置根标识（CodeBuddy 与 CodeBuddy CN 相同）；`None` 表示独立。
    /// 用于 MCP/Skills 写入去重，避免对同一物理根写两次。
    pub shared_root: Option<&'static str>,
}

/// 全部已登记的 agent。顺序即 UI 呈现与扫描顺序。
pub fn agents() -> &'static [AgentSpec] {
    AGENTS
}

/// 按 sniff `name` 查找规格。
pub fn find(name: &str) -> Option<&'static AgentSpec> {
    AGENTS.iter().find(|a| a.name == name)
}

/// Windows 上额外的静态安装路径候选（从环境变量解析，不硬编码盘符）。
/// 在 `cfg(windows)` 的 sniff 中与 `bin_paths` 合并；其它平台返回空。
#[cfg(windows)]
pub fn windows_bin_candidates(spec: &AgentSpec) -> Vec<std::path::PathBuf> {
    use crate::platform::windows_env_path;
    let mut out = Vec::new();
    let push = |out: &mut Vec<std::path::PathBuf>, p: Option<std::path::PathBuf>| {
        if let Some(p) = p {
            if !out.iter().any(|x| x == &p) {
                out.push(p);
            }
        }
    };

    match spec.name {
        "codex" => {
            push(&mut out, windows_env_path("LOCALAPPDATA", "Programs/Codex"));
            push(&mut out, windows_env_path("LOCALAPPDATA", "Programs/Codex/Codex.exe"));
            push(&mut out, windows_env_path("PROGRAMFILES", "Codex"));
            push(&mut out, windows_env_path("PROGRAMFILES", "Codex/Codex.exe"));
        }
        "claude-desktop" => {
            push(&mut out, windows_env_path("LOCALAPPDATA", "Programs/Claude"));
            push(&mut out, windows_env_path("LOCALAPPDATA", "Programs/Claude/Claude.exe"));
            push(&mut out, windows_env_path("PROGRAMFILES", "Claude"));
            push(&mut out, windows_env_path("PROGRAMFILES", "Claude/Claude.exe"));
        }
        "opencode" => {
            push(&mut out, windows_env_path("LOCALAPPDATA", "Programs/OpenCode"));
            push(
                &mut out,
                windows_env_path("LOCALAPPDATA", "Programs/OpenCode/OpenCode.exe"),
            );
            push(&mut out, windows_env_path("PROGRAMFILES", "OpenCode"));
        }
        "kiro" => {
            push(&mut out, windows_env_path("LOCALAPPDATA", "Programs/Kiro"));
            push(&mut out, windows_env_path("LOCALAPPDATA", "Programs/Kiro/Kiro.exe"));
        }
        "antigravity" => {
            push(
                &mut out,
                windows_env_path("LOCALAPPDATA", "Programs/Antigravity"),
            );
            push(
                &mut out,
                windows_env_path("LOCALAPPDATA", "Programs/Antigravity/Antigravity.exe"),
            );
        }
        "codebuddy" => {
            push(
                &mut out,
                windows_env_path("LOCALAPPDATA", "Programs/CodeBuddy"),
            );
            push(
                &mut out,
                windows_env_path("LOCALAPPDATA", "Programs/CodeBuddy/CodeBuddy.exe"),
            );
        }
        "codebuddy-cn" => {
            push(
                &mut out,
                windows_env_path("LOCALAPPDATA", "Programs/CodeBuddy CN"),
            );
            push(
                &mut out,
                windows_env_path("LOCALAPPDATA", "Programs/CodeBuddy CN/CodeBuddy.exe"),
            );
        }
        "workbuddy" => {
            push(
                &mut out,
                windows_env_path("LOCALAPPDATA", "Programs/WorkBuddy"),
            );
            push(
                &mut out,
                windows_env_path("LOCALAPPDATA", "Programs/WorkBuddy/WorkBuddy.exe"),
            );
        }
        // CLI-only agents rely on PATH + PATHEXT; no extra App paths.
        _ => {}
    }
    out
}


static AGENTS: &[AgentSpec] = &[
    // 1. Codex（CLI + App，共享 ~/.codex）
    AgentSpec {
        name: "codex",
        display_name: "Codex",
        icon: "Co",
        bin_paths: &[
            "/usr/local/bin/codex",
            "/opt/homebrew/bin/codex",
            "/Applications/ChatGPT.app",
        ],
        search_names: &["codex"],
        config_paths: &["~/.codex"],
        scan_app_support: false,
        mcp: McpSpec {
            dialect: McpDialect::TomlMcpServers,
            path: McpPath::Fixed(".codex/config.toml"),
            jsonc: false,
        },
        skills_roots: &["~/.codex/skills", "~/.agents/skills"],
        skills_supported: true,
        shared_root: None,
    },
    // 2. Claude Code CLI
    AgentSpec {
        name: "claude-code",
        display_name: "Claude Code",
        icon: "Cc",
        bin_paths: &[
            "~/.local/bin/claude",
            "/usr/local/bin/claude",
            "/opt/homebrew/bin/claude",
        ],
        search_names: &["claude", "claude-code"],
        config_paths: &["~/.claude"],
        scan_app_support: false,
        mcp: McpSpec {
            dialect: McpDialect::ClaudeJsonUser,
            path: McpPath::Fixed(".claude.json"),
            jsonc: false,
        },
        skills_roots: &["~/.claude/skills"],
        skills_supported: true,
        shared_root: None,
    },
    // 3. Claude Desktop App
    AgentSpec {
        name: "claude-desktop",
        display_name: "Claude Desktop",
        icon: "Cd",
        bin_paths: &["/Applications/Claude.app"],
        search_names: &[],
        config_paths: &[],
        scan_app_support: true,
        mcp: McpSpec {
            dialect: McpDialect::JsonMcpServers,
            path: McpPath::ClaudeDesktopScan,
            jsonc: false,
        },
        skills_roots: &[],
        skills_supported: false,
        shared_root: None,
    },
    // 4. OpenCode（CLI + App，共享配置）
    AgentSpec {
        name: "opencode",
        display_name: "OpenCode",
        icon: "Oc",
        bin_paths: &[
            "/usr/local/bin/opencode",
            "/opt/homebrew/bin/opencode",
            "~/.opencode/bin/opencode",
            "/Applications/OpenCode.app",
        ],
        search_names: &["opencode"],
        config_paths: &["~/.config/opencode"],
        scan_app_support: false,
        mcp: McpSpec {
            dialect: McpDialect::JsonMcp,
            path: McpPath::OpencodeConfig,
            jsonc: false,
        },
        skills_roots: &["~/.config/opencode/skills"],
        skills_supported: true,
        shared_root: None,
    },
    // 5. DevEco Code CLI（华为，npm 安装）
    AgentSpec {
        name: "deveco-code",
        display_name: "DevEco Code",
        icon: "De",
        bin_paths: &[],
        search_names: &["deveco", "devecocli"],
        config_paths: &["~/.config/deveco"],
        scan_app_support: false,
        mcp: McpSpec {
            dialect: McpDialect::JsonMcp,
            path: McpPath::Fixed(".config/deveco/deveco.jsonc"),
            jsonc: true,
        },
        skills_roots: &["~/.config/deveco/skills"],
        skills_supported: true,
        shared_root: None,
    },
    // 6. Kiro CLI（AWS）
    AgentSpec {
        name: "kiro",
        display_name: "Kiro",
        icon: "Ki",
        bin_paths: &["~/.local/bin/kiro-cli", "/Applications/Kiro CLI.app"],
        search_names: &["kiro-cli", "kiro"],
        config_paths: &["~/.kiro"],
        scan_app_support: false,
        mcp: McpSpec {
            dialect: McpDialect::JsonMcpServers,
            path: McpPath::Fixed(".kiro/settings/mcp.json"),
            jsonc: false,
        },
        skills_roots: &["~/.kiro/skills"],
        skills_supported: true,
        shared_root: None,
    },
    // 7. Antigravity（Google）
    AgentSpec {
        name: "antigravity",
        display_name: "Antigravity",
        icon: "Ag",
        bin_paths: &["~/.local/bin/agy", "/Applications/Antigravity.app"],
        search_names: &["agy", "antigravity"],
        config_paths: &["~/.gemini"],
        scan_app_support: false,
        mcp: McpSpec {
            dialect: McpDialect::JsonGeminiMixed,
            path: McpPath::Fixed(".gemini/settings.json"),
            jsonc: false,
        },
        skills_roots: &["~/.gemini/skills", "~/.gemini/antigravity-cli/skills"],
        skills_supported: true,
        shared_root: None,
    },
    // 8. CodeBuddy（腾讯）
    AgentSpec {
        name: "codebuddy",
        display_name: "CodeBuddy",
        icon: "Cb",
        bin_paths: &["~/.local/bin/codebuddy", "/Applications/CodeBuddy.app"],
        search_names: &["codebuddy"],
        config_paths: &["~/.codebuddy"],
        scan_app_support: false,
        mcp: McpSpec {
            dialect: McpDialect::JsonMcpServers,
            path: McpPath::CodebuddyMcp,
            jsonc: false,
        },
        skills_roots: &["~/.codebuddy/skills"],
        skills_supported: true,
        shared_root: Some("codebuddy-shared"),
    },
    // 9. CodeBuddy CN（腾讯国内），与 CodeBuddy 共享 ~/.codebuddy
    AgentSpec {
        name: "codebuddy-cn",
        display_name: "CodeBuddy CN",
        icon: "Cn",
        bin_paths: &["/Applications/CodeBuddy CN.app"],
        search_names: &[],
        config_paths: &["~/.codebuddy"],
        scan_app_support: false,
        mcp: McpSpec {
            dialect: McpDialect::JsonMcpServers,
            path: McpPath::CodebuddyMcp,
            jsonc: false,
        },
        skills_roots: &["~/.codebuddy/skills"],
        skills_supported: true,
        shared_root: Some("codebuddy-shared"),
    },
    // 10. WorkBuddy（腾讯）
    AgentSpec {
        name: "workbuddy",
        display_name: "WorkBuddy",
        icon: "Wb",
        bin_paths: &["/Applications/WorkBuddy.app"],
        search_names: &["workbuddy"],
        config_paths: &["~/.workbuddy"],
        scan_app_support: false,
        mcp: McpSpec {
            dialect: McpDialect::JsonMcpServers,
            path: McpPath::Fixed(".workbuddy/.mcp.json"),
            jsonc: false,
        },
        skills_roots: &["~/.workbuddy/skills"],
        skills_supported: true,
        shared_root: None,
    },
];
