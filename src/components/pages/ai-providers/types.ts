/** AI 供应商管理页的类型定义（与 Rust 端 ai_provider.rs 的 camelCase DTO 对齐）。 */

export type ProviderType = "anthropic" | "openai" | "universal";

export interface AiProvider {
  id: string;
  name: string;
  providerType: ProviderType;
  baseUrl: string;
  /** 通用类型下由 baseUrl 派生（追加 /v1）；其他类型恒为空串。 */
  openaiBaseUrl: string;
  defaultModel: string;
  /** 通用类型的 OpenAI 默认模型；其他类型恒为空串。 */
  openaiDefaultModel: string;
  /** Anthropic 档位模型覆盖（haiku/sonnet/opus/fable）；OpenAI 类型恒为空对象。 */
  models: Record<string, string>;
  /** 列表接口不回传明文密钥，仅有此标志。 */
  hasApiKey: boolean;
  notes: string;
  createdAt: number;
  updatedAt: number;
}

export interface AiProviderUpsertPayload {
  id: string;
  name: string;
  providerType: ProviderType;
  baseUrl: string;
  /** 新建必填；编辑留空=保留旧密钥。 */
  apiKey?: string;
  defaultModel?: string;
  /** 通用类型的 OpenAI 默认模型；其他类型忽略。 */
  openaiDefaultModel?: string;
  models?: Record<string, string>;
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

export const PROVIDER_TYPE_OPTIONS: Array<{ value: ProviderType; label: string; sub?: string }> = [
  { value: "anthropic", label: "Anthropic", sub: "支持按档位配置不同模型" },
  { value: "openai", label: "OpenAI" },
  { value: "universal", label: "通用", sub: "同时接入 Anthropic 与 OpenAI（OpenAI Base URL 自动派生 /v1）" },
];
