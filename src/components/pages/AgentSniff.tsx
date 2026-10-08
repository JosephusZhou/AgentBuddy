import { useCallback, useEffect, useMemo, useRef, useState, type CSSProperties } from "react";
import { getAgentIcon } from "../agent-icons";
import { useOverlayDismiss } from "../ui";
import { Toast } from "@/components/Toast";
import { useStatusMessage } from "@/lib/useStatusMessage";
import AgentDetail from "./AgentDetail";
import type { MainView } from "../../App";
import {
  Bot,
  Check,
  ChevronRight,
  CloudCog,
  Copy,
  ExternalLink,
  FileCog,
  FileJson,
  FolderOpen,
  Plus,
  Radar,
  Search,
  Sparkles,
  X,
  Zap,
} from "lucide-react";

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

interface AgentConfigStat {
  name: string;
  mcpCount: number;
  skillCount: number;
  /** 排序后的前 N 个 MCP 名称 */
  mcpTitles: string[];
  /** 排序后的前 N 个 Skill id */
  skillIds: string[];
}

/** 注册表静态元数据（形态 / 职责描述 / Skills 支持度） */
interface AgentProfile {
  name: string;
  form: "cli" | "desktop" | "hybrid";
  description: string;
  skillsSupported: boolean;
}

interface AgentVersionInfo {
  name: string;
  version: string | null;
}

/** 卡片筛选维度 */
type AgentFilter = "all" | "cli" | "desktop" | "missing";

/* ===== Helpers ===== */
function basename(path: string | null | undefined): string {
  if (!path) return "";
  const parts = path.split(/[/\\]/).filter(Boolean);
  return parts[parts.length - 1] ?? path;
}

function getInitials(name: string): string {
  const cleaned = name.replace(/[^a-zA-Z一-龥]/g, "");
  const base = cleaned.length >= 2 ? cleaned.substring(0, 2) : name.substring(0, 2);
  return base.toUpperCase();
}

const FORM_LABEL: Record<AgentProfile["form"], string> = {
  cli: "CLI",
  desktop: "桌面端",
  hybrid: "CLI + 桌面端",
};

/** Agent 头像底色：按名称散列到一组琥珀系渐变，纯装饰、稳定不随渲染变化。
 *  取色范围对齐设计稿实测（顶部 #e16916 → 底部 #d16213 一带），不引入过浅的橙。 */
const AVATAR_GRADIENTS = [
  "linear-gradient(150deg, #f97316, #b45309)",
  "linear-gradient(150deg, #f59e0b, #b45309)",
  "linear-gradient(150deg, #ea580c, #9a3412)",
  "linear-gradient(150deg, #fb923c, #c2410c)",
];

function avatarGradient(name: string): string {
  let hash = 0;
  for (let i = 0; i < name.length; i += 1) hash = (hash * 31 + name.charCodeAt(i)) >>> 0;
  return AVATAR_GRADIENTS[hash % AVATAR_GRADIENTS.length];
}

/** 把 Unix 秒时间戳格式化成"刚刚 / N 分钟前 / N 小时前 / M-D"。 */
function formatScanTime(ts: number | null): string {
  if (!ts) return "尚未扫描";
  const diff = Math.max(0, Math.floor(Date.now() / 1000) - ts);
  if (diff < 60) return "刚刚更新";
  if (diff < 3600) return `${Math.floor(diff / 60)} 分钟前更新`;
  if (diff < 86400) return `${Math.floor(diff / 3600)} 小时前更新`;
  const d = new Date(ts * 1000);
  return `${d.getMonth() + 1}-${d.getDate()} 更新`;
}

const DOC_URL = "https://github.com/JosephusZhou/AgentBuddy#readme";

/* ===== Component ===== */
interface AgentSniffProps {
  /** 跳转到其他主视图（详情页的"前往管理"入口用） */
  onNavigate?: (view: MainView) => void;
}

