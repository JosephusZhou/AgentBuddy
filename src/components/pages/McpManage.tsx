import { useState, useEffect, useCallback, useRef, useMemo } from "react";
import { useStatusMessage } from "@/lib/useStatusMessage";
import { Toast } from "@/components/Toast";
import { AgentBadge, getAgentIcon } from "../agent-icons";
import { useOverlayDismiss } from "../ui";

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

/** 前端复现后端 dedupe_write_targets 的共享根归一：codebuddy / codebuddy-cn 写同一文件。
 *  用于把「实际写入失败」的 canonical 结果映射回用户选择的 agent。 */
function canonicalWriteTarget(agent: string, selected: string[]): string {
  if (agent === "codebuddy" || agent === "codebuddy-cn") {
    return selected.includes("codebuddy") ? "codebuddy" : agent;
  }
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
  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
    <line x1="12" y1="5" x2="12" y2="19" />
    <line x1="5" y1="12" x2="19" y2="12" />
  </svg>
);

const IconSearch = () => (
  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
    <path d="M21 21l-4.35-4.35" />
    <circle cx="11" cy="11" r="7" />
  </svg>
);

const IconClose = () => (
  <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
    <line x1="18" y1="6" x2="6" y2="18" />
    <line x1="6" y1="6" x2="18" y2="18" />
  </svg>
);

const IconTrash = () => (
  <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
    <polyline points="3 6 5 6 21 6" />
    <path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2" />
    <line x1="10" y1="11" x2="10" y2="17" />
    <line x1="14" y1="11" x2="14" y2="17" />
  </svg>
);

const IconEdit = () => (
  <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
    <path d="M12 20h9" />
    <path d="M16.5 3.5a2.12 2.12 0 0 1 3 3L7 19l-4 1 1-4 12.5-12.5z" />
  </svg>
);

const IconMcp = () => (
  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
    <circle cx="12" cy="12" r="3" />
    <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83-2.83l.06-.06A1.65 1.65 0 0 0 4.68 15a1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 2.83-2.83l.06.06A1.65 1.65 0 0 0 9 4.68a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 2.83l.06.06A1.65 1.65 0 0 0 19.4 9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z" />
  </svg>
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

  const editorDismiss = useOverlayDismiss(() => closeEditor());
  const deleteDismiss = useOverlayDismiss(() => closeDelete());

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
      }
    };
    document.addEventListener("keydown", handler);
    return () => document.removeEventListener("keydown", handler);
  }, []);

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

  const typeLabel = (t: McpType) => t.toUpperCase();

  return (
    <>
      <div className="content-header">
        <div className="content-header-bar">
          <h1 className="content-title">MCP 管理</h1>
          <div className="header-actions">
            <button
              className={`action-btn ${isSniffing ? "sniffing" : ""}`}
              data-tooltip={isSniffing ? "扫描中..." : "扫描 MCP"}
              onClick={() => void doSniffMcp()}
              disabled={isSniffing}
            >
              <IconSearch />
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
            </div>
            <div className="mcp-list">
              {servers.map((server) => (
                <div key={server.id} className="mcp-card">
                  <div className="mcp-card-header">
                    <div className="mcp-card-icon">{server.title.slice(0, 2).toUpperCase()}</div>
                    <div className="mcp-card-main">
                      <div className="mcp-card-title-row">
                        <span className="mcp-card-title">{server.title}</span>
                        <span className={`mcp-type-badge mcp-type-${server.type}`}>
                          {typeLabel(server.type)}
                        </span>
                      </div>
                    </div>
                    <div className="mcp-card-actions">
                      <button
                        className="btn-icon-action mcp-card-action"
                        onClick={() => openEdit(server)}
                        title="编辑"
                      >
                        <IconEdit />
                      </button>
                      <button
                        className="btn-delete mcp-card-action"
                        onClick={() => openDelete(server.id)}
                        title="删除"
                      >
                        <IconTrash />
                      </button>
                    </div>
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
              ))}
            </div>
          </>
        )}
      </div>

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
                          title="删除"
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
                          title="删除"
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
                  style={{ borderColor: testResult.ok ? "#16a34a" : "#dc2626" }}
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
    </>
  );
}
