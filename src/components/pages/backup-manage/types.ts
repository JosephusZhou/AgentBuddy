export interface BackupUnitNode {
  id: string;
  label: string;
  kind: string;
  available: boolean;
  selectedByDefault: boolean;
  containsSecrets: boolean;
  estimatedBytes: number;
  pathSummary: string;
  warnings: string[];
  children?: BackupUnitNode[];
}

export interface BackupSettings {
  cliproxyapiConfPath: string;
  sub2apiRootPath: string;
  defaultRemoteDir: string;
}

export type AutoBackupMode = "interval" | "daily";

/** 自动备份设置（后端 DTO 不含口令，只含 hasPassphrase 存在性标志）。 */
export interface AutoBackupSettings {
  enabled: boolean;
  mode: AutoBackupMode;
  intervalHours: number;
  dailyTime: string;
  unitIds: string[];
  webdavConnectionIds: string[];
  hasPassphrase: boolean;
}

export interface AutoBackupSettingsUpdate {
  enabled: boolean;
  mode: AutoBackupMode;
  intervalHours: number;
  dailyTime: string;
  unitIds: string[];
  webdavConnectionIds: string[];
  /** undefined = 不变；"" = 清除；其余 = 设置新口令。 */
  passphrase?: string;
}

/** Event payload for `get_auto_backup_status` (from Rust `AutoBackupStatus`). */
export interface AutoBackupStatus {
  running: boolean;
  enabled: boolean;
  /** Unix 秒；未启用时为 null。 */
  nextRunAt: number | null;
  lastRunAt: number | null;
  lastOk: boolean | null;
  lastMessage: string | null;
}

export interface BackupRunPayload {
  unitIds: string[];
  webdavConnectionIds: string[];
  passphrase?: string;
  remotePrefix?: string;
  acknowledgePlaintextSecrets: boolean;
}

export interface BackupUploadTargetResult {
  connectionId: string;
  name: string;
  ok: boolean;
  message: string;
  remotePath: string;
}

export interface BackupRunResult {
  ok: boolean;
  archiveFileName: string;
  archiveBytes: number;
  encrypted: boolean;
  targets: BackupUploadTargetResult[];
  warnings: string[];
  message: string;
}

/** Event payload for `backup-progress` (from Rust `BackupProgressEvent`). */
export interface BackupProgressEvent {
  phase:
    | "collect"
    | "zip"
    | "encrypt"
    | "upload"
    | "download"
    | "decrypt"
    | "restore"
    | "finalize"
    | string;
  current: number;
  total: number;
  message: string;
  connectionId?: string | null;
  /** manual | auto；旧事件无该字段。 */
  trigger?: "manual" | "auto" | string | null;
  /** 仅 finalize 汇总事件携带：本轮是否成功；其余进度事件无该字段。 */
  ok?: boolean | null;
}

export interface RemoteBackupItem {
  name: string;
  bytes: number;
  lastModified: string;
  encrypted: boolean;
  /** 前端标记：条目所在远端目录（Rust DTO 不含）；恢复时按该目录下载。 */
  dir?: string;
  /** 前端标记：来自 <上传目录>/auto 的自动备份（Rust DTO 不含）。 */
  auto?: boolean;
}

export interface RestoreBackupResult {
  ok: boolean;
  message: string;
  restoredFiles: number;
  skippedFiles: number;
  warnings: string[];
}

export interface WebDAVConnectionLite {
  id: string;
  name: string;
  url: string;
  username: string;
  status: string;
}
