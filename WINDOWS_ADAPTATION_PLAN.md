# AgentBuddy Windows 系统适配方案

## 背景与目标

AgentBuddy 当前是 Tauri 2 + React/Vite + Rust 的桌面应用。前端基本具备跨平台基础，Windows 适配的主要工作集中在 Rust 后端：路径解析、Agent 发现、配置文件定位、打开文件管理器、shell 别名、软链接、文件权限、备份恢复边界。

本方案只描述实施方案，不执行构建、测试、安装或迁移动作。

## 适配目标

1. 在 Windows 10 22H2 及 Windows 11 上可开发、构建、安装、启动。
2. 已有 macOS 行为不回退，Linux 行为不被误伤。
3. Windows 下能正确识别常见 CLI 和 App 安装位置。
4. Windows 下能正确读写 AgentBuddy 自身数据、MCP 配置、Skills、备份文件。
5. 涉及 shell 或文件管理器的功能使用 Windows 原生方式，并有清晰降级。

## 非目标

1. 不在本阶段重构 UI 视觉。
2. 不改变现有 MCP 配置方言。
3. 不把不同 Agent 的真实配置目录强行迁移到 Windows 标准目录；优先兼容各 Agent 自身约定。
4. 不访问、扫描或备份敏感密钥目录，例如 `.ssh`、系统凭据库、浏览器密码库。

## 当前 Windows 阻塞点

### 编译阻塞

`src-tauri/src/codex_env.rs`、`src-tauri/src/claude_env.rs`、`src-tauri/src/opencode_config.rs`、`src-tauri/src/backup.rs` 使用了 `std::os::unix::fs::PermissionsExt`。该模块在 Windows 不存在，会直接导致编译失败。

`src-tauri/src/skills.rs` 使用 `std::os::unix::fs::symlink`。Windows 需要使用 `std::os::windows::fs::{symlink_dir, symlink_file}`，且普通用户可能没有创建符号链接的权限。

### 运行时阻塞

`src-tauri/src/agent_open.rs`、`src-tauri/src/opencode_config.rs`、`src-tauri/src/claude_env.rs`、`src-tauri/src/codex_env.rs` 使用 `Command::new("open")`，这是 macOS 命令。Windows 需要替换为 `explorer.exe` 或 Tauri opener 能力。

`src-tauri/src/sniff.rs` 用 `PATH.split(':')` 搜索命令。Windows 的 PATH 分隔符是 `;`，并且可执行文件可能是 `.exe`、`.cmd`、`.bat`。

### 行为风险

Agent 注册表里大量固定路径是 macOS 或 Unix 风格，例如 `/Applications/*.app`、`/usr/local/bin/*`、`/opt/homebrew/bin/*`、`~/.local/bin/*`。Windows 需要补充候选路径，但不能删除现有路径。

Claude Desktop 当前只扫描 `~/Library/Application Support`。Windows 需要扫描 `%APPDATA%\Claude` 及可能的 `Claude-*` 目录。

Codex/Claude 多环境目前默认写入 `.zshrc`、`.bashrc` 或 fish 配置。Windows 上应该支持 PowerShell Profile，同时保留“不自动写入别名”的默认策略。

备份恢复允许的绝对路径白名单包含 `/opt`、`/usr/local`、`/etc` 等 Unix 路径。Windows 需要独立白名单，否则恢复逻辑要么误拒绝，要么放开过度。

## 总体设计

### 平台抽象层

新增一个 Rust 平台模块，例如 `src-tauri/src/platform.rs`，集中封装以下能力：

1. `app_data_dir() -> Result<PathBuf, String>`
   - 优先使用 `dirs::data_local_dir()` 或 `dirs::config_dir()` 下的 `AgentBuddy`。
   - 兼容已有 `~/.agentbuddy`，迁移策略见后文。

2. `home_dir() -> Result<PathBuf, String>`
   - 保持 `dirs::home_dir()`，统一错误信息。

3. `expand_home(input: &str) -> Result<PathBuf, String>`
   - 支持 `~`、`~/foo`。
   - Windows 下额外接受 `%USERPROFILE%`、`%APPDATA%`、`%LOCALAPPDATA%` 展示格式，但内部统一转 `PathBuf`。

4. `path_list_separator() -> char`
   - Windows 返回 `;`，其他系统返回 `:`。
   - 也可直接使用 `std::env::split_paths`，更稳妥。

