import { useEffect, useState } from "react";
import Sidebar from "./components/Sidebar";
import AgentSniff from "./components/pages/AgentSniff";
import BackupManage from "./components/pages/BackupManage";
import OpenCodeConfig from "./components/pages/OpenCodeConfig";
import ProjectConfig from "./components/pages/ProjectConfig";
import SkillsManage from "./components/pages/SkillsManage";
import McpManage from "./components/pages/McpManage";
import ClaudeEnv from "./components/pages/ClaudeEnv";
import CodexEnv from "./components/pages/CodexEnv";
import Preferences from "./components/pages/Preferences";
import NetworkSettings from "./components/pages/NetworkSettings";
import WebDAV from "./components/pages/WebDAV";
import { applyTheme, loadAppConfig, saveTheme, DEFAULT_THEME, type Theme } from "./lib/theme";

export type MainView =
  | "agent-sniff"
  | "backup-manage"
  | "opencode-config"
  | "project-config"
  | "skills-manage"
  | "mcp-manage"
  | "claude-env"
  | "codex-env";
export type SettingsView = "preferences" | "network" | "webdav";
export type AppMode = "main" | "settings";

export default function App() {
  const [mode, setMode] = useState<AppMode>("main");
  const [mainView, setMainView] = useState<MainView>("agent-sniff");
  const [settingsView, setSettingsView] = useState<SettingsView>("preferences");
  const [theme, setTheme] = useState<Theme>(DEFAULT_THEME);
  const [themeReady, setThemeReady] = useState(false);

  useEffect(() => {
    // 平台标记（纯表现层）：macOS 侧栏启用毛玻璃等原生质感微调。
    document.documentElement.dataset.platform = /Mac|iPhone|iPad/.test(navigator.userAgent)
      ? "macos"
      : "other";
  }, []);

  useEffect(() => {
    let cancelled = false;

    (async () => {
      const config = await loadAppConfig();
      if (cancelled) return;
      applyTheme(config.theme);
      setTheme(config.theme);
      setThemeReady(true);
    })();

    return () => {
      cancelled = true;
    };
  }, []);

  const handleThemeChange = async (next: Theme) => {
    applyTheme(next);
    setTheme(next);
    const saved = await saveTheme(next);
    if (saved !== next) {
      applyTheme(saved);
      setTheme(saved);
    }
  };

  // Avoid a light→dark flash when saved theme is dark.
  if (!themeReady) {
    return <div className="app" aria-hidden />;
  }

  const activeView = mode === "main" ? mainView : settingsView;

  return (
    <div className="app">
      {/* Overlay 标题栏下的顶部拖拽区：鼠标按住顶部可拖动窗口 */}
      <div className="window-drag-region" data-tauri-drag-region aria-hidden />
      <Sidebar
        mode={mode}
        mainView={mainView}
        settingsView={settingsView}
        onNavigateMain={setMainView}
        onNavigateSettings={setSettingsView}
        onEnterSettings={() => setMode("settings")}
        onExitSettings={() => setMode("main")}
      />
      <main className="main-content">
        {activeView === "agent-sniff" && <AgentSniff />}
        {activeView === "backup-manage" && <BackupManage />}
        {activeView === "opencode-config" && <OpenCodeConfig />}
        {activeView === "project-config" && <ProjectConfig />}
        {activeView === "skills-manage" && <SkillsManage />}
        {activeView === "mcp-manage" && <McpManage />}
        {activeView === "claude-env" && <ClaudeEnv />}
        {activeView === "codex-env" && <CodexEnv />}
        {activeView === "preferences" && (
          <Preferences theme={theme} onThemeChange={handleThemeChange} />
        )}
        {activeView === "network" && <NetworkSettings />}
        {activeView === "webdav" && <WebDAV />}
      </main>
    </div>
  );
}
