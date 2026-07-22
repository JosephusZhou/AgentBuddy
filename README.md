# AgentBuddy

![License](https://img.shields.io/badge/license-PolyForm_Noncommercial-blue)
![Platform](https://img.shields.io/badge/platform-desktop-lightgrey)

AgentBuddy 是一个面向本地桌面场景的 Tauri + React 工具，用来统一管理常见 AI Agent 生态配置。

## 主要功能

- 扫描和管理本机 Agent 安装信息
- 管理 MCP 服务器配置
- 管理 Skills
- 管理 Claude / Codex 环境
- 管理 OpenCode 配置
- 备份与恢复
- 网络设置与 WebDAV 管理
- 主题与偏好设置

## 技术栈

- Tauri 2
- React 18
- TypeScript
- Vite
- Rust

## 开发

```bash
pnpm install
pnpm dev
```

前端单独运行：

```bash
pnpm dev:renderer
```

## 构建

```bash
pnpm build
```

仅构建前端：

```bash
pnpm build:renderer
```

## 下载与安装（macOS）

从 [GitHub Releases](https://github.com/JosephusZhou/AgentBuddy/releases) 下载对应架构的 DMG：

| 芯片 | 文件名示例 |
|------|------------|
| Apple Silicon（M 系列） | `AgentBuddy_x.y.z_aarch64.dmg` |
| Intel | `AgentBuddy_x.y.z_x86_64.dmg` |

1. 打开 DMG，将 **AgentBuddy** 拖入「应用程序」文件夹
2. 从「启动台」或 `/Applications` 启动

### 首次打开被拦截时

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

### 维护者：发布新版本

发布由 `vMAJOR.MINOR.PATCH` 格式的 Git tag 触发（见 `.github/workflows/release.yml`），会自动创建 GitHub Release 并上传 Intel / Apple Silicon 两个 DMG。打 tag 前请保证 `package.json`、`src-tauri/tauri.conf.json`、`src-tauri/Cargo.toml` 版本号一致：

```bash
git tag v0.1.0
git push origin v0.1.0
```

## 项目结构

- `src/`：前端界面
- `src-tauri/`：Tauri 与 Rust 后端
- `dist/`：前端构建产物

## 许可证

本项目采用 **PolyForm Noncommercial 1.0.0**。

你可以出于个人、研究、学习、测试和其他非商业目的使用、修改和分发本项目。
商业使用、商业二次开发、再分发或以盈利为目的的使用不被允许。

完整条款见 [LICENSE](./LICENSE)，附加声明见 [NOTICE](./NOTICE)。

## 免责声明

本项目按“原样”提供，不附带任何担保。使用前请自行评估其适用性与风险。
