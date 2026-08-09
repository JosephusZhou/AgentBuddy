import type { AppMode, MainView, SettingsView } from "../App";
/* ===== 图标：统一使用 lucide-react（16~18px / strokeWidth 1.8），替换原手写内联 SVG ===== */
import {
  Archive,
  ArrowLeft,
  Blocks,
  Bot,
  Braces,
  Cloud,
  CloudCog,
  Code,
  FolderCog,
  Globe,
  Layers,
  Network,
  Settings,
  SlidersHorizontal,
  Sparkles,
  SquareTerminal,
  Route,
  Cog,
  Wrench,
} from "lucide-react";
import type { LucideIcon } from "lucide-react";

interface SidebarProps {
  mode: AppMode;
  mainView: MainView;
  settingsView: SettingsView;
  onNavigateMain: (view: MainView) => void;
  onNavigateSettings: (view: SettingsView) => void;
  onEnterSettings: () => void;
  onExitSettings: () => void;
}

interface MenuChild {
  view: MainView;
  label: string;
  icon: LucideIcon;
}

interface MenuGroupDef {
  id: string;
  label: string;
  icon: LucideIcon;
  items: MenuChild[];
}

const MENU_GROUPS: MenuGroupDef[] = [
  {
    id: "management",
    label: "管理",
    icon: Wrench,
    items: [
      { view: "agent-sniff", label: "Agent管理", icon: Bot },
      { view: "mcp-manage", label: "MCP管理", icon: Blocks },
      { view: "skills-manage", label: "skills管理", icon: Sparkles },
    ],
  },
  {
    id: "routing",
    label: "路由",
    icon: Route,
    items: [
      { view: "ai-providers", label: "AI供应商", icon: CloudCog },
      { view: "route-aggregation", label: "路由聚合", icon: Network },
    ],
  },
  {
    id: "env-config",
    label: "环境与配置",
    icon: Cog,
    items: [
      { view: "claude-env", label: "Claude环境", icon: Layers },
      { view: "codex-env", label: "Codex环境", icon: SquareTerminal },
      { view: "model-config", label: "模型配置", icon: Braces },
      { view: "project-config", label: "项目AI配置", icon: FolderCog },
    ],
  },
];

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
            {MENU_GROUPS.map((group) => {
              const GroupIcon = group.icon;
              return (
                <div className="menu-group" key={group.id}>
                  <div className="menu-group-header">
                    <GroupIcon size={15} strokeWidth={1.8} />
                    <span>{group.label}</span>
                  </div>
                  <div className="menu-group-items">
                    {group.items.map((item) => {
                      const ItemIcon = item.icon;
                      return (
                        <button
                          key={item.view}
                          className={`menu-item ${mainView === item.view ? "active" : ""}`}
                          onClick={() => onNavigateMain(item.view)}
                        >
                          <ItemIcon size={18} strokeWidth={1.8} />
                          <span className="menu-label">{item.label}</span>
                        </button>
                      );
                    })}
                  </div>
                </div>
              );
            })}
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
            <button
              className={`menu-item ${settingsView === "backup" ? "active" : ""}`}
              onClick={() => onNavigateSettings("backup")}
            >
              <Archive size={18} strokeWidth={1.8} />
              <span className="menu-label">备份</span>
            </button>
          </nav>
        </>
      )}
    </aside>
  );
}
