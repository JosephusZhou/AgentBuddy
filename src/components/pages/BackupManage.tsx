import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { CheckGlyph } from "@/components/ui";
import { Toast } from "@/components/Toast";
import { useStatusMessage } from "@/lib/useStatusMessage";
import {
  collectAvailableIds,
  formatBytes,
  getBackupSettings,
  listBackupUnits,
  listRemoteBackups,
  listWebDavConnections,
  restoreRemoteBackup,
  runBackupUpload,
  updateBackupSettings,
} from "./backup-manage/api";
import type {
  BackupProgressEvent,
  BackupRunResult,
  BackupSettings,
  BackupUnitNode,
  RemoteBackupItem,
  RestoreBackupResult,
  WebDAVConnectionLite,
} from "./backup-manage/types";
import { ChevronDown, Search, Upload } from "lucide-react";

const PHASE_LABEL: Record<string, string> = {
  collect: "收集",
  zip: "打包",
  encrypt: "加密",
  upload: "上传",
  download: "下载",
  decrypt: "解密",
  restore: "还原",
  finalize: "收尾",
};

function phaseLabel(phase: string): string {
  return PHASE_LABEL[phase] ?? phase;
}

type AppSelectOption = {
  value: string;
  label: string;
  sub?: string;
};

/** 与 ClaudeEnv / OpenCode / Skills 共用的 app-select，替代原生 select。 */
function AppSelect({
  id,
  labelId,
  value,
  options,
  onChange,
  disabled = false,
  placeholder = "请选择",
  emptyText = "暂无选项",
}: {
  id?: string;
  labelId?: string;
  value: string;
  options: AppSelectOption[];
  onChange: (value: string) => void;
  disabled?: boolean;
  placeholder?: string;
  emptyText?: string;
}) {
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);
  const selected = options.find((o) => o.value === value) ?? null;

  useEffect(() => {
    if (!open) return;
    const onPointerDown = (e: MouseEvent) => {
      if (rootRef.current && !rootRef.current.contains(e.target as Node)) {
        setOpen(false);
      }
    };
    document.addEventListener("mousedown", onPointerDown);
    return () => document.removeEventListener("mousedown", onPointerDown);
  }, [open]);

  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      e.preventDefault();
      e.stopPropagation();
      setOpen(false);
    };
    document.addEventListener("keydown", onKey, true);
    return () => document.removeEventListener("keydown", onKey, true);
  }, [open]);

  useEffect(() => {
    if (disabled) setOpen(false);
  }, [disabled]);

  return (
    <div
      className={`app-select ${open ? "open" : ""} ${disabled ? "disabled" : ""}`}
      ref={rootRef}
    >
      <button
        type="button"
        id={id}
        className="app-select-trigger form-input"
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-labelledby={labelId}
        disabled={disabled}
        onClick={() => {
          if (!disabled) setOpen((v) => !v);
        }}
      >
        <span className={`app-select-value ${selected ? "" : "placeholder"}`}>
          {selected?.label ?? placeholder}
        </span>
        <span className="app-select-chevron" aria-hidden>
          <IconSelectChevron open={open} />
        </span>
      </button>
      {open && (
        <div className="app-select-menu" role="listbox" aria-labelledby={labelId}>
          {options.length === 0 ? (
            <div className="app-select-empty">{emptyText}</div>
          ) : (
            options.map((o) => {
              const isSelected = o.value === value;
              return (
                <button
                  key={o.value === "" ? "__empty__" : o.value}
                  type="button"
                  role="option"
                  aria-selected={isSelected}
                  className={`app-select-option ${isSelected ? "selected" : ""}`}
                  disabled={disabled}
                  onClick={() => {
                    onChange(o.value);
                    setOpen(false);
                  }}
                >
                  <span className="app-select-option-title">{o.label}</span>
                  {o.sub ? (
                    <span className="app-select-option-sub">{o.sub}</span>
                  ) : null}
                </button>
              );
            })
          )}
        </div>
      )}
    </div>
  );
}

/* ===== Header icons（与 Agent/MCP 页 action-btn 一致） ===== */
const IconSearch = () => (
  <Search strokeWidth={1.8} />
);

/** 上传到云端 / 开始备份 */
const IconUpload = () => (
  <Upload strokeWidth={1.8} />
);