5. `candidate_executable_names(name: &str) -> Vec<String>`
   - Windows 下返回 `name.exe`、`name.cmd`、`name.bat`、原名。
   - 其他系统返回原名。

6. `open_path(path: &Path) -> Result<(), String>`
   - macOS: `open -- <path>`。
   - Windows: `explorer.exe <path>`。
   - Linux: `xdg-open <path>`。
   - 路径参数必须作为单独参数传入，禁止拼接 shell 字符串。

7. `reveal_path(path: &Path) -> Result<(), String>`
   - macOS: `open -R <path>`。
   - Windows: `explorer.exe /select,<path>`，路径缺失时打开父目录。
   - Linux: 打开父目录。

8. `set_owner_only_permissions(path: &Path)`
   - Unix 下保留 `0o600` / `0o700`。
   - Windows 下先 no-op，后续可接入 ACL；不要为了“权限一致”导致编译失败。

9. `symlink_or_copy_dir(source, dest)`
   - Unix 下继续使用 symlink。
   - Windows 下优先 `symlink_dir`，失败时回退为目录复制，并在状态里标记 `copy`。
   - 对 Skills 应用功能，UI 文案需要区分“链接”和“复制同步”。

### 路径策略

AgentBuddy 自身数据建议迁移为：

| 类型 | 当前路径 | Windows 建议路径 | 兼容策略 |
| --- | --- | --- | --- |
| 应用配置 | `~/.agentbuddy/config.json` | `%APPDATA%\AgentBuddy\config.json` 或 `%LOCALAPPDATA%\AgentBuddy\config.json` | 启动时先读新路径；新路径不存在且旧路径存在时继续读旧路径，并提示可迁移 |
| SQLite | `~/.agentbuddy/agents.db` | 与应用配置同目录 | 同上 |
| Skills 库 | `~/.agentbuddy/skills` | 与应用配置同目录的 `skills` | 同上 |
| 模型缓存 | `~/.agentbuddy/cache` | 与应用配置同目录的 `cache` | 同上 |

Agent 自身配置路径不强制标准化，因为 CLI 工具往往仍使用类 Unix dotdir。Windows 下允许 `C:\Users\<user>\.codex`、`C:\Users\<user>\.claude`、`C:\Users\<user>\.config\opencode` 这类路径。

### Agent 发现路径扩展

在 `src-tauri/src/agents.rs` 的 `AgentSpec` 里引入平台化候选路径，不建议把 Windows 路径直接塞进现有 `bin_paths` 字段里长期维护。建议改为：

```rust
pub struct PlatformPaths {
    pub unix: &'static [&'static str],
    pub macos: &'static [&'static str],
    pub windows: &'static [&'static str],
}
```

短期可用低风险方式：保留 `bin_paths`，新增 `windows_bin_paths` 和 `windows_config_paths`，`sniff` 根据 `cfg!(windows)` 合并候选。

建议 Windows 候选：

| Agent | CLI 候选 | App/目录候选 | 配置候选 |
| --- | --- | --- | --- |
| Codex | `codex.exe`、`codex.cmd`、`codex.bat` | `%LOCALAPPDATA%\Programs\Codex`、`%PROGRAMFILES%\Codex` | `%USERPROFILE%\.codex` |
| Claude Code | `claude.exe`、`claude.cmd`、`claude.bat` | 空 | `%USERPROFILE%\.claude`、`%USERPROFILE%\.claude.json` |
| Claude Desktop | 空 | `%LOCALAPPDATA%\Programs\Claude`、`%PROGRAMFILES%\Claude` | `%APPDATA%\Claude\claude_desktop_config.json` |
| OpenCode | `opencode.exe`、`opencode.cmd`、`opencode.bat` | `%LOCALAPPDATA%\Programs\OpenCode` | `%USERPROFILE%\.config\opencode` |
| DevEco Code | `deveco.exe`、`devecocli.exe`、对应 `.cmd` | 视安装器补充 | `%USERPROFILE%\.config\deveco` |
| Kiro | `kiro.exe`、`kiro-cli.exe`、对应 `.cmd` | `%LOCALAPPDATA%\Programs\Kiro` | `%USERPROFILE%\.kiro` |
| Antigravity | `agy.exe`、`antigravity.exe`、对应 `.cmd` | `%LOCALAPPDATA%\Programs\Antigravity` | `%USERPROFILE%\.gemini` |
| CodeBuddy | `codebuddy.exe`、对应 `.cmd` | `%LOCALAPPDATA%\Programs\CodeBuddy` | `%USERPROFILE%\.codebuddy` |
| CodeBuddy CN | 视安装器补充 | `%LOCALAPPDATA%\Programs\CodeBuddy CN` | `%USERPROFILE%\.codebuddy` |
| WorkBuddy | `workbuddy.exe`、对应 `.cmd` | `%LOCALAPPDATA%\Programs\WorkBuddy` | `%USERPROFILE%\.workbuddy` |

