import type { AppMode, MainView, SettingsView } from "../App";
/* ===== 图标：统一使用 lucide-react（16~18px / strokeWidth 1.8），替换原手写内联 SVG ===== */
import {
  Archive,
  ArrowLeft,
  Blocks,
  Bot,
  Braces,
  Cloud,
  Code,
  FolderCog,
  Globe,
  Layers,
  Settings,
  SlidersHorizontal,
  Sparkles,
  SquareTerminal,
} from "lucide-react";

interface SidebarProps {
  mode: AppMode;
  mainView: MainView;
  settingsView: SettingsView;
  onNavigateMain: (view: MainView) => void;
  onNavigateSettings: (view: SettingsView) => void;
  onEnterSettings: () => void;
  onExitSettings: () => void;
}

export default function Sidebar({
  mode,
  mainView,
  settingsView,
  onNavigateMain,
  onNavigateSettings,
  onEnterSettings,
  onExitSettings,
}: SidebarProps) {
  return (
    <aside className="sidebar">
      {mode === "main" ? (
        /* ===== Main Sidebar ===== */
        <>
          <div className="sidebar-header" data-tauri-drag-region>
            <div className="sidebar-brand" data-tauri-drag-region>
              <div className="sidebar-brand-icon">
                <Code size={16} strokeWidth={2.5} />
              </div>
              <span className="sidebar-brand-name">AgentBuddy</span>
            </div>
          </div>
          <nav className="sidebar-nav">
            <button
              className={`menu-item ${mainView === "agent-sniff" ? "active" : ""}`}
              onClick={() => onNavigateMain("agent-sniff")}
            >
              <Bot size={18} strokeWidth={1.8} />
              <span className="menu-label">Agent管理</span>
            </button>
            <button
              className={`menu-item ${mainView === "mcp-manage" ? "active" : ""}`}
              onClick={() => onNavigateMain("mcp-manage")}
            >
              <Blocks size={18} strokeWidth={1.8} />
              <span className="menu-label">mcp管理</span>
            </button>
            <button
              className={`menu-item ${mainView === "skills-manage" ? "active" : ""}`}
              onClick={() => onNavigateMain("skills-manage")}
            >
              <Sparkles size={18} strokeWidth={1.8} />
              <span className="menu-label">skills管理</span>
            </button>
            <button
              className={`menu-item ${mainView === "claude-env" ? "active" : ""}`}
              onClick={() => onNavigateMain("claude-env")}
            >
              <Layers size={18} strokeWidth={1.8} />
              <span className="menu-label">Claude环境</span>
            </button>
            <button
              className={`menu-item ${mainView === "codex-env" ? "active" : ""}`}
              onClick={() => onNavigateMain("codex-env")}
            >
              <SquareTerminal size={18} strokeWidth={1.8} />
              <span className="menu-label">Codex环境</span>
            </button>
            <button
              className={`menu-item ${mainView === "opencode-config" ? "active" : ""}`}
              onClick={() => onNavigateMain("opencode-config")}
            >
              <Braces size={18} strokeWidth={1.8} />
              <span className="menu-label">OpenCode配置</span>
            </button>
            <button
              className={`menu-item ${mainView === "project-config" ? "active" : ""}`}
              onClick={() => onNavigateMain("project-config")}
            >
              <FolderCog size={18} strokeWidth={1.8} />
              <span className="menu-label">项目AI配置</span>
            </button>
            <button
              className={`menu-item ${mainView === "backup-manage" ? "active" : ""}`}
              onClick={() => onNavigateMain("backup-manage")}
            >
              <Archive size={18} strokeWidth={1.8} />
              <span className="menu-label">备份管理</span>
            </button>
          </nav>
          <div className="sidebar-bottom">
            <button className="menu-item" onClick={onEnterSettings}>
              <Settings size={18} strokeWidth={1.8} />
              <span className="menu-label">设置</span>
            </button>
          </div>
        </>
      ) : (
        /* ===== Settings Sidebar ===== */
        <>
          <div className="sidebar-header" style={{ borderBottom: "none", paddingBottom: 4 }}>
            <button className="menu-item" onClick={onExitSettings} style={{ color: "var(--seed-muted)", padding: "8px 12px" }}>
              <ArrowLeft size={18} strokeWidth={1.8} />
              <span className="menu-label">返回应用</span>
            </button>
          </div>
          <nav className="sidebar-nav">
            <button
              className={`menu-item ${settingsView === "preferences" ? "active" : ""}`}
              onClick={() => onNavigateSettings("preferences")}
            >
              <SlidersHorizontal size={18} strokeWidth={1.8} />
              <span className="menu-label">偏好设置</span>
            </button>
            <button
              className={`menu-item ${settingsView === "network" ? "active" : ""}`}
              onClick={() => onNavigateSettings("network")}
            >
              <Globe size={18} strokeWidth={1.8} />
              <span className="menu-label">网络设置</span>
            </button>
            <button
              className={`menu-item ${settingsView === "webdav" ? "active" : ""}`}
              onClick={() => onNavigateSettings("webdav")}
            >
              <Cloud size={18} strokeWidth={1.8} />
              <span className="menu-label">WebDAV</span>
            </button>
          </nav>
        </>
      )}
    </aside>
  );
}
