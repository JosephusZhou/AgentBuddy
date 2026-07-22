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
  phase: "collect" | "zip" | "encrypt" | "upload" | "finalize" | "download" | "decrypt" | "restore" | string;
  current: number;
  total: number;
  message: string;
  connectionId?: string | null;
}

export interface RemoteBackupItem {
  name: string;
  bytes: number;
  lastModified: string;
  encrypted: boolean;
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