/** app-select 下拉箭头：open 时朝上 */
const IconSelectChevron = ({ open }: { open: boolean }) => (
  <ChevronDown size={16} strokeWidth={1.8} style={{ transform: open ? "rotate(180deg)" : "rotate(0deg)", transition: "transform 0.15s ease" }} />
);

/** 折叠箭头：open=true 朝下（展开），false 朝右（收起） */
const IconChevron = ({ open }: { open: boolean }) => (
  <ChevronDown strokeWidth={2} style={{ transform: open ? "rotate(0deg)" : "rotate(-90deg)", transition: "transform 0.15s ease" }} />
);

function collectDefaultIds(nodes: BackupUnitNode[]): Set<string> {
  const set = new Set<string>();
  const walk = (list: BackupUnitNode[]) => {
    for (const n of list) {
      const kids = n.children ?? [];
      if (kids.length > 0) {
        walk(kids);
      } else if (n.available && n.selectedByDefault) {
        set.add(n.id);
      }
    }
  };
  walk(nodes);
  return set;
}

function selectedContainsSecrets(
  nodes: BackupUnitNode[],
  selected: Set<string>,
): boolean {
  let found = false;
  const walk = (list: BackupUnitNode[]) => {
    for (const n of list) {
      const kids = n.children ?? [];
      if (kids.length > 0) {
        // parent id selected → all available children
        if (selected.has(n.id)) {
          if (n.containsSecrets) found = true;
        }
        walk(kids);
      } else if (selected.has(n.id) && n.containsSecrets) {
        found = true;
      }
    }
  };
  walk(nodes);
  return found;
}

function UnitTree({
  nodes,
  selected,
  onToggle,
  depth = 0,
}: {
  nodes: BackupUnitNode[];
  selected: Set<string>;
  onToggle: (id: string, node: BackupUnitNode, checked: boolean) => void;
  depth?: number;
}) {
  return (
    <ul className={`backup-unit-list depth-${Math.min(depth, 3)}`}>
      {nodes.map((node) => {
        const kids = node.children ?? [];
        const hasKids = kids.length > 0;
        const childIds = hasKids
          ? collectAvailableIds([node])
          : node.available
            ? [node.id]
            : [];
        const selectedCount = childIds.filter((id) => selected.has(id)).length;
        const allSelected =
          childIds.length > 0 && selectedCount === childIds.length;
        const someSelected =
          selectedCount > 0 && selectedCount < childIds.length;
        const checked = hasKids ? allSelected : selected.has(node.id);
        const disabled = hasKids
          ? childIds.length === 0
          : !node.available;

        return (
          <li key={node.id} className="backup-unit-item">
            <label
              className={`ui-check backup-unit-check ${disabled ? "is-disabled" : ""}`}
            >
              <input
                type="checkbox"
                className="ui-check-input"
                checked={checked}
                disabled={disabled}
                ref={(el) => {
                  if (el) el.indeterminate = hasKids && someSelected;
                }}
                onChange={(e) => onToggle(node.id, node, e.target.checked)}
              />
              <CheckGlyph />
              <span className="backup-unit-body">
                <span className="backup-unit-label-row">
                  <span className="backup-unit-label">{node.label}</span>
                  {node.containsSecrets && (
                    <span className="backup-badge backup-badge-secret">含密钥</span>
                  )}
                  {!node.available && (
                    <span className="backup-badge">未检测到</span>
                  )}
                  {node.estimatedBytes > 0 && (
                    <span className="backup-unit-size">
                      {formatBytes(node.estimatedBytes)}
                    </span>
                  )}
                </span>
                {node.pathSummary && (
                  <span className="backup-unit-path">{node.pathSummary}</span>
                )}
                {node.warnings?.length > 0 && (
                  <span className="backup-unit-warn">
                    {node.warnings.join("；")}
                  </span>
                )}
              </span>
            </label>
            {hasKids && (
              <UnitTree
                nodes={kids}
                selected={selected}
                onToggle={onToggle}
                depth={depth + 1}
              />
            )}
          </li>
        );
      })}
    </ul>
  );
}

