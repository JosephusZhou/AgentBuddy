import { useCallback, useEffect, useMemo, useState } from "react";
import {
  AlertCircle,
  Check,
  ChevronDown,
  ChevronRight,
  Clock,
  Copy,
  KeyRound,
  Layers,
  Loader2,
  Plus,
  Power,
  RefreshCw,
  ScrollText,
  Server,
  Settings2,
  Trash2,
  X,
  Zap,
} from "lucide-react";
import * as api from "./route-aggregation/api";
import {
  DEFAULT_CONFIG,
  UNSUPPORTED_ROUTE_PROVIDER_HINT,
  isRouteableProviderType,
  type ProviderRouteStatus,
  type RouteAggregationConfig,
  type RouteAggregationStatus,
  type RouteLogEntry,
  type InboundProtocol,
} from "./route-aggregation/types";
import { invokeList } from "./ai-providers/api";
import type { AiProvider } from "./ai-providers/types";

/**
 * 收起供应商列表时展示的模型 ID。
 *
 * 与后端 `effective_custom_model_ids` 保持一致：别名优先，按供应商和模型原有
 * 顺序去重。没有状态记录的供应商按后端默认值视为已启用。
 */
function getSelectedProviderModelIds(
  providers: AiProvider[],
  providerStatuses: ProviderRouteStatus[],
): string[] {
  const enabledById = new Map(providerStatuses.map((provider) => [provider.id, provider.enabled]));
  const modelIds = new Set<string>();

  for (const provider of providers) {
    if (!isRouteableProviderType(provider.providerType) || enabledById.get(provider.id) === false) {
      continue;
    }

    for (const customModel of provider.customModels) {
      const modelId = customModel.aliasId.trim() || customModel.model.trim();
      if (modelId) {
        modelIds.add(modelId);
      }
    }
  }

  return [...modelIds];
}

