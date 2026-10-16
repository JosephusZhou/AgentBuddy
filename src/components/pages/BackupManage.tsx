import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { CheckGlyph, useOverlayDismiss } from "@/components/ui";
import { Toast } from "@/components/Toast";
import { useStatusMessage } from "@/lib/useStatusMessage";
import WebDavSection from "./WebDAV";
import {
  collectAvailableIds,
  formatBytes,
  getAutoBackupSettings,
  getAutoBackupStatus,
  getBackupSettings,
  listBackupUnits,
  listRemoteBackups,
  listWebDavConnections,
  restoreRemoteBackup,
  runAutoBackupNow,
  runBackupUpload,
  updateAutoBackupSettings,
  updateBackupSettings,
} from "./backup-manage/api";
import type {
  AutoBackupSettings,
  AutoBackupStatus,
  BackupProgressEvent,
  BackupRunResult,
  BackupSettings,
  BackupUnitNode,
  RemoteBackupItem,
  RestoreBackupResult,
  WebDAVConnectionLite,
} from "./backup-manage/types";
import { ChevronDown, Eye, EyeOff, X } from "lucide-react";

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

/** Unix 秒 → 本地 "yyyy-MM-dd HH:mm"；空值显示 —。 */
function formatAutoTime(unixSec: number | null): string {
  if (!unixSec) return "—";
  const d = new Date(unixSec * 1000);
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

const AUTO_INTERVAL_CHOICES = [6, 12, 24, 72, 168];

function intervalLabel(hours: number): string {
  return hours % 24 === 0 ? `每隔 ${hours / 24} 天` : `每隔 ${hours} 小时`;
}

/** 远程列表条目的稳定键：手动目录与 auto 目录可能存在同名旧归档，用目录消歧。 */
function remoteItemKey(item: RemoteBackupItem): string {
  return `${item.dir ?? ""}/${item.name}`;
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
    <div className={`app-select ${open ? "open" : ""} ${disabled ? "disabled" : ""}`} ref={rootRef}>
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
                  {o.sub ? <span className="app-select-option-sub">{o.sub}</span> : null}
                </button>
              );
            })
          )}
        </div>
      )}
    </div>
  );
}

