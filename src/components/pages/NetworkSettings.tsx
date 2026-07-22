import { useCallback, useEffect, useRef, useState } from "react";
import { useStatusMessage } from "@/lib/useStatusMessage";
import { Toast } from "@/components/Toast";

/* ===== Types (mirror Rust NetworkSettings / ProxySettings) ===== */

export type ProxyMode = "none" | "system" | "custom";
export type ProxyProtocol = "http" | "socks5";

export interface ProxySettings {
  mode: ProxyMode;
  protocol: ProxyProtocol;
  host: string;
  port: number;
  username: string;
  password: string;
}

export interface NetworkSettingsDto {
  proxy: ProxySettings;
}

const DEFAULT_PROXY: ProxySettings = {
  mode: "none",
  protocol: "http",
  host: "",
  port: 0,
  username: "",
  password: "",
};

const MODE_OPTIONS: {
  value: ProxyMode;
  label: string;
  desc: string;
}[] = [
  {
    value: "none",
    label: "无代理",
    desc: "直接连接，忽略环境变量与系统代理",
  },
  {
    value: "system",
    label: "系统代理",
    desc: "优先使用环境变量，其次读取 macOS 系统代理设置",
  },
  {
    value: "custom",
    label: "自定义代理",
    desc: "手动指定 HTTP 或 SOCKS5 代理服务器",
  },
];

const DEBOUNCE_MS = 400;

/* ===== Invoke ===== */

async function invokeGetNetwork(): Promise<NetworkSettingsDto> {
  const { invoke } = await import("@tauri-apps/api/core");
  const raw = await (invoke("get_network_settings") as Promise<{
    proxy?: Partial<ProxySettings> & { mode?: string; protocol?: string };
  }>);
  return normalizeDto(raw);
}

async function invokeUpdateNetwork(
  settings: NetworkSettingsDto,
): Promise<NetworkSettingsDto> {
  const { invoke } = await import("@tauri-apps/api/core");
  const raw = await (invoke("update_network_settings", { settings }) as Promise<{
    proxy?: Partial<ProxySettings> & { mode?: string; protocol?: string };
  }>);
  return normalizeDto(raw);
}

function normalizeMode(value: unknown): ProxyMode {
  if (value === "system" || value === "custom" || value === "none") return value;
  return "none";
}

function normalizeProtocol(value: unknown): ProxyProtocol {
  if (value === "socks5" || value === "http") return value;
  return "http";
}

function normalizeDto(raw: {
  proxy?: Partial<ProxySettings> & { mode?: string; protocol?: string };
} | null | undefined): NetworkSettingsDto {
  const p = raw?.proxy;
  const portRaw = typeof p?.port === "number" ? p.port : Number(p?.port);
  const port =
    Number.isFinite(portRaw) && portRaw > 0 && portRaw <= 65535
      ? Math.floor(portRaw)
      : 0;
  return {
    proxy: {
      mode: normalizeMode(p?.mode),
      protocol: normalizeProtocol(p?.protocol),
      host: typeof p?.host === "string" ? p.host : "",
      port,
      username: typeof p?.username === "string" ? p.username : "",
      password: typeof p?.password === "string" ? p.password : "",
    },
  };
}

function parsePort(portText: string): number {
  const portParsed = portText.trim() === "" ? 0 : Number(portText.trim());
  return Number.isFinite(portParsed) && portParsed > 0 && portParsed <= 65535
    ? Math.floor(portParsed)
    : 0;
}

/** Frontend validation. Returns error message or null if ok to persist. */
function validateForPersist(proxy: ProxySettings, portText: string): string | null {
  if (proxy.mode !== "custom") return null;
  if (!proxy.host) return "自定义代理需要填写主机地址";
  if (
    proxy.host.includes("://") ||
    proxy.host.includes("/") ||
    proxy.host.includes("@") ||
    /\s/.test(proxy.host)
  ) {
    return "主机地址只需填写域名或 IP，不要包含协议、路径或空格";
  }
  if (portText.trim() !== "" && !/^\d+$/.test(portText.trim())) {
    return "端口必须是 1–65535 的整数";
  }
  if (proxy.port === 0) return "请填写有效的代理端口（1–65535）";
  return null;
}

function proxyEqual(a: ProxySettings, b: ProxySettings): boolean {
  return (
    a.mode === b.mode &&
    a.protocol === b.protocol &&
    a.host === b.host &&
    a.port === b.port &&
    a.username === b.username &&
    a.password === b.password
  );
}

/* ===== Component ===== */

