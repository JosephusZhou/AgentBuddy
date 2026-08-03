import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  AlertCircle,
  Check,
  ChevronDown,
  Clock,
  Copy,
  Layers,
  Loader2,
  Plus,
  Power,
  RefreshCw,
  Server,
  Settings2,
  Trash2,
  Zap,
} from "lucide-react";
import * as api from "./route-aggregation/api";
import type {
  ModelEntry,
  ModelSource,
  RouteAggregationConfig,
  RouteAggregationStatus,
  RouteGroup,
  ProviderRouteStatus,
} from "./route-aggregation/types";
import { DEFAULT_CONFIG } from "./route-aggregation/types";

export default function RouteAggregation() {
  const [config, setConfig] = useState<RouteAggregationConfig>(DEFAULT_CONFIG);
  const [status, setStatus] = useState<RouteAggregationStatus | null>(null);
  const [loading, setLoading] = useState(true);
  const [actionLoading, setActionLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const loadData = useCallback(async () => {
    try {
      const [cfg, sts] = await Promise.all([
        api.getConfig(),
        api.getStatus(),
      ]);
      setConfig(cfg);
      setStatus(sts);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    loadData();
    const interval = setInterval(async () => {
      try {
        const sts = await api.getStatus();
        setStatus(sts);
      } catch {
        /* ignore */
      }
    }, 5000);
    return () => clearInterval(interval);
  }, [loadData]);

  const handleConfigUpdate = async (newConfig: RouteAggregationConfig) => {
    setActionLoading(true);
    setError(null);
    try {
      const sts = await api.updateConfig(newConfig);
      setConfig(newConfig);
      setStatus(sts);
    } catch (e) {
      setError(String(e));
    } finally {
      setActionLoading(false);
    }
  };

  const handleStartServer = async () => {
    setActionLoading(true);
    setError(null);
    try {
      const sts = await api.startServer();
      setStatus(sts);
    } catch (e) {
      setError(String(e));
    } finally {
      setActionLoading(false);
    }
  };

  const handleStopServer = async () => {
    setActionLoading(true);
    setError(null);
    try {
      await api.stopServer();
      const sts = await api.getStatus();
      setStatus(sts);
    } catch (e) {
      setError(String(e));
    } finally {
      setActionLoading(false);
    }
  };

  const handleToggleGroup = async (group: RouteGroup, enabled: boolean) => {
    const newConfig = {
      ...config,
      [group === "claude_code" ? "claudeCodeEnabled" : "codexEnabled"]: enabled,
    };
    await handleConfigUpdate(newConfig);
  };

  const handleToggleProvider = async (
    providerId: string,
    group: RouteGroup,
    enabled: boolean,
  ) => {
    setActionLoading(true);
    try {
      await api.toggleProviderRoute(providerId, group, enabled);
      const sts = await api.getStatus();
      setStatus(sts);
    } catch (e) {
      setError(String(e));
    } finally {
      setActionLoading(false);
    }
  };

  const handleResetCircuitBreaker = async (
    providerId: string,
    group: RouteGroup,
  ) => {
    setActionLoading(true);
    try {
      await api.resetCircuitBreaker(providerId, group);
      const sts = await api.getStatus();
      setStatus(sts);
    } catch (e) {
      setError(String(e));
    } finally {
      setActionLoading(false);
    }
  };

  const handleRegenerateApiKey = async (group: RouteGroup): Promise<string> => {
    setActionLoading(true);
    setError(null);
    try {
      const newKey = await api.regenerateApiKey(group);
      const keyField = group === "claude_code" ? "claudeCodeApiKey" : "codexApiKey";
      setConfig({ ...config, [keyField]: newKey });
      return newKey;
    } catch (e) {
      setError(String(e));
      throw e;
    } finally {
      setActionLoading(false);
    }
  };

  const handleUpdateModels = async (
    group: RouteGroup,
    models: ModelEntry[],
  ) => {
    const field = group === "claude_code" ? "claudeCodeModels" : "codexModels";
    setConfig({ ...config, [field]: models });
  };

  const handleResetModels = async (
    group: RouteGroup,
  ): Promise<ModelEntry[]> => {
    setActionLoading(true);
    setError(null);
    try {
      const entries = await api.resetModels(group);
      const field = group === "claude_code" ? "claudeCodeModels" : "codexModels";
      setConfig({ ...config, [field]: entries });
      return entries;
    } catch (e) {
      setError(String(e));
      throw e;
    } finally {
      setActionLoading(false);
    }
  };

  if (loading) {
    return (
      <>
        <div className="content-header">
          <h1 className="content-title">路由聚合</h1>
        </div>
        <div className="content-body" style={{ alignItems: "center", justifyContent: "center" }}>
          <Loader2 size={24} className="animate-spin" style={{ color: "var(--seed-muted)" }} />
        </div>
      </>
    );
  }

  const serverRunning = status?.serverRunning ?? false;
  const proxyUrl = `http://${config.listenAddress}:${config.listenPort}`;

  return (
    <>
      {/* Header */}
      <div className="content-header">
        <div className="content-header-bar">
          <h1 className="content-title">路由聚合</h1>
          <div className="header-actions">
            {serverRunning ? (
              <button
                className="btn btn-secondary"
                onClick={handleStopServer}
                disabled={actionLoading}
                style={{ fontSize: "var(--text-sm)" }}
              >
                <Power size={14} /> 停止
              </button>
            ) : (
              <button
                className="btn btn-primary"
                onClick={handleStartServer}
                disabled={actionLoading}
                style={{ fontSize: "var(--text-sm)" }}
              >
                <Power size={14} /> 启动
              </button>
            )}
          </div>
        </div>
      </div>

      {/* Body */}
      <div className="content-body">
        {error && (
          <div
            className="form-group"
            style={{
              display: "flex",
              alignItems: "center",
              gap: 8,
              padding: "8px 12px",
              background: "var(--seed-danger-bg)",
              borderRadius: "var(--seed-radius)",
              color: "var(--seed-danger)",
              fontSize: "var(--text-sm)",
              cursor: "pointer",
            }}
            onClick={() => setError(null)}
          >
            <AlertCircle size={14} />
            <span>{error}</span>
          </div>
        )}

        {/* Route status card */}
        <div className="pref-section" style={{ marginBottom: 16 }}>
          <div className="pref-section-title" style={{ display: "flex", alignItems: "center", gap: 8 }}>
            <Server size={15} />
            路由状态
            <span
              className={`status-dot ${serverRunning ? "connected" : "disconnected"}`}
              style={{ marginLeft: 4 }}
            />
            <span style={{ fontSize: "var(--text-sm)", color: "var(--seed-muted)", fontWeight: 400 }}>
              {serverRunning ? "运行中" : "已停止"}
            </span>
          </div>
          {serverRunning && (
            <div
              style={{
                marginTop: 12,
                display: "flex",
                alignItems: "center",
                gap: 8,
                padding: "9px 12px",
                background: "var(--seed-bg)",
                border: "1px solid var(--seed-border)",
                borderRadius: "var(--seed-radius)",
              }}
            >
              <code style={{ fontSize: "var(--text-sm)", color: "var(--seed-primary)", flex: 1 }}>
                {proxyUrl}
              </code>
              <button
                className="btn-icon-action"
                onClick={() => navigator.clipboard.writeText(proxyUrl)}
                data-tooltip="复制地址"
              >
                <Copy size={14} />
              </button>
            </div>
          )}
        </div>

        {/* Basic config */}
        <div className="pref-section" style={{ marginBottom: 16 }}>
          <div className="pref-section-title" style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 4 }}>
            <Settings2 size={15} />
            基本配置
          </div>

          <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 12, marginTop: 12 }}>
            <div className="form-group" style={{ marginBottom: 0 }}>
              <label className="form-label">监听地址</label>
              <div style={{ display: "flex", gap: 8 }}>
                {([
                  { value: "127.0.0.1", label: "本机" },
                  { value: "0.0.0.0", label: "局域网" },
                ] as const).map((opt) => (
                  <button
                    key={opt.value}
                    type="button"
                    className={`net-protocol-chip ${config.listenAddress === opt.value ? "selected" : ""}`}
                    onClick={() =>
                      handleConfigUpdate({
                        ...config,
                        listenAddress: opt.value,
                      })
                    }
                    disabled={actionLoading}
                  >
                    {opt.label}
                  </button>
                ))}
              </div>
              <div style={{ fontSize: "var(--text-xs)", color: "var(--seed-muted)", marginTop: 4 }}>
                {config.listenAddress}
              </div>
            </div>
            <div className="form-group" style={{ marginBottom: 0 }}>
              <label className="form-label">端口</label>
              <input
                className="form-input"
                type="text"
                inputMode="numeric"
                value={String(config.listenPort)}
                onChange={(e) => {
                  const v = e.target.value.replace(/\D/g, "").slice(0, 5);
                  setConfig({ ...config, listenPort: parseInt(v) || 0 });
                }}
                onBlur={() => {
                  const port = config.listenPort || 16888;
                  setConfig({ ...config, listenPort: port });
                  handleConfigUpdate({ ...config, listenPort: port });
                }}
                disabled={actionLoading}
              />
            </div>
            <div className="form-group" style={{ marginBottom: 0 }}>
              <label className="form-label">最大重试次数</label>
              <input
                className="form-input"
                type="text"
                inputMode="numeric"
                value={String(config.maxRetries)}
                onChange={(e) => {
                  const v = e.target.value.replace(/\D/g, "").slice(0, 2);
                  const n = Math.min(parseInt(v) || 0, 10);
                  setConfig({ ...config, maxRetries: n });
                }}
                onBlur={() => handleConfigUpdate(config)}
                disabled={actionLoading}
              />
            </div>
            <div className="form-group" style={{ marginBottom: 0 }}>
              <label className="form-label">伪装模式</label>
              <div style={{ display: "flex", gap: 8 }}>
                {([
                  { value: "auto", label: "自动" },
                  { value: "always", label: "强制" },
                  { value: "never", label: "关闭" },
                ] as const).map((opt) => (
                  <button
                    key={opt.value}
                    type="button"
                    className={`net-protocol-chip ${config.cloakingMode === opt.value ? "selected" : ""}`}
                    onClick={() =>
                      handleConfigUpdate({
                        ...config,
                        cloakingMode: opt.value,
                      })
                    }
                    disabled={actionLoading}
                  >
                    {opt.label}
                  </button>
                ))}
              </div>
            </div>
          </div>
          <label
            className="ui-check"
            style={{ marginTop: 12 }}
          >
            <input
              className="ui-check-input"
              type="checkbox"
              checked={config.autoFailover}
              onChange={(e) => handleConfigUpdate({ ...config, autoFailover: e.target.checked })}
              disabled={actionLoading}
            />
            <span className="ui-check-box">
              <Check size={12} />
            </span>
            <span className="ui-check-label">自动故障转移</span>
          </label>
        </div>

        {/* Claude Code route group */}
        <RouteGroupPanel
          title="Claude Code 路由"
          group="claude_code"
          enabled={config.claudeCodeEnabled}
          status={status?.claudeCode}
          proxyUrl={`${proxyUrl}/v1/messages`}
          version={config.claudeCodeVersion}
          apiKey={config.claudeCodeApiKey}
          models={config.claudeCodeModels}
          onToggle={(enabled) => handleToggleGroup("claude_code", enabled)}
          onToggleProvider={handleToggleProvider}
          onResetCircuitBreaker={handleResetCircuitBreaker}
          onRegenerateApiKey={() => handleRegenerateApiKey("claude_code")}
          onUpdateModels={(models) => handleUpdateModels("claude_code", models)}
          onResetModels={() => handleResetModels("claude_code")}
          actionLoading={actionLoading}
        />

        {/* Codex route group */}
        <RouteGroupPanel
          title="Codex 路由"
          group="codex"
          enabled={config.codexEnabled}
          status={status?.codex}
          proxyUrl={`${proxyUrl}/v1/responses`}
          version={config.codexVersion}
          apiKey={config.codexApiKey}
          models={config.codexModels}
          onToggle={(enabled) => handleToggleGroup("codex", enabled)}
          onToggleProvider={handleToggleProvider}
          onResetCircuitBreaker={handleResetCircuitBreaker}
          onRegenerateApiKey={() => handleRegenerateApiKey("codex")}
          onUpdateModels={(models) => handleUpdateModels("codex", models)}
          onResetModels={() => handleResetModels("codex")}
          actionLoading={actionLoading}
        />

        {/* Usage instructions */}
        <div className="pref-section" style={{ marginBottom: 16 }}>
          <div className="pref-section-title" style={{ display: "flex", alignItems: "center", gap: 8 }}>
            <Zap size={15} />
            使用说明
          </div>
          <div className="pref-section-desc" style={{ marginTop: 8, lineHeight: 1.8 }}>
            <p style={{ marginBottom: 4 }}>
              <strong>Claude Code:</strong> 设置{" "}
              <code style={{ background: "var(--seed-surface-alt)", padding: "2px 6px", borderRadius: 4, fontSize: "var(--text-xs)", color: "var(--seed-primary)" }}>
                ANTHROPIC_BASE_URL={proxyUrl}
              </code>
            </p>
            <p style={{ marginBottom: 4 }}>
              <strong>Codex CLI:</strong> 设置{" "}
              <code style={{ background: "var(--seed-surface-alt)", padding: "2px 6px", borderRadius: 4, fontSize: "var(--text-xs)", color: "var(--seed-primary)" }}>
                OPENAI_BASE_URL={proxyUrl}/v1
              </code>
            </p>
            <p style={{ color: "var(--seed-muted)", marginTop: 8 }}>
              客户端请求会自动经路由聚合代理转发到已启用的供应商，享受整流器伪装和自动故障转移能力。
            </p>
          </div>
        </div>
      </div>
    </>
  );
}

