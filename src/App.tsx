import { useEffect, useState } from "react";
import Sidebar from "./components/Sidebar";
import AgentSniff from "./components/pages/AgentSniff";
import AiProviders from "./components/pages/AiProviders";
import BackupManage from "./components/pages/BackupManage";
import ModelConfig from "./components/pages/OpenCodeConfig";
import ProjectConfig from "./components/pages/ProjectConfig";
import SkillsManage from "./components/pages/SkillsManage";
import McpManage from "./components/pages/McpManage";
import ClaudeEnv from "./components/pages/ClaudeEnv";
import CodexEnv from "./components/pages/CodexEnv";
import Preferences from "./components/pages/Preferences";
import NetworkSettings from "./components/pages/NetworkSettings";
import WebDAV from "./components/pages/WebDAV";
import RouteAggregation from "./components/pages/RouteAggregation";
import { applyTheme, loadAppConfig, saveTheme, DEFAULT_THEME, type Theme } from "./lib/theme";
import { useStatusMessage } from "./lib/useStatusMessage";
import { useGlobalModalA11y } from "./components/ui";
import { Toast } from "./components/Toast";

export type MainView =
  | "agent-sniff"
  | "ai-providers"
  | "model-config"
  | "project-config"
  | "skills-manage"
  | "mcp-manage"
  | "claude-env"
  | "codex-env"
  | "route-aggregation";
export type SettingsView = "preferences" | "network" | "webdav" | "backup";
export type AppMode = "main" | "settings";

/**
 * 自动备份后台失败时弹出全局 Toast（trigger=auto 的 finalize 汇总事件）。
 * 成功不打扰（备份页内已有「上次运行 · 成功」状态）；失败提示复用
 * useStatusMessage 的超时逻辑，5s 后自动滑出。
 */
function AutoBackupNotifier() {
  const [message, setMessage] = useStatusMessage(5000);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    (async () => {
      const { listen } = await import("@tauri-apps/api/event");
      if (disposed) return;
      unlisten = await listen<{
        phase: string;
        message: string;
        trigger?: string;
        ok?: boolean;
      }>("backup-progress", (ev) => {
        // ok 仅在 finalize 汇总事件上出现；成功与不带 ok 的事件都不弹。
        if (ev.payload.trigger !== "auto" || ev.payload.phase !== "finalize") return;
        if (ev.payload.ok !== false) return;
        setMessage(ev.payload.message);
      });
      // 清理发生在 listen 完成前时，这里补退订，避免监听器泄漏。
      if (disposed) {
        unlisten();
        unlisten = undefined;
      }
    })();
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  return <Toast message={message} />;
}

export default function App() {
  // 全局弹窗可访问性：打开自动聚焦主输入框 + Tab 焦点圈定（见 ui.tsx）
  useGlobalModalA11y();
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
        {activeView === "agent-sniff" && <AgentSniff onNavigate={setMainView} />}
        {activeView === "ai-providers" && <AiProviders />}
        {activeView === "model-config" && <ModelConfig />}
        {activeView === "project-config" && <ProjectConfig />}
        {activeView === "skills-manage" && <SkillsManage />}
        {activeView === "mcp-manage" && <McpManage />}
        {activeView === "claude-env" && <ClaudeEnv />}
        {activeView === "codex-env" && <CodexEnv />}
        {activeView === "route-aggregation" && <RouteAggregation />}
        {activeView === "preferences" && (
          <Preferences theme={theme} onThemeChange={handleThemeChange} />
        )}
        {activeView === "network" && <NetworkSettings />}
        {activeView === "webdav" && <WebDAV />}
        {activeView === "backup" && <BackupManage />}
      </main>
      <AutoBackupNotifier />
    </div>
  );
}
