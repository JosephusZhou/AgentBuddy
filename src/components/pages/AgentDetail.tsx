import { useCallback, useEffect, useState } from "react";
import {
  ArrowLeft,
  FileCog,
  FileJson,
  FolderOpen,
  Puzzle,
  Sparkles,
} from "lucide-react";
import { getAgentIcon } from "../agent-icons";
import { Toast } from "@/components/Toast";
import { useStatusMessage } from "@/lib/useStatusMessage";

/* ===== Types (mirror `get_agent_detail` in lib.rs, camelCase) ===== */
interface AgentMcpInfo {
  title: string;
  transport: string;
  command: string;
  args: string[];
  url: string;
}

interface AgentSkillInfo {
  id: string;
  title: string;
  description: string;
  path: string;
}

interface AgentDetailData {
  name: string;
  displayName: string;
  icon: string;
  found: boolean;
  installPaths: string[];
  configDirs: string[];
  configDir: string | null;
  mcpFile: string | null;
  settingsFile: string | null;
  mcps: AgentMcpInfo[];
  skills: AgentSkillInfo[];
}

interface AgentOpenResult {
  ok: boolean;
  message: string;
}

export interface AgentDetailProps {
  /** Agent name (registry id, e.g. "claude-code") from the list view. */
  name: string;
  /** Display name fallback while the detail loads. */
  displayName: string;
  icon: string;
  onBack: () => void;
}

function basename(path: string | null | undefined): string {
  if (!path) return "";
  const parts = path.split(/[/\\]/).filter(Boolean);
  return parts[parts.length - 1] ?? path;
}

