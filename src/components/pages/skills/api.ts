/** SkillsManage 页的 Tauri 命令封装（从页面组件抽出）。 */

import type {
  SkillsListResult,
  SniffPreviewResult,
  SniffImportResult,
  SkillUpdateCheckResult,
  SkillActionResult,
  SkillApplyResult,
  CcSwitchPreviewResult,
  CcSwitchMigrateResult,
  BatchSkillResult,
  BatchApplyMode,
  SkillInstallMode,
} from "./types";

export async function invokeListSkills(): Promise<SkillsListResult> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("list_skills") as Promise<SkillsListResult>;
}

export async function invokePreviewSniff(): Promise<SniffPreviewResult> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("preview_sniff_skills") as Promise<SniffPreviewResult>;
}

export async function invokeImportSniffed(keys: string[]): Promise<SniffImportResult> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("import_sniffed_skills", { keys }) as Promise<SniffImportResult>;
}

export async function invokeCheckUpdates(): Promise<SkillUpdateCheckResult> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("check_skill_updates") as Promise<SkillUpdateCheckResult>;
}

export async function invokePickLocal(tag: string): Promise<SkillActionResult> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("pick_and_add_skill_local", { tag }) as Promise<SkillActionResult>;
}

export async function invokeAddGithub(url: string, tag: string): Promise<SkillActionResult> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("add_skill_github", { url, tag }) as Promise<SkillActionResult>;
}

export async function invokeAddGitcode(url: string, tag: string): Promise<SkillActionResult> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("add_skill_gitcode", { url, tag }) as Promise<SkillActionResult>;
}

export async function invokeExportSkill(skillId: string): Promise<SkillActionResult> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("export_skill_to_dir", { skillId }) as Promise<SkillActionResult>;
}

export async function invokeUpdateSkill(skillId: string): Promise<SkillActionResult> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("update_skill", { skillId }) as Promise<SkillActionResult>;
}

export async function invokeDeleteSkill(
  skillId: string,
  deleteAgentCopies: boolean
): Promise<SkillActionResult> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("delete_skill", {
    skillId,
    deleteAgentCopies,
  }) as Promise<SkillActionResult>;
}

export async function invokeApplySkill(
  skillId: string,
  agents: string[],
  tag: string,
  installMode: SkillInstallMode
): Promise<SkillApplyResult> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("apply_skill_to_agents", {
    skillId,
    agents,
    tag,
    installMode,
  }) as Promise<SkillApplyResult>;
}

export async function invokeAddLocalPath(path: string, tag: string): Promise<SkillActionResult> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("add_skill_local", { path, tag }) as Promise<SkillActionResult>;
}

export async function invokePreviewCcSwitch(): Promise<CcSwitchPreviewResult> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("preview_cc_switch_skills") as Promise<CcSwitchPreviewResult>;
}

export async function invokeMigrateCcSwitch(ccIds: string[]): Promise<CcSwitchMigrateResult> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("migrate_cc_switch_skills", { ccIds }) as Promise<CcSwitchMigrateResult>;
}

export async function invokeBatchDelete(
  skillIds: string[],
  deleteAgentCopies: boolean
): Promise<BatchSkillResult> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("batch_delete_skills", {
    skillIds,
    deleteAgentCopies,
  }) as Promise<BatchSkillResult>;
}

export async function invokeBatchExport(skillIds: string[]): Promise<BatchSkillResult> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("batch_export_skills_to_dir", { skillIds }) as Promise<BatchSkillResult>;
}

export async function invokeBatchApply(
  skillIds: string[],
  agents: string[],
  mode: BatchApplyMode,
  installMode: SkillInstallMode
): Promise<BatchSkillResult> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("batch_apply_skills_to_agents", {
    skillIds,
    agents,
    mode,
    installMode,
  }) as Promise<BatchSkillResult>;
}

export async function invokeBatchSetTag(
  skillIds: string[],
  tag: string
): Promise<BatchSkillResult> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("batch_set_skill_tag", {
    skillIds,
    tag,
  }) as Promise<BatchSkillResult>;
}
