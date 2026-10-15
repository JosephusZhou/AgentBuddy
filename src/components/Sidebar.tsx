import { useState } from "react";
import type { SettingsView, View } from "../App";
/* ===== 图标：统一使用 lucide-react（16~18px / strokeWidth 1.8），替换原手写内联 SVG ===== */
import {
  Archive,
  Blocks,
  Bot,
  Braces,
  CloudCog,
  FolderCog,
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
  view: View;
  onNavigate: (view: View) => void;
}

interface MenuChild {
  view: View;
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

const SETTINGS_MENU: { view: SettingsView; label: string; icon: LucideIcon }[] = [
  { view: "preferences", label: "常规", icon: SlidersHorizontal },
  { view: "backup", label: "备份", icon: Archive },
];

function isSettingsView(view: View): view is SettingsView {
  return view === "preferences" || view === "backup";
}

export default function Sidebar({ view, onNavigate }: SidebarProps) {
  const [settingsOpen, setSettingsOpen] = useState(false);

  const handleMainNavigate = (next: View) => {
    setSettingsOpen(false);
    onNavigate(next);
  };

  const handleSettingsButtonClick = () => {
    const nextOpen = !settingsOpen;
    setSettingsOpen(nextOpen);
    // 打开浮层时若当前不在设置区，默认落到「常规」
    if (nextOpen && !isSettingsView(view)) {
      onNavigate("preferences");
    }
  };

  const settingsActive = settingsOpen || isSettingsView(view);

  return (
    <aside className="sidebar">
      <div className="sidebar-header" data-tauri-drag-region>
        <div className="sidebar-brand" data-tauri-drag-region>
          <div className="sidebar-brand-icon">
            <img src="/octopus-sidebar.png" alt="Octopus" />
          </div>
          <span className="sidebar-brand-name">Octopus</span>
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
                      className={`menu-item ${view === item.view ? "active" : ""}`}
                      onClick={() => handleMainNavigate(item.view)}
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
        {settingsOpen && (
          <div className="settings-popover" role="menu">
            {SETTINGS_MENU.map((item) => {
              const ItemIcon = item.icon;
              return (
                <button
                  key={item.view}
                  role="menuitem"
                  className={`settings-popover-item ${view === item.view ? "active" : ""}`}
                  onClick={() => onNavigate(item.view)}
                >
                  <ItemIcon size={16} strokeWidth={1.8} />
                  <span className="menu-label">{item.label}</span>
                </button>
              );
            })}
          </div>
        )}
        <button
          className={`menu-item ${settingsActive ? "active" : ""}`}
          onClick={handleSettingsButtonClick}
          aria-expanded={settingsOpen}
        >
          <Settings size={18} strokeWidth={1.8} />
          <span className="menu-label">设置</span>
        </button>
      </div>
    </aside>
  );
}