export default function AgentSniff({ onNavigate }: AgentSniffProps) {
  const [agents, setAgents] = useState<AgentResult[]>([]);
  const [isSniffing, setIsSniffing] = useState(false);
  const [summary, setSummary] = useState("");
  const [scanError, setScanError] = useState("");
  const [lastScanAt, setLastScanAt] = useState<number | null>(null);
  const [showAdd, setShowAdd] = useState(false);
  const [formName, setFormName] = useState("");
  const [formCliPath, setFormCliPath] = useState("");
  const [formConfigDir, setFormConfigDir] = useState("");
  const [openTargets, setOpenTargets] = useState<Record<string, AgentOpenTargets>>({});
  const [configStats, setConfigStats] = useState<Record<string, AgentConfigStat>>({});
  const [profiles, setProfiles] = useState<Record<string, AgentProfile>>({});
  const [versions, setVersions] = useState<Record<string, string | null>>({});
  const [busyKey, setBusyKey] = useState<string | null>(null);
  const [selectedAgent, setSelectedAgent] = useState<AgentResult | null>(null);
  const [statusMsg, setStatusMsg] = useStatusMessage();
  const [query, setQuery] = useState("");
  const [filter, setFilter] = useState<AgentFilter>("all");
  const [copiedKey, setCopiedKey] = useState<string | null>(null);
  const nameInputRef = useRef<HTMLInputElement>(null);
  const searchInputRef = useRef<HTMLInputElement>(null);
  const hasLoaded = useRef(false);

  const addDismiss = useOverlayDismiss(() => setShowAdd(false));

  /* ===== 数据加载 ===== */

  const loadOpenTargets = useCallback(async (list: AgentResult[]) => {
    const found = list.filter((a) => a.found);
    if (found.length === 0) {
      setOpenTargets({});
      return;
    }
    // 逐个 invoke，任一 agent 失败只影响它自己的入口
    const entries = await Promise.all(
      found.map(async (a) => {
        try {
          const { invoke } = await import("@tauri-apps/api/core");
          const t = await (invoke("agent_open_targets", {
            name: a.name,
          }) as Promise<AgentOpenTargets>);
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
  }, []);

  const loadConfigStats = useCallback(async (list: AgentResult[]) => {
    const found = list.filter((a) => a.found);
    if (found.length === 0) {
      setConfigStats({});
      return;
    }
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      const stats = await (invoke("get_agent_config_stats", {
        names: found.map((a) => a.name),
      }) as Promise<AgentConfigStat[]>);
      const next: Record<string, AgentConfigStat> = {};
      for (const s of stats) next[s.name] = s;
      setConfigStats(next);
    } catch {
      // 浏览器预览 / 读取失败：统计留空，卡片相应标签不渲染。
    }
  }, []);

  /** 版本探测：仅对已安装的注册表 agent 发起，失败静默（状态药丸退回"已就绪"）。 */
  const loadVersions = useCallback(async (list: AgentResult[]) => {
    const names = list.filter((a) => a.found).map((a) => a.name);
    if (names.length === 0) {
      setVersions({});
      return;
    }
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      const probes = await (invoke("get_agent_versions", {
        names,
      }) as Promise<AgentVersionInfo[]>);
      const next: Record<string, string | null> = {};
      for (const p of probes) next[p.name] = p.version;
      setVersions(next);
    } catch {
      setVersions({});
    }
  }, []);

  const loadScanTime = useCallback(async () => {
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      const ts = await (invoke("get_last_agent_scan_time") as Promise<number | null>);
      setLastScanAt(typeof ts === "number" ? ts : null);
    } catch {
      setLastScanAt(null);
    }
  }, []);

  const loadProfiles = useCallback(async () => {
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      const list = await (invoke("get_agent_profiles") as Promise<AgentProfile[]>);
      const next: Record<string, AgentProfile> = {};
      for (const p of list) next[p.name] = p;
      setProfiles(next);
    } catch {
      setProfiles({});
    }
  }, []);

  /** 扫描完成后拉取全部派生数据。 */
  const hydrate = useCallback(
    async (list: AgentResult[]) => {
      await Promise.all([
        loadOpenTargets(list),
        loadConfigStats(list),
        loadVersions(list),
        loadScanTime(),
      ]);
    },
    [loadOpenTargets, loadConfigStats, loadVersions, loadScanTime],
  );

  const doSniff = useCallback(async () => {
    if (isSniffing) return;
    setIsSniffing(true);
    setAgents([]);
    setOpenTargets({});
    setConfigStats({});
    setVersions({});
    setScanError("");
    setSummary("正在扫描已安装的 Agent...");

    try {
      const { invoke } = await import("@tauri-apps/api/core");
      const results = await (invoke("sniff_agents") as Promise<AgentResult[]>);
      setAgents(results);
      setSummary("");
      await hydrate(results);
    } catch (err) {
      setSummary("");
      setScanError(`扫描失败: ${err}`);
    } finally {
      setIsSniffing(false);
    }
  }, [isSniffing, hydrate]);

  // 加载缓存的扫描结果；无缓存时自动扫描一次
  useEffect(() => {
    if (hasLoaded.current) return;
    hasLoaded.current = true;
    void loadProfiles();

    (async () => {
      try {
        const { invoke } = await import("@tauri-apps/api/core");
        const cached = await (invoke("get_cached_agents") as Promise<AgentResult[]>);
        if (cached.length > 0) {
          setAgents(cached);
          setSummary("");
          void hydrate(cached);
        } else {
          await doSniff();
        }
      } catch {
        setSummary("在桌面应用中运行以扫描已安装的 Agent");
      }
    })();
  }, [doSniff, hydrate, loadProfiles]);

  /* ===== 交互 ===== */

  // 弹窗打开时聚焦首个输入框（nameInputRef 已由全局 a11y 兜底，这里做首屏聚焦）
  useEffect(() => {
    if (showAdd) {
      setTimeout(() => nameInputRef.current?.focus(), 100);
    }
  }, [showAdd]);

  // Esc 关闭弹窗；⌘K / Ctrl+K 聚焦搜索框（新 UI 全局快捷键）
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        setShowAdd(false);
        return;
      }
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        searchInputRef.current?.focus();
        searchInputRef.current?.select();
      }
    };
    document.addEventListener("keydown", handler);
    return () => document.removeEventListener("keydown", handler);
  }, []);

  const handleManualAdd = useCallback(async () => {
    const name = formName.trim();
    if (!name) return;

    const cliPath = formCliPath.trim() || null;
    const configDir = formConfigDir.trim() || null;

    let agent: AgentResult;
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      agent = await (invoke("add_agent_manual", {
        name,
        cliPath,
        configDir,
      }) as Promise<AgentResult>);
    } catch {
      // 浏览器预览 / 后端失败：本地插入一条占位，保证交互可继续
      agent = {
        name: name.toLowerCase().replace(/\s+/g, "-"),
        display_name: name,
        icon: getInitials(name),
        found: true,
        install_paths: cliPath ? [cliPath] : [],
        config_dirs: configDir ? [configDir] : [],
      };
    }

    setAgents((prev) => {
      const next = [...prev, agent];
      void hydrate(next);
      return next;
    });

    setShowAdd(false);
    setFormName("");
    setFormCliPath("");
    setFormConfigDir("");
  }, [formName, formCliPath, formConfigDir, hydrate]);

  const runOpen = useCallback(
    async (key: string, action: () => Promise<AgentOpenResult | void>) => {
      if (busyKey) return;
      setBusyKey(key);
      try {
        const result = await action();
        if (result) setStatusMsg(result.message || "已打开");
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
        return invoke("open_agent_config_file", {
          name: agentName,
          kind,
        }) as Promise<AgentOpenResult>;
      });
    },
    [runOpen],
  );

  const handleOpenDoc = useCallback(() => {
    void runOpen("doc", async () => {
      const { invoke } = await import("@tauri-apps/api/core");
      return invoke("open_external_url", { url: DOC_URL }) as Promise<void>;
    });
  }, [runOpen]);

  const handleCopy = useCallback(
    (key: string, text: string) => {
      void navigator.clipboard
        .writeText(text)
        .then(() => {
          setCopiedKey(key);
          setTimeout(() => setCopiedKey((cur) => (cur === key ? null : cur)), 1200);
        })
        .catch(() => setStatusMsg("复制失败"));
    },
    [setStatusMsg],
  );

  /** 未安装卡片上的"手动指定路径"：预填名称直接打开弹窗 */
  const openAddWithName = useCallback((displayName: string) => {
    setFormName(displayName);
    setShowAdd(true);
  }, []);

  /* ===== 派生数据 ===== */

  const sorted = useMemo(
    () =>
      agents
        .map((agent, index) => ({ agent, index }))
        .sort((a, b) => {
          if (a.agent.found !== b.agent.found) return a.agent.found ? -1 : 1;
          return a.index - b.index;
        })
        .map(({ agent }) => agent),
    [agents],
  );

  const installedCount = useMemo(() => agents.filter((a) => a.found).length, [agents]);
  // 「预设」= 注册表条目数；手动添加的 Agent 不属于预设，不计入分母
  const presetCount = useMemo(() => {
    const registered = Object.keys(profiles).length;
    return registered > 0 ? registered : agents.length;
  }, [agents.length, profiles]);
  const installRate = useMemo(
    () => (presetCount > 0 ? Math.min(100, Math.round((installedCount / presetCount) * 100)) : 0),
    [installedCount, presetCount],
  );

  const { mcpTotal, skillTotal, agentsWithMcp } = useMemo(() => {
    let mcp = 0;
    let skill = 0;
    let withMcp = 0;
    for (const stat of Object.values(configStats)) {
      mcp += stat.mcpCount;
      skill += stat.skillCount;
      if (stat.mcpCount > 0) withMcp += 1;
    }
    return { mcpTotal: mcp, skillTotal: skill, agentsWithMcp: withMcp };
  }, [configStats]);

  const filterCounts = useMemo(() => {
    let cli = 0;
    let desktop = 0;
    let missing = 0;
    for (const agent of agents) {
      if (!agent.found) {
        missing += 1;
        continue;
      }
      const form = profiles[agent.name]?.form;
      if (form === "desktop") desktop += 1;
      else if (form === "cli" || form === "hybrid") cli += 1;
    }
    return { all: agents.length, cli, desktop, missing };
  }, [agents, profiles]);

  const visible = useMemo(() => {
    const q = query.trim().toLowerCase();
    return sorted.filter((agent) => {
      const form = profiles[agent.name]?.form;
      if (filter === "missing" && agent.found) return false;
      if (filter === "cli" && (!agent.found || (form !== "cli" && form !== "hybrid"))) return false;
      if (filter === "desktop" && (!agent.found || form !== "desktop")) return false;
      if (!q) return true;
      const stat = configStats[agent.name];
      const targets = openTargets[agent.name];
      const haystack = [
        agent.display_name,
        agent.name,
        ...agent.install_paths,
        ...agent.config_dirs,
        ...(stat?.mcpTitles ?? []),
        ...(stat?.skillIds ?? []),
        targets?.mcpFile ?? "",
        targets?.settingsFile ?? "",
      ]
        .join(" ")
        .toLowerCase();
      return haystack.includes(q);
    });
  }, [sorted, query, filter, profiles, configStats, openTargets]);

  /* ===== 详情页（未迁移，仍用旧版式） ===== */
  const selected = selectedAgent
    ? (agents.find((a) => a.name === selectedAgent.name) ?? selectedAgent)
    : null;
  if (selected) {
    return (
      <AgentDetail
        name={selected.name}
        displayName={selected.display_name}
        icon={selected.icon}
        onBack={() => setSelectedAgent(null)}
        onManageMcps={onNavigate ? () => onNavigate("mcp-manage") : undefined}
        onManageSkills={onNavigate ? () => onNavigate("skills-manage") : undefined}
      />
    );
  }

  /* ===== 渲染 ===== */
  return (
    <>
      <div className="content-body">
        <Toast message={statusMsg} />
        <div className="kb-page">
          {/* ===== 页头 ===== */}
          <div className="kb-page-head">
            <div className="kb-page-head-text">
              <h1 className="kb-page-title kb-headline-xl">Agent 发现与管理</h1>
              <p className="kb-page-subtitle kb-body-md">
                实时探测宿主机已安装的 AI Agent、CLI 运行时、MCP 挂载与上下文配置
              </p>
            </div>
            <div className="kb-page-actions">
              <div className="kb-search">
                <span className="kb-search-icon">
                  <Search size={15} strokeWidth={1.8} />
                </span>
                <input
                  ref={searchInputRef}
                  type="text"
                  className="kb-search-input"
                  placeholder="过滤 Agent、路径或 MCP..."
                  value={query}
                  onChange={(e) => setQuery(e.target.value)}
                  aria-label="过滤 Agent、路径或 MCP"
                />
                <span className="kb-search-kbd">⌘K</span>
              </div>
              <button
                type="button"
                className="kb-btn kb-btn-primary"
                onClick={doSniff}
                disabled={isSniffing}
              >
                <Radar size={16} strokeWidth={2} className={isSniffing ? "kb-spin" : ""} />
                <span>{isSniffing ? "扫描中" : "重新扫描"}</span>
              </button>
              <button
                type="button"
                className="kb-btn kb-btn-ghost"
                onClick={() => {
                  setFormName("");
                  setFormCliPath("");
                  setFormConfigDir("");
                  setShowAdd(true);
                }}
              >
                <Plus size={16} strokeWidth={2} />
                <span>关联</span>
              </button>
            </div>
          </div>

          {/* ===== 指标条（全部为真实数据） ===== */}
          <div className="kb-metrics">
            <div
              className="kb-metric"
              style={
                {
                  "--kb-metric-accent": "var(--kb-amber)",
                  "--kb-metric-glow": "var(--kb-amber-tint)",
                } as CSSProperties
              }
            >
              <div className="kb-metric-main">
                <div className="kb-metric-icon">
                  <Bot size={20} strokeWidth={1.8} />
                </div>
                <div className="kb-metric-body">
                  <span className="kb-metric-label kb-label-sm">已识别 Agent</span>
                  <div className="kb-metric-value-row">
                    <span className="kb-metric-value kb-headline-lg">{installedCount}</span>
                    <span className="kb-metric-suffix kb-code-sm">/ {presetCount} 预设</span>
                  </div>
                </div>
              </div>
              <div className="kb-metric-side">
                <span
                  className={`kb-pill ${installRate === 100 ? "kb-pill-status" : "kb-pill-accent"}`}
                >
                  <span className="kb-dot" />
                  <span className="kb-code-sm">安装率 {installRate}%</span>
                </span>
                <span className="kb-metric-foot kb-code-sm">{formatScanTime(lastScanAt)}</span>
              </div>
            </div>

            <div
              className="kb-metric"
              style={
                {
                  "--kb-metric-accent": "var(--kb-cyan)",
                  "--kb-metric-glow": "var(--kb-cyan-tint)",
                } as CSSProperties
              }
            >
              <div className="kb-metric-main">
                <div className="kb-metric-icon">
                  <Zap size={20} strokeWidth={1.8} />
                </div>
                <div className="kb-metric-body">
                  <span className="kb-metric-label kb-label-sm">挂载 MCP 服务</span>
                  <div className="kb-metric-value-row">
                    <span className="kb-metric-value kb-headline-lg">{mcpTotal}</span>
                    <span className="kb-metric-suffix kb-code-sm">已激活连接</span>
                  </div>
                </div>
              </div>
              <div className="kb-metric-side">
                <span className="kb-metric-foot kb-code-sm">跨 {agentsWithMcp} 个 Agent</span>
              </div>
            </div>

            <div
              className="kb-metric"
              style={
                {
                  "--kb-metric-accent": "var(--kb-green)",
                  "--kb-metric-glow": "var(--kb-green-tint)",
                } as CSSProperties
              }
            >
              <div className="kb-metric-main">
                <div className="kb-metric-icon">
                  <Sparkles size={20} strokeWidth={1.8} />
                </div>
                <div className="kb-metric-body">
                  <span className="kb-metric-label kb-label-sm">共享 Skills 技能库</span>
                  <div className="kb-metric-value-row">
                    <span className="kb-metric-value kb-headline-lg">{skillTotal}</span>
                    <span className="kb-metric-suffix kb-code-sm">套现成注入</span>
                  </div>
                </div>
              </div>
              <div className="kb-metric-side">
                <span className="kb-metric-foot kb-code-sm">~/.agentbuddy/skills</span>
              </div>
            </div>
          </div>

          {/* ===== 筛选 ===== */}
          <div className="kb-filters" role="group" aria-label="按形态筛选 Agent">
            {(
              [
                ["all", "全部"],
                ["cli", "命令行"],
                ["desktop", "桌面端"],
                ["missing", "未安装"],
              ] as [AgentFilter, string][]
            ).map(([key, label]) => (
              <button
                key={key}
                type="button"
                className={`kb-filter ${filter === key ? "active" : ""}`}
                aria-pressed={filter === key}
                onClick={() => setFilter(key)}
              >
                <span>{label}</span>
                <span className="kb-filter-count">{filterCounts[key]}</span>
              </button>
            ))}
          </div>

          {/* ===== 状态提示 ===== */}
          {scanError && (
            <div className="kb-note error kb-body-md">
              <span>{scanError}</span>
            </div>
          )}
          {summary && !scanError && (
            <div className="kb-note kb-body-md">
              <Radar size={15} strokeWidth={1.8} />
              <span>{summary}</span>
            </div>
          )}

          {/* ===== 卡片列表 ===== */}
          <div className="kb-cards">
            {visible.length === 0 && (
              <div className="kb-note empty kb-body-md">
                {agents.length === 0 && !isSniffing
                  ? "暂无扫描结果，点击「重新扫描」开始探测"
                  : "没有符合当前筛选条件的 Agent"}
              </div>
            )}
            {visible.map((agent) => {
              const targets = openTargets[agent.name];
              const stat = configStats[agent.name];
              const profile = profiles[agent.name];
              const settingsFile = targets?.settingsFile ?? null;
              const mcpFile = targets?.mcpFile ?? null;
              const configDir = targets?.configDir ?? agent.config_dirs[0] ?? null;
              const appPath = agent.install_paths.find((p) => p.endsWith(".app")) ?? null;
              const cliPath = agent.install_paths.find((p) => !p.endsWith(".app")) ?? null;
              const busy = busyKey?.endsWith(`:${agent.name}`) ?? false;
              const version = versions[agent.name] ?? null;

              return (
                <div
                  key={agent.name}
                  className={`kb-card ${agent.found ? "clickable" : "missing"}`}
                  role={agent.found ? "button" : undefined}
                  tabIndex={agent.found ? 0 : undefined}
                  aria-label={agent.found ? `查看 ${agent.display_name} 详情` : undefined}
                  onClick={agent.found ? () => setSelectedAgent(agent) : undefined}
                  onKeyDown={
                    agent.found
                      ? (e) => {
                          if (e.key === "Enter" || e.key === " ") {
                            e.preventDefault();
                            setSelectedAgent(agent);
                          }
                        }
                      : undefined
                  }
                >
                  {/* ---- 卡片头 ---- */}
                  <div className="kb-card-head">
                    <div className="kb-card-id">
                      <div
                        className={`kb-avatar ${agent.found ? "" : "missing"}`}
                        style={agent.found ? { background: avatarGradient(agent.name) } : undefined}
                        aria-hidden
                      >
                        {agent.found ? (
                          <span style={{ color: "#552100" }}>
                            {getAgentIcon(agent.name) ?? (
                              <span className="kb-avatar-initials">{agent.icon}</span>
                            )}
                          </span>
                        ) : (
                          <span className="kb-avatar-initials">{agent.icon}</span>
                        )}
                      </div>
                      <div className="kb-card-id-text">
                        <div className="kb-card-title-row">
                          <h2 className="kb-card-title kb-headline-md">{agent.display_name}</h2>
                          {profile && (
                            <span className="kb-pill kb-pill-form">{FORM_LABEL[profile.form]}</span>
                          )}
                          <span
                            className={`kb-pill kb-pill-status ${agent.found ? "" : "missing"}`}
                          >
                            <span className={`kb-dot ${agent.found ? "pulse" : ""}`} />
                            <span>
                              {agent.found ? "已就绪" : "未检测到安装"}
                              {agent.found && version ? ` · ${version}` : ""}
                            </span>
                          </span>
                        </div>
                        {profile?.description && (
                          <p className="kb-card-desc kb-body-sm">{profile.description}</p>
                        )}
                      </div>
                    </div>

                    {agent.found ? (
                      <div className="kb-card-tools" onClick={(e) => e.stopPropagation()}>
                        {settingsFile && (
                          <button
                            type="button"
                            className="kb-icon-btn"
                            data-tooltip={`打开主配置 ${basename(settingsFile)}`}
                            onClick={() => handleOpenFile(agent.name, "settings")}
                            disabled={busy}
                            aria-label={`打开主配置 ${basename(settingsFile)}`}
                          >
                            <FileCog size={18} strokeWidth={1.8} />
                          </button>
                        )}
                        {mcpFile && (
                          <button
                            type="button"
                            className="kb-icon-btn"
                            data-tooltip={`打开 MCP 配置 ${basename(mcpFile)}`}
                            onClick={() => handleOpenFile(agent.name, "mcp")}
                            disabled={busy}
                            aria-label={`打开 MCP 配置 ${basename(mcpFile)}`}
                          >
                            <FileJson size={18} strokeWidth={1.8} />
                          </button>
                        )}
                        {configDir && (
                          <button
                            type="button"
                            className="kb-icon-btn"
                            data-tooltip={`在访达中打开 ${configDir}`}
                            onClick={() => handleRevealDir(agent.name)}
                            disabled={busy}
                            aria-label={`在访达中打开 ${configDir}`}
                          >
                            <FolderOpen size={18} strokeWidth={1.8} />
                          </button>
                        )}
                        <span className="kb-tool-sep" aria-hidden />
                        {/* 装饰性指示：详情入口在卡片主体上，这里不重复做成按钮 */}
                        <ChevronRight size={18} strokeWidth={2} className="kb-card-chevron" />
                      </div>
                    ) : (
                      <div className="kb-missing-actions" onClick={(e) => e.stopPropagation()}>
                        <button
                          type="button"
                          className="kb-btn kb-btn-ghost"
                          onClick={() => openAddWithName(agent.display_name)}
                        >
                          <Plus size={16} strokeWidth={2} />
                          <span>手动指定路径</span>
                        </button>
                        <button type="button" className="kb-doc-link" onClick={handleOpenDoc}>
                          <span className="kb-label-sm">配置文档</span>
                          <ExternalLink size={14} strokeWidth={1.8} />
                        </button>
                      </div>
                    )}
                  </div>

                  {/* ---- 路径矩阵 ---- */}
                  {agent.found && (appPath || cliPath || configDir) && (
                    <div className="kb-paths">
                      {(appPath || cliPath) && (
                        <div className="kb-path">
                          <div className="kb-path-main">
                            <span className="kb-path-label kb-code-sm">
                              {appPath ? "App 路径" : "CLI 路径"}
                            </span>
                            <span
                              className="kb-path-value kb-code-sm"
                              title={appPath ?? cliPath ?? ""}
                            >
                              <span dir="ltr">{appPath ?? cliPath}</span>
                            </span>
                          </div>
                          <button
                            type="button"
                            className={`kb-copy ${copiedKey === `${agent.name}:bin` ? "done" : ""}`}
                            data-tooltip="复制路径"
                            onClick={() =>
                              handleCopy(`${agent.name}:bin`, appPath ?? cliPath ?? "")
                            }
                            aria-label="复制路径"
                          >
                            {copiedKey === `${agent.name}:bin` ? (
                              <Check size={14} strokeWidth={2.2} />
                            ) : (
                              <Copy size={14} strokeWidth={1.8} />
                            )}
                          </button>
                        </div>
                      )}
                      {configDir && (
                        <div className="kb-path">
                          <div className="kb-path-main">
                            <span className="kb-path-label kb-code-sm">
                              <FolderOpen size={13} strokeWidth={1.8} />
                              配置目录
                            </span>
                            <span className="kb-path-value accent kb-code-sm" title={configDir}>
                              <span dir="ltr">{configDir}</span>
                            </span>
                          </div>
                          <button
                            type="button"
                            className={`kb-copy ${copiedKey === `${agent.name}:dir` ? "done" : ""}`}
                            data-tooltip="复制路径"
                            onClick={() => handleCopy(`${agent.name}:dir`, configDir)}
                            aria-label="复制配置目录"
                          >
                            {copiedKey === `${agent.name}:dir` ? (
                              <Check size={14} strokeWidth={2.2} />
                            ) : (
                              <Copy size={14} strokeWidth={1.8} />
                            )}
                          </button>
                        </div>
                      )}
                    </div>
                  )}

                  {/* ---- 能力标签 ---- */}
                  {agent.found && (
                    <div className="kb-tags">
                      <div className="kb-tags-left">
                        {/* 统计未取到时（stat 为空）整块留空：宁可少信息，也不谎报"未挂载" */}
                        {stat &&
                          (stat.mcpCount > 0 ? (
                            <span className="kb-tag kb-tag-mcp kb-code-sm">
                              <Zap size={14} strokeWidth={1.8} />
                              <span className="kb-tag-key">{stat.mcpCount} MCP:</span>
                              <span className="kb-tag-names">{stat.mcpTitles.join(", ")}</span>
                            </span>
                          ) : (
                            <span className="kb-tag kb-tag-muted kb-code-sm">
                              <span className="kb-dot" />
                              <span>未挂载 MCP 服务</span>
                            </span>
                          ))}
                        {stat && stat.mcpCount === 0 && onNavigate && (
                          <button
                            type="button"
                            className="kb-btn-xs kb-btn-accent"
                            onClick={() => onNavigate("mcp-manage")}
                          >
                            <CloudCog size={14} strokeWidth={1.8} />
                            <span className="kb-label-sm">分配 MCP</span>
                          </button>
                        )}
                        {profile && !profile.skillsSupported ? (
                          <span className="kb-tag kb-tag-muted kb-code-sm">
                            <span className="kb-dot" />
                            <span>不支持 Skills</span>
                          </span>
                        ) : (
                          stat && (
                            <span className="kb-tag kb-tag-skill kb-code-sm">
                              <Sparkles size={14} strokeWidth={1.8} />
                              <span className="kb-tag-key">{stat.skillCount} Skill:</span>
                              <span className="kb-tag-names">
                                {stat.skillIds.length > 0 ? stat.skillIds.join(", ") : "未挂载"}
                              </span>
                            </span>
                          )
                        )}
                      </div>
                      <div className="kb-tags-right">
                        {mcpFile && (
                          <span className="kb-code-sm">
                            配置方言: <span className="kb-meta-value">{basename(mcpFile)}</span>
                          </span>
                        )}
                      </div>
                    </div>
                  )}
                </div>
              );
            })}
          </div>
        </div>
      </div>

      {/* ===== 手动关联 Agent 弹窗 ===== */}
      <div className={`kb-modal-overlay ${showAdd ? "visible" : ""}`} {...addDismiss}>
        <div className="kb-modal">
          <div className="kb-modal-head">
            <h2 className="kb-modal-title kb-headline-md">
              <Plus size={18} strokeWidth={2} />
              手动关联 Agent 路径
            </h2>
            <button
              type="button"
              className="kb-icon-btn ghost"
              onClick={() => setShowAdd(false)}
              aria-label="关闭"
            >
              <X size={18} strokeWidth={2} />
            </button>
          </div>
          <div className="kb-modal-body">
            <div className="kb-field">
              <label className="kb-field-label kb-body-md" htmlFor="agent-name">
                Agent 名称
              </label>
              <input
                ref={nameInputRef}
                type="text"
                className="kb-input"
                id="agent-name"
                placeholder="例如: MyAgent"
                value={formName}
                onChange={(e) => setFormName(e.target.value)}
              />
            </div>
            <div className="kb-field">
              <label className="kb-field-label kb-body-md" htmlFor="agent-cli-path">
                CLI / App 路径
              </label>
              <input
                type="text"
                className="kb-input mono"
                id="agent-cli-path"
                placeholder="/usr/local/bin/myagent 或 /Applications/MyAgent.app"
                value={formCliPath}
                onChange={(e) => setFormCliPath(e.target.value)}
              />
              <span className="kb-field-hint kb-code-sm">应用内已登记的 Agent 无需手动添加</span>
            </div>
            <div className="kb-field">
              <label className="kb-field-label kb-body-md" htmlFor="agent-config-dir">
                配置目录
              </label>
              <input
                type="text"
                className="kb-input mono"
                id="agent-config-dir"
                placeholder="~/.myagent"
                value={formConfigDir}
                onChange={(e) => setFormConfigDir(e.target.value)}
              />
            </div>
          </div>
          <div className="kb-modal-foot">
            <button type="button" className="kb-btn kb-btn-ghost" onClick={() => setShowAdd(false)}>
              取消
            </button>
            <button
              type="button"
              className="kb-btn kb-btn-primary"
              onClick={handleManualAdd}
              disabled={!formName.trim()}
            >
              确认保存注册
            </button>
          </div>
        </div>
      </div>
    </>
  );
}