/* ===== Route Group Panel ===== */

/** Return a new array of model entries sorted alphabetically by model id. */
function sortModelEntries(entries: ModelEntry[]): ModelEntry[] {
  return [...entries].sort((a, b) => a.id.localeCompare(b.id));
}

interface RouteGroupPanelProps {
  title: string;
  group: RouteGroup;
  enabled: boolean;
  status?: { enabled: boolean; activeProviders: ProviderRouteStatus[]; totalProviders: number };
  proxyUrl: string;
  version: string;
  apiKey: string;
  models: ModelEntry[];
  onToggle: (enabled: boolean) => void;
  onToggleProvider: (providerId: string, group: RouteGroup, enabled: boolean) => void;
  onResetCircuitBreaker: (providerId: string, group: RouteGroup) => void;
  onRegenerateApiKey: () => Promise<string>;
  onUpdateModels: (models: ModelEntry[]) => Promise<void>;
  onResetModels: () => Promise<ModelEntry[]>;
  actionLoading: boolean;
}

function RouteGroupPanel({
  title,
  group,
  enabled,
  status,
  proxyUrl,
  version,
  apiKey,
  models,
  onToggle,
  onToggleProvider,
  onResetCircuitBreaker,
  onRegenerateApiKey,
  onUpdateModels,
  onResetModels,
  actionLoading,
}: RouteGroupPanelProps) {
  const providers = status?.activeProviders ?? [];
  const [localModels, setLocalModels] = useState<ModelEntry[]>(() =>
    sortModelEntries(models),
  );
  const [availableModels, setAvailableModels] = useState<ModelSource[]>([]);
  const [apiKeyCopied, setApiKeyCopied] = useState(false);
  const [copiedModelId, setCopiedModelId] = useState<string | null>(null);
  const [providersExpanded, setProvidersExpanded] = useState(true);
  const [modelsExpanded, setModelsExpanded] = useState(true);
  const [showResetConfirm, setShowResetConfirm] = useState(false);
  const [showAddDropdown, setShowAddDropdown] = useState(false);
  const [addFilterText, setAddFilterText] = useState("");
  const addDropdownRef = useRef<HTMLDivElement>(null);

  // Close add dropdown on click-outside or Escape
  useEffect(() => {
    if (!showAddDropdown) return;
    const onPointerDown = (e: MouseEvent) => {
      if (addDropdownRef.current && !addDropdownRef.current.contains(e.target as Node)) {
        setShowAddDropdown(false);
      }
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        setShowAddDropdown(false);
      }
    };
    document.addEventListener("mousedown", onPointerDown);
    document.addEventListener("keydown", onKey, true);
    return () => {
      document.removeEventListener("mousedown", onPointerDown);
      document.removeEventListener("keydown", onKey, true);
    };
  }, [showAddDropdown]);

  // Sync localModels when models prop changes
  useEffect(() => {
    setLocalModels(sortModelEntries(models));
  }, [models]);

  // Fetch available models for the add dropdown
  const refreshAvailableModels = useCallback(async () => {
    try {
      const all = await api.getRouteModels(group);
      setAvailableModels(all);
    } catch {
      /* ignore */
    }
  }, [group]);

  // Signature of the currently enabled providers — changes when a provider
  // is checked/unchecked, so the dropdown refetches its model list.
  const enabledProviderKey = useMemo(
    () =>
      providers
        .filter((p) => p.enabled)
        .map((p) => p.id)
        .join(","),
    [providers],
  );

  useEffect(() => {
    if (enabled) {
      refreshAvailableModels();
    }
  }, [enabled, enabledProviderKey, refreshAvailableModels]);

  const handleCopyApiKey = async () => {
    if (apiKey) {
      await navigator.clipboard.writeText(apiKey);
      setApiKeyCopied(true);
      setTimeout(() => setApiKeyCopied(false), 2000);
    }
  };

  const saveModels = async (newModels: ModelEntry[]) => {
    const sorted = sortModelEntries(newModels);
    setLocalModels(sorted);
    try {
      await api.updateModels(group, sorted);
      await onUpdateModels(sorted);
    } catch {
      /* ignore */
    }
  };

  const handleAliasChange = (index: number, alias: string) => {
    const updated = [...localModels];
    updated[index] = { ...updated[index], alias };
    setLocalModels(updated);
  };

  const handleAliasBlur = (index: number) => {
    const updated = [...localModels];
    updated[index] = {
      ...updated[index],
      alias: updated[index].alias.trim(),
    };
    saveModels(updated);
  };

  const handleDeleteModel = (index: number) => {
    const updated = localModels.filter((_, i) => i !== index);
    saveModels(updated);
  };

  const handleAddModel = (modelId: string) => {
    if (!modelId) return;
    if (localModels.some((m) => m.id === modelId)) return;
    saveModels([...localModels, { id: modelId, alias: "" }]);
    // Keep the dropdown open so the user can add multiple models; the newly
    // added one now renders as 已添加/disabled.
  };

  const handleResetConfirm = async () => {
    setShowResetConfirm(false);
    try {
      const entries = await onResetModels();
      setLocalModels(sortModelEntries(entries));
      await refreshAvailableModels();
    } catch {
      /* ignore */
    }
  };

  // Map model id -> provider names, used to show source providers on saved items
  const providersByModel = useMemo(() => {
    const map: Record<string, string[]> = {};
    for (const src of availableModels) {
      map[src.id] = src.providers;
    }
    return map;
  }, [availableModels]);

  // All available models (including already-added ones), filtered by the
  // search input. Already-added items are rendered as disabled with a marker
  // so the user can see what's already in the list and avoid duplicates.
  const filteredModels = useMemo(() => {
    const q = addFilterText.trim().toLowerCase();
    if (!q) return availableModels;
    return availableModels.filter((src) => src.id.toLowerCase().includes(q));
  }, [availableModels, addFilterText]);

  return (
    <div
      className="pref-section"
      style={{
        marginBottom: 16,
        opacity: enabled ? 1 : 0.55,
      }}
    >
      {/* Header row: title + version + toggle */}
      <div
        className="pref-section-title"
        style={{ display: "flex", alignItems: "center", gap: 8 }}
      >
        <span>{title}</span>
        <span
          style={{
            fontSize: "var(--text-xs)",
            color: "var(--seed-muted)",
            fontWeight: 400,
            background: "var(--seed-surface-alt)",
            padding: "2px 6px",
            borderRadius: 4,
          }}
        >
          v{version}
        </span>
        <div style={{ flex: 1 }} />
        <label className="ra-toggle">
          <input
            type="checkbox"
            checked={enabled}
            onChange={(e) => onToggle(e.target.checked)}
            disabled={actionLoading}
          />
          <span className="ra-toggle-slider" />
        </label>
      </div>

      {enabled && (
        <>
          {/* Proxy URL */}
          <div
            style={{
              marginTop: 12,
              display: "flex",
              alignItems: "center",
              gap: 8,
              padding: "9px 12px",
              background: "var(--seed-bg)",
              border: "1px solid var(--seed-border)",
              borderRadius: "var(--seed-radius)",
            }}
          >
            <code style={{ fontSize: "var(--text-sm)", color: "var(--seed-primary)", flex: 1 }}>
              {proxyUrl}
            </code>
            <button
              className="btn-icon-action"
              onClick={() => navigator.clipboard.writeText(proxyUrl)}
              data-tooltip="复制地址"
            >
              <Copy size={14} />
            </button>
          </div>

          {/* API Key */}
          <div className="form-group" style={{ marginBottom: 0, marginTop: 12 }}>
            <label className="form-label">API Key</label>
            <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
              <input
                className="form-input"
                type="text"
                value={
                  apiKey
                    ? apiKey.length > 10
                      ? apiKey.slice(0, 6) + "*".repeat(apiKey.length - 10) + apiKey.slice(-4)
                      : apiKey
                    : ""
                }
                readOnly
                disabled={actionLoading}
                style={{ flex: 1, fontFamily: "monospace", fontSize: "var(--text-sm)" }}
              />
              <button
                className="btn-icon-action"
                onClick={handleCopyApiKey}
                data-tooltip={apiKeyCopied ? "已复制" : "复制 API Key"}
                disabled={actionLoading || !apiKey}
              >
                {apiKeyCopied ? <Check size={14} /> : <Copy size={14} />}
              </button>
              <button
                className="btn-icon-action"
                onClick={async () => {
                  try { await onRegenerateApiKey(); } catch { /* ignore */ }
                }}
                data-tooltip="重新生成 API Key"
                disabled={actionLoading}
              >
                <RefreshCw size={14} />
              </button>
            </div>
          </div>

          {/* Providers (collapsible) */}
          <div style={{ marginTop: 16 }}>
            <div
              style={{
                display: "inline-flex",
                alignItems: "center",
                gap: 8,
                marginBottom: 8,
                cursor: "pointer",
                userSelect: "none",
              }}
              onClick={() => setProvidersExpanded((v) => !v)}
            >
              <ChevronDown
                size={16}
                style={{
                  color: "var(--seed-muted)",
                  transition: "transform 0.15s",
                  transform: providersExpanded ? "rotate(0deg)" : "rotate(-90deg)",
                }}
              />
              <Server size={15} style={{ color: "var(--seed-muted)" }} />
              <span style={{ fontSize: "var(--text-sm)", fontWeight: 500 }}>提供商</span>
              <span style={{ fontSize: "var(--text-xs)", color: "var(--seed-muted)" }}>
                {providers.length > 0 ? `${providers.length} 个` : ""}
              </span>
            </div>
            {providersExpanded && (
              <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
                {providers.length === 0 ? (
                  <div className="empty-state" style={{ minHeight: 120, padding: "24px 12px" }}>
                    <Server size={32} />
                    <div className="empty-state-text">暂无匹配的供应商</div>
                    <div className="empty-state-subtext">
                      请先在「AI 供应商」中添加对应类型的供应商
                    </div>
                  </div>
                ) : (
                  providers.map((p) => (
                    <ProviderRow
                      key={p.id}
                      provider={p}
                      onToggle={(en) => onToggleProvider(p.id, group, en)}
                      onResetCircuitBreaker={() => onResetCircuitBreaker(p.id, group)}
                      actionLoading={actionLoading}
                    />
                  ))
                )}
              </div>
            )}
          </div>

          {/* Model configuration (collapsible) */}
          <div style={{ marginTop: 16 }}>
            <div
              style={{
                display: "flex",
                alignItems: "center",
                gap: 8,
                marginBottom: 8,
              }}
            >
              <div
                style={{
                  display: "inline-flex",
                  alignItems: "center",
                  gap: 8,
                  cursor: "pointer",
                  userSelect: "none",
                }}
                onClick={() => setModelsExpanded((v) => !v)}
              >
                <ChevronDown
                  size={16}
                  style={{
                    color: "var(--seed-muted)",
                    transition: "transform 0.15s",
                    transform: modelsExpanded ? "rotate(0deg)" : "rotate(-90deg)",
                  }}
                />
                <Layers size={15} style={{ color: "var(--seed-muted)" }} />
                <span style={{ fontSize: "var(--text-sm)", fontWeight: 500 }}>模型配置</span>
                <span style={{ fontSize: "var(--text-xs)", color: "var(--seed-muted)" }}>
                  {localModels.length > 0 ? `${localModels.length} 个` : ""}
                </span>
              </div>
              <div style={{ flex: 1 }} />
              <button
                className="btn-icon-action"
                onClick={(e) => {
                  e.stopPropagation();
                  setAddFilterText("");
                  setShowAddDropdown(!showAddDropdown);
                }}
                data-tooltip="添加模型"
                disabled={actionLoading || availableModels.length === 0}
              >
                <Plus size={14} />
              </button>
              <button
                className="btn-icon-action"
                onClick={(e) => {
                  e.stopPropagation();
                  setShowResetConfirm(true);
                }}
                data-tooltip="重置为供应商模型列表"
                disabled={actionLoading}
              >
                <RefreshCw size={14} />
              </button>
            </div>
            {modelsExpanded && (
              <>
                {/* Add model dropdown */}
                {showAddDropdown && (
                  <div
                    ref={addDropdownRef}
                    style={{ position: "relative", marginBottom: 8 }}
                  >
                    <input
                      className="form-input"
                      type="text"
                      autoFocus
                      placeholder="输入模型名以筛选…"
                      value={addFilterText}
                      onChange={(e) => setAddFilterText(e.target.value)}
                      style={{ width: "100%", fontSize: "var(--text-sm)", boxSizing: "border-box", marginBottom: 6 }}
                    />
                    <div
                      className="app-select-menu"
                      role="listbox"
                      style={{ position: "relative", top: 0, left: 0, right: 0 }}
                    >
                      {filteredModels.length === 0 ? (
                        <div className="app-select-empty">无匹配模型</div>
                      ) : (
                        filteredModels.map((src) => {
                          const isAdded = localModels.some((m) => m.id === src.id);
                          return (
                            <button
                              key={src.id}
                              type="button"
                              role="option"
                              aria-selected={isAdded}
                              className={`app-select-option ${isAdded ? "disabled" : ""}`}
                              disabled={isAdded}
                              onClick={() => handleAddModel(src.id)}
                            >
                              <span
                                className="app-select-option-title"
                                style={{ display: "flex", alignItems: "center", gap: 6, width: "100%" }}
                              >
                                <span
                                  style={{
                                    flex: 1,
                                    overflow: "hidden",
                                    textOverflow: "ellipsis",
                                    whiteSpace: "nowrap",
                                  }}
                                >
                                  {src.id}
                                </span>
                                {isAdded && (
                                  <span
                                    style={{
                                      display: "inline-flex",
                                      alignItems: "center",
                                      gap: 2,
                                      flexShrink: 0,
                                      fontSize: "var(--text-xs)",
                                      color: "var(--seed-muted)",
                                      fontWeight: 400,
                                    }}
                                  >
                                    <Check size={12} /> 已添加
                                  </span>
                                )}
                              </span>
                              {src.providers.length > 0 && (
                                <span className="app-select-option-sub">
                                  来自 {src.providers.join("、")}
                                </span>
                              )}
                            </button>
                          );
                        })
                      )}
                    </div>
                  </div>
                )}
                {/* Reset confirmation */}
                {showResetConfirm && (
                  <div
                    style={{
                      marginBottom: 8,
                      padding: "10px 12px",
                      background: "var(--seed-danger-bg)",
                      border: "1px solid var(--seed-danger)",
                      borderRadius: "var(--seed-radius)",
                      display: "flex",
                      alignItems: "center",
                      gap: 8,
                      fontSize: "var(--text-sm)",
                    }}
                  >
                    <AlertCircle size={14} style={{ color: "var(--seed-danger)" }} />
                    <span style={{ flex: 1, color: "var(--seed-danger)" }}>
                      确定要重置吗？当前列表将被替换为供应商模型集合。
                    </span>
                    <button
                      className="btn btn-secondary"
                      onClick={handleResetConfirm}
                      disabled={actionLoading}
                      style={{ fontSize: "var(--text-xs)", padding: "3px 10px" }}
                    >
                      确定
                    </button>
                    <button
                      className="btn btn-secondary"
                      onClick={() => setShowResetConfirm(false)}
                      disabled={actionLoading}
                      style={{ fontSize: "var(--text-xs)", padding: "3px 10px" }}
                    >
                      取消
                    </button>
                  </div>
                )}
                {localModels.length === 0 ? (
                  <div className="empty-state" style={{ minHeight: 80, padding: "16px 12px" }}>
                    <Layers size={24} />
                    <div className="empty-state-text">暂无模型</div>
                    <div className="empty-state-subtext">
                      点击 + 添加模型，或点击刷新从供应商导入
                    </div>
                  </div>
                ) : (
                  <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
                    {localModels.map((entry, index) => (
                      <div
                        key={`${entry.id}-${index}`}
                        style={{
                          display: "flex",
                          alignItems: "center",
                          gap: 8,
                          padding: "8px 12px",
                          background: "var(--seed-surface)",
                          border: "1px solid var(--seed-border)",
                          borderRadius: "var(--seed-radius)",
                        }}
                      >
                        <div
                          style={{ display: "flex", alignItems: "center", gap: 6, flex: 1, cursor: "pointer" }}
                          onClick={() => {
                            navigator.clipboard.writeText(entry.id);
                            setCopiedModelId(entry.id);
                            setTimeout(() => setCopiedModelId(null), 2000);
                          }}
                        >
                          <code style={{ fontSize: "var(--text-sm)", color: "var(--seed-primary)" }}>
                            {entry.id}
                          </code>
                          <button
                            className="btn-icon-action"
                            data-tooltip={copiedModelId === entry.id ? "已复制" : "复制模型 ID"}
                            style={{ padding: 2 }}
                          >
                            {copiedModelId === entry.id ? <Check size={13} /> : <Copy size={13} />}
                          </button>
                          {(providersByModel[entry.id] ?? []).length > 0 && (
                            <span
                              style={{
                                fontSize: "var(--text-xs)",
                                color: "var(--seed-muted)",
                                whiteSpace: "nowrap",
                                overflow: "hidden",
                                textOverflow: "ellipsis",
                              }}
                            >
                              来自 {providersByModel[entry.id].join("、")}
                            </span>
                          )}
                        </div>
                        <span style={{ color: "var(--seed-muted)", fontSize: "var(--text-sm)" }}>→</span>
                        <input
                          className="form-input"
                          type="text"
                          value={entry.alias}
                          onChange={(e) => handleAliasChange(index, e.target.value)}
                          onBlur={() => handleAliasBlur(index)}
                          placeholder="别名（可选）"
                          disabled={actionLoading}
                          style={{ width: 200, fontSize: "var(--text-sm)" }}
                        />
                        <button
                          className="btn-icon-action"
                          onClick={() => handleDeleteModel(index)}
                          data-tooltip="删除"
                          disabled={actionLoading}
                          style={{ color: "var(--seed-danger)" }}
                        >
                          <Trash2 size={14} />
                        </button>
                      </div>
                    ))}
                  </div>
                )}
              </>
            )}
          </div>
        </>
      )}
    </div>
  );
}

