import { useState, useEffect, useCallback, useRef } from "react";
import { getAgentIcon } from "../agent-icons";
import { useOverlayDismiss } from "../ui";
import { Toast } from "@/components/Toast";
import { useStatusMessage } from "@/lib/useStatusMessage";

/* ===== Types ===== */
interface AgentResult {
  name: string;
  display_name: string;
  icon: string;
  found: boolean;
  install_paths: string[];
  config_dirs: string[];
}

interface AgentOpenTargets {
  configDir: string | null;
  mcpFile: string | null;
  settingsFile: string | null;
}

interface AgentOpenResult {
  ok: boolean;
  message: string;
}

/* ===== SVG Icons ===== */
const IconSearch = () => (
  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
    <path d="M21 21l-4.35-4.35" />
    <circle cx="11" cy="11" r="7" />
  </svg>
);

const IconPlus = () => (
  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
    <line x1="12" y1="5" x2="12" y2="19" />
    <line x1="5" y1="12" x2="19" y2="12" />
  </svg>
);

const IconClose = () => (
  <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
    <line x1="18" y1="6" x2="6" y2="18" />
    <line x1="6" y1="6" x2="18" y2="18" />
  </svg>
);

const IconFolder = () => (
  <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
    <path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z" />
  </svg>
);

const IconFile = () => (
  <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
    <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" />
    <polyline points="14 2 14 8 20 8" />
    <line x1="8" y1="13" x2="16" y2="13" />
    <line x1="8" y1="17" x2="14" y2="17" />
  </svg>
);

/* ===== Helpers ===== */
function getInitials(name: string): string {
  const cleaned = name.replace(/[^a-zA-Z\u4e00-\u9fa5]/g, "");
  if (cleaned.length >= 2) return cleaned.substring(0, 2);
  return name.substring(0, 2);
}

function basename(path: string | null | undefined): string {
  if (!path) return "";
  const parts = path.split(/[/\\]/).filter(Boolean);
  return parts[parts.length - 1] ?? path;
}

function displayHomePath(path: string | null | undefined): string {
  if (!path) return "";
  // Backend already returns absolute paths; keep tooltip readable if it starts with home-like prefix is unknown on FE.
  return path;
}