export default function NetworkSettings() {
  const [loaded, setLoaded] = useState(false);
  const [mode, setMode] = useState<ProxyMode>("none");
  const [protocol, setProtocol] = useState<ProxyProtocol>("http");
  const [host, setHost] = useState("");
  const [portText, setPortText] = useState("");
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  /** Last successfully persisted config (source of truth for equality / rollback UI). */
  const [savedSnapshot, setSavedSnapshot] = useState<ProxySettings>(DEFAULT_PROXY);
  const [formError, setFormError] = useState("");
  const [statusMsg, setStatusMsg] = useStatusMessage();

  const hasLoaded = useRef(false);
  const saveSeq = useRef(0);
  const debounceTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  // Latest field values for debounced persist (avoids stale closures).
  const draftRef = useRef({
    mode: "none" as ProxyMode,
    protocol: "http" as ProxyProtocol,
    host: "",
    portText: "",
    username: "",
    password: "",
  });

  const applyDto = useCallback((dto: NetworkSettingsDto) => {
    const p = dto.proxy;
    setMode(p.mode);
    setProtocol(p.protocol);
    setHost(p.host);
    setPortText(p.port > 0 ? String(p.port) : "");
    setUsername(p.username);
    setPassword(p.password);
    setSavedSnapshot(p);
    setFormError("");
    draftRef.current = {
      mode: p.mode,
      protocol: p.protocol,
      host: p.host,
      portText: p.port > 0 ? String(p.port) : "",
      username: p.username,
      password: p.password,
    };
  }, []);

  useEffect(() => {
    if (hasLoaded.current) return;
    hasLoaded.current = true;
    (async () => {
      try {
        const dto = await invokeGetNetwork();
        applyDto(dto);
      } catch (err) {
        setStatusMsg(
          `加载网络设置失败：${err instanceof Error ? err.message : String(err)}`,
        );
        applyDto({ proxy: DEFAULT_PROXY });
      } finally {
        setLoaded(true);
      }
    })();
  }, [applyDto, setStatusMsg]);

  useEffect(() => {
    return () => {
      if (debounceTimer.current) clearTimeout(debounceTimer.current);
    };
  }, []);

  const buildProxyFromDraft = useCallback((): ProxySettings => {
    const d = draftRef.current;
    return {
      mode: d.mode,
      protocol: d.protocol,
      host: d.host.trim(),
      port: parsePort(d.portText),
      username: d.username.trim(),
      password: d.password,
    };
  }, []);

  const persistNow = useCallback(
    async (proxy: ProxySettings, portTextForValidation: string) => {
      const err = validateForPersist(proxy, portTextForValidation);
      if (err) {
        setFormError(err);
        return;
      }
      if (proxyEqual(proxy, savedSnapshot)) {
        setFormError("");
        return;
      }

      const seq = ++saveSeq.current;
      setFormError("");
      try {
        const saved = await invokeUpdateNetwork({ proxy });
        // Ignore stale responses if a newer save started.
        if (seq !== saveSeq.current) return;
        setSavedSnapshot(saved.proxy);
        // Don't clobber in-progress typing: only sync snapshot.
      } catch (e) {
        if (seq !== saveSeq.current) return;
        setFormError(e instanceof Error ? e.message : String(e));
      }
    },
    [savedSnapshot],
  );

  const schedulePersist = useCallback(() => {
    if (debounceTimer.current) clearTimeout(debounceTimer.current);
    debounceTimer.current = setTimeout(() => {
      debounceTimer.current = null;
      const proxy = buildProxyFromDraft();
      void persistNow(proxy, draftRef.current.portText);
    }, DEBOUNCE_MS);
  }, [buildProxyFromDraft, persistNow]);

  const persistImmediate = useCallback(() => {
    if (debounceTimer.current) {
      clearTimeout(debounceTimer.current);
      debounceTimer.current = null;
    }
    const proxy = buildProxyFromDraft();
    void persistNow(proxy, draftRef.current.portText);
  }, [buildProxyFromDraft, persistNow]);

  const selectMode = useCallback(
    (next: ProxyMode) => {
      setMode(next);
      draftRef.current.mode = next;
      setFormError("");
      // none / system always valid → write immediately.
      // custom: only write when fields already valid (else wait for input debounce).
      if (next === "none" || next === "system") {
        persistImmediate();
        return;
      }
      const proxy = buildProxyFromDraft();
      const err = validateForPersist(proxy, draftRef.current.portText);
      if (err) {
        setFormError(err);
        return;
      }
      persistImmediate();
    },
    [buildProxyFromDraft, persistImmediate],
  );

  const selectProtocol = useCallback(
    (next: ProxyProtocol) => {
      setProtocol(next);
      draftRef.current.protocol = next;
      if (draftRef.current.mode === "custom") {
        persistImmediate();
      }
    },
    [persistImmediate],
  );

  return (
    <>
      <div className="content-header">
        <h1 className="content-title">网络设置</h1>
      </div>
      <div className="content-body">
        <Toast message={statusMsg} />

        {!loaded ? (
          <div className="empty-state">
            <div className="empty-state-text">加载中…</div>
          </div>
        ) : (
          <div className="pref-section">
            <div className="pref-section-title">网络代理</div>
            <div className="pref-section-desc">
              控制应用内出站请求（WebDAV、Skills 更新、OpenCode 模型目录、MCP
              连通性测试等）使用的代理。更改后立即生效。
            </div>

            <div className="pref-options">
              {MODE_OPTIONS.map((opt) => (
                <button
                  key={opt.value}
                  type="button"
                  className={`pref-option ${mode === opt.value ? "selected" : ""}`}
                  onClick={() => selectMode(opt.value)}
                >
                  <div className="pref-radio">
                    <div className="pref-radio-dot" />
                  </div>
                  <div className="pref-option-content">
                    <div className="pref-option-label">{opt.label}</div>
                    <div className="pref-option-desc">{opt.desc}</div>
                  </div>
                </button>
              ))}
            </div>

            {mode === "custom" && (
              <div className="net-custom-fields">
                <div className="form-group">
                  <label className="form-label" htmlFor="net-protocol">
                    协议
                  </label>
                  <div className="net-protocol-row">
                    {(
                      [
                        { value: "http", label: "HTTP" },
                        { value: "socks5", label: "SOCKS5" },
                      ] as const
                    ).map((opt) => (
                      <button
                        key={opt.value}
                        type="button"
                        className={`net-protocol-chip ${
                          protocol === opt.value ? "selected" : ""
                        }`}
                        onClick={() => selectProtocol(opt.value)}
                      >
                        {opt.label}
                      </button>
                    ))}
                  </div>
                </div>

                <div className="net-host-port-row">
                  <div className="form-group net-host-field">
                    <label className="form-label" htmlFor="net-host">
                      主机
                    </label>
                    <input
                      id="net-host"
                      type="text"
                      className="form-input"
                      placeholder="例如 127.0.0.1 或 proxy.example.com"
                      value={host}
                      onChange={(e) => {
                        const v = e.target.value;
                        setHost(v);
                        draftRef.current.host = v;
                        schedulePersist();
                      }}
                      autoComplete="off"
                      spellCheck={false}
                    />
                  </div>
                  <div className="form-group net-port-field">
                    <label className="form-label" htmlFor="net-port">
                      端口
                    </label>
                    <input
                      id="net-port"
                      type="text"
                      inputMode="numeric"
                      className="form-input"
                      placeholder="7890"
                      value={portText}
                      onChange={(e) => {
                        const v = e.target.value;
                        if (v === "" || /^\d{0,5}$/.test(v)) {
                          setPortText(v);
                          draftRef.current.portText = v;
                          schedulePersist();
                        }
                      }}
                      autoComplete="off"
                    />
                  </div>
                </div>

                <div className="net-host-port-row">
                  <div className="form-group net-host-field">
                    <label className="form-label" htmlFor="net-username">
                      用户名
                      <span className="form-label-optional">可选</span>
                    </label>
                    <input
                      id="net-username"
                      type="text"
                      className="form-input"
                      placeholder="如无需认证可留空"
                      value={username}
                      onChange={(e) => {
                        const v = e.target.value;
                        setUsername(v);
                        draftRef.current.username = v;
                        schedulePersist();
                      }}
                      autoComplete="username"
                    />
                  </div>
                  <div className="form-group net-host-field">
                    <label className="form-label" htmlFor="net-password">
                      密码
                      <span className="form-label-optional">可选</span>
                    </label>
                    <input
                      id="net-password"
                      type="password"
                      className="form-input"
                      placeholder="如无需认证可留空"
                      value={password}
                      onChange={(e) => {
                        const v = e.target.value;
                        setPassword(v);
                        draftRef.current.password = v;
                        schedulePersist();
                      }}
                      autoComplete="new-password"
                    />
                  </div>
                </div>
              </div>
            )}

            {formError && <div className="mcp-form-error">{formError}</div>}
          </div>
        )}
      </div>
    </>
  );
}
