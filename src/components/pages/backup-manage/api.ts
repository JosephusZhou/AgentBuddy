import type {
  AutoBackupSettings,
  AutoBackupSettingsUpdate,
  AutoBackupStatus,
  BackupRunPayload,
  BackupRunResult,
  BackupSettings,
  BackupUnitNode,
  RemoteBackupItem,
  RestoreBackupResult,
  WebDAVConnectionLite,
} from "./types";

export async function listBackupUnits(): Promise<BackupUnitNode[]> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("list_backup_units") as Promise<BackupUnitNode[]>;
}

export async function getBackupSettings(): Promise<BackupSettings> {
  const { invoke } = await import("@tauri-apps/api/core");
  const r = await (invoke("get_backup_settings") as Promise<Partial<BackupSettings>>);
  return {
    cliproxyapiConfPath: r.cliproxyapiConfPath ?? "",
    sub2apiRootPath: r.sub2apiRootPath ?? "",
    defaultRemoteDir: r.defaultRemoteDir?.trim() || "AgentBuddy",
  };
}

export async function updateBackupSettings(settings: BackupSettings): Promise<BackupSettings> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("update_backup_settings", { settings }) as Promise<BackupSettings>;
}

export async function runBackupUpload(payload: BackupRunPayload): Promise<BackupRunResult> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("run_backup_upload", { payload }) as Promise<BackupRunResult>;
}

export async function getAutoBackupSettings(): Promise<AutoBackupSettings> {
  const { invoke } = await import("@tauri-apps/api/core");
  const r = await (invoke("get_auto_backup_settings") as Promise<Partial<AutoBackupSettings>>);
  return {
    enabled: !!r.enabled,
    mode: r.mode === "daily" ? "daily" : "interval",
    intervalHours: typeof r.intervalHours === "number" ? r.intervalHours : 24,
    dailyTime: r.dailyTime ?? "",
    unitIds: r.unitIds ?? [],
    webdavConnectionIds: r.webdavConnectionIds ?? [],
    hasPassphrase: !!r.hasPassphrase,
  };
}

export async function updateAutoBackupSettings(
  update: AutoBackupSettingsUpdate,
): Promise<AutoBackupSettings> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("update_auto_backup_settings", { update }) as Promise<AutoBackupSettings>;
}

export async function getAutoBackupStatus(): Promise<AutoBackupStatus> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("get_auto_backup_status") as Promise<AutoBackupStatus>;
}

/** 按自动备份配置立即执行一轮（走同一互斥锁与 auto 远端子目录）。 */
export async function runAutoBackupNow(): Promise<BackupRunResult> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("run_auto_backup_now") as Promise<BackupRunResult>;
}

export async function listRemoteBackups(
  connectionId: string,
  remotePrefix?: string,
): Promise<RemoteBackupItem[]> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("list_remote_backups", {
    payload: {
      connectionId,
      remotePrefix: remotePrefix || undefined,
    },
  }) as Promise<RemoteBackupItem[]>;
}

export async function restoreRemoteBackup(payload: {
  connectionId: string;
  fileName: string;
  remotePrefix?: string;
  passphrase?: string;
}): Promise<RestoreBackupResult> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("restore_remote_backup", { payload }) as Promise<RestoreBackupResult>;
}

export async function listWebDavConnections(): Promise<WebDAVConnectionLite[]> {
  const { invoke } = await import("@tauri-apps/api/core");
  const rows = await (invoke("get_webdav_connections") as Promise<
    Array<{
      id: string;
      name: string;
      url: string;
      username: string;
      status?: string;
    }>
  >);
  return rows.map((r) => ({
    id: r.id,
    name: r.name,
    url: r.url,
    username: r.username,
    status: r.status === "connected" ? "connected" : "disconnected",
  }));
}

export function formatBytes(n: number): string {
  if (!Number.isFinite(n) || n < 0) return "0 B";
  if (n >= 1024 * 1024) return `${(n / (1024 * 1024)).toFixed(1)} MB`;
  if (n >= 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${n} B`;
}

/** Collect all available leaf unit ids under nodes (for select-all / parent toggle). */
export function collectAvailableIds(nodes: BackupUnitNode[]): string[] {
  const ids: string[] = [];
  const walk = (list: BackupUnitNode[]) => {
    for (const n of list) {
      const kids = n.children ?? [];
      if (kids.length > 0) {
        walk(kids);
      } else if (n.available && n.kind !== "group") {
        ids.push(n.id);
      }
    }
  };
  walk(nodes);
  return ids;
}