/* ===== Component ===== */
export default function AgentSniff() {
  const [agents, setAgents] = useState<AgentResult[]>([]);
  const [isSniffing, setIsSniffing] = useState(false);
  const [summary, setSummary] = useState("");
  const [showAdd, setShowAdd] = useState(false);
  const [formName, setFormName] = useState("");
  const [formCliPath, setFormCliPath] = useState("");
  const [formConfigDir, setFormConfigDir] = useState("");
  const [openTargets, setOpenTargets] = useState<Record<string, AgentOpenTargets>>({});
  const [busyKey, setBusyKey] = useState<string | null>(null);
  const [statusMsg, setStatusMsg] = useStatusMessage();
  const nameInputRef = useRef<HTMLInputElement>(null);
  const hasLoaded = useRef(false);

  const addDismiss = useOverlayDismiss(() => setShowAdd(false));

  const loadOpenTargets = useCallback(async (list: AgentResult[]) => {
    const found = list.filter((a) => a.found);
    if (found.length === 0) {
      setOpenTargets({});
      return;
    }
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      const entries = await Promise.all(
        found.map(async (a) => {
          try {
            const t = await (invoke("agent_open_targets", { name: a.name }) as Promise<AgentOpenTargets>);
            return [a.name, t] as const;
          } catch {
            return [
              a.name,
              {
                configDir: a.config_dirs[0] ?? null,
                mcpFile: null,
                settingsFile: null,
              } as AgentOpenTargets,
            ] as const;
          }
        }),
      );
      const next: Record<string, AgentOpenTargets> = {};
      for (const [name, t] of entries) next[name] = t;
      setOpenTargets(next);
    } catch {
      // Browser preview / Tauri unavailable — fall back to config_dirs only.
      const next: Record<string, AgentOpenTargets> = {};
      for (const a of found) {
        next[a.name] = {
          configDir: a.config_dirs[0] ?? null,
          mcpFile: null,
          settingsFile: null,
        };
      }
      setOpenTargets(next);
    }
  }, []);

  // Load cached agents on mount
  useEffect(() => {
    if (hasLoaded.current) return;
    hasLoaded.current = true;

    (async () => {
      try {
        const { invoke } = await import("@tauri-apps/api/core");
        const cached = await (invoke("get_cached_agents") as Promise<AgentResult[]>);
        if (cached.length > 0) {
          setAgents(cached);
          void loadOpenTargets(cached);
          const foundCount = cached.filter((a) => a.found).length;
          setSummary(`最近一次扫描 — 发现 ${foundCount} 个已安装 Agent（共 ${cached.length} 个）`);
        } else {
          // No cached data, auto-sniff
          doSniff();
        }
      } catch {
        // Tauri API not available (dev in browser), show placeholder
        setSummary("在桌面应用中运行以扫描已安装的 Agent");
      }
    })();
  }, []);

  // Focus name input when modal opens
  useEffect(() => {
    if (showAdd) {
      setTimeout(() => nameInputRef.current?.focus(), 100);
    }
  }, [showAdd]);

  // Close modals on Escape
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.key === "Escape") setShowAdd(false);
    };
    document.addEventListener("keydown", handler);
    return () => document.removeEventListener("keydown", handler);
  }, []);

  const doSniff = useCallback(async () => {
    if (isSniffing) return;
    setIsSniffing(true);
    setAgents([]);
    setOpenTargets({});
    setSummary("正在扫描已安装的 Agent...");

    try {
      const { invoke } = await import("@tauri-apps/api/core");
      const results = await (invoke("sniff_agents") as Promise<AgentResult[]>);
      setAgents(results);
      void loadOpenTargets(results);
      const foundCount = results.filter((a) => a.found).length;
      setSummary(`扫描完成 — 发现 ${foundCount} 个已安装 Agent（共检测 ${results.length} 个）`);
    } catch (err) {
      setSummary(`扫描失败: ${err}`);
    } finally {
      setIsSniffing(false);
    }
  }, [isSniffing, loadOpenTargets]);

  const handleManualAdd = useCallback(async () => {
    const name = formName.trim();
    if (!name) return;

    const cliPath = formCliPath.trim() || null;
    const configDir = formConfigDir.trim() || null;

    try {
      const { invoke } = await import("@tauri-apps/api/core");
      const agent = await (invoke("add_agent_manual", { name, cliPath, configDir }) as Promise<AgentResult>);
      setAgents((prev) => {
        const next = [...prev, agent];
        void loadOpenTargets(next);
        return next;
      });
      const foundCount = agents.filter((a) => a.found).length + 1;
      setSummary(`共 ${foundCount} 个 Agent`);
    } catch {
      // Fallback: add locally
      const initials = getInitials(name);
      const agent: AgentResult = {
        name: name.toLowerCase().replace(/\s+/g, "-"),
        display_name: name,
        icon: initials.toUpperCase(),
        found: true,
        install_paths: cliPath ? [cliPath] : [],
        config_dirs: configDir ? [configDir] : [],
      };
      setAgents((prev) => {
        const next = [...prev, agent];
        void loadOpenTargets(next);
        return next;
      });
    }

    setShowAdd(false);
    setFormName("");
    setFormCliPath("");
    setFormConfigDir("");
  }, [formName, formCliPath, formConfigDir, agents, loadOpenTargets]);

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

  const handleRevealDir = useCallback(
    (agentName: string) => {
      void runOpen(`dir:${agentName}`, async () => {
        const { invoke } = await import("@tauri-apps/api/core");
        return invoke("reveal_agent_config_dir", { name: agentName }) as Promise<AgentOpenResult>;
      });
    },
    [runOpen],
  );

  const handleOpenFile = useCallback(
    (agentName: string, kind: "mcp" | "settings") => {
      void runOpen(`${kind}:${agentName}`, async () => {
        const { invoke } = await import("@tauri-apps/api/core");
        return invoke("open_agent_config_file", { name: agentName, kind }) as Promise<AgentOpenResult>;
      });
    },
    [runOpen],
  );

  return (
    <>
      <div className="content-header">
        <div className="content-header-bar">
          <h1 className="content-title">Agent管理</h1>
          <div className="header-actions">
            <button
              className={`action-btn ${isSniffing ? "sniffing" : ""}`}
              data-tooltip={isSniffing ? "扫描中..." : "扫描Agent"}
              onClick={doSniff}
              disabled={isSniffing}
            >
              <IconSearch />
            </button>
            <button
              className="action-btn"
              data-tooltip="手动添加"
              onClick={() => setShowAdd(true)}
            >
              <IconPlus />
            </button>
          </div>
        </div>
      </div>
      <div className="content-body">
        <Toast message={statusMsg} />
        {summary && <div className="sniff-summary">{summary}</div>}
        <div className="agent-list">
          {agents
            .map((agent, index) => ({ agent, index }))
            .sort((a, b) => {
              if (a.agent.found !== b.agent.found) return a.agent.found ? -1 : 1;
              return a.index - b.index;
            })
            .map(({ agent }) => {
              const targets = openTargets[agent.name];
              const settingsFile = targets?.settingsFile ?? null;
              const mcpFile = targets?.mcpFile ?? null;
              const configDir = targets?.configDir ?? agent.config_dirs[0] ?? null;
              const showActions = agent.found;
              const hasAnyAction = !!(configDir || mcpFile || settingsFile);
              const busy = busyKey?.endsWith(`:${agent.name}`) ?? false;

              return (
            <div key={agent.name} className="agent-card">
              <div className="agent-card-header">
                <div className={`agent-icon ${agent.found ? "found" : ""}`}>
                  {getAgentIcon(agent.name) ?? agent.icon}
                </div>
                <div className="agent-name">{agent.display_name}</div>
                {showActions && hasAnyAction && (
                  <div className="agent-card-actions">
                    {settingsFile && (
                      <button
                        type="button"
                        className="claude-env-action-btn"
                        title={`打开主配置 ${basename(settingsFile)}`}
                        onClick={() => handleOpenFile(agent.name, "settings")}
                        disabled={busy}
                      >
                        <IconFile />
                      </button>
                    )}
                    {mcpFile && (
                      <button
                        type="button"
                        className="claude-env-action-btn"
                        title={
                          settingsFile
                            ? `打开 MCP 配置 ${basename(mcpFile)}`
                            : `打开配置文件 ${basename(mcpFile)}`
                        }
                        onClick={() => handleOpenFile(agent.name, "mcp")}
                        disabled={busy}
                      >
                        <IconFile />
                      </button>
                    )}
                    {configDir && (
                      <button
                        type="button"
                        className="claude-env-action-btn"
                        title={`在 Finder 中打开 ${displayHomePath(configDir)}`}
                        onClick={() => handleRevealDir(agent.name)}
                        disabled={busy}
                      >
                        <IconFolder />
                      </button>
                    )}
                  </div>
                )}
                <span className="agent-status">
                  <span className={`status-dot ${agent.found ? "connected" : "disconnected"}`} />
                  {agent.found ? "已安装" : "未找到"}
                </span>
              </div>
              {agent.found && (agent.install_paths.length > 0 || agent.config_dirs.length > 0) && (
                <div className="agent-paths">
                  {agent.install_paths.map((path, i) => (
                    <div key={`install-${i}`} className="agent-path-row">
                      <span className="agent-path-label">
                        {path.endsWith('.app') ? "App路径" : "CLI 路径"}
                      </span>
                      <span className="agent-path-value">{path}</span>
                    </div>
                  ))}
                  {agent.config_dirs.map((dir, i) => (
                    <div key={`config-${i}`} className="agent-path-row">
                      <span className="agent-path-label">配置目录</span>
                      <span className="agent-path-value">{dir}</span>
                    </div>
                  ))}
                </div>
              )}
            </div>
              );
            })}
        </div>
      </div>

      {/* ===== Add Agent Modal ===== */}
      <div
        className={`modal-overlay ${showAdd ? "visible" : ""}`}
        {...addDismiss}
      >
        <div className="modal">
          <div className="modal-header">
            <h2 className="modal-title">手动添加 Agent</h2>
            <button className="modal-close" onClick={() => setShowAdd(false)}>
              <IconClose />
            </button>
          </div>
          <div className="modal-body">
            <div className="form-group">
              <label className="form-label" htmlFor="agent-name">Agent 名称</label>
              <input
                ref={nameInputRef}
                type="text"
                className="form-input"
                id="agent-name"
                placeholder="例如: MyAgent"
                value={formName}
                onChange={(e) => setFormName(e.target.value)}
              />
            </div>
            <div className="form-group">
              <label className="form-label" htmlFor="agent-cli-path">CLI/App路径</label>
              <input
                type="text"
                className="form-input"
                id="agent-cli-path"
                placeholder="例如: /usr/local/bin/myagent 或 /Applications/MyAgent.app"
                value={formCliPath}
                onChange={(e) => setFormCliPath(e.target.value)}
              />
            </div>
            <div className="form-group">
              <label className="form-label" htmlFor="agent-config-dir">配置目录</label>
              <input
                type="text"
                className="form-input"
                id="agent-config-dir"
                placeholder="例如: ~/.myagent"
                value={formConfigDir}
                onChange={(e) => setFormConfigDir(e.target.value)}
              />
            </div>
          </div>
          <div className="modal-footer">
            <button className="btn btn-secondary" onClick={() => setShowAdd(false)}>取消</button>
            <button className="btn btn-primary" onClick={handleManualAdd}>保存</button>
          </div>
        </div>
      </div>
    </>
  );
}