/** app-select 下拉箭头：open 时朝上 */
const IconSelectChevron = ({ open }: { open: boolean }) => (
  <ChevronDown
    size={16}
    strokeWidth={1.8}
    style={{
      transform: open ? "rotate(180deg)" : "rotate(0deg)",
      transition: "transform 0.15s ease",
    }}
  />
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

function selectedContainsSecrets(nodes: BackupUnitNode[], selected: Set<string>): boolean {
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
        const childIds = hasKids ? collectAvailableIds([node]) : node.available ? [node.id] : [];
        const selectedCount = childIds.filter((id) => selected.has(id)).length;
        const allSelected = childIds.length > 0 && selectedCount === childIds.length;
        const someSelected = selectedCount > 0 && selectedCount < childIds.length;
        const checked = hasKids ? allSelected : selected.has(node.id);
        const disabled = hasKids ? childIds.length === 0 : !node.available;

        return (
          <li key={node.id} className="backup-unit-item">
            <label className={`ui-check backup-unit-check ${disabled ? "is-disabled" : ""}`}>
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
                  {!node.available && <span className="backup-badge">未检测到</span>}
                  {node.estimatedBytes > 0 && (
                    <span className="backup-unit-size">{formatBytes(node.estimatedBytes)}</span>
                  )}
                </span>
                {node.pathSummary && <span className="backup-unit-path">{node.pathSummary}</span>}
                {node.warnings?.length > 0 && (
                  <span className="backup-unit-warn">{node.warnings.join("；")}</span>
                )}
              </span>
            </label>
            {hasKids && (
              <UnitTree nodes={kids} selected={selected} onToggle={onToggle} depth={depth + 1} />
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
    defaultRemoteDir: "Octopus",
  });
  /** 本次备份的上传目录（相对 WebDAV 根）；空则回退 Octopus */
  const [uploadDir, setUploadDir] = useState("Octopus");
  const [passphrase, setPassphrase] = useState("");
  const [passphrase2, setPassphrase2] = useState("");
  const [passphraseVisible, setPassphraseVisible] = useState(false);
  const [passphrase2Visible, setPassphrase2Visible] = useState(false);
  const [ackPlaintext, setAckPlaintext] = useState(false);
  const [loading, setLoading] = useState(true);
  const [running, setRunning] = useState(false);
  const [result, setResult] = useState<BackupRunResult | null>(null);
  const [progress, setProgress] = useState<BackupProgressEvent | null>(null);

  // 自动备份（设置持久化在 config.json backup.auto；备份范围选择独立，
  // WebDAV 目标与手动备份共用上方勾选）
  const [autoSettings, setAutoSettings] = useState<AutoBackupSettings>({
    enabled: false,
    mode: "interval",
    intervalHours: 24,
    dailyTime: "09:30",
    unitIds: [],
    webdavConnectionIds: [],
    hasPassphrase: false,
  });
  const [autoStatus, setAutoStatus] = useState<AutoBackupStatus | null>(null);
  const [autoSelected, setAutoSelected] = useState<Set<string>>(new Set());
  const [autoPassphrase, setAutoPassphrase] = useState("");
  const [autoPassphrase2, setAutoPassphrase2] = useState("");
  const [autoPassphraseVisible, setAutoPassphraseVisible] = useState(false);
  const [autoPassphrase2Visible, setAutoPassphrase2Visible] = useState(false);
  /** 自动备份单元树折叠：默认收起，避免与手动树重复占用版面 */
  const [autoUnitsExpanded, setAutoUnitsExpanded] = useState(false);
  const [autoDirty, setAutoDirty] = useState(false);
  const [autoSaving, setAutoSaving] = useState(false);
  const [autoRunning, setAutoRunning] = useState(false);
  /** 已设置口令且输入新口令时，保存前弹窗确认覆盖。 */
  const [confirmOverwrite, setConfirmOverwrite] = useState(false);
  const overwriteDismiss = useOverlayDismiss(() => setConfirmOverwrite(false), autoSaving);

  // Restore
  const [restoreConnId, setRestoreConnId] = useState("");
  const [remoteItems, setRemoteItems] = useState<RemoteBackupItem[]>([]);
  const [remoteLoading, setRemoteLoading] = useState(false);
  const [selectedRemote, setSelectedRemote] = useState<string>("");
  const [restorePassphrase, setRestorePassphrase] = useState("");
  const [restorePassphraseVisible, setRestorePassphraseVisible] = useState(false);
  const [restoring, setRestoring] = useState(false);
  const [restoreResult, setRestoreResult] = useState<RestoreBackupResult | null>(null);

  const reload = useCallback(async () => {
    setLoading(true);
    try {
      const [u, s, dav, auto] = await Promise.all([
        listBackupUnits(),
        getBackupSettings(),
        listWebDavConnections(),
        getAutoBackupSettings(),
      ]);
      setUnits(u);
      setSettings(s);
      setUploadDir(s.defaultRemoteDir?.trim() || "Octopus");
      setConnections(dav);
      setSelected(collectDefaultIds(u));
      // 自动备份优先用已保存的选择；首次使用时预填默认选中项。
      setAutoSelected(auto.unitIds.length > 0 ? new Set(auto.unitIds) : collectDefaultIds(u));
      setAutoSettings(auto);
      setAutoDirty(false);
      // WebDAV 目标勾选为手动/自动备份共用：优先沿用已保存的自动备份目标，
      // 未保存过则默认全选全部连接。
      setSelectedDav(
        auto.webdavConnectionIds.length > 0
          ? new Set(auto.webdavConnectionIds)
          : new Set(dav.map((d) => d.id)),
      );
      if (dav.length > 0) {
        setRestoreConnId((prev) => prev || dav[0].id);
      }
    } catch (e) {
      setStatusMsg(`加载失败：${e instanceof Error ? e.message : String(e ?? "未知错误")}`);
    } finally {
      setLoading(false);
    }
  }, [setStatusMsg]);

  /** WebDAV 连接增删改后重新拉取列表，并剔除已删除连接的勾选状态。 */
  const refreshWebDavConnections = useCallback(async () => {
    try {
      const dav = await listWebDavConnections();
      setConnections(dav);
      const validIds = new Set(dav.map((d) => d.id));
      setSelectedDav((prev) => new Set([...prev].filter((id) => validIds.has(id))));
      setRestoreConnId((prev) => (prev && validIds.has(prev) ? prev : (dav[0]?.id ?? "")));
    } catch {
      // 刷新失败不打断页面，保留现有列表
    }
  }, []);

  useEffect(() => {
    void reload();
  }, [reload]);

  const availableLeafIds = useMemo(() => collectAvailableIds(units), [units]);

  const hasSecrets = useMemo(() => selectedContainsSecrets(units, selected), [units, selected]);

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

  const startBackup = async () => {
    if (selected.size === 0) {
      setStatusMsg("请先选择备份范围");
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

      const remotePrefix = uploadDir.trim() || "Octopus";
      // 记住用户填写的上传目录（空则存默认 Octopus）
      if (remotePrefix !== (settings.defaultRemoteDir || "Octopus")) {
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
      setStatusMsg(`备份失败：${e instanceof Error ? e.message : String(e ?? "未知错误")}`);
    } finally {
      unlisten?.();
      setRunning(false);
      setProgress(null);
    }
  };

  /** 拉取自动备份运行状态（下次/上次运行）。 */
  const refreshAutoStatus = useCallback(async () => {
    try {
      setAutoStatus(await getAutoBackupStatus());
    } catch {
      // 状态查询失败不打断页面，保留上次状态
    }
  }, []);

  useEffect(() => {
    void refreshAutoStatus();
    const timer = window.setInterval(() => void refreshAutoStatus(), 30000);
    return () => window.clearInterval(timer);
  }, [refreshAutoStatus]);

  // 自动备份在后台执行时也会发 backup-progress（trigger=auto）：监听以即时刷新状态。
  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    (async () => {
      const { listen } = await import("@tauri-apps/api/event");
      if (disposed) return;
      unlisten = await listen<BackupProgressEvent>("backup-progress", (ev) => {
        if (ev.payload.trigger !== "auto") return;
        if (ev.payload.phase === "finalize") void refreshAutoStatus();
      });
      // 清理发生在 listen 完成前时，这里补退订，避免监听器泄漏。
      if (disposed) {
        unlisten();
        unlisten = undefined;
      }
    })();
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [refreshAutoStatus]);

  /** 校验并写入自动备份设置；成功返回 true 并清空表单脏状态。 */
  const persistAutoSettings = async (): Promise<boolean> => {
    if (autoSelected.size === 0) {
      setStatusMsg("请至少选择一个自动备份内容");
      return false;
    }
    if (selectedDav.size === 0) {
      setStatusMsg("请至少选择一个 WebDAV 目标");
      return false;
    }
    if (autoPassphrase && autoPassphrase !== autoPassphrase2) {
      setStatusMsg("两次输入的口令不一致");
      return false;
    }
    const dailyTime = autoSettings.dailyTime.trim() || "09:30";
    setAutoSaving(true);
    try {
      const saved = await updateAutoBackupSettings({
        enabled: autoSettings.enabled,
        mode: autoSettings.mode,
        intervalHours: autoSettings.intervalHours,
        dailyTime,
        unitIds: Array.from(autoSelected),
        // WebDAV 目标勾选为手动/自动备份共用，落盘当前共享勾选。
        webdavConnectionIds: Array.from(selectedDav),
        // 留空 = 不修改口令；非空 = 设置/覆盖口令（覆盖前由前端弹窗确认）。
        passphrase: autoPassphrase || undefined,
      });
      setAutoSettings(saved);
      setAutoPassphrase("");
      setAutoPassphrase2("");
      setAutoDirty(false);
      return true;
    } catch (e) {
      setStatusMsg(`保存失败：${e instanceof Error ? e.message : String(e ?? "未知错误")}`);
      return false;
    } finally {
      setAutoSaving(false);
    }
  };

  const saveAutoSettings = async () => {
    if (!(await persistAutoSettings())) return;
    setStatusMsg("自动备份设置已保存");
    void refreshAutoStatus();
  };

  /** 保存自动备份设置：先做可即时反馈的校验；已设置口令且输入了新口令时，弹窗确认覆盖。 */
  const handleSaveAutoSettings = async () => {
    if (autoSelected.size === 0) {
      setStatusMsg("请至少选择一个自动备份内容");
      return;
    }
    if (selectedDav.size === 0) {
      setStatusMsg("请至少选择一个 WebDAV 目标");
      return;
    }
    if (autoPassphrase && autoPassphrase !== autoPassphrase2) {
      setStatusMsg("两次输入的口令不一致");
      return;
    }
    if (autoPassphrase && autoSettings.hasPassphrase) {
      setConfirmOverwrite(true);
      return;
    }
    await saveAutoSettings();
  };

  const confirmOverwritePassphrase = async () => {
    setConfirmOverwrite(false);
    await saveAutoSettings();
  };

  const runAutoNow = async () => {
    setAutoRunning(true);
    try {
      // 后端按已保存配置执行：表单有未保存改动（含刚输入的口令、共享勾选的
      // WebDAV 目标变化）时先落盘，否则会出现「运行的不是表单里看到的配置/
      // 口令未设置」的错位。
      const davChanged =
        Array.from(selectedDav).sort().join(",") !==
        [...autoSettings.webdavConnectionIds].sort().join(",");
      if (autoDirty || autoPassphrase || davChanged) {
        const saved = await persistAutoSettings();
        if (!saved) return;
      }
      const res = await runAutoBackupNow();
      setStatusMsg(res.message);
    } catch (e) {
      setStatusMsg(`自动备份失败：${e instanceof Error ? e.message : String(e ?? "未知错误")}`);
    } finally {
      setAutoRunning(false);
      void refreshAutoStatus();
    }
  };

  const onToggleAutoUnit = (id: string, node: BackupUnitNode, checked: boolean) => {
    setAutoSelected((prev) => {
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
    setAutoDirty(true);
  };

  const autoUnitsAllSelected =
    availableLeafIds.length > 0 && availableLeafIds.every((id) => autoSelected.has(id));
  const autoUnitsSomeSelected =
    availableLeafIds.some((id) => autoSelected.has(id)) && !autoUnitsAllSelected;
  // 输入框里已填写的口令也视为满足要求：hasPassphrase 只反映已落盘状态，
  // 否则首次保存（口令尚未存在）会被本条件永久禁用。
  const autoNeedsPassphrase =
    selectedContainsSecrets(units, autoSelected) && !autoSettings.hasPassphrase && !autoPassphrase;

  const allUnitsSelected =
    availableLeafIds.length > 0 && availableLeafIds.every((id) => selected.has(id));
  const someUnitsSelected = availableLeafIds.some((id) => selected.has(id)) && !allUnitsSelected;
  const loadRemoteList = useCallback(
    async (connId: string, prefix?: string) => {
      if (!connId) {
        setRemoteItems([]);
        return;
      }
      setRemoteLoading(true);
      try {
        const base = (prefix ?? uploadDir).trim() || "Octopus";
        const autoDir = `${base}/auto`;
        // 合并上传目录与其下 auto 子目录（自动备份）；auto 目录尚不存在时视为空。
        const [manual, auto] = await Promise.all([
          listRemoteBackups(connId, base),
          listRemoteBackups(connId, autoDir).catch(() => [] as RemoteBackupItem[]),
        ]);
        const merged: RemoteBackupItem[] = [
          ...manual.map((i) => ({ ...i, dir: base, auto: false })),
          ...auto.map((i) => ({ ...i, dir: autoDir, auto: true })),
        ];
        merged.sort((a, b) => b.name.localeCompare(a.name));
        setRemoteItems(merged);
        setSelectedRemote((prev) =>
          merged.some((i) => remoteItemKey(i) === prev)
            ? prev
            : merged[0]
              ? remoteItemKey(merged[0])
              : "",
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
    const item = remoteItems.find((i) => remoteItemKey(i) === selectedRemote);
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
      message: "准备恢复备份…",
    });
    let unlisten: (() => void) | undefined;
    try {
      const { listen } = await import("@tauri-apps/api/event");
      unlisten = await listen<BackupProgressEvent>("backup-progress", (ev) => {
        setProgress(ev.payload);
      });
      // 自动备份归档在 <上传目录>/auto 下，按条目来源目录下载。
      const remotePrefix = item?.dir || uploadDir.trim() || "Octopus";
      const res = await restoreRemoteBackup({
        connectionId: restoreConnId,
        fileName: item?.name ?? selectedRemote,
        remotePrefix,
        passphrase: restorePassphrase.trim() || undefined,
      });
      setRestoreResult(res);
      setStatusMsg(res.message);
    } catch (e) {
      setStatusMsg(`恢复失败：${e instanceof Error ? e.message : String(e ?? "未知错误")}`);
    } finally {
      unlisten?.();
      setRestoring(false);
      setRunning(false);
      setProgress(null);
    }
  };

  const canStart = !loading && !running && selected.size > 0 && selectedDav.size > 0;

  const selectedRemoteItem = remoteItems.find((i) => remoteItemKey(i) === selectedRemote);
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
          <h1 className="content-title">
            <span className="title-crumb">
              设置<span className="title-sep">/</span>
            </span>
            备份
          </h1>
        </div>
      </div>

      <div className="content-body backup-page">
        {loading ? (
          <div className="empty-state">
            <div className="empty-state-text">正在探测备份范围…</div>
          </div>
        ) : (
          <>
            <section className="backup-section backup-section-units">
              <div className="backup-section-head">
                <h2 className="backup-section-title">备份范围</h2>
                <button
                  type="button"
                  className="btn btn-secondary"
                  onClick={() => void reload()}
                  disabled={loading || running}
                >
                  {loading ? "探测中…" : "重新探测"}
                </button>
              </div>
              <div className="backup-card">
                <UnitTree nodes={units} selected={selected} onToggle={onToggleUnit} />
                <div className="backup-card-footer">
                  <span>
                    已选 {selected.size} 项
                    {estimatedTotal > 0 ? ` · 约 ${formatBytes(estimatedTotal)}` : ""}
                  </span>
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
                    <span>全选</span>
                  </label>
                </div>
              </div>
            </section>

            {/* WebDAV：连接管理（增删改测）+ 备份目标勾选（手动/自动共用）+ 上传目录 */}
            <WebDavSection
              connections={connections}
              selected={selectedDav}
              onToggle={onToggleDav}
              uploadDir={uploadDir}
              onUploadDirChange={setUploadDir}
              onChanged={refreshWebDavConnections}
            />

            {/* 手动备份 */}
            <section className="backup-section">
              <div className="backup-section-head">
                <h2 className="backup-section-title">手动备份</h2>
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
                    <div className="form-input-with-action">
                      <input
                        type={passphraseVisible ? "text" : "password"}
                        className="form-input"
                        autoComplete="new-password"
                        spellCheck={false}
                        value={passphrase}
                        onChange={(e) => setPassphrase(e.target.value)}
                        placeholder="留空则生成明文 zip"
                        disabled={running}
                      />
                      <button
                        type="button"
                        className="form-input-action"
                        data-tooltip={passphraseVisible ? "隐藏备份口令" : "显示备份口令"}
                        aria-label={passphraseVisible ? "隐藏备份口令" : "显示备份口令"}
                        aria-pressed={passphraseVisible}
                        onClick={() => setPassphraseVisible((visible) => !visible)}
                        disabled={running}
                      >
                        {passphraseVisible ? <EyeOff size={16} /> : <Eye size={16} />}
                      </button>
                    </div>
                  </label>
                  <label className="backup-field">
                    <span className="backup-field-label">确认口令</span>
                    <div className="form-input-with-action">
                      <input
                        type={passphrase2Visible ? "text" : "password"}
                        className="form-input"
                        autoComplete="new-password"
                        spellCheck={false}
                        value={passphrase2}
                        onChange={(e) => setPassphrase2(e.target.value)}
                        placeholder="再次输入"
                        disabled={running || !passphrase}
                      />
                      <button
                        type="button"
                        className="form-input-action"
                        data-tooltip={passphrase2Visible ? "隐藏确认口令" : "显示确认口令"}
                        aria-label={passphrase2Visible ? "隐藏确认口令" : "显示确认口令"}
                        aria-pressed={passphrase2Visible}
                        onClick={() => setPassphrase2Visible((visible) => !visible)}
                        disabled={running || !passphrase}
                      >
                        {passphrase2Visible ? <EyeOff size={16} /> : <Eye size={16} />}
                      </button>
                    </div>
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
                  上传成功后本地临时包会删除；每个 WebDAV 目录仅保留最新 3 份备份归档（自动备份在其
                  auto 子目录单独保留 3 份）。
                </p>

                <div className="backup-advanced-actions">
                  <button
                    type="button"
                    className="btn btn-primary"
                    data-tooltip={
                      running
                        ? "备份中..."
                        : selected.size === 0
                          ? "请先选择备份范围"
                          : selectedDav.size === 0
                            ? "请先选择 WebDAV 目标"
                            : "立即备份"
                    }
                    onClick={() => void startBackup()}
                    disabled={!canStart}
                  >
                    {running ? "备份中…" : "立即备份"}
                  </button>
                </div>
              </div>
            </section>

            {/* 自动备份：设置持久化在 config.json backup.auto，备份范围独立于手动，
                WebDAV 目标与手动备份共用上方勾选 */}
            <section className="backup-section">
              <div className="backup-section-head">
                <h2 className="backup-section-title">自动备份</h2>
                {autoStatus?.running && (
                  <span className="backup-badge backup-badge-ok">备份进行中</span>
                )}
              </div>
              <div className="backup-card backup-auto">
                <p className="backup-upload-dir-hint">
                  应用运行期间按计划自动备份并上传到每个 WebDAV 的{" "}
                  <code>{uploadDir.trim() || "Octopus"}/auto</code> 子目录，命名前缀为{" "}
                  <code>octopus-auto-backup-*</code>
                  ，自动强制口令加密，仅保留最近 3
                  份，不与手动备份挤占保留窗口；应用未运行时不执行。
                </p>

                <label className="ui-check backup-ack">
                  <input
                    type="checkbox"
                    className="ui-check-input"
                    checked={autoSettings.enabled}
                    onChange={(e) => {
                      setAutoSettings((s) => ({ ...s, enabled: e.target.checked }));
                      setAutoDirty(true);
                    }}
                    disabled={autoSaving || autoRunning}
                  />
                  <CheckGlyph />
                  <span>启用自动备份</span>
                </label>

                <div className="backup-field-row">
                  <label className="backup-field">
                    <span className="backup-field-label">触发方式</span>
                    <AppSelect
                      value={autoSettings.mode}
                      options={[
                        { value: "interval", label: "按时间间隔" },
                        { value: "daily", label: "每天固定时刻" },
                      ]}
                      onChange={(v) => {
                        setAutoSettings((s) => ({
                          ...s,
                          mode: v === "daily" ? "daily" : "interval",
                        }));
                        setAutoDirty(true);
                      }}
                      disabled={autoSaving || autoRunning}
                    />
                  </label>
                  {autoSettings.mode === "interval" ? (
                    <label className="backup-field">
                      <span className="backup-field-label">间隔</span>
                      <AppSelect
                        value={String(autoSettings.intervalHours)}
                        options={AUTO_INTERVAL_CHOICES.map((h) => ({
                          value: String(h),
                          label: intervalLabel(h),
                        }))}
                        onChange={(v) => {
                          setAutoSettings((s) => ({ ...s, intervalHours: Number(v) }));
                          setAutoDirty(true);
                        }}
                        disabled={autoSaving || autoRunning}
                      />
                    </label>
                  ) : (
                    <label className="backup-field">
                      <span className="backup-field-label">每天时刻</span>
                      <input
                        type="time"
                        className="form-input"
                        value={autoSettings.dailyTime}
                        onChange={(e) => {
                          setAutoSettings((s) => ({ ...s, dailyTime: e.target.value }));
                          setAutoDirty(true);
                        }}
                        disabled={autoSaving || autoRunning}
                      />
                    </label>
                  )}
                </div>

                {autoSettings.enabled && (
                  <div className="backup-auto-status">
                    <span>下次运行：{formatAutoTime(autoStatus?.nextRunAt ?? null)}</span>
                    <span>
                      上次运行：{formatAutoTime(autoStatus?.lastRunAt ?? null)}
                      {autoStatus?.lastOk === false ? " · 失败" : ""}
                      {autoStatus?.lastOk === true ? " · 成功" : ""}
                    </span>
                  </div>
                )}
                {autoStatus?.lastMessage && (
                  <div className="backup-unit-warn">{autoStatus.lastMessage}</div>
                )}

                <button
                  type="button"
                  className="btn btn-secondary backup-advanced-toggle"
                  onClick={() => setAutoUnitsExpanded((v) => !v)}
                >
                  {autoUnitsExpanded
                    ? "收起自动备份内容"
                    : `选择自动备份内容（已选 ${autoSelected.size} 项）`}
                </button>
                {autoUnitsExpanded && (
                  <>
                    <UnitTree nodes={units} selected={autoSelected} onToggle={onToggleAutoUnit} />
                    <label className="ui-check backup-select-all">
                      <input
                        type="checkbox"
                        className="ui-check-input"
                        checked={autoUnitsAllSelected}
                        ref={(el) => {
                          if (el) el.indeterminate = autoUnitsSomeSelected;
                        }}
                        onChange={(e) => {
                          setAutoSelected(e.target.checked ? new Set(availableLeafIds) : new Set());
                          setAutoDirty(true);
                        }}
                        disabled={availableLeafIds.length === 0}
                      />
                      <CheckGlyph />
                      <span>全选可用项</span>
                    </label>
                  </>
                )}

                {autoSettings.hasPassphrase && (
                  <div className="backup-auto-pass-set">
                    <span>
                      备份口令：<strong>已设置</strong>（不可回读，输入新口令保存可覆盖）
                    </span>
                  </div>
                )}
                <div className="backup-field-row">
                  <label className="backup-field">
                    <span className="backup-field-label">
                      备份口令{autoSettings.hasPassphrase ? "（留空保持不变）" : "（必填）"}
                    </span>
                    <div className="form-input-with-action">
                      <input
                        type={autoPassphraseVisible ? "text" : "password"}
                        className="form-input"
                        autoComplete="new-password"
                        spellCheck={false}
                        value={autoPassphrase}
                        onChange={(e) => {
                          setAutoPassphrase(e.target.value);
                          setAutoDirty(true);
                        }}
                        placeholder={
                          autoSettings.hasPassphrase ? "输入新口令可覆盖原口令" : "自动备份强制加密"
                        }
                        disabled={autoSaving || autoRunning}
                      />
                      <button
                        type="button"
                        className="form-input-action"
                        data-tooltip={autoPassphraseVisible ? "隐藏备份口令" : "显示备份口令"}
                        aria-label={autoPassphraseVisible ? "隐藏备份口令" : "显示备份口令"}
                        aria-pressed={autoPassphraseVisible}
                        onClick={() => setAutoPassphraseVisible((visible) => !visible)}
                        disabled={autoSaving || autoRunning}
                      >
                        {autoPassphraseVisible ? <EyeOff size={16} /> : <Eye size={16} />}
                      </button>
                    </div>
                  </label>
                  <label className="backup-field">
                    <span className="backup-field-label">确认口令</span>
                    <div className="form-input-with-action">
                      <input
                        type={autoPassphrase2Visible ? "text" : "password"}
                        className="form-input"
                        autoComplete="new-password"
                        spellCheck={false}
                        value={autoPassphrase2}
                        onChange={(e) => setAutoPassphrase2(e.target.value)}
                        placeholder="再次输入"
                        disabled={autoSaving || autoRunning || !autoPassphrase}
                      />
                      <button
                        type="button"
                        className="form-input-action"
                        data-tooltip={autoPassphrase2Visible ? "隐藏确认口令" : "显示确认口令"}
                        aria-label={autoPassphrase2Visible ? "隐藏确认口令" : "显示确认口令"}
                        aria-pressed={autoPassphrase2Visible}
                        onClick={() => setAutoPassphrase2Visible((visible) => !visible)}
                        disabled={autoSaving || autoRunning || !autoPassphrase}
                      >
                        {autoPassphrase2Visible ? <EyeOff size={16} /> : <Eye size={16} />}
                      </button>
                    </div>
                  </label>
                </div>

                {autoNeedsPassphrase && (
                  <div className="backup-alert">
                    自动备份所选内容可能包含密钥，必须设置备份口令后才能启用。
                  </div>
                )}

                <div className="backup-advanced-actions">
                  <button
                    type="button"
                    className="btn btn-primary"
                    disabled={
                      !autoDirty ||
                      autoSaving ||
                      autoRunning ||
                      running ||
                      autoSelected.size === 0 ||
                      selectedDav.size === 0 ||
                      (autoSettings.enabled && autoNeedsPassphrase)
                    }
                    onClick={() => void handleSaveAutoSettings()}
                  >
                    {autoSaving ? "保存中…" : "保存自动备份设置"}
                  </button>
                  <button
                    type="button"
                    className="btn btn-primary"
                    disabled={
                      running ||
                      autoRunning ||
                      autoSelected.size === 0 ||
                      selectedDav.size === 0 ||
                      (!autoSettings.hasPassphrase && !autoPassphrase)
                    }
                    onClick={() => void runAutoNow()}
                  >
                    {autoRunning ? "自动备份中…" : "按当前配置立即运行"}
                  </button>
                </div>
              </div>

              {/* 覆盖口令确认弹窗：已设置口令时输入新口令保存需确认 */}
              <div
                className={`modal-overlay ${confirmOverwrite ? "visible" : ""}`}
                {...overwriteDismiss}
              >
                <div className="modal">
                  <div className="modal-header">
                    <h2 className="modal-title">覆盖自动备份口令</h2>
                    <button
                      className="modal-close"
                      onClick={() => !autoSaving && setConfirmOverwrite(false)}
                      disabled={autoSaving}
                    >
                      <X size={16} />
                    </button>
                  </div>
                  <div className="modal-body">
                    <p>已设置自动备份口令。保存后将用新口令覆盖原口令，原口令无法找回。</p>
                  </div>
                  <div className="modal-footer">
                    <button
                      type="button"
                      className="btn btn-secondary"
                      onClick={() => setConfirmOverwrite(false)}
                      disabled={autoSaving}
                    >
                      取消
                    </button>
                    <button
                      type="button"
                      className="btn btn-primary"
                      onClick={() => void confirmOverwritePassphrase()}
                      disabled={autoSaving}
                    >
                      {autoSaving ? "保存中…" : "覆盖口令并保存"}
                    </button>
                  </div>
                </div>
              </div>
            </section>

            {/* 恢复备份 */}
            <section className="backup-section">
              <div className="backup-section-head">
                <h2 className="backup-section-title">恢复备份</h2>
              </div>
              <div className="backup-card backup-restore">
                <p className="backup-upload-dir-hint">
                  从 WebDAV 上传目录（含其下 auto 自动备份子目录）拉取备份列表，按 manifest
                  还原到本机路径。加密包需填写备份时使用的口令。恢复会覆盖同名文件，请谨慎操作。
                </p>

                {connections.length === 0 ? (
                  <div className="backup-empty-inline">请先在上方 WebDAV 区块添加连接</div>
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
                          void loadRemoteList(restoreConnId, uploadDir.trim() || "Octopus")
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
                          const itemKey = remoteItemKey(item);
                          const checked = selectedRemote === itemKey;
                          return (
                            <li key={itemKey} className="backup-unit-item">
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
                                  onChange={() => setSelectedRemote(itemKey)}
                                  disabled={running}
                                />
                                <CheckGlyph />
                                <span className="backup-unit-body">
                                  <span className="backup-unit-label-row">
                                    <span className="backup-unit-label">{item.name}</span>
                                    <span className="backup-unit-size">
                                      {formatBytes(item.bytes)}
                                    </span>
                                    {item.encrypted ? (
                                      <span className="backup-badge backup-badge-secret">加密</span>
                                    ) : (
                                      <span className="backup-badge">明文</span>
                                    )}
                                    {item.auto && <span className="backup-badge">自动</span>}
                                  </span>
                                  {item.lastModified ? (
                                    <span className="backup-unit-path">{item.lastModified}</span>
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
                        <div className="form-input-with-action">
                          <input
                            type={restorePassphraseVisible ? "text" : "password"}
                            className="form-input"
                            autoComplete="current-password"
                            value={restorePassphrase}
                            onChange={(e) => setRestorePassphrase(e.target.value)}
                            placeholder="加密备份的口令"
                            disabled={running}
                          />
                          <button
                            type="button"
                            className="form-input-action"
                            data-tooltip={
                              restorePassphraseVisible ? "隐藏备份口令" : "显示备份口令"
                            }
                            aria-label={restorePassphraseVisible ? "隐藏备份口令" : "显示备份口令"}
                            aria-pressed={restorePassphraseVisible}
                            onClick={() => setRestorePassphraseVisible((visible) => !visible)}
                            disabled={running}
                          >
                            {restorePassphraseVisible ? <EyeOff size={16} /> : <Eye size={16} />}
                          </button>
                        </div>
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
                        className={`backup-result ${restoreResult.ok ? "is-ok" : "is-fail"}`}
                        style={{ marginTop: 12 }}
                      >
                        <div className="backup-result-msg">{restoreResult.message}</div>
                        <div className="backup-result-meta">
                          还原 {restoreResult.restoredFiles} · 跳过 {restoreResult.skippedFiles}
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
                    <span className="backup-progress-phase">{phaseLabel(progress.phase)}</span>
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
                            ? Math.min(100, Math.round((progress.current / progress.total) * 100))
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
                <div className={`backup-card backup-result ${result.ok ? "is-ok" : "is-fail"}`}>
                  <div className="backup-result-msg">{result.message}</div>
                  <div className="backup-result-meta">
                    文件：{result.archiveFileName} · {formatBytes(result.archiveBytes)}
                    {result.encrypted ? " · 已加密" : " · 明文 zip"}
                  </div>
                  <ul className="backup-result-targets">
                    {result.targets.map((t) => (
                      <li key={t.connectionId}>
                        <span className={t.ok ? "backup-badge-ok" : "backup-badge-fail"}>
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
                    <div className="backup-unit-warn">{result.warnings.join("；")}</div>
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
