import { useState, useEffect, useCallback, useRef, useMemo } from "react";
import { useStatusMessage } from "@/lib/useStatusMessage";
import { Toast } from "@/components/Toast";
import { AgentBadge, getAgentIcon } from "../agent-icons";
import { AgentFilterChips } from "../agent-filter";
import { CheckGlyph, useOverlayDismiss } from "../ui";
import { Pencil, Plus, Radar, Settings, Trash2, X } from "lucide-react";
import { IconCheckSquare, IconChevron, IconSearch } from "./skills/icons";

/* ===== Types ===== */
type McpType = "stdio" | "http" | "sse";

interface KeyValue {
  id: string;
  key: string;
  value: string;
}

interface AgentResult {
  name: string;
  display_name: string;
  icon: string;
  found: boolean;
  install_paths: string[];
  config_dirs: string[];
}

interface McpServer {
  id: string;
  title: string;
  type: McpType;
  command: string;
  args: string[];
  env: Record<string, string>;
  url: string;
  headers: Record<string, string>;
  appliedAgents: string[];
  createdAt: number;
}

interface McpDraftPayload {
  title: string;
  type: McpType;
  command: string;
  args: string[];
  env: Record<string, string>;
  url: string;
  headers: Record<string, string>;
}

interface AgentMcpResult {
  agent: string;
  path: string;
  ok: boolean;
  message: string;
}

interface McpBatchResult {
  results: AgentMcpResult[];
  allOk: boolean;
}

interface McpTestResult {
  ok: boolean;
  message: string;
  detail: string;
}

function formatBatchMessage(result: McpBatchResult, action: "写入" | "删除"): string {
  const failed = result.results.filter((r) => !r.ok);
  const ok = result.results.filter((r) => r.ok);
  if (failed.length === 0) {
    return `已${action} ${ok.length} 个 Agent 配置`;
  }
  const detail = failed.map((r) => `${r.agent}: ${r.message}`).join("；");
  if (ok.length === 0) {
    return `${action}失败：${detail}`;
  }
  return `部分成功（${ok.length}/${result.results.length}）。失败：${detail}`;
}

/** 前端复现后端 dedupe_write_targets 的共享根归一。
 *  codebuddy（国际版）移除后每个物理根只剩一个可选 agent，canonical 恒等于 agent 自身；
 *  保留函数以隔离后端归一语义，未来再有共享根 agent 时在此扩展。 */
function canonicalWriteTarget(agent: string, _selected: string[]): string {
  return agent;
}

async function invokeApplyMcp(draft: McpDraftPayload, agents: string[]): Promise<McpBatchResult> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("apply_mcp_to_agents", { draft, agents }) as Promise<McpBatchResult>;
}

async function invokeRemoveMcp(title: string, agents: string[]): Promise<McpBatchResult> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("remove_mcp_from_agents", { title, agents }) as Promise<McpBatchResult>;
}

/* ===== Helpers ===== */
let idCounter = 0;
function nextId(prefix = "id"): string {
  idCounter += 1;
  return `${prefix}-${Date.now()}-${idCounter}`;
}

function emptyKv(): KeyValue {
  return { id: nextId("kv"), key: "", value: "" };
}

function kvListToRecord(list: KeyValue[]): Record<string, string> {
  const out: Record<string, string> = {};
  for (const item of list) {
    const k = item.key.trim();
    if (!k) continue;
    out[k] = item.value;
  }
  return out;
}

function recordToKvList(record: Record<string, string>): KeyValue[] {
  const entries = Object.entries(record);
  if (entries.length === 0) return [emptyKv()];
  return entries.map(([key, value]) => ({ id: nextId("kv"), key, value }));
}

function formatArgs(args: string[]): string {
  if (args.length === 0) return "";
  return args.join("\n");
}

function parseArgs(raw: string): string[] {
  const trimmed = raw.trim();
  if (!trimmed) return [];
  // Prefer JSON array
  if (trimmed.startsWith("[")) {
    try {
      const parsed = JSON.parse(trimmed);
      if (Array.isArray(parsed)) {
        return parsed.map((v) => String(v)).filter((v) => v.length > 0);
      }
    } catch {
      // ignore
    }
  }
  // One argument per line when multi-line; otherwise whitespace split
  if (raw.includes("\n")) {
    return raw
      .split("\n")
      .map((line) => line.trim())
      .filter(Boolean);
  }
  return trimmed.split(/\s+/).filter(Boolean);
}

function buildMcpJson(input: {
  title: string;
  type: McpType;
  command: string;
  argsRaw: string;
  env: KeyValue[];
  url: string;
  headers: KeyValue[];
}): Record<string, unknown> {
  const name = input.title.trim() || "mcp-server";
  const entry: Record<string, unknown> = {
    type: input.type,
  };

  if (input.type === "stdio") {
    entry.command = input.command.trim() || "";
    const args = parseArgs(input.argsRaw);
    if (args.length > 0) entry.args = args;
    const env = kvListToRecord(input.env);
    if (Object.keys(env).length > 0) entry.env = env;
  } else {
    entry.url = input.url.trim() || "";
    const headers = kvListToRecord(input.headers);
    if (Object.keys(headers).length > 0) entry.headers = headers;
  }

  return { [name]: entry };
}

function formatJson(value: unknown): string {
  try {
    return JSON.stringify(value, null, 2);
  } catch {
    return "{}";
  }
}

/** Map backend McpServerRecord (camelCase) to frontend McpServer */
function fromRecord(r: {
  id: string;
  title: string;
  type: McpType;
  command: string;
  args: string[];
  env: Record<string, string>;
  url: string;
  headers: Record<string, string>;
  appliedAgents: string[];
  createdAt: number;
}): McpServer {
  return {
    id: r.id,
    title: r.title,
    type: (r.type as McpType) || "stdio",
    command: r.command || "",
    args: r.args || [],
    env: r.env || {},
    url: r.url || "",
    headers: r.headers || {},
    appliedAgents: r.appliedAgents || [],
    createdAt: r.createdAt || Date.now(),
  };
}

function toRecord(s: McpServer) {
  return {
    id: s.id,
    title: s.title,
    type: s.type,
    command: s.command,
    args: s.args,
    env: s.env,
    url: s.url,
    headers: s.headers,
    appliedAgents: s.appliedAgents,
    createdAt: s.createdAt,
  };
}

async function persistServers(servers: McpServer[]) {
  // DB is the single source of truth; no localStorage mirror.
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    await invoke("save_mcp_servers", { servers: servers.map(toRecord) });
  } catch {
    // desktop-only; ignore in browser preview
  }
}

async function loadServersFromDb(): Promise<McpServer[] | null> {
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    const rows = await (invoke("get_mcp_servers") as Promise<
      Array<{
        id: string;
        title: string;
        type: McpType;
        command: string;
        args: string[];
        env: Record<string, string>;
        url: string;
        headers: Record<string, string>;
        appliedAgents: string[];
        createdAt: number;
      }>
    >);
    return rows.map(fromRecord);
  } catch {
    return null;
  }
}

async function invokeSniffMcp(): Promise<{
  servers: McpServer[];
  scannedAgents: number;
  foundEntries: number;
  message: string;
}> {
  const { invoke } = await import("@tauri-apps/api/core");
  const res = await (invoke("sniff_mcp_servers") as Promise<{
    servers: Array<{
      id: string;
      title: string;
      type: McpType;
      command: string;
      args: string[];
      env: Record<string, string>;
      url: string;
      headers: Record<string, string>;
      appliedAgents: string[];
      createdAt: number;
    }>;
    scannedAgents: number;
    foundEntries: number;
    message: string;
  }>);
  return {
    servers: res.servers.map(fromRecord),
    scannedAgents: res.scannedAgents,
    foundEntries: res.foundEntries,
    message: res.message,
  };
}