实际实现时不要硬编码 `%VAR%` 字符串直接创建 `PathBuf`，需要先从 `std::env::var_os` 解析。

## 模块改造清单

### `config.rs` 与 `db.rs`

1. 把 `app_dir()` 和 `get_db_path()` 改为调用平台模块。
2. 保留旧路径读取能力，避免升级后用户数据消失。
3. 迁移必须是可恢复的：复制到新目录成功后再写入迁移标记，不删除旧目录。
4. 错误信息中避免泄露敏感路径内容；普通配置路径可以展示，密钥值不能展示。

### `sniff.rs`

1. 用 `std::env::split_paths` 代替 `PATH.split(':')`。
2. Windows 下搜索 `PATHEXT`，未设置时默认 `.EXE;.CMD;.BAT;.COM`。
3. `is_shim_path` 需要兼容反斜杠和大小写：
   - 路径比较用 normalized lowercase。
   - 临时目录判断用 `Path::starts_with`，不要靠字符串 `/`。
4. App 优先排序规则改为跨平台：
   - macOS: `.app` 优先。
   - Windows: `.exe` 或安装目录优先。
   - 其他系统: 原顺序。

### `mcp_config.rs`

1. `McpPath::ClaudeDesktopScan` 分平台解析：
   - macOS: 现有 `~/Library/Application Support/Claude`。
   - Windows: `%APPDATA%\Claude\claude_desktop_config.json`，并扫描 `%APPDATA%\Claude-*`。
   - Linux: 使用 `dirs::config_dir()` 下的 Claude 候选。
2. 固定相对路径继续相对 home，例如 `.codex/config.toml` 在 Windows 会自然变成 `C:\Users\<user>\.codex\config.toml`。
3. atomic write 继续使用同目录临时文件 + rename；Windows rename 目标存在时可能失败，保留现有 fallback 写入，但要在 fallback 前尽量删除临时文件。
4. `test_stdio` 使用 `Command::new(command)` 和 args 是正确方向，不应改成 shell 拼接；Windows 下要允许 `.cmd` 命令通过 PATH 搜索。

### `agent_open.rs`

1. `open_existing_path` 改用 `platform::open_path`。
2. `reveal_config_dir` 和 `open_config_file` 的安全边界保持不变：只能打开后端解析出的路径。
3. Windows 下 `reveal` 文件时使用 `explorer.exe /select,<file>`；目录则直接打开目录。

### `opencode_config.rs`

1. 移除顶层 `std::os::unix::fs::PermissionsExt` 直接依赖，改用平台权限函数。
2. `auth_path()` 继续兼容 `~/.local/share/opencode/auth.json`，必要时新增 Windows 候选 `%APPDATA%\opencode\auth.json`，但默认写入路径应和 OpenCode 官方实际行为一致后再定。
3. `reveal_config()` 改用平台 `open_path` / `reveal_path`。
4. 模型目录缓存 `~/.agentbuddy/cache/models-dev.json` 改走 AgentBuddy app data 目录。

### `claude_env.rs` 与 `codex_env.rs`

1. 移除 Unix-only permissions 直接依赖。
2. shell 别名能力改为分平台：
   - Unix/macOS: 保持 zsh/bash/fish。
   - Windows PowerShell: 写入 `$PROFILE.CurrentUserCurrentHost` 或 `$PROFILE.CurrentUserAllHosts`，建议默认只预览不自动写入。
   - Windows cmd: 不建议自动写入别名，可生成 `.cmd` shim 到 AgentBuddy 管理目录，并提示用户把目录加入 PATH。
3. DTO 字段 `zshrc_path` 建议重命名为 `shell_config_path`。为兼容前端，可短期同时返回旧字段和新字段。
4. 别名语法：
   - Claude: `function <alias> { $env:CLAUDE_CONFIG_DIR = '<path>'; claude @args }`
   - Codex: `function <alias> { $env:CODEX_HOME = '<path>'; codex @args }`
   - 注意函数结束后是否清理环境变量，需要按用户预期决定。建议保存旧值、执行后恢复。
