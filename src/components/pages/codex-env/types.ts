/** CodexEnv 页的类型定义（从页面组件抽出，供页面与 api 层共享）。 */

/** 特殊供应商哨兵：让 Codex CLI 使用官方 OAuth 登录，不写第三方 API 配置。 */
export const OFFICIAL_OAUTH_PROVIDER_ID = "__codex_official_oauth__";

export interface CodexEnvironment {
  id: string;
  name: string;
  slug: string;
  configDir: string;
  aliasName: string;
  isDefault: boolean;
  source: string;
  notes: string;
  aliasInstalled: boolean;
  dirExists: boolean;
  hasConfig: boolean;
  hasSkills: boolean;
  skillCount?: number;
  skillsSyncStatus?: string;
  hasAuth: boolean;
  mcpSyncStatus?: string;
  mcpServerCount?: number;
  globalMcpServerCount?: number;
  // config.toml 实时读取（不入库）
  model: string;
  modelProvider: string;
  baseUrl: string;
  // 列表接口不回传明文 token；编辑时经 get_codex_env_secret 拉取。
  apiKey: string;
  // 关联的 AI 供应商 ID（空串=未关联）。供应商更新时自动反向同步。
  providerId: string;
  createdAt: number;
  updatedAt: number;
}

export interface CodexEnvCandidate {
  path: string;
  suggestedName: string;
  suggestedSlug: string;
  suggestedAlias: string;
  hasConfig: boolean;
  hasSkills: boolean;
  hasAuth: boolean;
}

export interface CodexEnvSniffResult {
  candidates: CodexEnvCandidate[];
  message: string;
}

export interface CodexEnvShellStatus {
  /** @deprecated 使用 shellConfigPath；兼容旧字段名 */
  zshrcPath: string;
  /** @deprecated 使用 shellConfigExists */
  zshrcExists: boolean;
  /** shell 配置文件路径（zshrc / bash_profile / fish / PowerShell profile） */
  shellConfigPath?: string;
  shellConfigExists?: boolean;
  /** zsh / bash / fish / powershell */
  shellKind?: string;
  blockPresent: boolean;
  aliases: string[];
  preview: string;
  message: string;
}

export interface CodexEnvActionResult {
  ok: boolean;
  message: string;
  environment: CodexEnvironment | null;
}

export interface CodexEnvMcpSyncResult {
  ok: boolean;
  message: string;
  globalServerCount: number;
  globalServerNames: string[];
  results: Array<{
    id: string;
    name: string;
    ok: boolean;
    status: string;
    serverCount: number;
    message: string;
  }>;
}
