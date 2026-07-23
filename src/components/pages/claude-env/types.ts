/** ClaudeEnv 页的类型定义（从页面组件抽出，供页面与 api 层共享）。 */

export interface ClaudeEnvironment {
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
  hasSettings: boolean;
  hasSkills: boolean;
  hasAgents: boolean;
  mcpSyncStatus?: string;
  mcpServerCount?: number;
  globalMcpServerCount?: number;
  // settings.json → env 节点实时读取（不入库），缺失为空串。
  baseUrl: string;
  // 列表接口出于安全不回传明文 token，此字段恒为空；编辑时经 get_claude_env_secret 拉取。
  apiKey: string;
  hasApiKey: boolean;
  model: string;
  createdAt: number;
  updatedAt: number;
}

export interface ClaudeEnvCandidate {
  path: string;
  suggestedName: string;
  suggestedSlug: string;
  suggestedAlias: string;
  hasSettings: boolean;
  hasSkills: boolean;
  hasAgents: boolean;
}

export interface ClaudeEnvSniffResult {
  candidates: ClaudeEnvCandidate[];
  message: string;
}

export interface ClaudeEnvShellStatus {
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

export interface ClaudeEnvActionResult {
  ok: boolean;
  message: string;
  environment: ClaudeEnvironment | null;
}

export interface ClaudeEnvMcpSyncResult {
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
