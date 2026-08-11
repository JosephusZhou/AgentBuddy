import { useCallback, useEffect, useState } from "react";
import {
  AlertCircle,
  Check,
  Clock,
  Copy,
  KeyRound,
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
  ProviderRouteStatus,
  RouteAggregationConfig,
  RouteAggregationStatus,
} from "./route-aggregation/types";
import { DEFAULT_CONFIG } from "./route-aggregation/types";
import { invokeList } from "./ai-providers/api";
import type { AiProvider } from "./ai-providers/types";

export default function RouteAggregation() {
  const [config, setConfig] = useState<RouteAggregationConfig>(DEFAULT_CONFIG);
  const [status, setStatus] = useState<RouteAggregationStatus | null>(null);
  const [providers, setProviders] = useState<AiProvider[]>([]);
  const [loading, setLoading] = useState(true);
  const [actionLoading, setActionLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

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
            提供商配置
            <span style={{ fontSize: "var(--text-xs)", color: "var(--seed-muted)", fontWeight: 400 }}>
              {providers.length > 0 ? `${providers.length} 个` : ""}
            </span>
          </div>
          <div className="pref-section-desc" style={{ marginTop: 4, marginBottom: 8 }}>
            勾选参与聚合的供应商；勾选即使用该供应商的全部模型（自定义模型优先，未配置时使用远程模型列表）。
          </div>
          {providers.length === 0 ? (
            <div className="empty-state" style={{ minHeight: 120, padding: "24px 12px" }}>
              <Server size={32} />
              <div className="empty-state-text">暂无供应商</div>
              <div className="empty-state-subtext">
                请先在「AI 供应商」中添加供应商
              </div>
            </div>
          ) : (
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
          )}
        </div>

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
              客户端请求会自动经路由聚合代理转发到已勾选的供应商，享受整流器伪装和自动故障转移能力。
            </p>
          </div>
        </div>
      </div>
    </>
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
    loadModels();
  }, [loadModels, provider.customModels, provider.updatedAt]);

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
        opacity: checked ? 1 : 0.6,
      }}
    >
      <div style={{ display: "flex", alignItems: "center", gap: 14 }}>
        {/* Enable checkbox */}
        <label className="ui-check">
          <input
            className="ui-check-input"
            type="checkbox"
            checked={checked}
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
          {routeStatus && (
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

        {/* Circuit breaker status */}
        <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
          <span className={`status-dot ${circuitClass}`} />
          <span style={{ fontSize: "var(--text-sm)", color: "var(--seed-muted)" }}>{circuitLabel}</span>
        </div>

        {/* Actions */}
        {routeStatus && (routeStatus.circuitState === "open" || routeStatus.circuitState === "half_open") && (
          <button
            className="btn btn-secondary"
            onClick={onResetCircuitBreaker}
            disabled={actionLoading}
            style={{ fontSize: "var(--text-xs)", padding: "4px 10px" }}
          >
            <RefreshCw size={12} /> 重置
          </button>
        )}
        <button
          className="btn-icon-action"
          onClick={loadModels}
          data-tooltip="刷新模型列表"
          disabled={modelsLoading}
        >
          {modelsLoading ? (
            <Loader2 size={14} className="animate-spin" />
          ) : (
            <RefreshCw size={14} />
          )}
        </button>
      </div>

      {/* Model chips — horizontal layout with wrapping */}
      <div style={{ marginTop: 10, paddingLeft: 30 }}>
        {modelsLoading ? (
          <div style={{ fontSize: "var(--text-xs)", color: "var(--seed-muted)", display: "flex", alignItems: "center", gap: 6 }}>
            <Loader2 size={12} className="animate-spin" /> 正在加载模型列表…
          </div>
        ) : modelsError ? (
          <div style={{ fontSize: "var(--text-xs)", color: "var(--seed-danger)" }}>
            模型列表加载失败：{modelsError}
          </div>
        ) : !models || models.length === 0 ? (
          <div style={{ fontSize: "var(--text-xs)", color: "var(--seed-muted)" }}>
            暂无模型（请在「AI 供应商」中配置自定义模型，或确认端点支持远程拉取）
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
    </div>
  );
}