5. Windows 路径展示不要强行替换成 `$HOME/...`，PowerShell 文案可用 `$HOME\...`，UI 展示可用 `~\...`。

### `skills.rs`

1. 抽象 skill 安装动作：`link`、`copy`、`remove`。
2. Windows 下优先尝试目录 symlink；失败时复制目录。
3. 复制模式需要记录来源和更新时间，否则远端 skill 更新后无法判断是否同步。
4. 删除时只删除 AgentBuddy 管理过的链接或复制目录，禁止删除用户原有同名目录。
5. 路径比较必须 canonicalize 后再判断，避免大小写和反斜杠导致重复应用。

### `backup.rs`

1. Unix-only permissions 使用 `cfg(unix)` 包裹，Windows 下 no-op 或 ACL 后续实现。
2. 备份源发现改用平台路径：
   - AgentBuddy 数据目录。
   - Agent 注册表配置目录。
   - Windows 下可选 cliproxy/sub2api 配置路径另行补充，不复用 `/etc`。
3. 恢复白名单按平台拆分：
   - Windows: 只允许 `%USERPROFILE%`、AgentBuddy app data 目录，以及明确配置的用户选择目录。
   - 默认禁止写入 `C:\Windows`、`C:\Program Files`、`C:\ProgramData`。
4. zip 内路径继续使用 `/` 作为归档分隔符，不受 Windows 影响。
5. 恢复时必须防 zip slip：归档路径 normalize 后必须仍在目标根内。

### Tauri 配置

1. `src-tauri/tauri.conf.json` 已包含 `icons/icon.ico`，Windows 图标路径具备基础条件。
2. `titleBarStyle: "Overlay"` 在 Windows 上可能表现不一致。建议按平台配置窗口样式，Windows 使用普通标题栏，macOS 保留 overlay。
3. 如果启用文件打开能力，优先使用 Tauri 官方 plugin opener，并在 capabilities 中最小化授权。
4. Windows 打包产物至少验证 NSIS 和 MSI 之一；当前 `targets: "all"` 会生成多个目标，CI 阶段可先收敛为明确目标。

## 建议实施顺序

### 第一阶段：让 Windows 可编译

1. 新增 `platform.rs`。
2. 用 `cfg(unix)` 包裹 `PermissionsExt`。
3. 替换 Unix symlink 调用。
4. 替换 `open` 命令调用。
5. 修正 PATH 搜索。

验收标准：Windows 上 `cargo check`、`pnpm typecheck`、`tauri build` 至少能进入正常依赖和编译流程，不再因 Unix-only API 失败。

### 第二阶段：让核心功能可用

1. AgentBuddy app data 目录平台化。
2. Agent 注册表补齐 Windows 候选路径。
3. Claude Desktop、OpenCode、Codex、Claude Code 的 MCP 路径完成 Windows 解析。
4. 打开配置目录、打开 MCP 文件、Skills 列表、MCP 写入可用。

验收标准：Windows 用户能看到 Agent 探测结果，能打开配置目录，能给至少 Codex、Claude Code、OpenCode 写入 MCP。

### 第三阶段：完善多环境与 Skills

1. Codex/Claude 多环境支持 PowerShell Profile 预览和可选写入。
2. Skills 应用支持 symlink 失败后复制，并有状态提示。
3. 前端文案从 `zshrc` 泛化为 shell 配置文件。

验收标准：Windows 下创建/导入环境不依赖 zsh，Skills 应用不会因为无 symlink 权限整体失败。

### 第四阶段：备份恢复与打包

1. Windows 备份源和恢复白名单落地。
2. 验证加密备份、WebDAV 上传、远端恢复。
3. 固化 Windows 打包脚本和发布说明。

验收标准：Windows 安装包可安装启动，备份恢复不越权写系统目录。

## 回归风险与控制

### 可维护性

1. 平台分支必须集中在 `platform.rs` 和 Agent 注册表路径层，不在业务函数里散落 `cfg!(windows)`。
2. 路径展示和真实路径分离：真实路径用 `PathBuf`，展示路径单独格式化。
3. DTO 字段重命名要兼容旧前端字段，避免一次性大改。

### 边界条件

