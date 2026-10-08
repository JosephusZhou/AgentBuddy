import { useEffect, useState } from "react";
import type { AppMode, MainView, SettingsView } from "../App";
import { version as appVersion } from "../../package.json";
/* ===== 图标：统一使用 lucide-react（16~18px / strokeWidth 1.8），替换原手写内联 SVG ===== */
import {
  Archive,
  ArrowLeft,
  Blocks,
  Bot,
  Braces,
  ChevronRight,
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
  items: MenuChild[];
}

/** 导航项右侧计数徽章的取数口径（三个总数都是"该页面的条目数"）。
 *  每一项都可能取不到（浏览器预览 / 非 Tauri 环境），故允许为 null → 不渲染徽章。 */
interface NavCounts {
  /** 已安装的 Agent 数（扫描结果里 found=true 的条数）。 */
  agents: number | null;
  /** MCP 服务总数。 */
  mcp: number | null;
  /** Skills 库条目数。 */
  skills: number | null;
}

const MENU_GROUPS: MenuGroupDef[] = [
  {
    id: "management",
    label: "管理",
    items: [
      { view: "agent-sniff", label: "Agent管理", icon: Bot },
      { view: "mcp-manage", label: "MCP管理", icon: Blocks },
      { view: "skills-manage", label: "Skills管理", icon: Sparkles },
    ],
  },
  {
    id: "routing",
    label: "路由",
    items: [
      { view: "ai-providers", label: "AI供应商", icon: CloudCog },
      { view: "route-aggregation", label: "路由聚合", icon: Network },
    ],
  },
  {
    id: "env-config",
    label: "环境与配置",
    items: [
      { view: "claude-env", label: "Claude环境", icon: Layers },
      { view: "codex-env", label: "Codex环境", icon: SquareTerminal },
      { view: "model-config", label: "模型配置", icon: Braces },
      { view: "project-config", label: "项目AI配置", icon: FolderCog },
    ],
  },
];

/** 每个导航项的计数取数函数；未取到时徽章不渲染。 */
const COUNT_SOURCE: Partial<Record<MainView, keyof NavCounts>> = {
  "agent-sniff": "agents",
  "mcp-manage": "mcp",
  "skills-manage": "skills",
};

/**
 * 侧栏计数徽章的总数。三个来源相互独立且都可能失败（浏览器预览、非 Tauri 环境），
 * 因此用 allSettled 逐项降级：任一项失败只少一枚徽章，不影响侧栏其余功能。
 *
 * 取数刻意延后 250ms：目标页自身也在拉同一批数据（Skills 页要读技能库文档），
 * 让徽章请求排在页面首屏之后，避免与首屏竞争 CPU。
 */
function useNavCounts(mode: AppMode, mainView: MainView): NavCounts | null {
  const [counts, setCounts] = useState<NavCounts | null>(null);

  useEffect(() => {
    if (mode !== "main") {
      setCounts(null);
      return;
    }
    let disposed = false;
    const timer = setTimeout(() => {
      (async () => {
        try {
          const { invoke } = await import("@tauri-apps/api/core");
          const [agents, mcpServers, skillsResult] = await Promise.allSettled([
            invoke("get_cached_agents") as Promise<{ found: boolean }[]>,
            invoke("get_mcp_servers") as Promise<unknown[]>,
            invoke("list_skills") as Promise<{ skills?: unknown[] }>,
          ]);
          if (disposed) return;
          setCounts({
            agents:
              agents.status === "fulfilled" ? agents.value.filter((a) => a.found).length : null,
            mcp: mcpServers.status === "fulfilled" ? mcpServers.value.length : null,
            skills:
              skillsResult.status === "fulfilled"
                ? (skillsResult.value.skills?.length ?? null)
                : null,
          });
        } catch {
          // 非 Tauri 环境（浏览器预览）：不显示徽章。
          if (!disposed) setCounts(null);
        }
      })();
    }, 250);
    return () => {
      disposed = true;
      clearTimeout(timer);
    };
  }, [mode, mainView]);

  return counts;
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
  const counts = useNavCounts(mode, mainView);

  return (
    <aside className="sidebar">
      {mode === "main" ? (
        /* ===== Main Sidebar ===== */
        <>
          <div className="sidebar-header" data-tauri-drag-region>
            <div className="sidebar-brand" data-tauri-drag-region>
              <div className="sidebar-brand-icon">
                <Code size={20} strokeWidth={2.5} />
              </div>
              <div className="sidebar-brand-text">
                <span className="sidebar-brand-name kb-headline-md">AgentBuddy</span>
                {/* 版本号取自 package.json，避免再出现一处需要同步的硬编码 */}
                <span className="sidebar-brand-version">v{appVersion}</span>
              </div>
            </div>
          </div>
          <nav className="sidebar-nav">
            {MENU_GROUPS.map((group) => (
              <div className="menu-group" key={group.id}>
                <div className="menu-group-header kb-label-sm">{group.label}</div>
                <div className="menu-group-items">
                  {group.items.map((item) => {
                    const ItemIcon = item.icon;
                    const countKey = COUNT_SOURCE[item.view];
                    const count = countKey ? counts?.[countKey] : null;
                    return (
                      <button
                        key={item.view}
                        className={`menu-item ${mainView === item.view ? "active" : ""}`}
                        onClick={() => onNavigateMain(item.view)}
                      >
                        <ItemIcon size={18} strokeWidth={1.8} />
                        <span className="menu-label">{item.label}</span>
                        {count != null && <span className="menu-count">{count}</span>}
                      </button>
                    );
                  })}
                </div>
              </div>
            ))}
          </nav>
          <div className="sidebar-bottom">
            <button className="menu-item" onClick={onEnterSettings}>
              <Settings size={18} strokeWidth={1.8} />
              <span className="menu-label">设置</span>
              <ChevronRight size={16} strokeWidth={1.8} className="menu-trailing" />
            </button>
          </div>
        </>
      ) : (
        /* ===== Settings Sidebar ===== */
        <>
          <div className="sidebar-header sidebar-header-compact">
            <button className="menu-item" onClick={onExitSettings}>
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