export default function RouteAggregation() {
  const [config, setConfig] = useState<RouteAggregationConfig>(DEFAULT_CONFIG);
  const [status, setStatus] = useState<RouteAggregationStatus | null>(null);
  const [providers, setProviders] = useState<AiProvider[]>([]);
  const [loading, setLoading] = useState(true);
  const [actionLoading, setActionLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [providersExpanded, setProvidersExpanded] = useState(true);

  // 进出日志状态：列表 + 选中的详情弹窗
  const [logs, setLogs] = useState<RouteLogEntry[]>([]);
  const [logsLoading, setLogsLoading] = useState(false);
  const [logsError, setLogsError] = useState<string | null>(null);
  const [selectedLog, setSelectedLog] = useState<RouteLogEntry | null>(null);
  const [logAutoRefresh, setLogAutoRefresh] = useState(true);
  const [logFilePath, setLogFilePath] = useState<string | null>(null);

  const selectedProviderModels = useMemo(
    () => getSelectedProviderModelIds(providers, status?.providers ?? []),
    [providers, status],
  );

  const loadData = useCallback(async () => {
    try {
      const [cfg, sts, list] = await Promise.all([
        api.getConfig(),
        api.getStatus(),
        invokeList(),
      ]);
      setConfig(cfg);
      setStatus(sts);
      setProviders(list);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }, []);

  const loadLogs = useCallback(async () => {
    setLogsLoading(true);
    try {
      const [list, currentLogPath] = await Promise.all([
        api.getRouteLogs(),
        api.getRouteLogFilePath(),
      ]);
      setLogFilePath(currentLogPath);
      // 环形缓冲区满后数量不再变化，必须比较内容，不能只比较长度。
      setLogs((prev) => {
        const unchanged =
          prev.length === list.length && prev.every((entry, index) => entry.id === list[index]?.id);
        return unchanged ? prev : list;
      });
      setSelectedLog((selected) =>
        selected && list.some((entry) => entry.id === selected.id) ? selected : null,
      );
      setLogsError(null);
    } catch (e) {
      setLogsError(String(e));
    } finally {
      setLogsLoading(false);
    }
  }, []);

  useEffect(() => {
    loadData();
    loadLogs();
    const interval = setInterval(async () => {
      try {
        const sts = await api.getStatus();
        setStatus(sts);
      } catch {
        /* ignore */
      }
      if (logAutoRefresh) {
        loadLogs();
      }
    }, 5000);
    return () => clearInterval(interval);
  }, [loadData, loadLogs, logAutoRefresh]);

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
      setConfig((prev) => ({ ...prev, autoStart: true }));
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
      setConfig((prev) => ({ ...prev, autoStart: false }));
    } catch (e) {
      setError(String(e));
    } finally {
      setActionLoading(false);
    }
  };

  const handleToggleProvider = async (providerId: string, enabled: boolean) => {
    setActionLoading(true);
    setError(null);
    try {
      await api.toggleProviderRoute(providerId, enabled);
      const sts = await api.getStatus();
      setStatus(sts);
    } catch (e) {
      setError(String(e));
    } finally {
      setActionLoading(false);
    }
  };

  const handleResetCircuitBreaker = async (providerId: string) => {
    setActionLoading(true);
    setError(null);
    try {
      await api.resetCircuitBreaker(providerId);
      const sts = await api.getStatus();
      setStatus(sts);
    } catch (e) {
      setError(String(e));
    } finally {
      setActionLoading(false);
    }
  };

  /* ===== API Key 管理 ===== */

  const handleAddApiKey = async () => {
    setActionLoading(true);
    setError(null);
    try {
      const newKey = await api.addApiKey();
      setConfig((prev) => ({ ...prev, apiKeys: [...prev.apiKeys, newKey] }));
    } catch (e) {
      setError(String(e));
    } finally {
      setActionLoading(false);
    }
  };

  const handleDeleteApiKey = async (index: number) => {
    setActionLoading(true);
    setError(null);
    try {
      await api.deleteApiKey(index);
      setConfig((prev) => ({
        ...prev,
        apiKeys: prev.apiKeys.filter((_, i) => i !== index),
      }));
    } catch (e) {
      setError(String(e));
    } finally {
      setActionLoading(false);
    }
  };

  const handleRegenerateApiKey = async (index: number) => {
    setActionLoading(true);
    setError(null);
    try {
      const newKey = await api.regenerateApiKey(index);
      setConfig((prev) => ({
        ...prev,
        apiKeys: prev.apiKeys.map((k, i) => (i === index ? newKey : k)),
      }));
    } catch (e) {
      setError(String(e));
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
  const statusById = new Map<string, ProviderRouteStatus>(
    (status?.providers ?? []).map((p) => [p.id, p]),
  );

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
            <>
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
              <div className="pref-section-desc" style={{ marginTop: 8 }}>
                开启后同时支持 Claude Code（/v1/messages）与 Codex（/v1/responses）两种接口请求。
              </div>
            </>
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

          {/* API Keys */}
          <ApiKeySection
            apiKeys={config.apiKeys}
            actionLoading={actionLoading}
            onAdd={handleAddApiKey}
            onDelete={handleDeleteApiKey}
            onRegenerate={handleRegenerateApiKey}
          />
        </div>

        {/* Provider selection */}
        <div className="pref-section" style={{ marginBottom: 16 }}>
          <div className="pref-section-title" style={{ display: "flex", alignItems: "center", gap: 8 }}>
            <Layers size={15} />
            供应商配置
            <span style={{ fontSize: "var(--text-xs)", color: "var(--seed-muted)", fontWeight: 400 }}>
              {providers.length > 0 ? `${providers.length} 个` : ""}
            </span>
            <button
              type="button"
              className="btn-icon-action"
              onClick={() => setProvidersExpanded((expanded) => !expanded)}
              data-tooltip={providersExpanded ? "收起供应商列表" : "展开供应商列表"}
              aria-label={providersExpanded ? "收起供应商列表" : "展开供应商列表"}
              aria-expanded={providersExpanded}
              style={{ marginLeft: "auto" }}
            >
              {providersExpanded ? <ChevronDown size={16} /> : <ChevronRight size={16} />}
            </button>
          </div>
          {providers.length === 0 ? (
            <div className="empty-state" style={{ minHeight: 120, padding: "24px 12px" }}>
              <Server size={32} />
              <div className="empty-state-text">暂无供应商</div>
              <div className="empty-state-subtext">
                请先在「AI 供应商」中添加供应商
              </div>
            </div>
          ) : providersExpanded ? (
            <>
              <div className="pref-section-desc" style={{ marginTop: 4, marginBottom: 8 }}>
                勾选参与聚合的供应商；勾选即使用该供应商配置的全部自定义模型。
              </div>
              <div style={{ display: "flex", flexDirection: "column", gap: 8, marginTop: 4 }}>
                {providers.map((p) => (
                  <ProviderSelectRow
                    key={p.id}
                    provider={p}
                    routeStatus={statusById.get(p.id)}
                    onToggle={(en) => handleToggleProvider(p.id, en)}
                    onResetCircuitBreaker={() => handleResetCircuitBreaker(p.id)}
                    actionLoading={actionLoading}
                  />
                ))}
              </div>
            </>
          ) : (
            <SelectedProviderModels models={selectedProviderModels} />
          )}
        </div>

        {/* Logs */}
        <LogsSection
          logs={logs}
          loading={logsLoading}
          error={logsError}
          autoRefresh={logAutoRefresh}
          onToggleAutoRefresh={() => setLogAutoRefresh((v) => !v)}
          onRefresh={loadLogs}
          onClear={async () => {
            try {
              await api.clearRouteLogs();
              setLogs([]);
            } catch (e) {
              setLogsError(String(e));
            }
          }}
          onSelect={setSelectedLog}
          logFilePath={logFilePath}
          onRevealFile={async () => {
            try {
              await api.revealRouteLogFile();
            } catch (e) {
              setLogsError(`打开日志文件失败: ${e}`);
            }
          }}
        />

        {/* Log detail modal */}
      <LogDetailModal entry={selectedLog} onClose={() => setSelectedLog(null)} />

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
            <p style={{ marginBottom: 4 }}>
              <strong>OpenCode / 其他 OpenAI 兼容客户端:</strong> 使用 baseURL{" "}
              <code style={{ background: "var(--seed-surface-alt)", padding: "2px 6px", borderRadius: 4, fontSize: "var(--text-xs)", color: "var(--seed-primary)" }}>
                {proxyUrl}/v1
              </code>
              （转发 <code style={{ fontSize: "var(--text-xs)" }}>/v1/responses</code> 请求到勾选的供应商，未勾选或不匹配则透传失败）
            </p>
            <p style={{ color: "var(--seed-muted)", marginTop: 8 }}>
              客户端请求会自动经路由聚合代理转发到已勾选的供应商，享受整流器伪装和自动故障转移能力。
            </p>
          </div>
        </div>
      </div>
    </>
  );
}

/* ===== Collapsed Provider Models ===== */

function SelectedProviderModels({ models }: { models: string[] }) {
  const [copiedModel, setCopiedModel] = useState<string | null>(null);

  if (models.length === 0) {
    return (
      <div style={{ fontSize: "var(--text-xs)", color: "var(--seed-muted)", marginTop: 8 }}>
        已选供应商暂无自定义模型
      </div>
    );
  }

  return (
    <div style={{ display: "flex", flexWrap: "wrap", gap: 6, marginTop: 8 }}>
      {models.map((model) => (
        <code
          key={model}
          onClick={() => {
            navigator.clipboard.writeText(model);
            setCopiedModel(model);
            setTimeout(() => setCopiedModel((current) => (current === model ? null : current)), 2000);
          }}
          data-tooltip={copiedModel === model ? "已复制" : "点击复制"}
          style={{
            fontSize: "var(--text-xs)",
            padding: "3px 8px",
            borderRadius: 4,
            background: "var(--seed-surface-alt)",
            border: "1px solid var(--seed-border)",
            color: "var(--seed-primary)",
            cursor: "pointer",
            whiteSpace: "nowrap",
          }}
        >
          {model}
        </code>
      ))}
    </div>
  );
}

/* ===== API Key Section ===== */

interface ApiKeySectionProps {
  apiKeys: string[];
  actionLoading: boolean;
  onAdd: () => void;
  onDelete: (index: number) => void;
  onRegenerate: (index: number) => void;
}

function maskKey(key: string): string {
  if (key.length <= 10) return key;
  return key.slice(0, 6) + "*".repeat(key.length - 10) + key.slice(-4);
}

function ApiKeySection({ apiKeys, actionLoading, onAdd, onDelete, onRegenerate }: ApiKeySectionProps) {
  const [copiedIndex, setCopiedIndex] = useState<number | null>(null);

  const handleCopy = async (index: number, key: string) => {
    await navigator.clipboard.writeText(key);
    setCopiedIndex(index);
    setTimeout(() => setCopiedIndex(null), 2000);
  };

  return (
    <div style={{ marginTop: 16 }}>
      <div style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 8 }}>
        <KeyRound size={15} style={{ color: "var(--seed-muted)" }} />
        <span style={{ fontSize: "var(--text-sm)", fontWeight: 500 }}>API Key</span>
        <span style={{ fontSize: "var(--text-xs)", color: "var(--seed-muted)" }}>
          {apiKeys.length > 0 ? `${apiKeys.length} 个` : ""}
        </span>
        <div style={{ flex: 1 }} />
        <button
          className="btn btn-secondary"
          onClick={onAdd}
          disabled={actionLoading}
          style={{ fontSize: "var(--text-xs)", padding: "4px 10px" }}
        >
          <Plus size={12} /> 生成新 Key
        </button>
      </div>
      {apiKeys.length === 0 ? (
        <div className="pref-section-desc">未配置 Key 时端点无需鉴权；点击下方按钮生成。</div>
      ) : (
        <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
          {apiKeys.map((key, index) => (
            <div
              key={`${index}-${key}`}
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
              <code
                style={{
                  flex: 1,
                  fontSize: "var(--text-sm)",
                  color: "var(--seed-primary)",
                  fontFamily: "monospace",
                  overflow: "hidden",
                  textOverflow: "ellipsis",
                  whiteSpace: "nowrap",
                }}
              >
                {maskKey(key)}
              </code>
              {index === 0 && (
                <span
                  style={{
                    fontSize: "var(--text-xs)",
                    fontWeight: 600,
                    padding: "2px 6px",
                    borderRadius: 4,
                    background: "var(--seed-surface-alt)",
                    color: "var(--seed-muted)",
                    border: "1px solid var(--seed-border)",
                    flexShrink: 0,
                  }}
                >
                  主 Key
                </span>
              )}
              <button
                className="btn-icon-action"
                onClick={() => handleCopy(index, key)}
                data-tooltip={copiedIndex === index ? "已复制" : "复制 API Key"}
                disabled={actionLoading}
              >
                {copiedIndex === index ? <Check size={14} /> : <Copy size={14} />}
              </button>
              <button
                className="btn-icon-action"
                onClick={() => onRegenerate(index)}
                data-tooltip="重新生成"
                disabled={actionLoading}
              >
                <RefreshCw size={14} />
              </button>
              {index > 0 ? (
                <button
                  className="btn-icon-action"
                  onClick={() => onDelete(index)}
                  data-tooltip="删除"
                  disabled={actionLoading}
                  style={{ color: "var(--seed-danger)" }}
                >
                  <Trash2 size={14} />
                </button>
              ) : (
                <span
                  className="btn-icon-action"
                  data-tooltip="主 Key 不能删除，只能重新生成"
                  style={{ opacity: 0.35, cursor: "not-allowed" }}
                >
                  <Trash2 size={14} />
                </span>
              )}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

/* ===== Provider Select Row ===== */

interface ProviderSelectRowProps {
  provider: AiProvider;
  routeStatus?: ProviderRouteStatus;
  onToggle: (enabled: boolean) => void;
  onResetCircuitBreaker: () => void;
  actionLoading: boolean;
}

function ProviderSelectRow({
  provider,
  routeStatus,
  onToggle,
  onResetCircuitBreaker,
  actionLoading,
}: ProviderSelectRowProps) {
  const [models, setModels] = useState<string[] | null>(null);
  const [modelsError, setModelsError] = useState<string | null>(null);
  const [modelsLoading, setModelsLoading] = useState(true);
  const [copiedModel, setCopiedModel] = useState<string | null>(null);

  // 路由聚合仅接受 Anthropic / OpenAI / Universal 三类 backend；其它类型
  // 不会进 pool，强制禁用勾选并隐藏下游相关 UI。
  const isRouteable = isRouteableProviderType(provider.providerType);
  // 无状态记录时视为默认勾选（与后端默认 enabled=true 对齐）
  const checked = routeStatus ? routeStatus.enabled : true;

  const loadModels = useCallback(async () => {
    setModelsLoading(true);
    setModelsError(null);
    try {
      const list = await api.getRouteProviderModels(provider.id);
      setModels(list);
    } catch (e) {
      setModelsError(String(e));
      setModels([]);
    } finally {
      setModelsLoading(false);
    }
  }, [provider.id]);

  useEffect(() => {
    // 不支持路由聚合的供应商不需要拉取模型列表（根本不会进 pool）。
    if (!isRouteable) return;
    loadModels();
  }, [loadModels, provider.customModels, provider.updatedAt, isRouteable]);

  const circuitClass =
    !routeStatus || routeStatus.circuitState === "closed"
      ? "connected"
      : routeStatus.circuitState === "open"
        ? "disconnected"
        : "checking";

  const circuitLabel =
    !routeStatus || routeStatus.circuitState === "closed"
      ? "正常"
      : routeStatus.circuitState === "open"
        ? "熔断"
        : "探测中";

  return (
    <div
      style={{
        padding: "14px 16px",
        background: "var(--seed-surface)",
        border: "1px solid var(--seed-border)",
        borderRadius: "var(--seed-radius-lg)",
        opacity: !isRouteable ? 0.55 : checked ? 1 : 0.6,
      }}
    >
      <div style={{ display: "flex", alignItems: "center", gap: 14 }}>
        {/* Enable checkbox — 协议不兼容的类型强制禁用，避免 toggle 落 DB 但 UI 永远显示勾选 */}
        <label
          className="ui-check"
          data-tooltip={!isRouteable ? UNSUPPORTED_ROUTE_PROVIDER_HINT : undefined}
        >
          <input
            className="ui-check-input"
            type="checkbox"
            checked={checked}
            onChange={(e) => {
              if (!isRouteable) return; // 防御：协议不兼容直接吞掉点击，不下发 toggle
              onToggle(e.target.checked);
            }}
            disabled={actionLoading || !isRouteable}
          />
          <span className="ui-check-box">
            <Check size={12} />
          </span>
        </label>

        {/* Provider info */}
        <div style={{ flex: 1, minWidth: 0 }}>
          <div style={{ fontSize: "var(--text-base)", fontWeight: 500, display: "flex", alignItems: "center", gap: 6, flexWrap: "wrap" }}>
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
            {!isRouteable && (
              <span
                data-tooltip={UNSUPPORTED_ROUTE_PROVIDER_HINT}
                style={{
                  fontSize: "var(--text-xs)",
                  fontWeight: 500,
                  padding: "2px 6px",
                  borderRadius: 4,
                  background: "color-mix(in srgb, var(--seed-danger) 10%, transparent)",
                  color: "var(--seed-danger)",
                  border: "1px solid color-mix(in srgb, var(--seed-danger) 30%, transparent)",
                  cursor: "help",
                }}
              >
                不支持路由聚合
              </span>
            )}
          </div>
          {!isRouteable && (
            <div style={{ fontSize: "var(--text-sm)", color: "var(--seed-muted)", marginTop: 2 }}>
              {UNSUPPORTED_ROUTE_PROVIDER_HINT}
            </div>
          )}
          {isRouteable && routeStatus && (
            <div style={{ fontSize: "var(--text-sm)", color: "var(--seed-muted)", display: "flex", gap: 12, marginTop: 2 }}>
              <span>请求 {routeStatus.requestCount}</span>
              <span>成功 {routeStatus.successCount}</span>
              {routeStatus.consecutiveFailures > 0 && (
                <span style={{ color: "var(--seed-danger)" }}>连续失败 {routeStatus.consecutiveFailures}</span>
              )}
              {routeStatus.lastError && (
                <span style={{ display: "inline-flex", alignItems: "center", gap: 3, color: "var(--seed-danger)" }}>
                  <Clock size={12} /> 最近错误
                </span>
              )}
            </div>
          )}
        </div>

        {/* Circuit breaker status — 仅对支持路由的供应商展示 */}
        {isRouteable && (
          <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
            <span className={`status-dot ${circuitClass}`} />
            <span style={{ fontSize: "var(--text-sm)", color: "var(--seed-muted)" }}>{circuitLabel}</span>
          </div>
        )}

        {/* Actions */}
        {isRouteable && routeStatus && (routeStatus.circuitState === "open" || routeStatus.circuitState === "half_open") && (
          <button
            className="btn btn-secondary"
            onClick={onResetCircuitBreaker}
            disabled={actionLoading}
            style={{ fontSize: "var(--text-xs)", padding: "4px 10px" }}
          >
            <RefreshCw size={12} /> 重置
          </button>
        )}
        {isRouteable && (
          <button
            className="btn-icon-action"
            onClick={loadModels}
            data-tooltip="刷新自定义模型列表"
            disabled={modelsLoading}
          >
            {modelsLoading ? (
              <Loader2 size={14} className="animate-spin" />
            ) : (
              <RefreshCw size={14} />
            )}
          </button>
        )}
      </div>

      {/* Model chips — horizontal layout with wrapping; 仅对支持路由的供应商展示 */}
      {isRouteable && (
      <div style={{ marginTop: 10, paddingLeft: 30 }}>
        {modelsLoading ? (
          <div style={{ fontSize: "var(--text-xs)", color: "var(--seed-muted)", display: "flex", alignItems: "center", gap: 6 }}>
            <Loader2 size={12} className="animate-spin" /> 正在读取自定义模型…
          </div>
        ) : modelsError ? (
          <div style={{ fontSize: "var(--text-xs)", color: "var(--seed-danger)" }}>
            自定义模型读取失败：{modelsError}
          </div>
        ) : !models || models.length === 0 ? (
          <div style={{ fontSize: "var(--text-xs)", color: "var(--seed-muted)" }}>
            暂无自定义模型。供应商对外可见模型仅来自「AI 供应商」中的自定义模型列表，请前往配置。
          </div>
        ) : (
          <div style={{ display: "flex", flexWrap: "wrap", gap: 6 }}>
            {models.map((m) => (
              <code
                key={m}
                onClick={() => {
                  navigator.clipboard.writeText(m);
                  setCopiedModel(m);
                  setTimeout(
                    () => setCopiedModel((cur) => (cur === m ? null : cur)),
                    2000,
                  );
                }}
                data-tooltip={copiedModel === m ? "已复制" : "点击复制"}
                style={{
                  fontSize: "var(--text-xs)",
                  padding: "3px 8px",
                  borderRadius: 4,
                  background: "var(--seed-surface-alt)",
                  border: "1px solid var(--seed-border)",
                  color: "var(--seed-primary)",
                  cursor: "pointer",
                  whiteSpace: "nowrap",
                }}
              >
                {m}
              </code>
            ))}
          </div>
        )}
      </div>
      )}
    </div>
  );
}

/* ===== Logs ( ===== Logs Section ===== */

interface LogsSectionProps {
  logs: RouteLogEntry[];
  loading: boolean;
  error: string | null;
  autoRefresh: boolean;
  onToggleAutoRefresh: () => void;
  onRefresh: () => void;
  onClear: () => void;
  onSelect: (entry: RouteLogEntry) => void;
  onRevealFile: () => void;
  logFilePath: string | null;
}

const PROTOCOL_LABEL: Record<InboundProtocol, string> = {
  claudeMessages: "Claude",
  codexResponses: "Codex",
  openaiModelsList: "Models",
};

const PROTOCOL_COLOR: Record<InboundProtocol, string> = {
  claudeMessages: "var(--seed-primary)",
  codexResponses: "#10a37f",
  openaiModelsList: "var(--seed-muted)",
};

function formatTimestamp(ms: number): string {
  const d = new Date(ms);
  const hh = String(d.getHours()).padStart(2, "0");
  const mm = String(d.getMinutes()).padStart(2, "0");
  const ss = String(d.getSeconds()).padStart(2, "0");
  const mmm = String(d.getMilliseconds()).padStart(3, "0");
  return `${hh}:${mm}:${ss}.${mmm}`;
}

function statusColor(status: number | null, success: boolean): string {
  if (!success || status === null) return "var(--seed-danger)";
  if (status >= 500) return "var(--seed-danger)";
  if (status >= 400) return "#d4a017";
  if (status >= 300) return "var(--seed-muted)";
  return "#1aaa55";
}

function LogsSection({
  logs,
  loading,
  error,
  autoRefresh,
  onToggleAutoRefresh,
  onRefresh,
  onClear,
  onSelect,
  onRevealFile,
  logFilePath,
}: LogsSectionProps) {
  // 倒序：最新的在最上面（后端返回 newest-last）
  const ordered = useMemo(() => [...logs].reverse(), [logs]);

  return (
    <div className="pref-section" style={{ marginBottom: 16 }}>
      <div
        className="pref-section-title"
        style={{ display: "flex", alignItems: "center", gap: 8 }}
      >
        <ScrollText size={15} />
        进出日志
        <span style={{ fontSize: "var(--text-xs)", color: "var(--seed-muted)", fontWeight: 400 }}>
          {logs.length > 0 ? `${logs.length} 条` : "暂无"}
        </span>
        <div style={{ flex: 1 }} />
        <label
          className="ui-check"
          style={{
            fontSize: "var(--text-xs)",
            color: "var(--seed-muted)",
            /* reset .ui-check 默认的 16px 上 margin：此处是 pref-section-title 的横向 flex 子项，
               保留该 margin 会让 "自动刷新" 视觉中心比左边的 "进出日志" 标题低约 8px */
            marginTop: 0,
          }}
        >
          <input
            className="ui-check-input"
            type="checkbox"
            checked={autoRefresh}
            onChange={onToggleAutoRefresh}
          />
          <span className="ui-check-box">
            <Check size={10} />
          </span>
          <span className="ui-check-label">自动刷新</span>
        </label>
        <button
          className="btn-icon-action"
          onClick={onRefresh}
          data-tooltip="手动刷新"
        >
          <RefreshCw size={14} className={loading ? "animate-spin" : ""} />
        </button>
        <button
          className="btn btn-secondary"
          onClick={onClear}
          disabled={logs.length === 0}
          style={{ fontSize: "var(--text-xs)", padding: "4px 10px" }}
        >
          <Trash2 size={12} /> 清空
        </button>
      </div>

      {logFilePath && (
        <div
          style={{
            marginTop: 8,
            display: "flex",
            alignItems: "center",
            gap: 8,
            padding: "6px 10px",
            background: "var(--seed-surface-alt)",
            border: "1px solid var(--seed-border)",
            borderRadius: "var(--seed-radius)",
            fontSize: "var(--text-xs)",
            color: "var(--seed-muted)",
          }}
        >
          <span style={{ flexShrink: 0 }}>本地日志:</span>
          <code
            style={{
              flex: 1,
              fontFamily: "monospace",
              color: "var(--seed-primary)",
              overflow: "hidden",
              textOverflow: "ellipsis",
              whiteSpace: "nowrap",
            }}
            title={logFilePath}
          >
            {logFilePath}
          </code>
          <button
            className="btn btn-secondary"
            onClick={onRevealFile}
            style={{ fontSize: "var(--text-xs)", padding: "2px 8px" }}
          >
            在 Finder 打开
          </button>
        </div>
      )}

      {error && (
        <div
          style={{
            display: "flex",
            alignItems: "center",
            gap: 6,
            padding: "8px 12px",
            background: "var(--seed-danger-bg)",
            borderRadius: "var(--seed-radius)",
            color: "var(--seed-danger)",
            fontSize: "var(--text-sm)",
            marginTop: 8,
          }}
        >
          <AlertCircle size={14} />
          <span>{error}</span>
        </div>
      )}

      <div
        style={{
          marginTop: 10,
          border: "1px solid var(--seed-border)",
          borderRadius: "var(--seed-radius)",
          background: "var(--seed-surface)",
          overflow: "hidden",
        }}
      >
        {ordered.length === 0 ? (
          <div className="empty-state" style={{ minHeight: 100, padding: "20px 12px" }}>
            <ScrollText size={28} />
            <div className="empty-state-text">暂无请求记录</div>
            <div className="empty-state-subtext">
              启动路由聚合后，客户端发起的请求会出现在这里
            </div>
          </div>
        ) : (
          <div style={{ maxHeight: 360, overflowY: "auto" }}>
            {ordered.map((entry) => {
              const protoColor = PROTOCOL_COLOR[entry.protocol] ?? "var(--seed-muted)";
              const protoLabel = PROTOCOL_LABEL[entry.protocol] ?? entry.protocol;
              const color = statusColor(entry.upstreamStatus, entry.success);
              return (
                <div
                  key={entry.id}
                  onClick={() => onSelect(entry)}
                  style={{
                    display: "grid",
                    gridTemplateColumns: "92px 160px minmax(160px, 1fr) auto auto auto",
                    alignItems: "center",
                    gap: 10,
                    padding: "8px 12px",
                    borderBottom: "1px solid var(--seed-border)",
                    cursor: "pointer",
                    fontSize: "var(--text-sm)",
                    background: "transparent",
                    transition: "background 0.1s ease",
                  }}
                  onMouseEnter={(e) => {
                    (e.currentTarget as HTMLElement).style.background =
                      "var(--seed-surface-alt)";
                  }}
                  onMouseLeave={(e) => {
                    (e.currentTarget as HTMLElement).style.background = "transparent";
                  }}
                >
                  <span style={{ fontFamily: "monospace", color: "var(--seed-muted)", fontSize: "var(--text-xs)", whiteSpace: "nowrap" }}>
                    {formatTimestamp(entry.timestampMs)}
                  </span>
                  <span
                    style={{
                      display: "inline-flex",
                      alignItems: "center",
                      gap: 4,
                      minWidth: 0,
                    }}
                  >
                    <span
                      style={{
                        fontSize: "var(--text-xs)",
                        fontWeight: 600,
                        color: protoColor,
                        letterSpacing: "0.02em",
                        whiteSpace: "nowrap",
                        overflow: "hidden",
                        textOverflow: "ellipsis",
                        minWidth: 0,
                      }}
                    >
                      {protoLabel}
                    </span>
                    {entry.stream ? (
                      <span
                        style={{
                          fontSize: "10px",
                          fontWeight: 600,
                          color: "var(--seed-muted)",
                          background: "var(--seed-surface-alt)",
                          padding: "0 4px",
                          borderRadius: 3,
                          border: "1px solid var(--seed-border)",
                          lineHeight: 1.4,
                          flexShrink: 0,
                        }}
                        title="Server-Sent Events"
                      >
                        SSE
                      </span>
                    ) : null}
                  </span>
                  <span
                    style={{
                      fontFamily: "monospace",
                      color: "var(--seed-primary)",
                      overflow: "hidden",
                      textOverflow: "ellipsis",
                      whiteSpace: "nowrap",
                      minWidth: 0,
                    }}
                    title={entry.inboundModel ?? undefined}
                  >
                    {entry.inboundModel ?? "(无 model)"}
                  </span>
                  <span
                    style={{
                      fontSize: "var(--text-xs)",
                      color,
                      fontWeight: 600,
                      minWidth: 32,
                      textAlign: "right",
                      fontFamily: "monospace",
                      flexShrink: 0,
                    }}
                  >
                    {entry.upstreamStatus != null
                      ? entry.upstreamStatus
                      : entry.error
                        ? "ERR"
                        : "—"}
                  </span>
                  <span
                    style={{
                      fontSize: "var(--text-xs)",
                      color: "var(--seed-muted)",
                      fontFamily: "monospace",
                      minWidth: 52,
                      textAlign: "right",
                      whiteSpace: "nowrap",
                      flexShrink: 0,
                    }}
                  >
                    {entry.durationMs} ms
                  </span>
                  <ChevronRight size={14} style={{ color: "var(--seed-muted)", flexShrink: 0 }} />
                </div>
              );
            })}
          </div>
        )}
      </div>
    </div>
  );
}

/* ===== Log Detail Modal ===== */

interface LogDetailModalProps {
  entry: RouteLogEntry | null;
  onClose: () => void;
}

function formatJson(value: unknown): string {
  try {
    return JSON.stringify(value, null, 2);
  } catch {
    return String(value);
  }
}

function headersToText(headers: Array<[string, string]>): string {
  return headers.map(([k, v]) => `${k}: ${v}`).join("\n");
}

function LogDetailModal({ entry, onClose }: LogDetailModalProps) {
  if (!entry) return null;

  const protoColor = PROTOCOL_COLOR[entry.protocol] ?? "var(--seed-muted)";
  const protoLabel = PROTOCOL_LABEL[entry.protocol] ?? entry.protocol;
  const statusColorHex = statusColor(entry.upstreamStatus, entry.success);
  const inboundJson = entry.inboundBody !== null ? formatJson(entry.inboundBody) : "(无 body)";
  const upstreamBodyText =
    entry.stream && !entry.upstreamBody
      ? "(流式响应，未记录 body)"
      : entry.upstreamBody != null && entry.upstreamBody !== ""
        ? entry.upstreamBody
        : entry.upstreamBody === ""
          ? "(空 body)"
          : "(无 body)";
  const inboundHeadersText = headersToText(entry.inboundHeaders);
  const upstreamHeadersText =
    entry.upstreamHeaders.length > 0
      ? headersToText(entry.upstreamHeaders)
      : "(无)";

  return (
    <div
      onClick={onClose}
      style={{
        position: "fixed",
        inset: 0,
        background: "color-mix(in srgb, black 55%, transparent)",
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        zIndex: 100,
        padding: 24,
      }}
    >
      <div
        onClick={(e) => e.stopPropagation()}
        style={{
          background: "var(--seed-bg)",
          borderRadius: "var(--seed-radius-lg)",
          width: "min(960px, 100%)",
          maxHeight: "90vh",
          display: "flex",
          flexDirection: "column",
          overflow: "hidden",
          border: "1px solid var(--seed-border)",
        }}
      >
        {/* Header */}
        <div
          style={{
            display: "flex",
            alignItems: "center",
            gap: 12,
            padding: "12px 16px",
            borderBottom: "1px solid var(--seed-border)",
            background: "var(--seed-surface)",
          }}
        >
          <span
            style={{
              fontSize: "var(--text-xs)",
              fontWeight: 600,
              color: protoColor,
              padding: "2px 8px",
              borderRadius: 4,
              border: `1px solid ${protoColor}`,
            }}
          >
            {protoLabel}
            {entry.stream ? " · SSE" : ""}
          </span>
          <span
            style={{
              fontFamily: "monospace",
              fontSize: "var(--text-sm)",
              color: "var(--seed-primary)",
            }}
          >
            {entry.inboundMethod} {entry.inboundPath}
          </span>
          <span
            style={{
              fontFamily: "monospace",
              fontSize: "var(--text-xs)",
              color: statusColorHex,
              fontWeight: 600,
            }}
          >
            {entry.upstreamStatus != null ? `→ ${entry.upstreamStatus}` : entry.error ? "ERROR" : ""}
          </span>
          <span style={{ flex: 1 }} />
          <span style={{ fontSize: "var(--text-xs)", color: "var(--seed-muted)", fontFamily: "monospace" }}>
            {formatTimestamp(entry.timestampMs)} · {entry.durationMs} ms
          </span>
          <button className="btn-icon-action" onClick={onClose} data-tooltip="关闭">
            <X size={14} />
          </button>
        </div>

        {/* Summary line */}
        <div
          style={{
            padding: "8px 16px",
            fontSize: "var(--text-xs)",
            color: "var(--seed-muted)",
            background: "var(--seed-surface-alt)",
            borderBottom: "1px solid var(--seed-border)",
            display: "flex",
            flexWrap: "wrap",
            gap: 14,
          }}
        >
          <span>
            模型:{" "}
            <code style={{ color: "var(--seed-primary)" }}>
              {entry.inboundModel ?? "(无)"}
            </code>
          </span>
          {entry.providerName && (
            <span>
              供应商:{" "}
              <code style={{ color: "var(--seed-primary)" }}>{entry.providerName}</code>
            </span>
          )}
          {entry.upstreamUrl && (
            <span>
              上游 URL:{" "}
              <code style={{ color: "var(--seed-primary)" }}>{entry.upstreamUrl}</code>
            </span>
          )}
          {entry.error && (
            <span style={{ color: "var(--seed-danger)" }}>错误: {entry.error}</span>
          )}
        </div>

        {/* Body */}
        <div
          style={{
            flex: 1,
            overflowY: "auto",
            padding: 16,
            display: "flex",
            flexDirection: "column",
            gap: 16,
          }}
        >
          <DetailSection title="入站请求头" copyText={inboundHeadersText}>
            <pre style={codeBlockStyle}>{inboundHeadersText}</pre>
          </DetailSection>
          <DetailSection
            title={`入站请求 body${entry.inboundBodyTruncated ? " (已截断)" : ""}`}
            copyText={inboundJson}
          >
            <pre style={codeBlockStyle}>{inboundJson}</pre>
          </DetailSection>
          <DetailSection title="上游响应头" copyText={upstreamHeadersText}>
            <pre style={codeBlockStyle}>{upstreamHeadersText}</pre>
          </DetailSection>
          <DetailSection
            title={`上游响应 body${entry.upstreamBodyTruncated ? " (已截断)" : ""}`}
            copyText={upstreamBodyText}
          >
            <pre style={codeBlockStyle}>{upstreamBodyText}</pre>
          </DetailSection>
        </div>
      </div>
    </div>
  );
}

interface DetailSectionProps {
  title: string;
  children: React.ReactNode;
  /** 复制图标点击时写入剪贴板的文本；不传则不渲染复制按钮 */
  copyText?: string;
}

function DetailSection({ title, children, copyText }: DetailSectionProps) {
  const [open, setOpen] = useState(true);
  const [copied, setCopied] = useState(false);

  const handleCopy = async (e: React.MouseEvent) => {
    // 复制按钮与折叠按钮在同一行，必须阻止冒泡，避免点复制时触发折叠
    e.stopPropagation();
    if (copyText === undefined) return;
    try {
      await navigator.clipboard.writeText(copyText);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch {
      /* 剪贴板权限/写失败时静默降级，不打断用户 */
    }
  };

  return (
    <div
      style={{
        border: "1px solid var(--seed-border)",
        borderRadius: "var(--seed-radius)",
        background: "var(--seed-surface)",
      }}
    >
      <div
        style={{
          display: "flex",
          alignItems: "center",
          gap: 6,
          padding: "6px 6px 6px 12px",
          borderBottom: open ? "1px solid var(--seed-border)" : "none",
        }}
      >
        <button
          type="button"
          onClick={() => setOpen((v) => !v)}
          style={{
            flex: 1,
            minWidth: 0,
            display: "flex",
            alignItems: "center",
            gap: 6,
            background: "transparent",
            border: "none",
            cursor: "pointer",
            color: "var(--seed-muted)",
            fontSize: "var(--text-sm)",
            fontWeight: 500,
            padding: 0,
            textAlign: "left",
          }}
        >
          {open ? <ChevronDown size={14} /> : <ChevronRight size={14} />}
          <span
            style={{
              overflow: "hidden",
              textOverflow: "ellipsis",
              whiteSpace: "nowrap",
            }}
          >
            {title}
          </span>
        </button>
        {copyText !== undefined && (
          <button
            type="button"
            className="btn-icon-action"
            onClick={handleCopy}
            data-tooltip={copied ? "已复制" : "复制内容"}
            aria-label="复制内容"
            style={{ width: 26, height: 26, flexShrink: 0 }}
          >
            {copied ? <Check size={12} /> : <Copy size={12} />}
          </button>
        )}
      </div>
      {open && <div style={{ padding: 12 }}>{children}</div>}
    </div>
  );
}

const codeBlockStyle: React.CSSProperties = {
  margin: 0,
  padding: 12,
  background: "var(--seed-surface-alt)",
  borderRadius: "var(--seed-radius)",
  color: "var(--seed-primary)",
  fontSize: "var(--text-xs)",
  fontFamily: "monospace",
  whiteSpace: "pre-wrap",
  wordBreak: "break-all",
  maxHeight: 280,
  overflowY: "auto",
};
