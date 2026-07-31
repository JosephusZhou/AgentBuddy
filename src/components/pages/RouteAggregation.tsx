import { useCallback, useEffect, useState } from "react";
import {
  AlertCircle,
  Check,
  Clock,
  Copy,
  Loader2,
  Power,
  RefreshCw,
  Server,
  Settings2,
  Zap,
} from "lucide-react";
import * as api from "./route-aggregation/api";
import type {
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
      const [cfg, sts] = await Promise.all([api.getConfig(), api.getStatus()]);
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
              <input
                className="form-input"
                type="text"
                value={config.listenAddress}
                onChange={(e) => setConfig({ ...config, listenAddress: e.target.value })}
                onBlur={() => handleConfigUpdate(config)}
                disabled={actionLoading}
              />
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
          onToggle={(enabled) => handleToggleGroup("claude_code", enabled)}
          onToggleProvider={handleToggleProvider}
          onResetCircuitBreaker={handleResetCircuitBreaker}
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
          onToggle={(enabled) => handleToggleGroup("codex", enabled)}
          onToggleProvider={handleToggleProvider}
          onResetCircuitBreaker={handleResetCircuitBreaker}
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

interface RouteGroupPanelProps {
  title: string;
  group: RouteGroup;
  enabled: boolean;
  status?: { enabled: boolean; activeProviders: ProviderRouteStatus[]; totalProviders: number };
  proxyUrl: string;
  version: string;
  onToggle: (enabled: boolean) => void;
  onToggleProvider: (providerId: string, group: RouteGroup, enabled: boolean) => void;
  onResetCircuitBreaker: (providerId: string, group: RouteGroup) => void;
  actionLoading: boolean;
}

function RouteGroupPanel({
  title,
  group,
  enabled,
  status,
  proxyUrl,
  version,
  onToggle,
  onToggleProvider,
  onResetCircuitBreaker,
  actionLoading,
}: RouteGroupPanelProps) {
  const providers = status?.activeProviders ?? [];

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

          {/* Provider list */}
          <div style={{ display: "flex", flexDirection: "column", gap: 8, marginTop: 12 }}>
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
                  onToggle={(enabled) => onToggleProvider(p.id, group, enabled)}
                  onResetCircuitBreaker={() => onResetCircuitBreaker(p.id, group)}
                  actionLoading={actionLoading}
                />
              ))
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