export default function BackupManage() {
  const [statusMsg, setStatusMsg] = useStatusMessage(5000);
  const [units, setUnits] = useState<BackupUnitNode[]>([]);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [connections, setConnections] = useState<WebDAVConnectionLite[]>([]);
  const [selectedDav, setSelectedDav] = useState<Set<string>>(new Set());
  const [settings, setSettings] = useState<BackupSettings>({
    cliproxyapiConfPath: "",
    sub2apiRootPath: "",
    defaultRemoteDir: "AgentBuddy",
  });
  /** 本次备份的上传目录（相对 WebDAV 根）；空则回退 AgentBuddy */
  const [uploadDir, setUploadDir] = useState("AgentBuddy");
  const [passphrase, setPassphrase] = useState("");
  const [passphrase2, setPassphrase2] = useState("");
  const [ackPlaintext, setAckPlaintext] = useState(false);
  const [loading, setLoading] = useState(true);
  const [running, setRunning] = useState(false);
  const [result, setResult] = useState<BackupRunResult | null>(null);
  const [progress, setProgress] = useState<BackupProgressEvent | null>(null);
  const [showAdvanced, setShowAdvanced] = useState(false);
  const [settingsDirty, setSettingsDirty] = useState(false);
  /** 备份内容区折叠：默认展开 */
  const [unitsExpanded, setUnitsExpanded] = useState(true);

  // Restore
  const [restoreConnId, setRestoreConnId] = useState("");
  const [remoteItems, setRemoteItems] = useState<RemoteBackupItem[]>([]);
  const [remoteLoading, setRemoteLoading] = useState(false);
  const [selectedRemote, setSelectedRemote] = useState<string>("");
  const [restorePassphrase, setRestorePassphrase] = useState("");
  const [restoring, setRestoring] = useState(false);
  const [restoreResult, setRestoreResult] = useState<RestoreBackupResult | null>(null);

  const reload = useCallback(async () => {
    setLoading(true);
    try {
      const [u, s, dav] = await Promise.all([
        listBackupUnits(),
        getBackupSettings(),
        listWebDavConnections(),
      ]);
      setUnits(u);
      setSettings(s);
      setUploadDir(s.defaultRemoteDir?.trim() || "AgentBuddy");
      setConnections(dav);
      setSelected(collectDefaultIds(u));
      setSelectedDav(new Set(dav.map((d) => d.id)));
      setSettingsDirty(false);
      if (dav.length > 0) {
        setRestoreConnId((prev) => prev || dav[0].id);
      }
    } catch (e) {
      setStatusMsg(
        `加载失败：${e instanceof Error ? e.message : String(e ?? "未知错误")}`,
      );
    } finally {
      setLoading(false);
    }
  }, [setStatusMsg]);

  useEffect(() => {
    void reload();
  }, [reload]);

  const availableLeafIds = useMemo(
    () => collectAvailableIds(units),
    [units],
  );

  const hasSecrets = useMemo(
    () => selectedContainsSecrets(units, selected),
    [units, selected],
  );

  const estimatedTotal = useMemo(() => {
    let total = 0;
    const walk = (list: BackupUnitNode[]) => {
      for (const n of list) {
        const kids = n.children ?? [];
        if (kids.length) walk(kids);
        else if (selected.has(n.id)) total += n.estimatedBytes || 0;
      }
    };
    walk(units);
    return total;
  }, [units, selected]);

  const onToggleUnit = (id: string, node: BackupUnitNode, checked: boolean) => {
    setSelected((prev) => {
      const next = new Set(prev);
      const kids = node.children ?? [];
      if (kids.length > 0) {
        const ids = collectAvailableIds([node]);
        for (const i of ids) {
          if (checked) next.add(i);
          else next.delete(i);
        }
      } else {
        if (checked) next.add(id);
        else next.delete(id);
      }
      return next;
    });
  };

  const selectAllUnits = (checked: boolean) => {
    setSelected(checked ? new Set(availableLeafIds) : new Set());
  };

  const onToggleDav = (id: string, checked: boolean) => {
    setSelectedDav((prev) => {
      const next = new Set(prev);
      if (checked) next.add(id);
      else next.delete(id);
      return next;
    });
  };

  const saveSettings = async () => {
    try {
      const saved = await updateBackupSettings(settings);
      setSettings(saved);
      setSettingsDirty(false);
      setStatusMsg("备份路径设置已保存");
      const u = await listBackupUnits();
      setUnits(u);
    } catch (e) {
      setStatusMsg(
        `保存失败：${e instanceof Error ? e.message : String(e ?? "未知错误")}`,
      );
    }
  };

  const startBackup = async () => {
    if (selected.size === 0) {
      setStatusMsg("请至少选择一个备份内容");
      return;
    }
    if (selectedDav.size === 0) {
      setStatusMsg("请至少选择一个 WebDAV 目标");
      return;
    }
    if (passphrase && passphrase !== passphrase2) {
      setStatusMsg("两次输入的口令不一致");
      return;
    }
    if (hasSecrets && !passphrase && !ackPlaintext) {
      setStatusMsg("包内可能含密钥：请设置口令，或勾选「我已知晓风险」");
      return;
    }

    setRunning(true);
    setResult(null);
    setProgress({
      phase: "collect",
      current: 0,
      total: 1,
      message: "准备开始备份…",
    });

    let unlisten: (() => void) | undefined;
    try {
      const { listen } = await import("@tauri-apps/api/event");
      unlisten = await listen<BackupProgressEvent>("backup-progress", (ev) => {
        setProgress(ev.payload);
      });

      const remotePrefix = uploadDir.trim() || "AgentBuddy";
      // 记住用户填写的上传目录（空则存默认 AgentBuddy）
      if (remotePrefix !== (settings.defaultRemoteDir || "AgentBuddy")) {
        try {
          const saved = await updateBackupSettings({
            ...settings,
            defaultRemoteDir: remotePrefix,
          });
          setSettings(saved);
        } catch {
          // 记忆失败不阻断备份
        }
      }
      const res = await runBackupUpload({
        unitIds: Array.from(selected),
        webdavConnectionIds: Array.from(selectedDav),
        passphrase: passphrase || undefined,
        remotePrefix,
        acknowledgePlaintextSecrets: ackPlaintext || !!passphrase,
      });
      setResult(res);
      setStatusMsg(res.message);
      // Refresh remote list if restore target matches any successful upload
      if (restoreConnId && res.ok) {
        void loadRemoteList(restoreConnId, remotePrefix);
      }
    } catch (e) {
      setStatusMsg(
        `备份失败：${e instanceof Error ? e.message : String(e ?? "未知错误")}`,
      );
    } finally {
      unlisten?.();
      setRunning(false);
      setProgress(null);
    }
  };

  const allUnitsSelected =
    availableLeafIds.length > 0 &&
    availableLeafIds.every((id) => selected.has(id));
  const someUnitsSelected =
    availableLeafIds.some((id) => selected.has(id)) && !allUnitsSelected;

  const loadRemoteList = useCallback(
    async (connId: string, prefix?: string) => {
      if (!connId) {
        setRemoteItems([]);
        return;
      }
      setRemoteLoading(true);
      try {
        const items = await listRemoteBackups(
          connId,
          (prefix ?? uploadDir).trim() || "AgentBuddy",
        );
        setRemoteItems(items);
        setSelectedRemote((prev) =>
          items.some((i) => i.name === prev) ? prev : items[0]?.name ?? "",
        );
      } catch (e) {
        setRemoteItems([]);
        setStatusMsg(
          `列举远程备份失败：${e instanceof Error ? e.message : String(e ?? "未知错误")}`,
        );
      } finally {
        setRemoteLoading(false);
      }
    },
    [uploadDir, setStatusMsg],
  );

  const startRestore = async () => {
    if (!restoreConnId) {
      setStatusMsg("请选择用于恢复的 WebDAV 连接");
      return;
    }
    if (!selectedRemote) {
      setStatusMsg("请选择要恢复的远程备份");
      return;
    }
    const item = remoteItems.find((i) => i.name === selectedRemote);
    if (item?.encrypted && !restorePassphrase.trim()) {
      setStatusMsg("该备份已加密，请填写备份口令");
      return;
    }

    setRestoring(true);
    setRestoreResult(null);
    setRunning(true);
    setProgress({
      phase: "download",
      current: 0,
      total: 1,
      message: "准备从远程恢复…",
    });
    let unlisten: (() => void) | undefined;
    try {
      const { listen } = await import("@tauri-apps/api/event");
      unlisten = await listen<BackupProgressEvent>("backup-progress", (ev) => {
        setProgress(ev.payload);
      });
      const remotePrefix = uploadDir.trim() || "AgentBuddy";
      const res = await restoreRemoteBackup({
        connectionId: restoreConnId,
        fileName: selectedRemote,
        remotePrefix,
        passphrase: restorePassphrase.trim() || undefined,
      });
      setRestoreResult(res);
      setStatusMsg(res.message);
    } catch (e) {
      setStatusMsg(
        `恢复失败：${e instanceof Error ? e.message : String(e ?? "未知错误")}`,
      );
    } finally {
      unlisten?.();
      setRestoring(false);
      setRunning(false);
      setProgress(null);
    }
  };

  const canStart =
    !loading &&
    !running &&
    selected.size > 0 &&
    selectedDav.size > 0;

  const selectedRemoteItem = remoteItems.find((i) => i.name === selectedRemote);
  const canRestore =
    !loading &&
    !running &&
    !!restoreConnId &&
    !!selectedRemote &&
    (!selectedRemoteItem?.encrypted || !!restorePassphrase.trim());

  return (
    <>
      <div className="content-header">
        <div className="content-header-bar">
          <h1 className="content-title">备份管理</h1>
          <div className="header-actions">
            <button
              type="button"
              className={`action-btn ${loading ? "sniffing" : ""}`}
              data-tooltip={loading ? "探测中..." : "重新探测"}
              onClick={() => void reload()}
              disabled={loading || running}
            >
              <IconSearch />
            </button>
            <button
              type="button"
              className={`action-btn ${running ? "sniffing" : ""}`}
              data-tooltip={
                running
                  ? "备份中..."
                  : selected.size === 0
                    ? "请先选择备份内容"
                    : selectedDav.size === 0
                      ? "请先选择 WebDAV 目标"
                      : "开始备份"
              }
              onClick={() => void startBackup()}
              disabled={!canStart}
            >
              <IconUpload />
            </button>
          </div>
        </div>
      </div>

      <div className="content-body backup-page">
        {loading ? (
          <div className="empty-state">
            <div className="empty-state-text">正在探测可备份内容…</div>
          </div>
        ) : (
          <>
            {/* Units：说明文案放在 section 内，避免折叠时与标题间距因负 margin 变乱 */}
            <section className="backup-section backup-section-units">
              <p className="backup-lead">
                勾选要备份的配置范围，打包后上传到一个或多个 WebDAV。MVP
                仅支持备份上传；恢复功能将在后续版本提供。
              </p>
              <div className="backup-section-head">
                <button
                  type="button"
                  className="backup-section-toggle"
                  aria-expanded={unitsExpanded}
                  onClick={() => setUnitsExpanded((v) => !v)}
                >
                  <span className="backup-section-chevron" aria-hidden>
                    <IconChevron open={unitsExpanded} />
                  </span>
                  <h2 className="backup-section-title">备份内容</h2>
                  {!unitsExpanded && (
                    <span className="backup-section-collapsed-meta">
                      已选 {selected.size} 项
                      {estimatedTotal > 0
                        ? ` · 约 ${formatBytes(estimatedTotal)}`
                        : ""}
                    </span>
                  )}
                </button>
                {unitsExpanded && (
                  <label className="ui-check backup-select-all">
                    <input
                      type="checkbox"
                      className="ui-check-input"
                      checked={allUnitsSelected}
                      ref={(el) => {
                        if (el) el.indeterminate = someUnitsSelected;
                      }}
                      onChange={(e) => selectAllUnits(e.target.checked)}
                      disabled={availableLeafIds.length === 0}
                    />
                    <CheckGlyph />
                    <span>全选可用项</span>
                  </label>
                )}
              </div>
              {unitsExpanded && (
                <div className="backup-card">
                  <UnitTree
                    nodes={units}
                    selected={selected}
                    onToggle={onToggleUnit}
                  />
                  <div className="backup-card-footer">
                    已选 {selected.size} 项
                    {estimatedTotal > 0
                      ? ` · 约 ${formatBytes(estimatedTotal)}`
                      : ""}
                  </div>
                </div>
              )}
            </section>

            {/* WebDAV */}
            <section className="backup-section">
              <div className="backup-section-head">
                <h2 className="backup-section-title">WebDAV</h2>
              </div>
              <div className="backup-card">
                {connections.length === 0 ? (
                  <div className="backup-empty-inline">
                    尚未配置 WebDAV 连接。请到「设置 → WebDAV」添加服务器后再备份。
                  </div>
                ) : (
                  <ul className="backup-unit-list depth-0">
                    {connections.map((c) => (
                      <li key={c.id} className="backup-unit-item">
                        <label className="ui-check backup-unit-check">
                          <input
                            type="checkbox"
                            className="ui-check-input"
                            checked={selectedDav.has(c.id)}
                            onChange={(e) =>
                              onToggleDav(c.id, e.target.checked)
                            }
                          />
                          <CheckGlyph />
                          <span className="backup-unit-body">
                            <span className="backup-unit-label-row">
                              <span className="backup-unit-label">{c.name}</span>
                              <span
                                className={`backup-badge ${
                                  c.status === "connected"
                                    ? "backup-badge-ok"
                                    : ""
                                }`}
                              >
                                {c.status === "connected" ? "已连接" : "未检测"}
                              </span>
                            </span>
                            <span className="backup-unit-path">
                              {c.url}
                              {c.username ? ` · ${c.username}` : ""}
                            </span>
                          </span>
                        </label>
                      </li>
                    ))}
                  </ul>
                )}

                <div className="backup-upload-dir">
                  <label className="backup-field">
                    <span className="backup-field-label">上传目录</span>
                    <input
                      type="text"
                      className="form-input"
                      value={uploadDir}
                      onChange={(e) => setUploadDir(e.target.value)}
                      placeholder="留空默认是 AgentBuddy"
                      disabled={running}
                      autoComplete="off"
                      spellCheck={false}
                    />
                  </label>
                  <p className="backup-upload-dir-hint">
                    相对 WebDAV 连接根路径的目录；不存在时会通过 MKCOL 自动创建。备份文件直接放在该目录下。
                  </p>
                </div>
              </div>
            </section>

            {/* Options */}
            <section className="backup-section">
              <div className="backup-section-head">
                <h2 className="backup-section-title">选项</h2>
              </div>
              <div className="backup-card backup-options">
                {hasSecrets && (
                  <div className="backup-alert">
                    当前选择可能包含 API Key、OAuth 令牌或 secretsKey。强烈建议设置备份口令。
                  </div>
                )}

                <div className="backup-field-row">
                  <label className="backup-field">
                    <span className="backup-field-label">备份口令（可选）</span>
                    <input
                      type="password"
                      className="form-input"
                      autoComplete="new-password"
                      value={passphrase}
                      onChange={(e) => setPassphrase(e.target.value)}
                      placeholder="留空则生成明文 zip"
                      disabled={running}
                    />
                  </label>
                  <label className="backup-field">
                    <span className="backup-field-label">确认口令</span>
                    <input
                      type="password"
                      className="form-input"
                      autoComplete="new-password"
                      value={passphrase2}
                      onChange={(e) => setPassphrase2(e.target.value)}
                      placeholder="再次输入"
                      disabled={running || !passphrase}
                    />
                  </label>
                </div>

                {hasSecrets && !passphrase && (
                  <label className="ui-check backup-ack">
                    <input
                      type="checkbox"
                      className="ui-check-input"
                      checked={ackPlaintext}
                      onChange={(e) => setAckPlaintext(e.target.checked)}
                      disabled={running}
                    />
                    <CheckGlyph />
                    <span>我已知晓风险：将以明文 zip 上传含密钥的备份</span>
                  </label>
                )}

                <p className="backup-upload-dir-hint">
                  上传成功后本地临时包会删除；每个 WebDAV 目录仅保留最新 3 份
                  <code> agentbuddy-backup-*</code> 归档。
                </p>

                <button
                  type="button"
                  className="btn btn-secondary backup-advanced-toggle"
                  onClick={() => setShowAdvanced((v) => !v)}
                >
                  {showAdvanced ? "收起高级设置" : "高级设置（本地路径）"}
                </button>

                {showAdvanced && (
                  <div className="backup-advanced">
                    <label className="backup-field">
                      <span className="backup-field-label">
                        cliproxyapi 配置文件路径（空=自动探测）
                      </span>
                      <input
                        type="text"
                        className="form-input"
                        value={settings.cliproxyapiConfPath}
                        onChange={(e) => {
                          setSettings((s) => ({
                            ...s,
                            cliproxyapiConfPath: e.target.value,
                          }));
                          setSettingsDirty(true);
                        }}
                        placeholder="/usr/local/etc/cliproxyapi.conf"
                        disabled={running}
                      />
                    </label>
                    <label className="backup-field">
                      <span className="backup-field-label">
                        sub2api 根目录（空=自动探测）
                      </span>
                      <input
                        type="text"
                        className="form-input"
                        value={settings.sub2apiRootPath}
                        onChange={(e) => {
                          setSettings((s) => ({
                            ...s,
                            sub2apiRootPath: e.target.value,
                          }));
                          setSettingsDirty(true);
                        }}
                        placeholder="~/Downloads/sub2api"
                        disabled={running}
                      />
                    </label>
                    <div className="backup-advanced-actions">
                      <button
                        type="button"
                        className="btn btn-secondary"
                        disabled={!settingsDirty || running}
                        onClick={() => void saveSettings()}
                      >
                        保存路径设置
                      </button>
                    </div>
                  </div>
                )}
              </div>
            </section>

            {/* Restore from remote */}
            <section className="backup-section">
              <div className="backup-section-head">
                <h2 className="backup-section-title">从远程恢复</h2>
              </div>
              <div className="backup-card backup-restore">
                <p className="backup-upload-dir-hint">
                  从 WebDAV 上传目录下载备份并按 manifest 还原到本机路径。加密包需填写备份时使用的口令。恢复会覆盖同名文件，请谨慎操作。
                </p>

                {connections.length === 0 ? (
                  <div className="backup-empty-inline">
                    请先在「设置 → WebDAV」添加连接
                  </div>
                ) : (
                  <>
                    <div className="backup-restore-toolbar">
                      <div className="backup-field backup-restore-conn">
                        <span className="backup-field-label" id="backup-restore-conn-label">
                          WebDAV 连接
                        </span>
                        <AppSelect
                          id="backup-restore-conn"
                          labelId="backup-restore-conn-label"
                          value={restoreConnId}
                          options={connections.map((c) => ({
                            value: c.id,
                            label: c.name,
                            sub: c.url,
                          }))}
                          onChange={(id) => {
                            setRestoreConnId(id);
                            setRemoteItems([]);
                            setSelectedRemote("");
                            setRestoreResult(null);
                          }}
                          disabled={running}
                          placeholder="请选择 WebDAV 连接"
                          emptyText="暂无 WebDAV 连接"
                        />
                      </div>
                      <button
                        type="button"
                        className="btn btn-primary backup-restore-refresh"
                        disabled={running || !restoreConnId || remoteLoading}
                        onClick={() =>
                          void loadRemoteList(
                            restoreConnId,
                            uploadDir.trim() || "AgentBuddy",
                          )
                        }
                      >
                        {remoteLoading ? "加载中…" : "刷新列表"}
                      </button>
                    </div>

                    {remoteItems.length === 0 ? (
                      <div className="backup-empty-inline">
                        {remoteLoading
                          ? "正在列举远程备份…"
                          : "暂无列表。请确认上传目录后点击「刷新列表」。"}
                      </div>
                    ) : (
                      <ul className="backup-remote-list">
                        {remoteItems.map((item) => {
                          const checked = selectedRemote === item.name;
                          return (
                            <li key={item.name} className="backup-unit-item">
                              <label
                                className={`ui-check backup-unit-check ${
                                  checked ? "is-selected" : ""
                                }`}
                              >
                                <input
                                  type="radio"
                                  className="ui-check-input"
                                  name="backup-remote-pick"
                                  checked={checked}
                                  onChange={() => setSelectedRemote(item.name)}
                                  disabled={running}
                                />
                                <CheckGlyph />
                                <span className="backup-unit-body">
                                  <span className="backup-unit-label-row">
                                    <span className="backup-unit-label">
                                      {item.name}
                                    </span>
                                    <span className="backup-unit-size">
                                      {formatBytes(item.bytes)}
                                    </span>
                                    {item.encrypted ? (
                                      <span className="backup-badge backup-badge-secret">
                                        加密
                                      </span>
                                    ) : (
                                      <span className="backup-badge">明文</span>
                                    )}
                                  </span>
                                  {item.lastModified ? (
                                    <span className="backup-unit-path">
                                      {item.lastModified}
                                    </span>
                                  ) : null}
                                </span>
                              </label>
                            </li>
                          );
                        })}
                      </ul>
                    )}

                    {selectedRemoteItem?.encrypted && (
                      <label className="backup-field">
                        <span className="backup-field-label">备份口令</span>
                        <input
                          type="password"
                          className="form-input"
                          autoComplete="current-password"
                          value={restorePassphrase}
                          onChange={(e) => setRestorePassphrase(e.target.value)}
                          placeholder="加密备份的口令"
                          disabled={running}
                        />
                      </label>
                    )}

                    <div className="backup-restore-actions">
                      <button
                        type="button"
                        className="btn btn-primary"
                        disabled={!canRestore}
                        onClick={() => void startRestore()}
                      >
                        {restoring ? "恢复中…" : "恢复所选备份"}
                      </button>
                    </div>

                    {restoreResult && !running && (
                      <div
                        className={`backup-result ${
                          restoreResult.ok ? "is-ok" : "is-fail"
                        }`}
                        style={{ marginTop: 12 }}
                      >
                        <div className="backup-result-msg">
                          {restoreResult.message}
                        </div>
                        <div className="backup-result-meta">
                          还原 {restoreResult.restoredFiles} · 跳过{" "}
                          {restoreResult.skippedFiles}
                        </div>
                        {restoreResult.warnings?.length > 0 && (
                          <div className="backup-unit-warn">
                            {restoreResult.warnings.join("；")}
                          </div>
                        )}
                      </div>
                    )}
                  </>
                )}
              </div>
            </section>

            {/* Progress (live during run) */}
            {running && progress && (
              <section className="backup-section">
                <div className="backup-section-head">
                  <h2 className="backup-section-title">备份进度</h2>
                </div>
                <div className="backup-card backup-progress">
                  <div className="backup-progress-row">
                    <span className="backup-progress-phase">
                      {phaseLabel(progress.phase)}
                    </span>
                    <span className="backup-progress-count">
                      {progress.total > 0
                        ? `${Math.min(progress.current, progress.total)} / ${progress.total}`
                        : "…"}
                    </span>
                  </div>
                  <div
                    className="backup-progress-track"
                    role="progressbar"
                    aria-valuemin={0}
                    aria-valuemax={progress.total || 1}
                    aria-valuenow={progress.current}
                    aria-label={progress.message}
                  >
                    <div
                      className="backup-progress-fill"
                      style={{
                        width: `${
                          progress.total > 0
                            ? Math.min(
                                100,
                                Math.round(
                                  (progress.current / progress.total) * 100,
                                ),
                              )
                            : 8
                        }%`,
                      }}
                    />
                  </div>
                  <div className="backup-progress-msg">{progress.message}</div>
                </div>
              </section>
            )}

            {/* Result */}
            {result && !running && (
              <section className="backup-section">
                <div className="backup-section-head">
                  <h2 className="backup-section-title">最近一次结果</h2>
                </div>
                <div
                  className={`backup-card backup-result ${
                    result.ok ? "is-ok" : "is-fail"
                  }`}
                >
                  <div className="backup-result-msg">{result.message}</div>
                  <div className="backup-result-meta">
                    文件：{result.archiveFileName} ·{" "}
                    {formatBytes(result.archiveBytes)}
                    {result.encrypted ? " · 已加密" : " · 明文 zip"}
                  </div>
                  <ul className="backup-result-targets">
                    {result.targets.map((t) => (
                      <li key={t.connectionId}>
                        <span
                          className={
                            t.ok ? "backup-badge-ok" : "backup-badge-fail"
                          }
                        >
                          {t.ok ? "成功" : "失败"}
                        </span>{" "}
                        <strong>{t.name}</strong> — {t.message}
                        {t.remotePath ? (
                          <div className="backup-unit-path">{t.remotePath}</div>
                        ) : null}
                      </li>
                    ))}
                  </ul>
                  {result.warnings?.length > 0 && (
                    <div className="backup-unit-warn">
                      {result.warnings.join("；")}
                    </div>
                  )}
                </div>
              </section>
            )}
          </>
        )}
      </div>

      <Toast message={statusMsg} />
    </>
  );
}