export default function AgentDetail({ name, displayName, icon, onBack }: AgentDetailProps) {
  const [detail, setDetail] = useState<AgentDetailData | null>(null);
  const [loadError, setLoadError] = useState("");
  const [busyKey, setBusyKey] = useState<string | null>(null);
  const [statusMsg, setStatusMsg] = useStatusMessage();

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const { invoke } = await import("@tauri-apps/api/core");
        const data = await (invoke("get_agent_detail", { name }) as Promise<AgentDetailData>);
        if (!cancelled) setDetail(data);
      } catch (err) {
        if (!cancelled) setLoadError(err instanceof Error ? err.message : String(err));
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [name]);

  // Escape 返回列表
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.key === "Escape") onBack();
    };
    document.addEventListener("keydown", handler);
    return () => document.removeEventListener("keydown", handler);
  }, [onBack]);

  const runOpen = useCallback(
    async (key: string, action: () => Promise<AgentOpenResult>) => {
      if (busyKey) return;
      setBusyKey(key);
      try {
        const result = await action();
        setStatusMsg(result.message || "已打开");
      } catch (err) {
        setStatusMsg(err instanceof Error ? err.message : String(err));
      } finally {
        setBusyKey(null);
      }
    },
    [busyKey, setStatusMsg],
  );

  const handleRevealDir = useCallback(() => {
    void runOpen("dir", async () => {
      const { invoke } = await import("@tauri-apps/api/core");
      return invoke("reveal_agent_config_dir", { name }) as Promise<AgentOpenResult>;
    });
  }, [name, runOpen]);

  const handleOpenFile = useCallback(
    (kind: "mcp" | "settings") => {
      void runOpen(kind, async () => {
        const { invoke } = await import("@tauri-apps/api/core");
        return invoke("open_agent_config_file", { name, kind }) as Promise<AgentOpenResult>;
      });
    },
    [name, runOpen],
  );

  const title = detail?.displayName || displayName;
  const installPaths = detail?.installPaths ?? [];
  const configDirs = detail?.configDirs ?? [];
  const appPaths = installPaths.filter((p) => p.endsWith(".app"));
  const cliPaths = installPaths.filter((p) => !p.endsWith(".app"));
  const mcpFile = detail?.mcpFile ?? null;
  const settingsFile = detail?.settingsFile ?? null;
  const configDir = detail?.configDir ?? configDirs[0] ?? null;
  const mcps = detail?.mcps ?? [];
  const skills = detail?.skills ?? [];
  const busy = busyKey !== null;

  return (
    <>
      <div className="content-header">
        <div className="content-header-bar">
          <button
            type="button"
            className="action-btn"
            data-tooltip="返回 Agent 列表"
            data-tooltip-align="start"
            aria-label="返回 Agent 列表"
            onClick={onBack}
          >
            <ArrowLeft strokeWidth={1.8} />
          </button>
          <h1 className="content-title">{title}</h1>
        </div>
      </div>
      <div className="content-body">
        <Toast message={statusMsg} />

        {!detail && !loadError && <div className="sniff-summary">正在加载详情...</div>}
        {loadError && <div className="sniff-summary">加载详情失败: {loadError}</div>}

        {detail && (
          <div className="agent-detail">
            {/* ===== 概览 ===== */}
            <div className="agent-card">
              <div className="agent-card-header">
                <div className={`agent-icon ${detail.found ? "found" : ""}`}>
                  {getAgentIcon(detail.name) ?? icon}
                </div>
                <div className="agent-name">{title}</div>
                <span className="agent-status">
                  <span className={`status-dot ${detail.found ? "connected" : "disconnected"}`} />
                  {detail.found ? "已安装" : "未找到"}
                </span>
              </div>
            </div>

            {/* ===== 路径信息 ===== */}
            <section className="agent-detail-section">
              <h2 className="agent-detail-section-title">路径信息</h2>
              {appPaths.length === 0 && cliPaths.length === 0 && configDirs.length === 0 && !configDir && (
                <div className="agent-detail-empty">暂无路径信息</div>
              )}
              <div className="agent-detail-rows">
                {appPaths.map((p) => (
                  <div key={`app-${p}`} className="agent-path-row">
                    <span className="agent-path-label">App 路径</span>
                    <span className="agent-path-value">{p}</span>
                  </div>
                ))}
                {cliPaths.map((p) => (
                  <div key={`cli-${p}`} className="agent-path-row">
                    <span className="agent-path-label">CLI 路径</span>
                    <span className="agent-path-value">{p}</span>
                  </div>
                ))}
                {(configDirs.length > 0 ? configDirs : configDir ? [configDir] : []).map((dir) => (
                  <div key={`cfg-${dir}`} className="agent-path-row">
                    <span className="agent-path-label">配置目录</span>
                    <span className="agent-path-value">{dir}</span>
                    {configDir && (
                      <button
                        type="button"
                        className="claude-env-action-btn"
                        data-tooltip={`在 Finder 中打开 ${configDir}`}
                        onClick={handleRevealDir}
                        disabled={busy}
                      >
                        <FolderOpen size={16} strokeWidth={1.8} />
                      </button>
                    )}
                  </div>
                ))}
              </div>
            </section>

            {/* ===== 配置文件 ===== */}
            <section className="agent-detail-section">
              <h2 className="agent-detail-section-title">配置文件</h2>
              {!mcpFile && !settingsFile && (
                <div className="agent-detail-empty">该 Agent 没有已知的配置文件</div>
              )}
              <div className="agent-detail-rows">
                {settingsFile && (
                  <div className="agent-path-row">
                    <span className="agent-path-label">主配置</span>
                    <span className="agent-path-value">{settingsFile}</span>
                    <button
                      type="button"
                      className="claude-env-action-btn"
                      data-tooltip={`打开主配置 ${basename(settingsFile)}`}
                      onClick={() => handleOpenFile("settings")}
                      disabled={busy}
                    >
                      <FileCog size={16} strokeWidth={1.8} />
                    </button>
                  </div>
                )}
                {mcpFile && (
                  <div className="agent-path-row">
                    <span className="agent-path-label">MCP 配置</span>
                    <span className="agent-path-value">{mcpFile}</span>
                    <button
                      type="button"
                      className="claude-env-action-btn"
                      data-tooltip={`打开 MCP 配置 ${basename(mcpFile)}`}
                      onClick={() => handleOpenFile("mcp")}
                      disabled={busy}
                    >
                      <FileJson size={16} strokeWidth={1.8} />
                    </button>
                  </div>
                )}
              </div>
            </section>

            {/* ===== MCP 列表 ===== */}
            <section className="agent-detail-section">
              <h2 className="agent-detail-section-title">
                <Puzzle size={14} strokeWidth={1.8} />
                MCP 服务（{mcps.length}）
              </h2>
              {mcps.length === 0 ? (
                <div className="agent-detail-empty">未配置 MCP 服务</div>
              ) : (
                <div className="agent-detail-items">
                  {mcps.map((mcp) => (
                    <div key={mcp.title} className="agent-detail-item">
                      <div className="agent-detail-item-header">
                        <span className="agent-detail-item-title">{mcp.title}</span>
                        {mcp.transport && (
                          <span className="agent-detail-tag">{mcp.transport}</span>
                        )}
                      </div>
                      {mcp.command && (
                        <div className="agent-detail-item-sub">
                          {[mcp.command, ...mcp.args].join(" ")}
                        </div>
                      )}
                      {!mcp.command && mcp.url && (
                        <div className="agent-detail-item-sub">{mcp.url}</div>
                      )}
                    </div>
                  ))}
                </div>
              )}
            </section>

            {/* ===== Skills 列表 ===== */}
            <section className="agent-detail-section">
              <h2 className="agent-detail-section-title">
                <Sparkles size={14} strokeWidth={1.8} />
                Skills（{skills.length}）
              </h2>
              {skills.length === 0 ? (
                <div className="agent-detail-empty">未安装 Skills</div>
              ) : (
                <div className="agent-detail-items">
                  {skills.map((skill) => (
                    <div key={skill.id} className="agent-detail-item">
                      <div className="agent-detail-item-header">
                        <span className="agent-detail-item-title">{skill.title}</span>
                        {skill.title !== skill.id && (
                          <span className="agent-detail-tag">{skill.id}</span>
                        )}
                      </div>
                      {skill.description && (
                        <div className="agent-detail-item-sub">{skill.description}</div>
                      )}
                      <div className="agent-detail-item-sub agent-detail-item-path">
                        {skill.path}
                      </div>
                    </div>
                  ))}
                </div>
              )}
            </section>
          </div>
        )}
      </div>
    </>
  );
}
