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
