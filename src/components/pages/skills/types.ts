/** SkillsManage 页的类型定义（从页面组件抽出，供页面与 api 层共享）。 */

export type SkillSource = "local" | "github" | "gitcode";

export interface SkillRecord {
  id: string;
  title: string;
  description: string;
  source: SkillSource;
  repoUrl: string;
  githubOwner: string;
  githubRepo: string;
  githubPath: string;
  localPath: string;
  tag: string;
  appliedAgents: string[];
  createdAt: number;
  updatedAt: number;
  updateAvailable: boolean;
}

export interface AgentResult {
  name: string;
  display_name: string;
  icon: string;
  found: boolean;
}

export interface SkillsListResult {
  skills: SkillRecord[];
  message: string;
}

export interface SniffPreviewItem {
  key: string;
  directory: string;
  title: string;
  description: string;
  sourcePath: string;
  foundAgents: string[];
  status: "import" | "skip_exists" | string;
  statusLabel: string;
}

export interface SniffPreviewResult {
  ok: boolean;
  items: SniffPreviewItem[];
  scannedAgents: number;
  total: number;
  importable: number;
  skipExists: number;
  message: string;
}

export interface SniffImportResult {
  ok: boolean;
  imported: number;
  skipped: number;
  failed: number;
  skills: SkillRecord[];
  message: string;
  errors: string[];
}

export interface SkillUpdateCheckResult {
  skills: SkillRecord[];
  checked: number;
  updates: number;
  message: string;
}

export interface SkillActionResult {
  ok: boolean;
  skill: SkillRecord | null;
  message: string;
}

export interface SkillApplyResult {
  ok: boolean;
  skill: SkillRecord | null;
  linked: number;
  unlinked: number;
  message: string;
  errors: string[];
}

// 与后端 agent_skills_targets 中 supported=false 对齐：这些 Agent 无标准全局 Skills 目录
export const SKILL_UNSUPPORTED_AGENTS = new Set<string>(["claude-desktop"]);

export interface CcSwitchPreviewItem {
  ccId: string;
  directory: string;
  title: string;
  description: string;
  source: SkillSource;
  repoUrl: string;
  githubOwner: string;
  githubRepo: string;
  githubPath: string;
  sourcePath: string;
  enabledAgents: string[];
  status: "import" | "skip_exists" | "missing" | string;
  statusLabel: string;
}

export interface CcSwitchPreviewResult {
  ok: boolean;
  items: CcSwitchPreviewItem[];
  total: number;
  importable: number;
  skipExists: number;
  missing: number;
  message: string;
  ccSwitchRoot: string;
}

export interface CcSwitchMigrateResult {
  ok: boolean;
  imported: number;
  skipped: number;
  failed: number;
  skills: SkillRecord[];
  message: string;
  errors: string[];
}

// 批量操作（删除 / 复制 / 应用）的统一聚合返回，skills 为刷新后的整表
export interface BatchSkillResult {
  ok: boolean;
  succeeded: number;
  failed: number;
  skipped: number;
  skills: SkillRecord[];
  message: string;
  errors: string[];
}

export type BatchApplyMode = "add" | "replace";
export type SkillInstallMode = "link" | "copy";
