/** 供应商管理页的类型定义（与 Rust 端 ai_provider.rs 的 camelCase DTO 对齐）。 */

/** 单个 API 协议类型：可在一个供应商上多选组合。 */
export type ApiType = "anthropic" | "openai_chat" | "openai_responses";

/** 自定义模型条目：从供应商端点拉取后用户筛选保留的模型，可自定义别名 ID。 */
export interface CustomModel {
  model: string;
  aliasId: string;
}

export interface AiProvider {
  id: string;
  name: string;
  /** 已勾选的 API 类型集合（1~3 个）。 */
  providerTypes: ApiType[];
  /** Anthropic 侧 Base URL（勾选 anthropic 时有值）。 */
  baseUrl: string;
  /** OpenAI 侧 Base URL（勾选任一 OpenAI 类型时有值），用户独立填写、落库。 */
  openaiBaseUrl: string;
  /** Anthropic 侧默认模型。 */
  defaultModel: string;
  /** OpenAI 侧默认模型。 */
  openaiDefaultModel: string;
  /** Anthropic 档位模型覆盖（haiku/sonnet/opus/fable）；未勾选 anthropic 时为空对象。 */
  models: Record<string, string>;
  /** 列表接口不回传明文密钥，仅有此标志。 */
  hasApiKey: boolean;
  /** 已存储的 API Key 数量。 */
  apiKeyCount: number;
  /** 自定义模型列表。 */
  customModels: CustomModel[];
  notes: string;
  createdAt: number;
  updatedAt: number;
  sortOrder: number;
}

export interface AiProviderUpsertPayload {
  id: string;
  name: string;
  /** 勾选的 API 类型集合（至少一个）。 */
  providerTypes: ApiType[];
  /** Anthropic 侧 Base URL（勾选 anthropic 时必填）。 */
  baseUrl: string;
  /** OpenAI 侧 Base URL（勾选任一 OpenAI 类型时必填）。 */
  openaiBaseUrl?: string;
  /** 单个 API Key（旧字段，兼容）；新建必填；编辑留空=保留旧密钥。 */
  apiKey?: string;
  /** 多 API Key（明文数组）；Some 时替换全部密钥，None 时保持旧密钥。 */
  apiKeys?: string[];
  /** Anthropic 侧默认模型。 */
  defaultModel?: string;
  /** OpenAI 侧默认模型。 */
  openaiDefaultModel?: string;
  /** Anthropic 档位模型覆盖；勾选 anthropic 时有效。 */
  models?: Record<string, string>;
  /** 自定义模型列表；Some 时替换，None 时保持旧列表。 */
  customModels?: CustomModel[];
  notes?: string;
}

export interface AiProviderActionResult {
  ok: boolean;
  message: string;
  provider: AiProvider | null;
}

/** Anthropic 档位模型覆盖的固定档位（与后端 MODEL_TIERS 对齐）。 */
export const MODEL_TIERS: Array<{ key: string; label: string }> = [
  { key: "haiku", label: "Haiku" },
  { key: "sonnet", label: "Sonnet" },
  { key: "opus", label: "Opus" },
  { key: "fable", label: "Fable" },
];

/** 可勾选的 API 类型选项（表单 checkbox 与筛选展示共用）。 */
export const API_TYPE_OPTIONS: Array<{
  value: ApiType;
  label: string;
  sub: string;
}> = [
  { value: "openai_chat", label: "OpenAI Chat Completions", sub: "兼容 OpenAI SDK 的 Chat 接口" },
  { value: "openai_responses", label: "OpenAI Responses", sub: "Codex CLI 使用的 Responses 接口" },
  {
    value: "anthropic",
    label: "Anthropic",
    sub: "Claude Code 使用的 Messages 接口，支持按档位配置模型",
  },
];

/** 供应商是否勾选了某 API 类型。 */
export function hasApiType(types: ApiType[] | undefined | null, t: ApiType): boolean {
  return !!types && types.includes(t);
}

/** 供应商是否勾选了任一 OpenAI 协议类型。 */
export function hasAnyOpenaiType(types: ApiType[] | undefined | null): boolean {
  return hasApiType(types, "openai_chat") || hasApiType(types, "openai_responses");
}

/** 类型集合 → 展示标签（徽标、下拉 sub 等共用）。 */
export function apiTypesLabel(types: ApiType[] | undefined | null): string {
  if (!types || types.length === 0) return "未配置";
  return types.map((t) => API_TYPE_OPTIONS.find((o) => o.value === t)?.label ?? t).join(" · ");
}
