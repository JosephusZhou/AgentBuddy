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
    /// Claude Desktop / CodeBuddy CN / WorkBuddy: 顶层 `mcpServers`
    JsonMcpServers,
    /// OpenCode: 顶层 `mcp`（JSON/JSONC）
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
    /// CodeBuddy CN: `~/.codebuddy/{.mcp.json,mcp.json}`（存在优先，默认 `.mcp.json`）。
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

/// agent 的形态：命令行 / 桌面应用 / 两者皆有。
/// 仅描述"安装形态"这一客观事实，供前端展示类型药丸（CLI / 桌面端 / CLI + 桌面端）。
#[derive(Clone, Copy)]
pub enum AgentForm {
    /// 纯命令行工具。
    Cli,
    /// 仅桌面应用（无 CLI 入口，`search_names` 为空）。
    Desktop,
    /// 同时提供 CLI 与桌面应用。
    Hybrid,
}

impl AgentForm {
    /// 前端展示用的稳定小写标识（camelCase JSON 字段）。
    pub fn as_str(self) -> &'static str {
        match self {
            AgentForm::Cli => "cli",
            AgentForm::Desktop => "desktop",
            AgentForm::Hybrid => "hybrid",
        }
    }
}

/// 一个 agent 的完整规格：身份 + 发现 + MCP + Skills。
pub struct AgentSpec {
    pub name: &'static str,
    pub display_name: &'static str,
    pub icon: &'static str,
    /// 安装形态（CLI / 桌面端 / 两者皆有）。
    pub form: AgentForm,
    /// 一句话职责描述，供 Agent 发现页卡片展示。必须与本 spec 的真实路径、
    /// MCP 方言与 Skills 支持度一致，不得臆造未实现的能力。
    pub description: &'static str,

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

    /// 共享物理配置根标识（多个 agent 指向同一物理根时使用）；`None` 表示独立。
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
            push(
                &mut out,
                windows_env_path("LOCALAPPDATA", "Programs/Codex/Codex.exe"),
            );
            push(&mut out, windows_env_path("PROGRAMFILES", "Codex"));
            push(
                &mut out,
                windows_env_path("PROGRAMFILES", "Codex/Codex.exe"),
            );
        }
        "claude-desktop" => {
            push(
                &mut out,
                windows_env_path("LOCALAPPDATA", "Programs/Claude"),
            );
            push(
                &mut out,
                windows_env_path("LOCALAPPDATA", "Programs/Claude/Claude.exe"),
            );
            push(&mut out, windows_env_path("PROGRAMFILES", "Claude"));
            push(
                &mut out,
                windows_env_path("PROGRAMFILES", "Claude/Claude.exe"),
            );
        }
        "opencode" => {
            push(
                &mut out,
                windows_env_path("LOCALAPPDATA", "Programs/OpenCode"),
            );
            push(
                &mut out,
                windows_env_path("LOCALAPPDATA", "Programs/OpenCode/OpenCode.exe"),
            );
            push(&mut out, windows_env_path("PROGRAMFILES", "OpenCode"));
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
        form: AgentForm::Hybrid,
        description: "OpenAI Codex CLI 与 ChatGPT 桌面端共享 ~/.codex，MCP 走 config.toml 的 [mcp_servers]",
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
        form: AgentForm::Cli,
        description: "Anthropic 官方终端编程 Agent，MCP 写入 ~/.claude.json，支持 CLAUDE_CONFIG_DIR 多环境",
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
        form: AgentForm::Desktop,
        description: "Anthropic 官方桌面客户端，MCP 配置在 ~/Library/Application Support 扫描，当前不支持本地 Skills",
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
        form: AgentForm::Hybrid,
        description: "本地多模型 CLI 代理，MCP 走 opencode.json(c) 顶层 mcp，接入 models.dev 统一分派",
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
    // 5. Antigravity（Google）
    AgentSpec {
        name: "antigravity",
        display_name: "Antigravity",
        icon: "Ag",
        form: AgentForm::Hybrid,
        description: "Google Antigravity，agy CLI 与桌面端共享 ~/.gemini，MCP 混用 stdio 与远程 httpUrl",
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
    // 7. CodeBuddy CN（腾讯国内），独占 ~/.codebuddy
    // （原国际版 CodeBuddy 已移除；历史共享根标识保留，旧配置不受影响）
    AgentSpec {
        name: "codebuddy-cn",
        display_name: "CodeBuddy CN",
        icon: "Cn",
        form: AgentForm::Desktop,
        description: "腾讯国内版桌面客户端，MCP 与 Skills 写入 ~/.codebuddy（与 WorkBuddy 共享物理根）",
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
    // 8. WorkBuddy（腾讯）
    AgentSpec {
        name: "workbuddy",
        display_name: "WorkBuddy",
        icon: "Wb",
        form: AgentForm::Hybrid,
        description: "腾讯 WorkBuddy 桌面端与 CLI，MCP 写入 ~/.workbuddy/.mcp.json，与 CodeBuddy CN 共享物理根",
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
    // 9. Pi（badlogic/pi-mono，CLI-only）
    // MCP 非内置：社区标准 pi-mcp-adapter 扩展读取 `~/.pi/agent/mcp.json`（标准 mcpServers）。
    AgentSpec {
        name: "pi",
        display_name: "Pi",
        icon: "Pi",
        form: AgentForm::Cli,
        description: "badlogic/pi-mono 纯 CLI，MCP 依赖社区 pi-mcp-adapter 扩展，Skills 位于 ~/.pi/agent/skills",
        bin_paths: &[
            "~/.local/bin/pi",
            "/usr/local/bin/pi",
            "/opt/homebrew/bin/pi",
        ],
        search_names: &["pi"],
        config_paths: &["~/.pi"],
        scan_app_support: false,
        mcp: McpSpec {
            dialect: McpDialect::JsonMcpServers,
            path: McpPath::Fixed(".pi/agent/mcp.json"),
            jsonc: false,
        },
        skills_roots: &["~/.pi/agent/skills"],
        skills_supported: true,
        shared_root: None,
    },
    // 10. Oh-My-Pi（can1357/oh-my-pi，CLI `omp`，Pi 的全功能 fork）
    AgentSpec {
        name: "oh-my-pi",
        display_name: "Oh-My-Pi",
        icon: "Om",
        form: AgentForm::Cli,
        description: "Pi 的全功能 fork（omp），纯 CLI，MCP 与 Skills 位于 ~/.omp/agent",
        bin_paths: &[
            "~/.bun/bin/omp",
            "~/.local/bin/omp",
            "/usr/local/bin/omp",
            "/opt/homebrew/bin/omp",
        ],
        search_names: &["omp"],
        config_paths: &["~/.omp"],
        scan_app_support: false,
        mcp: McpSpec {
            dialect: McpDialect::JsonMcpServers,
            path: McpPath::Fixed(".omp/agent/mcp.json"),
            jsonc: false,
        },
        skills_roots: &["~/.omp/agent/skills"],
        skills_supported: true,
        shared_root: None,
    },
];
