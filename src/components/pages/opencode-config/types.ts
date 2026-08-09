/** 通用模型配置页的共享类型（OpenCode / Pi / Oh-My-Pi）。 */

/** 支持可视化模型配置的 agent 标识。 */
export type ModelConfigAgentId = "opencode" | "pi" | "oh-my-pi";

export interface AgentVariantView {
  id: string;
  disabled?: boolean | null;
  reasoningEffort?: string | null;
  extra: Record<string, unknown>;
}

export interface AgentModelView {
  id: string;
  name?: string | null;
  limitContext?: number | null;
  limitInput?: number | null;
  limitOutput?: number | null;
  modalitiesInput: string[];
  modalitiesOutput: string[];
  reasoning?: boolean | null;
  toolCall?: boolean | null;
  attachment?: boolean | null;
  status?: string | null;
  thinkingType?: string | null;
  thinkingBudgetTokens?: number | null;
  reasoningEffort?: string | null;
  textVerbosity?: string | null;
  variants: AgentVariantView[];
  extraOptions: Record<string, unknown>;
}

export interface AgentProviderView {
  id: string;
  name?: string | null;
  npm?: string | null;
  api?: string | null;
  hasApiKey: boolean;
  /** auth | config | both | none */
  apiKeySource: string;
  baseUrl?: string | null;
  setCacheKey?: boolean | null;
  timeout?: number | null;
  chunkTimeout?: number | null;
  whitelist: string[];
  blacklist: string[];
  models: AgentModelView[];
}

export interface AgentModelConfigView {
  /** opencode | pi | oh-my-pi */
  agent: ModelConfigAgentId;
  configPath: string;
  configExists: boolean;
  isJsonc: boolean;
  /** Agent App/CLI 是否已安装（与 agent sniff 同规则；仅配置目录不算）。 */
  installed: boolean;
  /** 是否支持可视化编辑默认模型。 */
  defaultsSupported: boolean;
  /** 是否支持 small model（如 OpenCode 的 small_model）。 */
  smallModelSupported: boolean;
  model?: string | null;
  smallModel?: string | null;
  enabledProviders?: string[] | null;
  disabledProviders?: string[] | null;
  providers: AgentProviderView[];
  warnings: string[];
}

export interface AgentActionResult {
  ok: boolean;
  message: string;
  view?: AgentModelConfigView | null;
}

export interface CatalogReasoningOption {
  type: string;
  values?: string[] | null;
  min?: number | null;
}

export interface CatalogModelSummary {
  id: string;
  name: string;
  limitContext?: number | null;
  limitInput?: number | null;
  limitOutput?: number | null;
  modalitiesInput: string[];
  modalitiesOutput: string[];
  reasoning: boolean;
  reasoningOptions: CatalogReasoningOption[];
  toolCall: boolean;
  attachment: boolean;
  status?: string | null;
}

export interface CatalogProvider {
  id: string;
  name: string;
  env: string[];
  npm?: string | null;
  models: CatalogModelSummary[];
}

export interface ModelsDevCatalog {
  fetchedAt: number;
  fromCache: boolean;
  providers: CatalogProvider[];
}

export interface ProbeModelsResult {
  ok: boolean;
  message: string;
  modelIds: string[];
}

export interface SetDefaultsPayload {
  model?: string | null;
  smallModel?: string | null;
  enabledProviders?: string[] | null | undefined;
  disabledProviders?: string[] | null | undefined;
}

export interface UpsertProviderPayload {
  id: string;
  previousId?: string | null;
  name?: string | null;
  npm?: string | null;
  api?: string | null;
  baseUrl?: string | null;
  setCacheKey?: boolean | null;
  timeout?: number | null;
  chunkTimeout?: number | null;
  whitelist?: string[] | null;
  blacklist?: string[] | null;
  /** 三态：undefined 不改；"" 清除；有值则写入 */
  apiKey?: string | null;
}

export interface UpsertModelPayload {
  providerId: string;
  id: string;
  previousId?: string | null;
  name?: string | null;
  limitContext?: number | null;
  limitInput?: number | null;
  limitOutput?: number | null;
  modalitiesInput?: string[] | null;
  modalitiesOutput?: string[] | null;
  reasoning?: boolean | null;
  toolCall?: boolean | null;
  attachment?: boolean | null;
  status?: string | null;
  thinkingType?: string | null;
  thinkingBudgetTokens?: number | null;
  reasoningEffort?: string | null;
  textVerbosity?: string | null;
  variants?: AgentVariantView[] | null;
  extraOptions?: Record<string, unknown> | null;
  replaceExtraOptions?: boolean | null;
}

export const MODALITY_OPTIONS = ["text", "image", "pdf", "audio", "video"] as const;
export type Modality = (typeof MODALITY_OPTIONS)[number];

export const EFFORT_PRESETS = ["none", "minimal", "low", "medium", "high", "xhigh", "max"] as const;
