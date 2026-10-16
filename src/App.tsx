import { useEffect, useState } from "react";
import Sidebar from "./components/Sidebar";
import AgentSniff from "./components/pages/AgentSniff";
import AiProviders from "./components/pages/AiProviders";
import BackupManage from "./components/pages/BackupManage";
import OpenCodeConfig from "./components/pages/OpenCodeConfig";
import PiConfig from "./components/pages/PiConfig";
import ProjectConfig from "./components/pages/ProjectConfig";
import SkillsManage from "./components/pages/SkillsManage";
import McpManage from "./components/pages/McpManage";
import ClaudeEnv from "./components/pages/ClaudeEnv";
import CodexEnv from "./components/pages/CodexEnv";
import Preferences from "./components/pages/Preferences";
import RouteAggregation from "./components/pages/RouteAggregation";
import {
  applyTheme,
  applyTextSize,
  loadAppConfig,
  saveTheme,
  saveTextSize,
  subscribeSystemTheme,
  syncWindowAppearance,
  DEFAULT_THEME,
  DEFAULT_TEXT_SIZE,
  TEXT_SIZES,
  type Theme,
} from "./lib/theme";
import { useStatusMessage } from "./lib/useStatusMessage";
import { useGlobalModalA11y } from "./components/ui";
import { Toast } from "./components/Toast";

export type MainView =
  | "agent-sniff"
  | "ai-providers"
  | "opencode-config"
  | "pi-config"
  | "project-config"
  | "skills-manage"
  | "mcp-manage"
  | "claude-env"
  | "codex-env"
  | "route-aggregation";
export type SettingsView = "preferences" | "backup";
/** 主视图与设置子视图统一为单一视图状态 */
export type View = MainView | SettingsView;

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
  const [view, setView] = useState<View>("agent-sniff");
  const [theme, setTheme] = useState<Theme>(DEFAULT_THEME);
  const [textSize, setTextSize] = useState<number>(DEFAULT_TEXT_SIZE);
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
      applyTextSize(config.textSize);
      setTheme(config.theme);
      setTextSize(config.textSize);
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

  const handleTextSizeChange = async (next: number) => {
    applyTextSize(next);
    setTextSize(next);
    const saved = await saveTextSize(next);
    if (saved !== next) {
      applyTextSize(saved);
      setTextSize(saved);
    }
  };

  // ⌘/Ctrl + + / − / 0：按档位循环文字大小（与 magpie 一致）。
  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if (!(e.metaKey || e.ctrlKey)) return;
      const key = e.key.toLowerCase();
      const idx = TEXT_SIZES.indexOf(textSize);
      if (key === "=" || key === "+") {
        e.preventDefault();
        const next = TEXT_SIZES[Math.min(TEXT_SIZES.length - 1, idx + 1)];
        if (next !== textSize) void handleTextSizeChange(next);
      } else if (key === "-") {
        e.preventDefault();
        const next = TEXT_SIZES[Math.max(0, idx - 1)];
        if (next !== textSize) void handleTextSizeChange(next);
      } else if (key === "0") {
        e.preventDefault();
        if (textSize !== DEFAULT_TEXT_SIZE) void handleTextSizeChange(DEFAULT_TEXT_SIZE);
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [textSize]);

  // system 模式下跟随系统外观：CSS 由 prefers-color-scheme 自动重绘，
  // 这里只负责在系统明暗变化时同步原生窗口外观（macOS traffic lights / Windows 标题栏）。
  useEffect(() => {
    if (theme !== "system") return;
    const unsubscribe = subscribeSystemTheme(() => {
      void syncWindowAppearance(theme);
    });
    return unsubscribe;
  }, [theme]);

  // Avoid a light→dark flash when saved theme is dark.
  if (!themeReady) {
    return <div className="app" aria-hidden />;
  }

  return (
    <div className="app">
      {/* Overlay 标题栏下的顶部拖拽区：鼠标按住顶部可拖动窗口 */}
      <div className="window-drag-region" data-tauri-drag-region aria-hidden />
      <Sidebar view={view} onNavigate={setView} />
      <main className="main-content">
        {view === "agent-sniff" && <AgentSniff onNavigate={setView} />}
        {view === "ai-providers" && <AiProviders />}
        {view === "opencode-config" && <OpenCodeConfig />}
        {view === "pi-config" && <PiConfig />}
        {view === "project-config" && <ProjectConfig />}
        {view === "skills-manage" && <SkillsManage />}
        {view === "mcp-manage" && <McpManage />}
        {view === "claude-env" && <ClaudeEnv />}
        {view === "codex-env" && <CodexEnv />}
        {view === "route-aggregation" && <RouteAggregation />}
        {view === "preferences" && (
          <Preferences
            theme={theme}
            onThemeChange={handleThemeChange}
            textSize={textSize}
            onTextSizeChange={handleTextSizeChange}
          />
        )}
        {view === "backup" && <BackupManage />}
      </main>
      <AutoBackupNotifier />
    </div>
  );
}
