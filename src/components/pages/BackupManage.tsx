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
  setBackupPassphrase,
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
  const [loading, setLoading] = useState(true);
  const [running, setRunning] = useState(false);
  const [result, setResult] = useState<BackupRunResult | null>(null);
  const [progress, setProgress] = useState<BackupProgressEvent | null>(null);

  // 备份口令（手动/自动共用）：不可回读，只有 hasPassphrase；输入新口令保存即覆盖。
  const [hasPassphrase, setHasPassphrase] = useState(false);
  const [passphraseInput, setPassphraseInput] = useState("");
  const [passphraseInput2, setPassphraseInput2] = useState("");
  const [passphraseInputVisible, setPassphraseInputVisible] = useState(false);
  const [passphraseInput2Visible, setPassphraseInput2Visible] = useState(false);
  const [passphraseSaving, setPassphraseSaving] = useState(false);
  /** 已设置口令且输入新口令时，保存前弹窗确认覆盖。 */
  const [confirmOverwrite, setConfirmOverwrite] = useState(false);
  const overwriteDismiss = useOverlayDismiss(() => setConfirmOverwrite(false), passphraseSaving);

  // 手动备份加密方式：null = 未选择（此时不能点立即备份）
  const [manualMode, setManualMode] = useState<"encrypted" | "plaintext" | null>(null);

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
  const [autoSaving, setAutoSaving] = useState(false);
  const [autoRunning, setAutoRunning] = useState(false);
  /** 每天时刻输入的自动保存防抖 timer。 */
  const autoTimerRef = useRef<number | null>(null);

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
      setHasPassphrase(auto.hasPassphrase);
      // 自动备份与手动共用备份范围，无需单独恢复 autoSelected。
      setAutoSettings(auto);
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
    const next = new Set(selected);
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
    setSelected(next);
    // 自动备份与手动共用备份范围：变化即静默同步落盘。
    void persistAutoSettings(autoSettings, selectedDav, next, true);
  };

  const selectAllUnits = (checked: boolean) => {
    const next = checked ? new Set(availableLeafIds) : new Set<string>();
    setSelected(next);
    void persistAutoSettings(autoSettings, selectedDav, next, true);
  };

  const onToggleDav = (id: string, checked: boolean) => {
    const next = new Set(selectedDav);
    if (checked) next.add(id);
    else next.delete(id);
    setSelectedDav(next);
    // WebDAV 目标手动/自动共用：变化即静默落盘自动备份配置。
    void persistAutoSettings(autoSettings, next, undefined, true);
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
    if (!manualMode) {
      setStatusMsg("请选择备份方式（口令加密或明文上传）");
      return;
    }
    if (manualMode === "encrypted" && !hasPassphrase) {
      setStatusMsg("请先在上方「备份口令」卡片设置口令");
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
        remotePrefix,
        // 加密分支由后端解密共享口令；明文分支确认含密钥风险。
        useStoredPassphrase: manualMode === "encrypted",
        acknowledgePlaintextSecrets: manualMode === "plaintext",
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

  /** 把指定状态立即落盘到自动备份配置（无「保存」按钮，改动即生效）。
   *  备份范围与手动备份共用；显式传入 nextSelected，未传则读当前 state。
   *  quiet = true 时（由范围/WebDAV 勾选触发）校验失败不弹 Toast。 */
  const persistAutoSettings = async (
    nextSettings: AutoBackupSettings,
    nextDav: Set<string>,
    nextSelected?: Set<string>,
    quiet = false,
  ): Promise<boolean> => {
    const units = nextSelected ?? selected;
    if (units.size === 0 || nextDav.size === 0) {
      if (!quiet) {
        setStatusMsg(units.size === 0 ? "请至少选择一个备份范围" : "请至少选择一个 WebDAV 目标");
      }
      return false;
    }
    setAutoSaving(true);
    try {
      const saved = await updateAutoBackupSettings({
        enabled: nextSettings.enabled,
        mode: nextSettings.mode,
        intervalHours: nextSettings.intervalHours,
        dailyTime: nextSettings.dailyTime.trim() || "09:30",
        unitIds: Array.from(units),
        // WebDAV 目标勾选为手动/自动共用，落盘当前共享勾选。
        webdavConnectionIds: Array.from(nextDav),
      });
      setAutoSettings(saved);
      setHasPassphrase(saved.hasPassphrase);
      return true;
    } catch (e) {
      setStatusMsg(
        `自动备份设置保存失败：${e instanceof Error ? e.message : String(e ?? "未知错误")}`,
      );
      return false;
    } finally {
      setAutoSaving(false);
    }
  };

  /** 共享口令卡片：保存新口令；已设置时先弹窗确认覆盖。 */
  const handleSavePassphrase = () => {
    if (!passphraseInput) {
      setStatusMsg("请输入新口令");
      return;
    }
    if (passphraseInput !== passphraseInput2) {
      setStatusMsg("两次输入的口令不一致");
      return;
    }
    if (hasPassphrase) {
      setConfirmOverwrite(true);
      return;
    }
    void doSavePassphrase();
  };

  const doSavePassphrase = async () => {
    setConfirmOverwrite(false);
    setPassphraseSaving(true);
    try {
      const res = await setBackupPassphrase(passphraseInput);
      setHasPassphrase(res.hasPassphrase);
      setPassphraseInput("");
      setPassphraseInput2("");
      setStatusMsg("备份口令已保存");
    } catch (e) {
      setStatusMsg(`保存备份口令失败：${e instanceof Error ? e.message : String(e ?? "未知错误")}`);
    } finally {
      setPassphraseSaving(false);
    }
  };

  const runAutoNow = async () => {
    // 先 flush 每天时刻输入可能挂起的防抖保存。
    if (autoTimerRef.current) {
      window.clearTimeout(autoTimerRef.current);
      autoTimerRef.current = null;
    }
    setAutoRunning(true);
    try {
      // 后端按已保存配置执行：先把当前表单状态落盘，避免运行的不是看到的配置。
      const saved = await persistAutoSettings(autoSettings, selectedDav);
      if (!saved) return;
      const res = await runAutoBackupNow();
      setStatusMsg(res.message);
    } catch (e) {
      setStatusMsg(`自动备份失败：${e instanceof Error ? e.message : String(e ?? "未知错误")}`);
    } finally {
      setAutoRunning(false);
      void refreshAutoStatus();
    }
  };

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

  const canStart =
    !loading &&
    !running &&
    selected.size > 0 &&
    selectedDav.size > 0 &&
    !!manualMode &&
    (manualMode === "plaintext" || hasPassphrase);

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

            {/* 备份口令（手动/自动共用）：不可回读，输入新口令保存即覆盖 */}
            <section className="backup-section">
              <div className="backup-section-head">
                <h2 className="backup-section-title">备份口令</h2>
                {hasPassphrase && <span className="backup-badge backup-badge-ok">已设置</span>}
              </div>
              <div className="backup-card backup-options">
                <p className="backup-upload-dir-hint">
                  加密备份时使用的口令，手动与自动备份共用。口令只加密落盘、不可回读；
                  恢复加密备份时需手动输入当时设置的口令。
                </p>
                <div className="backup-field-row">
                  <label className="backup-field">
                    <span className="backup-field-label">
                      {hasPassphrase ? "新口令（留空则不修改）" : "口令"}
                    </span>
                    <div className="form-input-with-action">
                      <input
                        type={passphraseInputVisible ? "text" : "password"}
                        className="form-input"
                        autoComplete="new-password"
                        spellCheck={false}
                        value={passphraseInput}
                        onChange={(e) => setPassphraseInput(e.target.value)}
                        placeholder={
                          hasPassphrase ? "输入新口令可覆盖原口令" : "用于加密备份压缩包"
                        }
                        disabled={passphraseSaving}
                      />
                      <button
                        type="button"
                        className="form-input-action"
                        data-tooltip={passphraseInputVisible ? "隐藏口令" : "显示口令"}
                        aria-label={passphraseInputVisible ? "隐藏口令" : "显示口令"}
                        aria-pressed={passphraseInputVisible}
                        onClick={() => setPassphraseInputVisible((v) => !v)}
                        disabled={passphraseSaving}
                      >
                        {passphraseInputVisible ? <EyeOff size={16} /> : <Eye size={16} />}
                      </button>
                    </div>
                  </label>
                  <label className="backup-field">
                    <span className="backup-field-label">确认口令</span>
                    <div className="form-input-with-action">
                      <input
                        type={passphraseInput2Visible ? "text" : "password"}
                        className="form-input"
                        autoComplete="new-password"
                        spellCheck={false}
                        value={passphraseInput2}
                        onChange={(e) => setPassphraseInput2(e.target.value)}
                        placeholder="再次输入"
                        disabled={passphraseSaving || !passphraseInput}
                      />
                      <button
                        type="button"
                        className="form-input-action"
                        data-tooltip={passphraseInput2Visible ? "隐藏确认口令" : "显示确认口令"}
                        aria-label={passphraseInput2Visible ? "隐藏确认口令" : "显示确认口令"}
                        aria-pressed={passphraseInput2Visible}
                        onClick={() => setPassphraseInput2Visible((v) => !v)}
                        disabled={passphraseSaving || !passphraseInput}
                      >
                        {passphraseInput2Visible ? <EyeOff size={16} /> : <Eye size={16} />}
                      </button>
                    </div>
                  </label>
                </div>
                <div className="backup-advanced-actions">
                  <button
                    type="button"
                    className="btn btn-primary"
                    onClick={handleSavePassphrase}
                    disabled={passphraseSaving || !passphraseInput}
                  >
                    {passphraseSaving ? "保存中…" : hasPassphrase ? "覆盖口令" : "设置口令"}
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
                    <h2 className="modal-title">覆盖备份口令</h2>
                    <button
                      className="modal-close"
                      onClick={() => !passphraseSaving && setConfirmOverwrite(false)}
                      disabled={passphraseSaving}
                    >
                      <X size={16} />
                    </button>
                  </div>
                  <div className="modal-body">
                    <p>已设置备份口令。保存后将用新口令覆盖原口令，原口令无法找回。</p>
                  </div>
                  <div className="modal-footer">
                    <button
                      type="button"
                      className="btn btn-secondary"
                      onClick={() => setConfirmOverwrite(false)}
                      disabled={passphraseSaving}
                    >
                      取消
                    </button>
                    <button
                      type="button"
                      className="btn btn-primary"
                      onClick={() => void doSavePassphrase()}
                      disabled={passphraseSaving}
                    >
                      {passphraseSaving ? "保存中…" : "覆盖口令并保存"}
                    </button>
                  </div>
                </div>
              </div>
            </section>

            {/* 手动备份 */}
            <section className="backup-section">
              <div className="backup-section-head">
                <h2 className="backup-section-title">手动备份</h2>
              </div>
              <div className="backup-card backup-options">
                {hasSecrets && (
                  <div className="backup-alert">
                    当前选择可能包含 API Key、OAuth 令牌或 secretsKey，建议使用口令加密。
                  </div>
                )}

                <div className="backup-mode-radios" role="radiogroup" aria-label="备份方式">
                  <label className="ui-radio backup-mode-radio">
                    <input
                      type="radio"
                      className="ui-radio-input"
                      name="manual-backup-mode"
                      checked={manualMode === "encrypted"}
                      disabled={running || !hasPassphrase}
                      onChange={() => setManualMode("encrypted")}
                    />
                    <span className="ui-radio-box" aria-hidden />
                    <span className="ui-radio-label">
                      使用口令加密 zip 上传含密钥的备份
                      {!hasPassphrase && (
                        <span className="backup-unit-warn">（需先在上方「备份口令」设置）</span>
                      )}
                    </span>
                  </label>
                  <label className="ui-radio backup-mode-radio">
                    <input
                      type="radio"
                      className="ui-radio-input"
                      name="manual-backup-mode"
                      checked={manualMode === "plaintext"}
                      disabled={running}
                      onChange={() => setManualMode("plaintext")}
                    />
                    <span className="ui-radio-box" aria-hidden />
                    <span className="ui-radio-label">
                      我已知晓风险：将以明文 zip 上传含密钥的备份
                    </span>
                  </label>
                </div>

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
                            : !manualMode
                              ? "请选择备份方式"
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

            {/* 自动备份：设置持久化在 config.json backup.auto；备份范围与 WebDAV 目标
                均与手动备份共用上方勾选；改动即自动保存 */}
            <section className="backup-section">
              <div className="backup-section-head">
                <h2 className="backup-section-title">自动备份</h2>
                {autoStatus?.running && (
                  <span className="backup-badge backup-badge-ok">备份进行中</span>
                )}
                {autoSaving && <span className="backup-unit-warn">保存中…</span>}
              </div>
              <div className="backup-card backup-auto">
                <p className="backup-upload-dir-hint">
                  应用运行期间按计划自动备份并上传到每个 WebDAV 的{" "}
                  <code>{uploadDir.trim() || "Octopus"}/auto</code> 子目录，命名前缀为{" "}
                  <code>octopus-auto-backup-*</code>
                  ，自动强制口令加密，仅保留最近 3
                  份，不与手动备份挤占保留窗口；应用未运行时不执行。
                </p>

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
                        const nextSettings: AutoBackupSettings = {
                          ...autoSettings,
                          mode: v === "daily" ? "daily" : "interval",
                        };
                        setAutoSettings(nextSettings);
                        void persistAutoSettings(nextSettings, selectedDav);
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
                          const nextSettings = {
                            ...autoSettings,
                            intervalHours: Number(v),
                          };
                          setAutoSettings(nextSettings);
                          void persistAutoSettings(nextSettings, selectedDav);
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
                          const nextSettings = { ...autoSettings, dailyTime: e.target.value };
                          setAutoSettings(nextSettings);
                          if (autoTimerRef.current) window.clearTimeout(autoTimerRef.current);
                          autoTimerRef.current = window.setTimeout(() => {
                            void persistAutoSettings(nextSettings, selectedDav);
                          }, 600);
                        }}
                        disabled={autoSaving || autoRunning}
                      />
                    </label>
                  )}
                </div>

                <label className="ui-check backup-ack">
                  <input
                    type="checkbox"
                    className="ui-check-input"
                    checked={autoSettings.enabled}
                    onChange={(e) => {
                      const nextEnabled = e.target.checked;
                      if (nextEnabled && !hasPassphrase) {
                        setStatusMsg("请先在上方「备份口令」卡片设置口令");
                        return;
                      }
                      const nextSettings = { ...autoSettings, enabled: nextEnabled };
                      setAutoSettings(nextSettings);
                      void persistAutoSettings(nextSettings, selectedDav);
                    }}
                    disabled={autoSaving || autoRunning}
                  />
                  <CheckGlyph />
                  <span>启用自动备份</span>
                </label>

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

                <div className="backup-advanced-actions">
                  <button
                    type="button"
                    className="btn btn-primary"
                    disabled={
                      running ||
                      autoRunning ||
                      selected.size === 0 ||
                      selectedDav.size === 0 ||
                      !hasPassphrase
                    }
                    onClick={() => void runAutoNow()}
                  >
                    {autoRunning ? "自动备份中…" : "按当前配置立即运行"}
                  </button>
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
