import type { AppMode, MainView, SettingsView } from "../App";

interface SidebarProps {
  mode: AppMode;
  mainView: MainView;
  settingsView: SettingsView;
  onNavigateMain: (view: MainView) => void;
  onNavigateSettings: (view: SettingsView) => void;
  onEnterSettings: () => void;
  onExitSettings: () => void;
}

/* ===== SVG Icons (inline, matching design) ===== */
const IconCode = () => (
  <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
    <polyline points="16 18 22 12 16 6" />
    <polyline points="8 6 2 12 8 18" />
  </svg>
);

const IconSearch = () => (
  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
    <path d="M21 21l-4.35-4.35" />
    <circle cx="11" cy="11" r="7" />
    <path d="M11 8v6" />
    <path d="M8 11h6" />
  </svg>
);

const IconUpload = () => (
  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
    <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" />
    <polyline points="17 8 12 3 7 8" />
    <line x1="12" y1="3" x2="12" y2="15" />
  </svg>
);

const IconBrackets = () => (
  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
    <polyline points="16 18 22 12 16 6" />
    <polyline points="8 6 2 12 8 18" />
  </svg>
);

const IconBriefcase = () => (
  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
    <rect x="2" y="7" width="20" height="14" rx="2" ry="2" />
    <path d="M16 21V5a2 2 0 0 0-2-2h-4a2 2 0 0 0-2 2v16" />
  </svg>
);

const IconSettings = () => (
  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
    <circle cx="12" cy="12" r="3" />
    <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1 0 2.83 2 2 0 0 1-2.83 0l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-2 2 2 2 0 0 1-2-2v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83 0 2 2 0 0 1 0-2.83l.06-.06A1.65 1.65 0 0 0 4.68 15a1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1-2-2 2 2 0 0 1 2-2h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 0-2.83 2 2 0 0 1 2.83 0l.06.06A1.65 1.65 0 0 0 9 4.68a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 2-2 2 2 0 0 1 2 2v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 0 2 2 0 0 1 0 2.83l-.06.06A1.65 1.65 0 0 0 19.4 9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 2 2 2 2 0 0 1-2 2h-.09a1.65 1.65 0 0 0-1.51 1z" />
  </svg>
);

const IconBack = () => (
  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
    <line x1="19" y1="12" x2="5" y2="12" />
    <polyline points="12 19 5 12 12 5" />
  </svg>
);

const IconSliders = () => (
  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
    <circle cx="12" cy="12" r="3" />
    <line x1="3" y1="12" x2="9" y2="12" />
    <line x1="15" y1="12" x2="21" y2="12" />
    <line x1="3" y1="6" x2="15" y2="6" />
    <line x1="17" y1="6" x2="21" y2="6" />
    <line x1="3" y1="18" x2="13" y2="18" />
    <line x1="15" y1="18" x2="21" y2="18" />
  </svg>
);

const IconDatabase = () => (
  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
    <ellipse cx="12" cy="5" rx="9" ry="3" />
    <path d="M21 12c0 1.66-4 3-9 3s-9-1.34-9-3" />
    <path d="M3 5v14c0 1.66 4 3 9 3s9-1.34 9-3V5" />
  </svg>
);

const IconNetwork = () => (
  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
    <circle cx="12" cy="12" r="3" />
    <path d="M12 2v3" />
    <path d="M12 19v3" />
    <path d="M2 12h3" />
    <path d="M19 12h3" />
    <path d="M4.93 4.93l2.12 2.12" />
    <path d="M16.95 16.95l2.12 2.12" />
    <path d="M4.93 19.07l2.12-2.12" />
    <path d="M16.95 7.05l2.12-2.12" />
  </svg>
);

const IconLayers = () => (
  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
    <polygon points="12 2 2 7 12 12 22 7 12 2" />
    <polyline points="2 17 12 22 22 17" />
    <polyline points="2 12 12 17 22 12" />
  </svg>
);

const IconFolderConfig = () => (
  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
    <path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z" />
    <circle cx="16" cy="15" r="2" />
    <path d="M16 11v2M16 17v2M12 15h2M18 15h2" />
  </svg>
);

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
                <IconCode />
              </div>
              <span className="sidebar-brand-name">AgentBuddy</span>
            </div>
          </div>
          <nav className="sidebar-nav">
            <button
              className={`menu-item ${mainView === "agent-sniff" ? "active" : ""}`}
              onClick={() => onNavigateMain("agent-sniff")}
            >
              <IconSearch />
              <span className="menu-label">Agent管理</span>
            </button>
            <button
              className={`menu-item ${mainView === "mcp-manage" ? "active" : ""}`}
              onClick={() => onNavigateMain("mcp-manage")}
            >
              <IconSettings />
              <span className="menu-label">mcp管理</span>
            </button>
            <button
              className={`menu-item ${mainView === "skills-manage" ? "active" : ""}`}
              onClick={() => onNavigateMain("skills-manage")}
            >
              <IconBriefcase />
              <span className="menu-label">skills管理</span>
            </button>
            <button
              className={`menu-item ${mainView === "claude-env" ? "active" : ""}`}
              onClick={() => onNavigateMain("claude-env")}
            >
              <IconLayers />
              <span className="menu-label">Claude环境</span>
            </button>
            <button
              className={`menu-item ${mainView === "codex-env" ? "active" : ""}`}
              onClick={() => onNavigateMain("codex-env")}
            >
              <IconLayers />
              <span className="menu-label">Codex环境</span>
            </button>
            <button
              className={`menu-item ${mainView === "opencode-config" ? "active" : ""}`}
              onClick={() => onNavigateMain("opencode-config")}
            >
              <IconBrackets />
              <span className="menu-label">OpenCode配置</span>
            </button>
            <button
              className={`menu-item ${mainView === "project-config" ? "active" : ""}`}
              onClick={() => onNavigateMain("project-config")}
            >
              <IconFolderConfig />
              <span className="menu-label">项目AI配置</span>
            </button>
            <button
              className={`menu-item ${mainView === "backup-manage" ? "active" : ""}`}
              onClick={() => onNavigateMain("backup-manage")}
            >
              <IconUpload />
              <span className="menu-label">备份管理</span>
            </button>
          </nav>
          <div className="sidebar-bottom">
            <button className="menu-item" onClick={onEnterSettings}>
              <IconSettings />
              <span className="menu-label">设置</span>
            </button>
          </div>
        </>
      ) : (
        /* ===== Settings Sidebar ===== */
        <>
          <div className="sidebar-header" style={{ borderBottom: "none", paddingBottom: 4 }}>
            <button className="menu-item" onClick={onExitSettings} style={{ color: "var(--seed-muted)", padding: "8px 12px" }}>
              <IconBack />
              <span className="menu-label">返回应用</span>
            </button>
          </div>
          <nav className="sidebar-nav">
            <button
              className={`menu-item ${settingsView === "preferences" ? "active" : ""}`}
              onClick={() => onNavigateSettings("preferences")}
            >
              <IconSliders />
              <span className="menu-label">偏好设置</span>
            </button>
            <button
              className={`menu-item ${settingsView === "network" ? "active" : ""}`}
              onClick={() => onNavigateSettings("network")}
            >
              <IconNetwork />
              <span className="menu-label">网络设置</span>
            </button>
            <button
              className={`menu-item ${settingsView === "webdav" ? "active" : ""}`}
              onClick={() => onNavigateSettings("webdav")}
            >
              <IconDatabase />
              <span className="menu-label">WebDAV</span>
            </button>
          </nav>
        </>
      )}
    </aside>
  );
}
