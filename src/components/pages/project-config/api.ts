import type { AgentConfigRequest, CheckResult, InitMode, InitResult } from "./types";

export async function invokePickProjectFolder(): Promise<string | null> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<string | null>("pick_project_folder");
}

export async function invokeCheckProjectConfig(
  targetDir: string,
  selectedAgents: AgentConfigRequest[],
  mode: InitMode,
): Promise<CheckResult> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<CheckResult>("check_project_config_exists", { targetDir, selectedAgents, mode });
}

export async function invokeInitProjectConfig(
  targetDir: string,
  selectedAgents: AgentConfigRequest[],
  mode: InitMode,
  overwrite: boolean,
): Promise<InitResult> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<InitResult>("init_project_config", { targetDir, selectedAgents, mode, overwrite });
}