/* ===== Provider Row ===== */

interface ProviderRowProps {
  provider: ProviderRouteStatus;
  onToggle: (enabled: boolean) => void;
  onResetCircuitBreaker: () => void;
  actionLoading: boolean;
}

function ProviderRow({ provider, onToggle, onResetCircuitBreaker, actionLoading }: ProviderRowProps) {
  const circuitClass =
    provider.circuitState === "closed"
      ? "connected"
      : provider.circuitState === "open"
        ? "disconnected"
        : "checking";

  const circuitLabel =
    provider.circuitState === "closed"
      ? "正常"
      : provider.circuitState === "open"
        ? "熔断"
        : "探测中";

  return (
    <div
      style={{
        display: "flex",
        alignItems: "center",
        gap: 14,
        padding: "14px 16px",
        background: "var(--seed-surface)",
        border: "1px solid var(--seed-border)",
        borderRadius: "var(--seed-radius-lg)",
      }}
    >
      {/* Enable checkbox */}
      <label className="ui-check">
        <input
          className="ui-check-input"
          type="checkbox"
          checked={provider.enabled}
          onChange={(e) => onToggle(e.target.checked)}
          disabled={actionLoading}
        />
        <span className="ui-check-box">
          <Check size={12} />
        </span>
      </label>

      {/* Provider info */}
      <div style={{ flex: 1, minWidth: 0 }}>
        <div style={{ fontSize: "var(--text-base)", fontWeight: 500, display: "flex", alignItems: "center", gap: 6 }}>
          {provider.name}
          <span
            style={{
              fontSize: "var(--text-xs)",
              fontWeight: 600,
              letterSpacing: "0.04em",
              padding: "2px 6px",
              borderRadius: 4,
              background: "var(--seed-surface-alt)",
              color: "var(--seed-muted)",
              border: "1px solid var(--seed-border)",
            }}
          >
            {provider.providerType}
          </span>
        </div>
        <div style={{ fontSize: "var(--text-sm)", color: "var(--seed-muted)", display: "flex", gap: 12, marginTop: 2 }}>
          <span>请求 {provider.requestCount}</span>
          <span>成功 {provider.successCount}</span>
          {provider.consecutiveFailures > 0 && (
            <span style={{ color: "var(--seed-danger)" }}>连续失败 {provider.consecutiveFailures}</span>
          )}
          {provider.lastError && (
            <span style={{ display: "inline-flex", alignItems: "center", gap: 3, color: "var(--seed-danger)" }}>
              <Clock size={12} /> 最近错误
            </span>
          )}
        </div>
      </div>

      {/* Circuit breaker status */}
      <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
        <span className={`status-dot ${circuitClass}`} />
        <span style={{ fontSize: "var(--text-sm)", color: "var(--seed-muted)" }}>{circuitLabel}</span>
      </div>

      {/* Reset button */}
      {(provider.circuitState === "open" || provider.circuitState === "half_open") && (
        <button
          className="btn btn-secondary"
          onClick={onResetCircuitBreaker}
          disabled={actionLoading}
          style={{ fontSize: "var(--text-xs)", padding: "4px 10px" }}
        >
          <RefreshCw size={12} /> 重置
        </button>
      )}
    </div>
  );
}
