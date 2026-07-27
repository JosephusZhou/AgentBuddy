/**
 * Keep in sync with `src-tauri/src/project_config.rs` (`AGENT_SPECS`).
 * Backend is source of truth for paths; this list drives the picker UI only.
 * Excluded (same as Rust): claude-desktop, codebuddy（国际版已移除，仅保留 CodeBuddy CN）.
 */
export type InitMode = "full" | "symlink";

export interface AgentConfigRequest {
  name: string;
}

export interface ExistingItem {
  path: string;
  isDir: boolean;
}

export interface CheckResult {
  existing: ExistingItem[];
}

export interface InitResult {
  created: string[];
  skipped: string[];
  errors: string[];
}

export interface AgentProjectInfo {
  name: string;
  displayName: string;
  rootFile: string | null;
  configDir: string;
}

export const AGENT_PROJECT_INFOS: AgentProjectInfo[] = [
  { name: "claude-code",  displayName: "Claude Code",  rootFile: "CLAUDE.md",  configDir: ".claude"    },
  { name: "codex",        displayName: "Codex",        rootFile: "AGENTS.md",  configDir: ".codex"     },
  { name: "opencode",     displayName: "OpenCode",     rootFile: "AGENTS.md",  configDir: ".opencode"  },
  { name: "antigravity",  displayName: "Antigravity",  rootFile: "GEMINI.md",  configDir: ".gemini"    },
  { name: "codebuddy-cn", displayName: "CodeBuddy CN", rootFile: "AGENTS.md",  configDir: ".codebuddy" },
  { name: "workbuddy",    displayName: "WorkBuddy",    rootFile: "AGENTS.md",  configDir: ".workbuddy" },
  { name: "deveco-code",  displayName: "DevEco Code",  rootFile: "AGENTS.md",  configDir: ".deveco"    },
];