/* ===== Icons ===== */
const IconPlus = () => (
  <Plus strokeWidth={2} />
);

const IconScan = () => (
  <Radar strokeWidth={1.8} />
);

const IconClose = () => (
  <X size={16} strokeWidth={2} />
);

const IconTrash = () => (
  <Trash2 size={16} strokeWidth={1.8} />
);

const IconEdit = () => (
  <Pencil size={16} strokeWidth={1.8} />
);

const IconMcp = () => (
  <Settings strokeWidth={1.8} />
);

const TYPE_OPTIONS: { value: McpType; label: string; desc: string }[] = [
  { value: "stdio", label: "stdio", desc: "本地进程，通过标准输入输出通信" },
  { value: "http", label: "http", desc: "HTTP 远程 MCP 服务" },
  { value: "sse", label: "sse", desc: "Server-Sent Events 远程 MCP 服务" },
];

/* ===== Component ===== */
export default function McpManage() {
  const [servers, setServers] = useState<McpServer[]>([]);
  const [agents, setAgents] = useState<AgentResult[]>([]);
  const [showEditor, setShowEditor] = useState(false);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<string | null>(null);
  const [removeFromAgentConfigs, setRemoveFromAgentConfigs] = useState(false);
  const [formError, setFormError] = useState("");
  const [statusMsg, setStatusMsg] = useStatusMessage();
  const [isSaving, setIsSaving] = useState(false);
  const [isDeleting, setIsDeleting] = useState(false);
  const [isSniffing, setIsSniffing] = useState(false);
  const [isTesting, setIsTesting] = useState(false);
  const [testResult, setTestResult] = useState<McpTestResult | null>(null);
  const hasReconciled = useRef(false);

  // ===== 搜索与筛选 =====
  // 搜索输入即时响应；查询值延迟更新，避免每次按键都重新筛选整表
  const [searchInput, setSearchInput] = useState("");
  const [searchQuery, setSearchQuery] = useState("");
  // 类型筛选：单选，"all" 表示全部（与搜索为「与」关系）
  const [activeType, setActiveType] = useState<"all" | McpType>("all");
  // Agent 筛选：单选，"all" 表示全部（与类型/搜索为「与」关系）
  const [activeAgent, setActiveAgent] = useState<string>("all");
  // 筛选区折叠态：默认展开；收起仅切换显隐，保留当前选中项并在标题处提示
  const [typeExpanded, setTypeExpanded] = useState(true);
  const [agentExpanded, setAgentExpanded] = useState(true);

  // ===== 批量管理 =====
  // 显式选择模式：进入后卡片改为可勾选，隐藏单卡操作，底部浮出操作条
  const [batchMode, setBatchMode] = useState(false);
  const [selectedIds, setSelectedIds] = useState<Set<string>>(new Set());
  const [isBatchRunning, setIsBatchRunning] = useState(false);
  // 批量删除确认
  const [batchDeleteOpen, setBatchDeleteOpen] = useState(false);
  const [batchDeleteAgentConfigs, setBatchDeleteAgentConfigs] = useState(false);
  // 批量应用 / 移除 Agent
  const [batchApplyOpen, setBatchApplyOpen] = useState(false);
  const [batchApplyAgents, setBatchApplyAgents] = useState<Set<string>>(new Set());
  const [batchApplyMode, setBatchApplyMode] = useState<"add" | "remove">("add");

  const editorDismiss = useOverlayDismiss(() => closeEditor());
  const deleteDismiss = useOverlayDismiss(() => closeDelete());
  const batchDeleteDismiss = useOverlayDismiss(
    () => setBatchDeleteOpen(false),
    !isBatchRunning
  );
  const batchApplyDismiss = useOverlayDismiss(
    () => setBatchApplyOpen(false),
    !isBatchRunning
  );

  // Form state
  const [title, setTitle] = useState("");
  const [mcpType, setMcpType] = useState<McpType>("stdio");
  const [command, setCommand] = useState("");
  const [argsRaw, setArgsRaw] = useState("");
  const [envList, setEnvList] = useState<KeyValue[]>([emptyKv()]);
  const [url, setUrl] = useState("");
  const [headerList, setHeaderList] = useState<KeyValue[]>([emptyKv()]);
  const [selectedAgents, setSelectedAgents] = useState<Set<string>>(new Set());

  const titleInputRef = useRef<HTMLInputElement>(null);

  // Reconcile persisted MCP definitions with Agent config files on every mount.
  // If scanning is unavailable, retain the persisted list rather than showing an empty page.
  useEffect(() => {
    if (hasReconciled.current) return;
    hasReconciled.current = true;
    (async () => {
      try {
        const res = await invokeSniffMcp();
        setServers(res.servers);
      } catch {
        const fromDb = await loadServersFromDb();
        if (fromDb) setServers(fromDb);
      }
    })();
  }, []);

  const loadAgents = useCallback(async () => {
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      const cached = await (invoke("get_cached_agents") as Promise<AgentResult[]>);
      setAgents(cached.filter((a) => a.found));
    } catch {
      setAgents([]);
    }
  }, []);

  // Load sniffed agents on mount
  useEffect(() => {
    void loadAgents();
  }, [loadAgents]);

  // Refresh agents + focus title when modal opens
  useEffect(() => {
    if (showEditor) {
      void loadAgents();
      setTimeout(() => titleInputRef.current?.focus(), 100);
    }
  }, [showEditor, loadAgents]);

  // Escape closes modals
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        setShowEditor(false);
        setEditingId(null);
        setDeleteTarget(null);
        if (!isBatchRunning) {
          setBatchDeleteOpen(false);
          setBatchApplyOpen(false);
        }
      }
    };
    document.addEventListener("keydown", handler);
    return () => document.removeEventListener("keydown", handler);
  }, [isBatchRunning]);

  // 搜索防抖：输入停顿 250ms 后再筛选；清空时立即恢复全表
  useEffect(() => {
    const query = searchInput.trim().toLocaleLowerCase();
    if (!query) {
      setSearchQuery("");
      return;
    }
    const timer = window.setTimeout(() => setSearchQuery(query), 250);
    return () => window.clearTimeout(timer);
  }, [searchInput]);

  const doSniffMcp = useCallback(async () => {
    if (isSniffing) return;
    setIsSniffing(true);
    setStatusMsg("正在从 Agent 配置中扫描 MCP…");
    try {
      const res = await invokeSniffMcp();
      setServers(res.servers);
      setStatusMsg(res.message);
    } catch (e) {
      setStatusMsg(`MCP 扫描失败: ${e}`);
    } finally {
      setIsSniffing(false);
    }
  }, [isSniffing]);

  const handleTest = useCallback(async () => {
    if (isTesting) return;
    const draft: McpDraftPayload = {
      title: title.trim() || "test",
      type: mcpType,
      command: mcpType === "stdio" ? command.trim() : "",
      args: mcpType === "stdio" ? parseArgs(argsRaw) : [],
      env: mcpType === "stdio" ? kvListToRecord(envList) : {},
      url: mcpType !== "stdio" ? url.trim() : "",
      headers: mcpType !== "stdio" ? kvListToRecord(headerList) : {},
    };
    if (mcpType === "stdio" && !draft.command) {
      setTestResult({ ok: false, message: "请先填写命令", detail: "" });
      return;
    }
    if (mcpType !== "stdio" && !draft.url) {
      setTestResult({ ok: false, message: "请先填写 URL", detail: "" });
      return;
    }
    setIsTesting(true);
    setTestResult(null);
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      const res = await (invoke("test_mcp_connection", { draft }) as Promise<McpTestResult>);
      setTestResult(res);
    } catch (e) {
      setTestResult({ ok: false, message: `测试失败: ${e}`, detail: "" });
    } finally {
      setIsTesting(false);
    }
  }, [isTesting, title, mcpType, command, argsRaw, envList, url, headerList]);

  const resetForm = useCallback(() => {
    setTitle("");
    setMcpType("stdio");
    setCommand("");
    setArgsRaw("");
    setEnvList([emptyKv()]);
    setUrl("");
    setHeaderList([emptyKv()]);
    setSelectedAgents(new Set());
    setFormError("");
    setEditingId(null);
    setTestResult(null);
  }, []);

  const openAdd = useCallback(() => {
    resetForm();
    setShowEditor(true);
  }, [resetForm]);

  const openEdit = useCallback((server: McpServer) => {
    setEditingId(server.id);
    setTitle(server.title);
    setMcpType(server.type);
    setCommand(server.command);
    setArgsRaw(formatArgs(server.args));
    setEnvList(recordToKvList(server.env));
    setUrl(server.url);
    setHeaderList(recordToKvList(server.headers));
    setSelectedAgents(new Set(server.appliedAgents));
    setFormError("");
    setTestResult(null);
    setShowEditor(true);
  }, []);

  const closeEditor = useCallback(() => {
    setShowEditor(false);
    setFormError("");
    setEditingId(null);
  }, []);

  const jsonPreview = useMemo(
    () =>
      buildMcpJson({
        title,
        type: mcpType,
        command,
        argsRaw,
        env: envList,
        url,
        headers: headerList,
      }),
    [title, mcpType, command, argsRaw, envList, url, headerList]
  );

  const updateEnvKv = useCallback((id: string, field: "key" | "value", value: string) => {
    setEnvList((prev) =>
      prev.map((item) => (item.id === id ? { ...item, [field]: value } : item))
    );
  }, []);

  const updateHeaderKv = useCallback((id: string, field: "key" | "value", value: string) => {
    setHeaderList((prev) =>
      prev.map((item) => (item.id === id ? { ...item, [field]: value } : item))
    );
  }, []);

  const addEnvRow = useCallback(() => {
    setEnvList((prev) => [...prev, emptyKv()]);
  }, []);

  const addHeaderRow = useCallback(() => {
    setHeaderList((prev) => [...prev, emptyKv()]);
  }, []);

  const removeEnvRow = useCallback((id: string) => {
    setEnvList((prev) => {
      const next = prev.filter((item) => item.id !== id);
      return next.length > 0 ? next : [emptyKv()];
    });
  }, []);

  const removeHeaderRow = useCallback((id: string) => {
    setHeaderList((prev) => {
      const next = prev.filter((item) => item.id !== id);
      return next.length > 0 ? next : [emptyKv()];
    });
  }, []);

  const toggleAgent = useCallback((name: string) => {
    setSelectedAgents((prev) => {
      const next = new Set(prev);
      if (next.has(name)) next.delete(name);
      else next.add(name);
      return next;
    });
  }, []);

  const selectAllAgents = useCallback(() => {
    setSelectedAgents(new Set(agents.map((a) => a.name)));
  }, [agents]);

  const clearSelectedAgents = useCallback(() => {
    setSelectedAgents(new Set());
  }, []);

  const handleSave = useCallback(async () => {
    const name = title.trim();
    if (!name) {
      setFormError("请填写标题");
      return;
    }

    if (mcpType === "stdio") {
      if (!command.trim()) {
        setFormError("stdio 类型需要填写命令");
        return;
      }
    } else if (!url.trim()) {
      setFormError(`${mcpType.toUpperCase()} 类型需要填写 URL`);
      return;
    }

    // Duplicate title check (allow keeping own title while editing)
    const titleTaken = servers.some(
      (s) => s.title.toLowerCase() === name.toLowerCase() && s.id !== editingId
    );
    if (titleTaken) {
      setFormError("已存在同名 MCP，请换一个标题");
      return;
    }

    const payload: Omit<McpServer, "id" | "createdAt"> = {
      title: name,
      type: mcpType,
      command: mcpType === "stdio" ? command.trim() : "",
      args: mcpType === "stdio" ? parseArgs(argsRaw) : [],
      env: mcpType === "stdio" ? kvListToRecord(envList) : {},
      url: mcpType !== "stdio" ? url.trim() : "",
      headers: mcpType !== "stdio" ? kvListToRecord(headerList) : {},
      appliedAgents: Array.from(selectedAgents),
    };

    const previous = editingId ? servers.find((s) => s.id === editingId) : null;
    const prevAgents = previous?.appliedAgents ?? [];
    const nextAgents = payload.appliedAgents;
    const removedAgents = prevAgents.filter((a) => !nextAgents.includes(a));

    setIsSaving(true);
    setFormError("");
    setStatusMsg("");

    try {
      // If title renamed while editing, remove old key from previous agents first
      if (previous && previous.title !== name && prevAgents.length > 0) {
        try {
          await invokeRemoveMcp(previous.title, prevAgents);
        } catch {
          // best-effort
        }
      }

      // Remove from agents that were unchecked
      if (removedAgents.length > 0) {
        try {
          await invokeRemoveMcp(previous?.title ?? name, removedAgents);
        } catch {
          // best-effort
        }
      }

      // Apply to selected agents (may be empty — list-only save)
      let effectiveApplied = payload.appliedAgents;
      if (nextAgents.length > 0) {
        const draft: McpDraftPayload = {
          title: name,
          type: payload.type,
          command: payload.command,
          args: payload.args,
          env: payload.env,
          url: payload.url,
          headers: payload.headers,
        };
        const batch = await invokeApplyMcp(draft, nextAgents);
        setStatusMsg(formatBatchMessage(batch, "写入"));
        if (!batch.allOk && batch.results.every((r) => !r.ok)) {
          setFormError(formatBatchMessage(batch, "写入"));
          return;
        }
        // 只把「实际写入成功」的 Agent 记为已应用，避免部分失败时 UI 与磁盘漂移。
        const failedTargets = new Set(
          batch.results.filter((r) => !r.ok).map((r) => r.agent)
        );
        effectiveApplied = nextAgents.filter(
          (a) => !failedTargets.has(canonicalWriteTarget(a, nextAgents))
        );
      } else {
        setStatusMsg("已保存到本应用列表（未选择 Agent，未写配置文件）");
      }

      const finalPayload = { ...payload, appliedAgents: effectiveApplied };
      if (editingId) {
        const nextList = servers.map((s) =>
          s.id === editingId ? { ...s, ...finalPayload } : s
        );
        setServers(nextList);
        await persistServers(nextList);
      } else {
        const server: McpServer = {
          id: nextId("mcp"),
          createdAt: Date.now(),
          ...finalPayload,
        };
        const nextList = [server, ...servers];
        setServers(nextList);
        await persistServers(nextList);
      }

      setShowEditor(false);
      resetForm();
    } catch (e) {
      setFormError(`写入配置失败: ${e}`);
    } finally {
      setIsSaving(false);
    }
  }, [
    title,
    mcpType,
    command,
    argsRaw,
    envList,
    url,
    headerList,
    selectedAgents,
    servers,
    editingId,
    resetForm,
  ]);

  const removeMcpFromAgentConfigs = useCallback(async (server: McpServer) => {
    if (server.appliedAgents.length === 0) return;
    const batch = await invokeRemoveMcp(server.title, server.appliedAgents);
    setStatusMsg(formatBatchMessage(batch, "删除"));
  }, []);

  const openDelete = useCallback((id: string) => {
    setRemoveFromAgentConfigs(false);
    setDeleteTarget(id);
  }, []);

  const closeDelete = useCallback(() => {
    setDeleteTarget(null);
    setRemoveFromAgentConfigs(false);
  }, []);

  const handleDelete = useCallback(async () => {
    if (!deleteTarget) return;
    const target = servers.find((s) => s.id === deleteTarget);
    setIsDeleting(true);
    try {
      if (target && removeFromAgentConfigs) {
        await removeMcpFromAgentConfigs(target);
      }
      const nextList = servers.filter((s) => s.id !== deleteTarget);
      setServers(nextList);
      await persistServers(nextList);
      setDeleteTarget(null);
      setRemoveFromAgentConfigs(false);
      if (target && !removeFromAgentConfigs) {
        setStatusMsg("已从本应用列表删除（未改写 Agent 配置文件）");
      }
    } catch (e) {
      setStatusMsg(`删除失败: ${e}`);
    } finally {
      setIsDeleting(false);
    }
  }, [deleteTarget, servers, removeFromAgentConfigs, removeMcpFromAgentConfigs]);

  // 折叠/展开筛选区：仅切换显隐，保留当前选中项
  const toggleTypeExpanded = useCallback(() => {
    setTypeExpanded((prev) => !prev);
  }, []);
  const toggleAgentExpanded = useCallback(() => {
    setAgentExpanded((prev) => !prev);
  }, []);

  // ===== 搜索与筛选派生值 =====
  // 类型筛选项：全部 + 出现过的类型（带数量），只展示有数据的类型
  const typeOptions = useMemo(() => {
    const counts = new Map<McpType, number>();
    for (const s of servers) counts.set(s.type, (counts.get(s.type) ?? 0) + 1);
    const opts: { key: "all" | McpType; label: string; count: number }[] = [
      { key: "all", label: "全部", count: servers.length },
    ];
    for (const t of ["stdio", "http", "sse"] as McpType[]) {
      const count = counts.get(t) ?? 0;
      if (count > 0) opts.push({ key: t, label: t, count });
    }
    return opts;
  }, [servers]);

  // 若当前选中的类型随数据变动而消失，回退到「全部」
  useEffect(() => {
    if (activeType === "all") return;
    if (!typeOptions.some((o) => o.key === activeType)) {
      setActiveType("all");
    }
  }, [activeType, typeOptions]);

  // Agent 筛选项：仅本机已存在（扫描发现 + 手动添加，agents 已按 found 过滤）的 Agent，
  // 计数为该 Agent 已应用的 MCP 数（可为 0，便于发现尚未应用任何 MCP 的 Agent）
  const agentFilterOptions = useMemo(() => {
    const counts = new Map<string, number>();
    for (const s of servers) {
      for (const name of s.appliedAgents) {
        counts.set(name, (counts.get(name) ?? 0) + 1);
      }
    }
    return agents.map((a) => ({
      name: a.name,
      display_name: a.display_name,
      icon: a.icon,
      count: counts.get(a.name) ?? 0,
    }));
  }, [agents, servers]);

  // 若当前选中的 Agent 被移除（卸载/删除），回退到「全部」
  useEffect(() => {
    if (activeAgent === "all") return;
    if (!agents.some((a) => a.name === activeAgent)) {
      setActiveAgent("all");
    }
  }, [activeAgent, agents]);

  // 类型、Agent 与关键词为「与」筛选；关键词覆盖卡片当前展示的主要信息（含 Agent 显示名）
  const filteredServers = useMemo(
    () =>
      servers.filter((s) => {
        if (activeType !== "all" && s.type !== activeType) return false;
        if (activeAgent !== "all" && !s.appliedAgents.includes(activeAgent)) return false;
        if (!searchQuery) return true;
        return [
          s.title,
          s.type,
          s.command,
          s.args.join(" "),
          s.url,
          ...Object.keys(s.env),
          ...Object.keys(s.headers),
          ...s.appliedAgents.map(
            (name) => agents.find((a) => a.name === name)?.display_name ?? name
          ),
        ].some((value) => value.toLocaleLowerCase().includes(searchQuery));
      }),
    [servers, agents, activeType, activeAgent, searchQuery]
  );

  // ===== 批量选择派生值与操作 =====
  // 当前筛选结果的 id 集，供「全选可见项」与选择态统计使用
  const filteredIdSet = useMemo(
    () => new Set(filteredServers.map((s) => s.id)),
    [filteredServers]
  );
  // 已选中且仍存在于列表的 id（条目被删除后自动收敛）
  const validSelectedIds = useMemo(
    () => new Set(servers.filter((s) => selectedIds.has(s.id)).map((s) => s.id)),
    [servers, selectedIds]
  );
  // 已选中但不在当前筛选内的数量（提示用户选择未丢失，只是被筛选隐藏）
  const hiddenSelectedCount = useMemo(() => {
    let n = 0;
    for (const id of validSelectedIds) if (!filteredIdSet.has(id)) n += 1;
    return n;
  }, [validSelectedIds, filteredIdSet]);
  // 当前筛选内是否已全部选中（用于全选/取消全选切换）
  const allFilteredSelected =
    filteredServers.length > 0 && filteredServers.every((s) => selectedIds.has(s.id));

  const exitBatchMode = useCallback(() => {
    setBatchMode(false);
    setSelectedIds(new Set());
    setBatchDeleteOpen(false);
    setBatchApplyOpen(false);
  }, []);

  const toggleSelect = useCallback((id: string) => {
    setSelectedIds((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }, []);

  // 全选/取消全选：以当前筛选结果为准，不影响筛选外的已选项
  const toggleSelectAllFiltered = useCallback(() => {
    setSelectedIds((prev) => {
      const next = new Set(prev);
      const everySelected =
        filteredServers.length > 0 && filteredServers.every((s) => next.has(s.id));
      if (everySelected) {
        for (const s of filteredServers) next.delete(s.id);
      } else {
        for (const s of filteredServers) next.add(s.id);
      }
      return next;
    });
  }, [filteredServers]);

  const clearSelection = useCallback(() => setSelectedIds(new Set()), []);

  // 批量删除：确认后可选同步清理各 Agent 配置文件，再更新列表并退出批量模式
  const confirmBatchDelete = useCallback(async () => {
    if (isBatchRunning) return;
    const targets = servers.filter((s) => validSelectedIds.has(s.id));
    if (targets.length === 0) {
      setStatusMsg("请先选择要删除的 MCP");
      return;
    }
    setIsBatchRunning(true);
    setStatusMsg(`正在删除 ${targets.length} 个 MCP 配置…`);
    try {
      if (batchDeleteAgentConfigs) {
        for (const server of targets) {
          if (server.appliedAgents.length === 0) continue;
          try {
            await invokeRemoveMcp(server.title, server.appliedAgents);
          } catch {
            // best-effort：单个 Agent 清理失败不阻断列表删除
          }
        }
      }
      const nextList = servers.filter((s) => !validSelectedIds.has(s.id));
      setServers(nextList);
      await persistServers(nextList);
      setStatusMsg(
        batchDeleteAgentConfigs
          ? `已删除 ${targets.length} 个 MCP 配置，并同步清理 Agent 配置文件`
          : `已从本应用列表删除 ${targets.length} 个 MCP 配置（未改写 Agent 配置文件）`
      );
      setBatchDeleteOpen(false);
      exitBatchMode();
    } catch (e) {
      setStatusMsg(`批量删除失败: ${e}`);
    } finally {
      setIsBatchRunning(false);
    }
  }, [isBatchRunning, servers, validSelectedIds, batchDeleteAgentConfigs, exitBatchMode]);

  // 打开批量应用弹窗：默认追加模式，Agent 选择初始为空
  const openBatchApply = useCallback(() => {
    if (validSelectedIds.size === 0) {
      setStatusMsg("请先选择要应用的 MCP");
      return;
    }
    setBatchApplyAgents(new Set());
    setBatchApplyMode("add");
    setBatchApplyOpen(true);
  }, [validSelectedIds]);

  const toggleBatchApplyAgent = useCallback((name: string) => {
    setBatchApplyAgents((prev) => {
      const next = new Set(prev);
      if (next.has(name)) next.delete(name);
      else next.add(name);
      return next;
    });
  }, []);

  const selectAllBatchApplyAgents = useCallback(() => {
    setBatchApplyAgents(new Set(agents.map((a) => a.name)));
  }, [agents]);

  const clearBatchApplyAgents = useCallback(() => {
    setBatchApplyAgents(new Set());
  }, []);

  // 批量应用 / 移除：逐条调用后端写/删接口，聚合结果并按实际成功项回写 appliedAgents
  const confirmBatchApply = useCallback(async () => {
    if (isBatchRunning) return;
    const targets = servers.filter((s) => validSelectedIds.has(s.id));
    if (targets.length === 0) {
      setStatusMsg("请先选择要应用的 MCP");
      return;
    }
    const targetAgents = Array.from(batchApplyAgents);
    if (targetAgents.length === 0) {
      setStatusMsg(
        batchApplyMode === "add" ? "请至少选择一个 Agent" : "请至少选择一个要移除的 Agent"
      );
      return;
    }
    setIsBatchRunning(true);
    setStatusMsg(
      batchApplyMode === "add"
        ? `正在将 ${targets.length} 个 MCP 写入 ${targetAgents.length} 个 Agent 配置…`
        : `正在从 ${targetAgents.length} 个 Agent 配置移除 ${targets.length} 个 MCP…`
    );
    // 每个 server 实际成功的 Agent 集，用于回写 appliedAgents
    const appliedDelta = new Map<string, Set<string>>();
    let okCount = 0;
    let failCount = 0;
    const errors: string[] = [];
    try {
      for (const server of targets) {
        try {
          const batch =
            batchApplyMode === "add"
              ? await invokeApplyMcp(
                  {
                    title: server.title,
                    type: server.type,
                    command: server.command,
                    args: server.args,
                    env: server.env,
                    url: server.url,
                    headers: server.headers,
                  },
                  targetAgents
                )
              : await invokeRemoveMcp(server.title, targetAgents);
          const okAgents = new Set(
            batch.results.filter((r) => r.ok).map((r) => r.agent)
          );
          appliedDelta.set(server.id, okAgents);
          okCount += okAgents.size;
          const failed = batch.results.filter((r) => !r.ok);
          failCount += failed.length;
          for (const f of failed) errors.push(`${server.title} → ${f.agent}: ${f.message}`);
        } catch (e) {
          appliedDelta.set(server.id, new Set());
          failCount += targetAgents.length;
          errors.push(`${server.title}: ${String(e)}`);
        }
      }
      // 回写 appliedAgents：add 并入成功项，remove 移除成功项
      const nextList = servers.map((s) => {
        const delta = appliedDelta.get(s.id);
        if (!delta) return s;
        const nextAgents =
          batchApplyMode === "add"
            ? Array.from(new Set([...s.appliedAgents, ...delta]))
            : s.appliedAgents.filter((a) => !delta.has(a));
        return { ...s, appliedAgents: nextAgents };
      });
      setServers(nextList);
      await persistServers(nextList);
      if (failCount === 0) {
        setStatusMsg(
          batchApplyMode === "add"
            ? `已将 ${targets.length} 个 MCP 写入 ${okCount} 个 Agent 配置`
            : `已从 Agent 配置移除 ${okCount} 个 MCP 条目`
        );
        setBatchApplyOpen(false);
        exitBatchMode();
      } else {
        // 部分失败时保留弹窗与批量模式，便于修正后重试
        setStatusMsg(
          `部分成功（成功 ${okCount}，失败 ${failCount}）。${errors.slice(0, 3).join("；")}`
        );
      }
    } finally {
      setIsBatchRunning(false);
    }
  }, [
    isBatchRunning,
    servers,
    validSelectedIds,
    batchApplyAgents,
    batchApplyMode,
    exitBatchMode,
  ]);

  const typeLabel = (t: McpType) => t.toUpperCase();

  return (
    <>
      <div className="content-header">
        <div className="content-header-bar">
          <h1 className="content-title">MCP 管理</h1>
          <div className="header-actions">
            <button
              className={`action-btn ${batchMode ? "active" : ""}`}
              data-tooltip={batchMode ? "退出批量管理" : "批量管理"}
              onClick={() => (batchMode ? exitBatchMode() : setBatchMode(true))}
              disabled={servers.length === 0 || isSniffing}
              aria-pressed={batchMode}
            >
              <IconCheckSquare />
            </button>
            <button
              className={`action-btn ${isSniffing ? "sniffing" : ""}`}
              data-tooltip={isSniffing ? "扫描中..." : "扫描 MCP"}
              onClick={() => void doSniffMcp()}
              disabled={isSniffing}
            >
              <IconScan />
            </button>
            <button className="action-btn" data-tooltip="添加 MCP" onClick={openAdd}>
              <IconPlus />
            </button>
          </div>
        </div>
      </div>

      <div className="content-body">
        <Toast message={statusMsg} />
        {servers.length === 0 ? (
          <div className="empty-state">
            <IconMcp />
            <div className="empty-state-text">还没有 MCP 配置，可扫描已有配置或点击右上角添加</div>
          </div>
        ) : (
          <>
            <div className="mcp-summary">
              共 <strong>{servers.length}</strong> 个 MCP 配置
              {filteredServers.length !== servers.length && (
                <>
                  ，筛选出 <strong>{filteredServers.length}</strong> 个
                </>
              )}
            </div>
            <div className="skill-search">
              <IconSearch />
              <input
                type="search"
                value={searchInput}
                onChange={(event) => {
                  const value = event.target.value;
                  setSearchInput(value);
                  if (!value.trim()) setSearchQuery("");
                }}
                placeholder="搜索 MCP 名称、命令、URL 或 Agent"
                aria-label="搜索 MCP"
              />
              {searchInput && (
                <button
                  type="button"
                  className="skill-search-clear"
                  data-tooltip="清空搜索"
                  aria-label="清空搜索"
                  onClick={() => {
                    setSearchInput("");
                    setSearchQuery("");
                  }}
                >
                  <IconClose />
                </button>
              )}
            </div>
            {typeOptions.length > 2 && (
              <div className="skill-filter-section">
                <button
                  type="button"
                  className="skill-filter-heading"
                  aria-expanded={typeExpanded}
                  onClick={toggleTypeExpanded}
                >
                  <span className="skill-filter-heading-chevron" aria-hidden>
                    <IconChevron open={typeExpanded} />
                  </span>
                  <span className="skill-filter-heading-title">类型</span>
                  {!typeExpanded && activeType !== "all" && (
                    <span className="skill-filter-heading-active">
                      {typeOptions.find((o) => o.key === activeType)?.label ?? ""}
                    </span>
                  )}
                </button>
                {typeExpanded && (
                  <div className="skill-tag-filter" role="group" aria-label="按类型筛选">
                    {typeOptions.map((opt) => {
                      const active = opt.key === activeType;
                      return (
                        <button
                          key={opt.key}
                          type="button"
                          className={`skill-source-chip skill-source-chip-tag ${active ? "active" : ""}`}
                          aria-pressed={active}
                          onClick={() => setActiveType(opt.key)}
                        >
                          <span className="skill-source-chip-label">{opt.label}</span>
                          <span className="skill-source-chip-count">{opt.count}</span>
                        </button>
                      );
                    })}
                  </div>
                )}
              </div>
            )}
            {agents.length > 0 && (
              <div className="skill-filter-section">
                <button
                  type="button"
                  className="skill-filter-heading"
                  aria-expanded={agentExpanded}
                  onClick={toggleAgentExpanded}
                >
                  <span className="skill-filter-heading-chevron" aria-hidden>
                    <IconChevron open={agentExpanded} />
                  </span>
                  <span className="skill-filter-heading-title">Agents</span>
                  {!agentExpanded && activeAgent !== "all" && (
                    <span className="skill-filter-heading-active">
                      {agentFilterOptions.find((o) => o.name === activeAgent)?.display_name ?? ""}
                    </span>
                  )}
                </button>
                {agentExpanded && (
                  <AgentFilterChips
                    items={agentFilterOptions}
                    total={servers.length}
                    active={activeAgent}
                    onSelect={setActiveAgent}
                  />
                )}
              </div>
            )}
            <div className="mcp-list">
              {filteredServers.length === 0 ? (
                <div className="empty-state skill-filter-empty">
                  <IconSearch />
                  <div className="empty-state-text">没有匹配的 MCP 配置</div>
                </div>
              ) : (
                filteredServers.map((server) => {
                  const checked = selectedIds.has(server.id);
                  return (
                    <div
                      key={server.id}
                      className={`mcp-card ${batchMode ? "selectable" : ""} ${
                        batchMode && checked ? "selected" : ""
                      }`}
                      onClick={batchMode ? () => toggleSelect(server.id) : undefined}
                      role={batchMode ? "checkbox" : undefined}
                      aria-checked={batchMode ? checked : undefined}
                    >
                      <div className="mcp-card-header">
                        {batchMode && (
                          <label
                            className="ui-check mcp-card-check"
                            onClick={(e) => e.stopPropagation()}
                          >
                            <input
                              type="checkbox"
                              className="ui-check-input"
                              checked={checked}
                              onChange={() => toggleSelect(server.id)}
                            />
                            <CheckGlyph />
                          </label>
                        )}
                        <div className="mcp-card-icon">{server.title.slice(0, 2).toUpperCase()}</div>
                        <div className="mcp-card-main">
                          <div className="mcp-card-title-row">
                            <span className="mcp-card-title">{server.title}</span>
                            <span className={`mcp-type-badge mcp-type-${server.type}`}>
                              {typeLabel(server.type)}
                            </span>
                          </div>
                        </div>
                        {!batchMode && (
                          <div className="mcp-card-actions">
                            <button
                              className="btn-icon-action mcp-card-action"
                              onClick={() => openEdit(server)}
                              data-tooltip="编辑"
                            >
                              <IconEdit />
                            </button>
                            <button
                              className="btn-delete mcp-card-action"
                              onClick={() => openDelete(server.id)}
                              data-tooltip="删除"
                            >
                              <IconTrash />
                            </button>
                          </div>
                        )}
                      </div>
                      {server.appliedAgents.length > 0 ? (
                        <div className="mcp-card-agents">
                          <span className="mcp-card-agents-label">已应用</span>
                          <div className="agent-badge-list">
                            {server.appliedAgents.map((name) => {
                              const agent = agents.find((a) => a.name === name);
                              return (
                                <AgentBadge
                                  key={name}
                                  name={name}
                                  label={agent?.display_name ?? name}
                                />
                              );
                            })}
                          </div>
                        </div>
                      ) : (
                        <div className="mcp-card-agents">
                          <span className="mcp-card-agents-empty">未应用到任何 Agent</span>
                        </div>
                      )}
                    </div>
                  );
                })
              )}
            </div>
          </>
        )}
      </div>

      {/* ===== 批量操作条（批量模式下浮出） ===== */}
      {batchMode && servers.length > 0 && (
        <div className="skill-batch-bar">
          <div className="skill-batch-bar-left">
            <button
              type="button"
              className="mcp-agent-action"
              onClick={toggleSelectAllFiltered}
              disabled={isBatchRunning || filteredServers.length === 0}
            >
              {allFilteredSelected ? "取消全选" : "全选"}
            </button>
            <button
              type="button"
              className="mcp-agent-action"
              onClick={clearSelection}
              disabled={isBatchRunning || validSelectedIds.size === 0}
            >
              清除
            </button>
            <span className="skill-batch-count">
              已选 <strong>{validSelectedIds.size}</strong> / {servers.length}
              {hiddenSelectedCount > 0 && (
                <span className="skill-batch-count-hint">
                  （{hiddenSelectedCount} 项不在当前筛选内）
                </span>
              )}
            </span>
          </div>
          <div className="skill-batch-bar-right">
            <button
              type="button"
              className="btn btn-secondary"
              onClick={openBatchApply}
              disabled={isBatchRunning || validSelectedIds.size === 0}
            >
              应用到 Agent
            </button>
            <button
              type="button"
              className="btn btn-danger"
              onClick={() => {
                if (validSelectedIds.size === 0) {
                  setStatusMsg("请先选择要删除的 MCP");
                  return;
                }
                setBatchDeleteAgentConfigs(false);
                setBatchDeleteOpen(true);
              }}
              disabled={isBatchRunning || validSelectedIds.size === 0}
            >
              删除
            </button>
            <button
              type="button"
              className="btn btn-secondary"
              onClick={exitBatchMode}
              disabled={isBatchRunning}
            >
              退出
            </button>
          </div>
        </div>
      )}

      {/* ===== Add / Edit MCP Modal ===== */}
      <div
        className={`modal-overlay ${showEditor ? "visible" : ""}`}
        {...editorDismiss}
      >
        <div className="modal modal-lg mcp-modal">
          <div className="modal-header">
            <h2 className="modal-title">{editingId ? "编辑 MCP 配置" : "添加 MCP 配置"}</h2>
            <button className="modal-close" onClick={closeEditor}>
              <IconClose />
            </button>
          </div>

          <div className="modal-body mcp-modal-body">
            {/* Title */}
            <div className="form-group">
              <label className="form-label" htmlFor="mcp-title">
                标题
              </label>
              <input
                ref={titleInputRef}
                type="text"
                className="form-input"
                id="mcp-title"
                placeholder="例如: filesystem"
                value={title}
                onChange={(e) => setTitle(e.target.value)}
              />
            </div>

            {/* Type radios */}
            <div className="form-group">
              <label className="form-label">类型</label>
              <div className="mcp-type-options">
                {TYPE_OPTIONS.map((opt) => (
                  <button
                    key={opt.value}
                    type="button"
                    className={`mcp-type-option ${mcpType === opt.value ? "selected" : ""}`}
                    onClick={() => setMcpType(opt.value)}
                  >
                    <div className="pref-radio">
                      <div className="pref-radio-dot" />
                    </div>
                    <div className="mcp-type-option-content">
                      <div className="mcp-type-option-label">{opt.label}</div>
                      <div className="mcp-type-option-desc">{opt.desc}</div>
                    </div>
                  </button>
                ))}
              </div>
            </div>

            {/* Type-specific fields */}
            {mcpType === "stdio" ? (
              <>
                <div className="form-group">
                  <label className="form-label" htmlFor="mcp-command">
                    命令
                  </label>
                  <input
                    type="text"
                    className="form-input"
                    id="mcp-command"
                    placeholder="例如: npx 或 /usr/local/bin/node"
                    value={command}
                    onChange={(e) => setCommand(e.target.value)}
                  />
                </div>
                <div className="form-group">
                  <label className="form-label" htmlFor="mcp-args">
                    参数 <span className="form-label-optional">可选</span>
                  </label>
                  <textarea
                    className="form-input form-textarea"
                    id="mcp-args"
                    rows={4}
                    placeholder={"每行一个参数，例如:\n-y\n@modelcontextprotocol/server-filesystem\n/tmp"}
                    value={argsRaw}
                    onChange={(e) => setArgsRaw(e.target.value)}
                  />
                </div>
                <div className="form-group">
                  <label className="form-label">
                    环境变量 <span className="form-label-optional">可选</span>
                  </label>
                  <div className="kv-list">
                    {envList.map((item) => (
                      <div key={item.id} className="kv-row">
                        <input
                          type="text"
                          className="form-input kv-key"
                          placeholder="KEY"
                          value={item.key}
                          onChange={(e) => updateEnvKv(item.id, "key", e.target.value)}
                        />
                        <input
                          type="text"
                          className="form-input kv-value"
                          placeholder="value"
                          value={item.value}
                          onChange={(e) => updateEnvKv(item.id, "value", e.target.value)}
                        />
                        <button
                          type="button"
                          className="kv-remove"
                          onClick={() => removeEnvRow(item.id)}
                          data-tooltip="删除"
                        >
                          <IconClose />
                        </button>
                      </div>
                    ))}
                    <button type="button" className="kv-add" onClick={addEnvRow}>
                      + 添加环境变量
                    </button>
                  </div>
                </div>
              </>
            ) : (
              <>
                <div className="form-group">
                  <label className="form-label" htmlFor="mcp-url">
                    URL
                  </label>
                  <input
                    type="text"
                    className="form-input"
                    id="mcp-url"
                    placeholder={
                      mcpType === "http"
                        ? "例如: https://api.example.com/mcp"
                        : "例如: https://api.example.com/sse"
                    }
                    value={url}
                    onChange={(e) => setUrl(e.target.value)}
                  />
                </div>
                <div className="form-group">
                  <label className="form-label">
                    请求头 <span className="form-label-optional">可选</span>
                  </label>
                  <div className="kv-list">
                    {headerList.map((item) => (
                      <div key={item.id} className="kv-row">
                        <input
                          type="text"
                          className="form-input kv-key"
                          placeholder="Header-Name"
                          value={item.key}
                          onChange={(e) => updateHeaderKv(item.id, "key", e.target.value)}
                        />
                        <input
                          type="text"
                          className="form-input kv-value"
                          placeholder="value"
                          value={item.value}
                          onChange={(e) => updateHeaderKv(item.id, "value", e.target.value)}
                        />
                        <button
                          type="button"
                          className="kv-remove"
                          onClick={() => removeHeaderRow(item.id)}
                          data-tooltip="删除"
                        >
                          <IconClose />
                        </button>
                      </div>
                    ))}
                    <button type="button" className="kv-add" onClick={addHeaderRow}>
                      + 添加请求头
                    </button>
                  </div>
                </div>
              </>
            )}

            {/* JSON Preview */}
            <div className="form-group">
              <label className="form-label">JSON 预览</label>
              <pre className="mcp-json-preview">{formatJson(jsonPreview)}</pre>
            </div>

            {/* 连通性测试 */}
            <div className="form-group">
              <div className="mcp-agent-header">
                <label className="form-label">连通性测试</label>
                <div className="mcp-agent-actions">
                  <button
                    type="button"
                    className="mcp-agent-action"
                    onClick={() => void handleTest()}
                    disabled={isTesting}
                  >
                    {isTesting ? "测试中…" : "测试连接"}
                  </button>
                </div>
              </div>
              {testResult && (
                <pre
                  className="mcp-json-preview"
                  style={{ borderColor: testResult.ok ? "var(--seed-status-connected)" : "var(--seed-status-disconnected)" }}
                >
                  {(testResult.ok ? "✓ " : "✗ ") + testResult.message}
                  {testResult.detail ? "\n\n" + testResult.detail : ""}
                </pre>
              )}
            </div>

            {/* Apply to agents */}
            <div className="form-group">
              <div className="mcp-agent-header">
                <label className="form-label">
                  应用到 Agent
                  {selectedAgents.size > 0 && (
                    <span className="form-label-optional">已选 {selectedAgents.size} 个</span>
                  )}
                </label>
                {agents.length > 0 && (
                  <div className="mcp-agent-actions">
                    <button
                      type="button"
                      className="mcp-agent-action"
                      onClick={selectAllAgents}
                      disabled={selectedAgents.size === agents.length}
                    >
                      全选
                    </button>
                    <button
                      type="button"
                      className="mcp-agent-action"
                      onClick={clearSelectedAgents}
                      disabled={selectedAgents.size === 0}
                    >
                      清除
                    </button>
                  </div>
                )}
              </div>
              {agents.length === 0 ? (
                <div className="mcp-agent-empty">
                  暂无已安装的 Agent，请先到「Agent 管理」完成扫描
                </div>
              ) : (
                <div className="mcp-agent-grid">
                  {agents.map((agent) => {
                    const selected = selectedAgents.has(agent.name);
                    return (
                      <button
                        key={agent.name}
                        type="button"
                        className={`mcp-agent-pick ${selected ? "selected" : ""}`}
                        onClick={() => toggleAgent(agent.name)}
                      >
                        <span className={`mcp-agent-pick-icon ${selected ? "found" : ""}`}>
                          {getAgentIcon(agent.name) ?? agent.icon}
                        </span>
                        <span className="mcp-agent-pick-name">{agent.display_name}</span>
                        <span className={`mcp-agent-check ${selected ? "checked" : ""}`}>
                          {selected ? "✓" : ""}
                        </span>
                      </button>
                    );
                  })}
                </div>
              )}
            </div>

            {formError && <div className="mcp-form-error">{formError}</div>}
          </div>

          <div className="modal-footer">
            <button className="btn btn-secondary" onClick={closeEditor} disabled={isSaving}>
              取消
            </button>
            <button
              className="btn btn-primary"
              onClick={() => void handleSave()}
              disabled={isSaving}
            >
              {isSaving ? "保存中…" : editingId ? "保存修改" : "保存并应用"}
            </button>
          </div>
        </div>
      </div>

      {/* ===== Delete confirm ===== */}
      <div
        className={`modal-overlay ${deleteTarget ? "visible" : ""}`}
        {...deleteDismiss}
      >
        <div className="modal">
          <div className="modal-header">
            <h2 className="modal-title">删除 MCP</h2>
            <button className="modal-close" onClick={closeDelete}>
              <IconClose />
            </button>
          </div>
          <div className="confirm-body">
            <div className="confirm-text">确定删除这个 MCP 配置吗？</div>
            <div className="confirm-subtext">
              {removeFromAgentConfigs
                ? "将从本应用列表移除，并同步删除已应用 Agent 配置文件中的对应 MCP 条目。"
                : "默认仅从本应用列表移除，不会改写各 Agent 配置文件。"}
            </div>
            <label className="mcp-delete-option">
              <input
                type="checkbox"
                className="mcp-delete-checkbox"
                checked={removeFromAgentConfigs}
                onChange={(e) => setRemoveFromAgentConfigs(e.target.checked)}
              />
              <span className="mcp-delete-option-text">
                同时删除已应用 Agent 配置文件中的对应 MCP
              </span>
            </label>
          </div>
          <div className="modal-footer">
            <button className="btn btn-secondary" onClick={closeDelete} disabled={isDeleting}>
              取消
            </button>
            <button
              className="btn btn-danger"
              onClick={() => void handleDelete()}
              disabled={isDeleting}
            >
              {isDeleting ? "删除中…" : "删除"}
            </button>
          </div>
        </div>
      </div>

      {/* ===== 批量删除确认 ===== */}
      <div
        className={`modal-overlay ${batchDeleteOpen ? "visible" : ""}`}
        {...batchDeleteDismiss}
      >
        <div className="modal">
          <div className="modal-header">
            <h2 className="modal-title">
              批量删除 MCP（{validSelectedIds.size} 个）
            </h2>
            <button
              className="modal-close"
              onClick={() => !isBatchRunning && setBatchDeleteOpen(false)}
              disabled={isBatchRunning}
            >
              <IconClose />
            </button>
          </div>
          <div className="confirm-body">
            <div className="confirm-text">
              确定删除选中的 {validSelectedIds.size} 个 MCP 配置吗？
            </div>
            <div className="confirm-subtext">
              {batchDeleteAgentConfigs
                ? "将从本应用列表移除，并同步删除各 Agent 配置文件中的对应 MCP 条目。"
                : "默认仅从本应用列表移除，不会改写各 Agent 配置文件。"}
            </div>
            <label className="mcp-delete-option">
              <input
                type="checkbox"
                className="mcp-delete-checkbox"
                checked={batchDeleteAgentConfigs}
                onChange={(e) => setBatchDeleteAgentConfigs(e.target.checked)}
                disabled={isBatchRunning}
              />
              <span className="mcp-delete-option-text">
                同时删除已应用 Agent 配置文件中的对应 MCP
              </span>
            </label>
          </div>
          <div className="modal-footer">
            <button
              className="btn btn-secondary"
              onClick={() => setBatchDeleteOpen(false)}
              disabled={isBatchRunning}
            >
              取消
            </button>
            <button
              className="btn btn-danger"
              onClick={() => void confirmBatchDelete()}
              disabled={isBatchRunning}
            >
              {isBatchRunning ? "删除中…" : "删除"}
            </button>
          </div>
        </div>
      </div>

      {/* ===== 批量应用到 Agent 弹窗 ===== */}
      <div
        className={`modal-overlay ${batchApplyOpen ? "visible" : ""}`}
        {...batchApplyDismiss}
      >
        <div className="modal skill-edit-modal">
          <div className="modal-header">
            <h2 className="modal-title">
              批量应用到 Agent（{validSelectedIds.size} 个 MCP）
            </h2>
            <button
              className="modal-close"
              onClick={() => !isBatchRunning && setBatchApplyOpen(false)}
              disabled={isBatchRunning}
            >
              <IconClose />
            </button>
          </div>

          <div className="modal-body">
            {/* 模式：追加 / 移除 */}
            <div className="form-group">
              <label className="form-label">应用方式</label>
              <div className="skill-batch-mode" role="radiogroup" aria-label="应用方式">
                <button
                  type="button"
                  role="radio"
                  aria-checked={batchApplyMode === "add"}
                  className={`skill-batch-mode-opt ${batchApplyMode === "add" ? "active" : ""}`}
                  onClick={() => setBatchApplyMode("add")}
                  disabled={isBatchRunning}
                >
                  <span className="skill-batch-mode-title">追加</span>
                  <span className="skill-batch-mode-desc">
                    将选中的 MCP 写入下列 Agent 的配置文件
                  </span>
                </button>
                <button
                  type="button"
                  role="radio"
                  aria-checked={batchApplyMode === "remove"}
                  className={`skill-batch-mode-opt ${batchApplyMode === "remove" ? "active" : ""}`}
                  onClick={() => setBatchApplyMode("remove")}
                  disabled={isBatchRunning}
                >
                  <span className="skill-batch-mode-title">移除</span>
                  <span className="skill-batch-mode-desc">
                    从下列 Agent 的配置文件中删除选中的 MCP
                  </span>
                </button>
              </div>
            </div>

            {/* 选择 Agent */}
            <div className="form-group">
              <div className="mcp-agent-header">
                <label className="form-label">
                  目标 Agent
                  {batchApplyAgents.size > 0 && (
                    <span className="form-label-optional">已选 {batchApplyAgents.size} 个</span>
                  )}
                </label>
                {agents.length > 0 && (
                  <div className="mcp-agent-actions">
                    <button
                      type="button"
                      className="mcp-agent-action"
                      onClick={selectAllBatchApplyAgents}
                      disabled={isBatchRunning || batchApplyAgents.size === agents.length}
                    >
                      全选
                    </button>
                    <button
                      type="button"
                      className="mcp-agent-action"
                      onClick={clearBatchApplyAgents}
                      disabled={isBatchRunning || batchApplyAgents.size === 0}
                    >
                      清除
                    </button>
                  </div>
                )}
              </div>
              <p className="skill-add-hint">
                {batchApplyMode === "add"
                  ? "将把每个选中 MCP 的完整配置写入下列勾选的 Agent，并更新其应用记录。"
                  : "将从下列勾选的 Agent 配置中删除每个选中 MCP 的对应条目，并更新其应用记录。"}
              </p>
              {agents.length === 0 ? (
                <div className="mcp-agent-empty">
                  暂无已安装的 Agent，请先到「Agent 管理」完成扫描
                </div>
              ) : (
                <div className="mcp-agent-grid">
                  {agents.map((agent) => {
                    const selected = batchApplyAgents.has(agent.name);
                    return (
                      <button
                        key={agent.name}
                        type="button"
                        className={`mcp-agent-pick ${selected ? "selected" : ""}`}
                        onClick={() => toggleBatchApplyAgent(agent.name)}
                        disabled={isBatchRunning}
                      >
                        <span className={`mcp-agent-pick-icon ${selected ? "found" : ""}`}>
                          {getAgentIcon(agent.name) ?? agent.icon}
                        </span>
                        <span className="mcp-agent-pick-name">{agent.display_name}</span>
                        <span className={`mcp-agent-check ${selected ? "checked" : ""}`}>
                          {selected ? "✓" : ""}
                        </span>
                      </button>
                    );
                  })}
                </div>
              )}
            </div>
          </div>

          <div className="modal-footer">
            <button
              className="btn btn-secondary"
              onClick={() => setBatchApplyOpen(false)}
              disabled={isBatchRunning}
            >
              取消
            </button>
            <button
              className="btn btn-primary"
              onClick={() => void confirmBatchApply()}
              disabled={
                isBatchRunning ||
                validSelectedIds.size === 0 ||
                batchApplyAgents.size === 0
              }
            >
              {isBatchRunning
                ? "同步中…"
                : batchApplyMode === "remove"
                  ? "从 Agent 移除"
                  : "应用到 Agent"}
            </button>
          </div>
        </div>
      </div>
    </>
  );
}