1. Windows 用户名含空格、中文、`&`、括号时，所有命令调用必须使用参数数组。
2. PATH 中存在 `.cmd` shim 时能识别，但临时目录 shim 仍要过滤。
3. symlink 权限不足时 Skills 应用应降级复制，而不是失败。
4. OneDrive 用户目录、漫游配置目录、非 C 盘用户目录都要通过 `dirs` 和环境变量解析。
5. 配置文件不存在时应创建父目录，但不得创建敏感目录或系统目录。

### 回归风险

1. 修改 app data 目录可能让老用户看起来“配置丢失”，必须有旧路径兼容。
2. 备份恢复白名单如果实现过宽，风险高；如果过窄，Windows 恢复不可用。需按平台测试。
3. PowerShell Profile 写入属于用户 shell 配置修改，默认应保持显式确认。
4. Windows 路径大小写不敏感，重复记录和重复 symlink 判断要 canonicalize。

## 测试矩阵

### 单元测试

1. `expand_home`：`~`、`~/x`、`~\x`、`%APPDATA%`、相对路径、非法 `..`。
2. PATH 搜索：`;` 分隔、PATHEXT、空 PATH entry、临时 shim。
3. Claude Desktop 路径：macOS、Windows、Linux 各自候选。
4. reveal/open 参数：路径含空格、中文、以 `-` 开头。
5. restore 白名单：用户目录内允许，系统目录拒绝，zip slip 拒绝。

### 手工验证

1. Windows 11 普通用户，无开发者模式：Skills 应用应复制降级。
2. Windows 11 开发者模式：Skills 应用可创建 symlink。
3. PowerShell 7 与 Windows PowerShell 5.1：别名预览正确。
4. Claude Desktop 安装和未安装两种状态。
5. Codex、Claude Code、OpenCode CLI 通过 npm 或 standalone 安装后的 PATH 探测。
6. WebDAV 备份上传和恢复。

### CI 建议

1. 新增 Windows runner：`windows-latest`。
2. 至少运行：
   - `pnpm install --frozen-lockfile`
   - `pnpm typecheck`
   - `cargo check --manifest-path src-tauri/Cargo.toml`
3. 打包验证可先手动，稳定后加入 release workflow。

## 安全要求

1. 禁止扫描或备份 `.ssh`、密码库、浏览器凭据、系统凭据目录。
2. 打开文件和目录必须由后端解析目标，前端不能传任意 shell 命令。
3. 所有外部命令必须用 `Command::new(program).arg(...)`，禁止拼接 shell 字符串。
4. 恢复备份时必须限制目标路径，默认只允许用户目录和 AgentBuddy 管理目录。
5. 日志和错误信息不得输出 token、密码、Authorization header、加密 passphrase。

## 建议文件变更摘要

| 文件 | 建议动作 |
| --- | --- |
| `src-tauri/src/platform.rs` | 新增平台能力封装 |
| `src-tauri/src/lib.rs` | 注册 `platform` 模块 |
| `src-tauri/src/config.rs` | 应用数据目录平台化 |
| `src-tauri/src/db.rs` | 数据库路径平台化 |
| `src-tauri/src/agents.rs` | 增加 Windows 候选路径 |
| `src-tauri/src/sniff.rs` | 修正 PATH/PATHEXT/临时 shim |
| `src-tauri/src/mcp_config.rs` | Claude Desktop 和固定路径分平台 |
| `src-tauri/src/agent_open.rs` | 替换 `open` |
| `src-tauri/src/opencode_config.rs` | 替换权限和 reveal 逻辑 |
| `src-tauri/src/claude_env.rs` | PowerShell Profile 支持 |
| `src-tauri/src/codex_env.rs` | PowerShell Profile 支持 |
| `src-tauri/src/skills.rs` | Windows symlink/copy 降级 |
| `src-tauri/src/backup.rs` | Windows 备份恢复白名单 |
| `src-tauri/tauri.conf.json` | Windows 窗口和打包目标检查 |

## 最小可交付版本

优先交付一个 Windows MVP：

1. 应用能启动。
2. AgentBuddy 自身配置和数据库能读写。
3. Codex、Claude Code、OpenCode 的 MCP 管理可用。
4. 打开配置目录可用。
5. Skills 在 Windows 下至少能复制应用。
6. 备份只开放 AgentBuddy 自身数据和用户目录内 Agent 配置，暂不开放系统目录恢复。

该 MVP 能最大限度降低回归风险，并为后续完善 PowerShell Profile、安装包和更多 Agent 探测留出稳定基础。
