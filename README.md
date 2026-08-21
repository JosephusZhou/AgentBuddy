# AgentBuddy

![License](https://img.shields.io/badge/license-PolyForm_Noncommercial-blue)
![Platform](https://img.shields.io/badge/platform-desktop-lightgrey)
![Version](https://img.shields.io/badge/version-0.1.9-green)

AgentBuddy 是一个面向本地桌面场景的 Tauri + React 工具，用来统一管理常见 AI Agent 生态配置。

## 主要功能

| 模块 | 说明 |
|------|------|
| Agent 发现 | 自动扫描本机已安装的 AI Agent（Claude Code、Codex CLI、OpenCode、Antigravity、CodeBuddy CN、WorkBuddy），展示安装路径、配置目录、MCP 状态等详情 |
| MCP 管理 | 跨 Agent 统一管理 MCP 服务器配置（增删改、批量导入导出、连接测试），支持 TOML / JSON / JSONC 等多方言写入 |
| Skills 管理 | 本地 / GitHub / GitCode 多源导入 Skill，支持批量应用到多个 Agent、标签管理、导出 |
| Claude 多环境 | 管理多个 `CLAUDE_CONFIG_DIR` 环境（别名安装、Token 配置、MCP 同步、模型选择） |
| Codex 多环境 | 管理多个 `CODEX_HOME` 环境（别名安装、Auth 配置、MCP 同步） |
| OpenCode 配置 | 管理 Provider / Model、API Key、Models.dev 目录同步 |
| AI 供应商库 | 集中管理 AI 供应商（Anthropic / OpenAI），加密存储 API Key，支持 Base URL 自定义 |
| 项目配置 | 一键为项目初始化 AI Agent 骨架（Full / Symlink 模式），可选注入 MCP 和 Skills |
| 备份与恢复 | 本地打包 + 多 WebDAV 上传（支持加密），待实现恢复功能 |
| 网络设置 | 代理模式（无 / 系统 / 自定义 HTTP/SOCKS5），应用于 WebDAV / Skills 下载 / MCP 探测 |
| WebDAV 管理 | 多 WebDAV 连接管理，连通性探测 |
| 主题与偏好 | 深色 / 浅色主题切换 |

## 技术栈

| 层级 | 技术 |
|------|------|
| 前端 | React 18 + TypeScript + Vite 6 + Tailwind 3 |
| 后端 | Tauri 2 + Rust |
| 存储 | SQLite（`~/.agentbuddy/agents.db`） |
| 包管理 | pnpm |

## 开发

```bash
# 安装依赖
pnpm install

# 完整桌面应用开发（Vite + Rust/Tauri）
pnpm dev

# 仅前端（浏览器预览，Tauri invoke 调用会静默失败）
pnpm dev:renderer
```

## 构建

```bash
# 生产构建（前端 → dist，然后 Tauri 打包）
pnpm build

# 仅构建前端
pnpm build:renderer

# Rust 单元测试
cd src-tauri && cargo test

# 按名称过滤单个测试
cd src-tauri && cargo test encrypt_decrypt
```

## 平台说明

- **macOS**：完整支持，提供 DMG 发布包（Intel + Apple Silicon）。
- **Windows**：路径 / PATH / 文件管理器 / Skills 软链降级 / PowerShell 别名等已适配（见 `WINDOWS_ADAPTATION_PLAN.md`）；Release 提供 NSIS 安装包。

## 下载与安装

从 [GitHub Releases](https://github.com/JosephusZhou/AgentBuddy/releases) 下载对应平台安装包：

| 平台 | 文件名示例 |
|------|------------|
| macOS Apple Silicon（M 系列） | `AgentBuddy_x.y.z_aarch64.dmg` |
| macOS Intel | `AgentBuddy_x.y.z_x86_64.dmg` |
| Windows x64 | `AgentBuddy_x.y.z_x64-setup.exe` |

### macOS

1. 打开 DMG，将 **AgentBuddy** 拖入「应用程序」文件夹
2. 从「启动台」或 `/Applications` 启动

#### 首次打开被拦截时

当前发布包**未**配置 Apple Developer 签名与公证。从浏览器下载的应用会带上隔离属性（quarantine），首次打开可能提示「已损坏，无法打开」或「无法验证开发者」。

在终端执行（路径按实际安装位置调整）：

```bash
# 移除隔离属性
xattr -cr /Applications/AgentBuddy.app

# 可选：本地自签名（ad-hoc），降低 Gatekeeper 再次拦截的概率
codesign --force --deep --sign - /Applications/AgentBuddy.app
```

然后再次双击打开。若仍被拦截：

1. 打开 **系统设置 → 隐私与安全性**
2. 在被拦截提示附近点击 **仍要打开**
3. 或对应用图标 **右键 → 打开**，在确认框中选择打开

> 上述命令只作用于本机已安装的 App，不会向 Apple 注册证书；自签名是本地 ad-hoc 签名。

### Windows

1. 运行 `AgentBuddy_x.y.z_x64-setup.exe`，按向导完成安装
2. 从「开始」菜单启动 **AgentBuddy**

当前 Windows 安装包**未**配置代码签名。首次运行可能被 SmartScreen 拦截：选择「更多信息」→「仍要运行」即可。安装与运行需要 WebView2 Runtime（Windows 10/11 通常已预装；缺失时 NSIS 安装器会引导安装）。

## 数据存储

应用数据默认存放在 `~/.agentbuddy/`（macOS / Linux）：

| 文件/目录 | 用途 |
|-----------|------|
| `config.json` | 主题、网络代理、备份设置 |
| `agents.db` | SQLite 数据库（Agent、MCP 服务器、Skills、环境、供应商等） |
| `skills/` | Skills 库文件 |

MCP 配置写入各 Agent 的原生配置文件（由 `agents.rs` 注册表定义路径和方言），不写入 AgentBuddy 本地。

## 项目结构

```
src/                          React 前端
  App.tsx                     主路由（Main / Settings）
  components/
    Sidebar.tsx               侧边栏导航
    ui.tsx                    通用 UI 原语
    agent-filter.tsx          Agent 过滤组件（共享）
    ModelComboBox.tsx         模型下拉选择组件
    pages/
      AgentSniff.tsx          Agent 发现
      AgentDetail.tsx         Agent 详情面板
      McpManage.tsx           MCP 服务器管理
      SkillsManage.tsx        Skills 管理
      ClaudeEnv.tsx           Claude 多环境管理
      CodexEnv.tsx            Codex 多环境管理
      OpenCodeConfig.tsx      OpenCode 配置
      AiProviders.tsx         AI 供应商管理
      ProjectConfig.tsx       项目配置初始化
      BackupManage.tsx        备份管理
      Preferences.tsx         偏好设置
      NetworkSettings.tsx     网络设置
      WebDAV.tsx              WebDAV 连接管理

src-tauri/src/                Rust 后端
  lib.rs                      Tauri 命令注册与 setup
  agents.rs                   Agent 注册表（唯一真实来源）
  sniff.rs                    Agent 发现逻辑
  mcp_config.rs               MCP 多方言读写
  skills.rs                   Skills 库管理
  claude_env.rs               Claude 多环境管理
  codex_env.rs                Codex 多环境管理
  opencode_config.rs          OpenCode Provider/Model 配置
  ai_provider.rs              AI 供应商注册与加密存储
  project_config.rs           项目级 AI 配置初始化
  db.rs                       SQLite 持久化
  config.rs                   应用配置读写
  crypto.rs                   AES-256-GCM 加密
  webdav.rs                   WebDAV 连接与上传
  backup.rs                   备份打包与多 WebDAV 上传
  platform.rs                 跨平台路径与工具函数
```

## 维护者：发布新版本

发布由 `vMAJOR.MINOR.PATCH` 格式的 Git tag 触发（见 `.github/workflows/release.yml`），会自动创建 GitHub Release 并上传：

- macOS：Intel + Apple Silicon 两个 DMG
- Windows：x64 NSIS 安装包（`*-setup.exe`）

打 tag 前请保证 `package.json`、`src-tauri/tauri.conf.json`、`src-tauri/Cargo.toml` 版本号一致：

```bash
git tag v0.1.9
git push origin main --tags
```

## 许可证

本项目采用 **PolyForm Noncommercial 1.0.0**。

你可以出于个人、研究、学习、测试和其他非商业目的使用、修改和分发本项目。
商业使用、商业二次开发、再分发或以盈利为目的的使用不被允许。

完整条款见 [LICENSE](./LICENSE)，附加声明见 [NOTICE](./NOTICE)。

## 免责声明

本项目按"原样"提供，不附带任何担保。使用前请自行评估其适用性与风险。
