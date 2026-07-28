import type {
  AgentConfigRequest,
  CheckResult,
  InitMode,
  InitResult,
  McpServerDraft,
  SkillInstallMode,
  SkillOption,
} from "./types";

export async function invokePickProjectFolder(): Promise<string | null> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<string | null>("pick_project_folder");
}

/** MCP servers configured in AgentBuddy (SQLite list), used to populate the picker. */
export async function invokeListMcpServers(): Promise<McpServerDraft[]> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<McpServerDraft[]>("get_mcp_servers");
}

/** Skills in the AgentBuddy library, used to populate the picker. */
export async function invokeListSkillOptions(): Promise<SkillOption[]> {
  const { invoke } = await import("@tauri-apps/api/core");
  const res = await invoke<{ skills: SkillOption[] }>("list_skills");
  return res.skills;
}

export async function invokeCheckProjectConfig(
  targetDir: string,
  selectedAgents: AgentConfigRequest[],
  mode: InitMode,
  skillIds: string[],
): Promise<CheckResult> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<CheckResult>("check_project_config_exists", { targetDir, selectedAgents, mode, skillIds });
}

export async function invokeInitProjectConfig(
  targetDir: string,
  selectedAgents: AgentConfigRequest[],
  mode: InitMode,
  overwrite: boolean,
  mcpServers: McpServerDraft[],
  skillIds: string[],
  skillMode: SkillInstallMode,
): Promise<InitResult> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<InitResult>("init_project_config", { targetDir, selectedAgents, mode, overwrite, mcpServers, skillIds, skillMode });
}
