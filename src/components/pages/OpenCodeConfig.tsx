import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useStatusMessage } from "@/lib/useStatusMessage";
import { Toast } from "@/components/Toast";
import { CheckGlyph, useOverlayDismiss } from "../ui";

import type {
  AgentModelConfigView,
  AgentModelView,
  AgentProviderView,
  AgentVariantView,
  CatalogModelSummary,
  CatalogReasoningOption,
  ModelConfigAgentId,
  ModelsDevCatalog,
} from "./opencode-config/types";
import {
  EFFORT_PRESETS,
  MODALITY_OPTIONS,
  PI_MODALITY_OPTIONS,
} from "./opencode-config/types";
import {
  invokeDeleteModel,
  invokeDeleteProvider,
  invokeFetchCatalog,
  invokeGetConfig,
  invokeGetSecret,
  invokeRevealConfig,
  invokeSetDefaults,
  invokeUpsertModel,
  invokeUpsertProvider,
} from "./opencode-config/api";
import {
  invokeFetchRemoteModels,
  invokeList as invokeAiProviderList,
} from "./ai-providers/api";
import type { AiProvider } from "./ai-providers/types";
import {
  fetchRouteAggregationProvider,
  isRouteAggregationProvider,
  resolveProviderSecret,
} from "./route-aggregation/virtual-provider";
import { ChevronDown, Code, Eye, EyeOff, FolderOpen, Key, Pencil, Plus, RefreshCw, Trash2, X } from "lucide-react";

/* ===== Icons ===== */

const IconPlus = () => (
  <Plus size={16} strokeWidth={2} />
);

const IconRefresh = () => (
  <RefreshCw size={16} strokeWidth={1.8} />
);

const IconFolderOpen = () => (
  <FolderOpen size={16} strokeWidth={1.8} />
);

const IconTrash = () => (
  <Trash2 size={16} strokeWidth={1.8} />
);

const IconEdit = () => (
  <Pencil size={16} strokeWidth={1.8} />
);

const IconClose = () => (
  <X size={16} strokeWidth={2} />
);

const IconEye = () => (
  <Eye size={16} strokeWidth={1.8} />
);

const IconEyeOff = () => (
  <EyeOff size={16} strokeWidth={1.8} />
);

const IconKey = () => (
  <Key size={14} strokeWidth={1.8} />
);

const IconEmpty = () => (
  <Code size={40} strokeWidth={1.5} />
);

const IconChevron = ({ open }: { open?: boolean }) => (
  <ChevronDown size={16} strokeWidth={1.8} style={{ transform: open ? "rotate(180deg)" : undefined, transition: "transform 0.15s ease" }} />
);

/* ===== Helpers ===== */

function formatTokens(n: number | null | undefined): string | null {
  if (n == null || !Number.isFinite(n) || n <= 0) return null;
  if (n >= 1_000_000) {
    const v = n / 1_000_000;
    return `${Number.isInteger(v) ? v : v.toFixed(1)}M`;
  }
  if (n >= 1000) {
    const v = n / 1000;
    return `${Number.isInteger(v) ? v : v.toFixed(1)}k`;
  }
  return String(Math.round(n));
}

function formatLimit(
  context?: number | null,
  output?: number | null,
  input?: number | null,
): string {
  const parts: string[] = [];
  const ctx = formatTokens(context);
  const out = formatTokens(output);
  const inp = formatTokens(input);
  if (ctx) parts.push(`${ctx} ctx`);
  if (inp) parts.push(`${inp} in`);
  if (out) parts.push(`${out} out`);
  return parts.join(" / ") || "未设置 limit";
}

const MODALITY_LABEL: Record<string, string> = {
  text: "文本",
  image: "图像",
  pdf: "PDF",
  audio: "音频",
  video: "视频",
};

/** 把 input/output modalities 拼成「文本/图像/PDF in | 文本 out」形式，没有则返回空串。 */
function formatModalityTags(
  inputMods?: ReadonlyArray<string>,
  outputMods?: ReadonlyArray<string>,
): string {
  const inPart = inputMods?.length
    ? inputMods.map((m) => MODALITY_LABEL[m] ?? m).join("/")
    : "";
  const outPart = outputMods?.length
    ? outputMods.map((m) => MODALITY_LABEL[m] ?? m).join("/")
    : "";
  const parts: string[] = [];
  if (inPart) parts.push(`${inPart} in`);
  if (outPart) parts.push(`${outPart} out`);
  return parts.join(" | ");
}

function apiKeySourceLabel(source: string): string {
  switch (source) {
    case "auth":
      return "auth.json";
    case "config":
      return "配置内明文";
    case "both":
      return "auth + 配置";
    default:
      return "未配置";
  }
}

function parseCsvTags(raw: string): string[] {
  return raw
    .split(/[,，\n]/)
    .map((s) => s.trim())
    .filter(Boolean);
}

function tagsToCsv(tags: string[]): string {
  return tags.join(", ");
}

function modelRef(providerId: string, modelId: string): string {
  return `${providerId}/${modelId}`;
}

function findCatalogModel(
  catalog: ModelsDevCatalog | null,
  providerId: string,
  modelId: string,
): CatalogModelSummary | null {
  if (!catalog || !modelId) return null;
  // 1. 收集所有同名 model 的条目（严格同 provider 优先排序在后取）
  //    不同 provider 可能对同一个 modelId 给出不同字段：
  //    例如 grok-4.5 在 opencode 条目 input 只有 ['text','image']，但 xai 条目还带 pdf。
  //    合并策略：union 所有 limit 字段 + 模态字段——谁有填谁，互不冲突。
  // 2. priority 上下文：表单 providerId 与目录 provider id 严格匹配时，把这一条置顶，
  //    用它的 name / reasoning / toolCall / attachment 等身份类字段。
  //    典型场景：用户使用自定义聚合/路由 provider（如 router、openai-compat 等），
  //    目录中不同 provider 可能都收录了同名模型（model id 在业内通常全局唯一）。
  const matches: { providerId: string; model: CatalogModelSummary }[] = [];
  for (const p of catalog.providers) {
    const found = p.models.find((m) => m.id === modelId);
    if (found) matches.push({ providerId: p.id, model: found });
    if (matches.length > 64) break; // 安全护栏
  }
  if (matches.length === 0) return null;

  // 身份字段以"严格匹配"的那条优先；如果没有，取第一条
  const identity =
    matches.find((m) => m.providerId === providerId)?.model ?? matches[0].model;

  // limit 字段取并集：任一条目有非空值就采纳（第一个非空生效）
  const ctx = matches.find((m) => m.model.limitContext != null)?.model.limitContext;
  const inp = matches.find((m) => m.model.limitInput != null)?.model.limitInput;
  const out = matches.find((m) => m.model.limitOutput != null)?.model.limitOutput;

  // 模态字段取并集：把同名模型出现在不同 provider 下的 modalities 能力并到一起
  // （不去重到 identity 条目，避免 grok-4.5 在 xai 之外的条目漏掉 pdf 这类能力）
  const inputMods = unionModalityStrings(
    matches.map((m) => m.model.modalitiesInput),
  );
  const outputMods = unionModalityStrings(
    matches.map((m) => m.model.modalitiesOutput),
  );

  return {
    ...identity,
    limitContext: ctx ?? null,
    limitInput: inp ?? null,
    limitOutput: out ?? null,
    modalitiesInput: inputMods,
    modalitiesOutput: outputMods,
  };
}

/** 把多个 modalities 数组合并成去重并集（保留首次出现顺序）。空数组会被忽略。 */
function unionModalityStrings(lists: ReadonlyArray<ReadonlyArray<string>>): string[] {
  const seen = new Set<string>();
  const out: string[] = [];
  for (const list of lists) {
    if (!list || list.length === 0) continue;
    for (const m of list) {
      if (!seen.has(m)) {
        seen.add(m);
        out.push(m);
      }
    }
  }
  return out;
}

function reasoningOptionsFor(
  catalog: ModelsDevCatalog | null,
  providerId: string,
  modelId: string,
): CatalogReasoningOption[] {
  return findCatalogModel(catalog, providerId, modelId)?.reasoningOptions ?? [];
}

/**
 * 按 providerId/modelId 在 Models.dev 目录中匹配，把命中项的：
 *   - context/input/output 限制
 *   - 输入/输出模态（input modalities / output modalities）
 * 写回表单。
 * agent 无关：opencode / pi / oh-my-pi 三个 tab 共用本函数，因为 ModelForm 的 limit 字段对三者都是
 * 同一组（`limitContext` / `limitInput` / `limitOutput`），后端在 `UpsertModelPayload` 里也接受。
 * - 默认（overwrite=false）只补空白字段，避免覆盖用户已输入的值。
 * - overwrite=true 时强制按目录覆盖（含「把已填值清空」）：用于「从目录补全」按钮和
 *   供应商模型列表选择场景——必须支持从有 input 的模型切到无 input 的模型时把 input 字段清空。
 * - `agent` 用于决定 `limitInput` 与「输出模态」是否生效：
 *   - Pi / Oh-My-Pi 后端没有 limit.input / output modalities 字段，填了会被静默丢弃。
 * - 模态字段（modalities）永远跟随 overwrite 一起被覆盖，因为目录里这些字段几乎齐全
 *   （都有至少 text），让用户从 picker 切换模型时能立刻看到正确的能力范围。
 * 目录未命中时返回原表单。
 */
function applyCatalogLimits(
  form: ModelForm,
  catalog: ModelsDevCatalog | null,
  options: { overwrite?: boolean; agent?: ModelConfigAgentId } = {},
): ModelForm {
  const hit = findCatalogModel(catalog, form.providerId, form.id.trim());
  if (!hit) return form;
  const overwrite = options.overwrite ?? false;
  // Pi / Oh-My-Pi 后端对 limit_input / output modalities 是 no-op，跳过避免无效回填
  const supportsInputLimit = options.agent == null || options.agent === "opencode";
  const supportsOutputModality = options.agent == null || options.agent === "opencode";
  let next: ModelForm = form;

  // context
  if (overwrite || !form.limitContext.trim()) {
    const v = hit.limitContext != null ? numToInput(hit.limitContext) : "";
    next = { ...next, limitContext: v };
  }
  // input (limit)
  if (supportsInputLimit && (overwrite || !form.limitInput.trim())) {
    const v = hit.limitInput != null ? numToInput(hit.limitInput) : "";
    next = { ...next, limitInput: v };
  }
  // output (limit)
  if (overwrite || !form.limitOutput.trim()) {
    const v = hit.limitOutput != null ? numToInput(hit.limitOutput) : "";
    next = { ...next, limitOutput: v };
  }

  // —— 模态字段 ——
  // 目录里 modalities 字段几乎一定能拿到（非空向量），overwrite 模式直接覆盖；
  // 非 overwrite 模式（用户手输 ID 触发）只在当前为空且目录非空时补全。
  const hitInputMods =
    options.agent == null || options.agent === "opencode"
      ? hit.modalitiesInput
      : normalizePiInputModalities(hit.modalitiesInput);
  const hitOutputMods = hit.modalitiesOutput;
  if (overwrite) {
    // 强制覆盖：input 始终写（Pi 系也支持），output 仅 opencode 写
    if (hitInputMods && hitInputMods.length > 0) {
      next = { ...next, modalitiesInput: [...hitInputMods] };
    } else if (hitInputMods && hitInputMods.length === 0) {
      // 目录返回空数组（极少）——保留用户当前选择，不强行清空
    }
    if (supportsOutputModality && hitOutputMods && hitOutputMods.length > 0) {
      next = { ...next, modalitiesOutput: [...hitOutputMods] };
    }
  } else {
    // 仅当表单为空且目录非空时补全
    if (
      form.modalitiesInput.length === 0 &&
      hitInputMods &&
      hitInputMods.length > 0
    ) {
      next = { ...next, modalitiesInput: [...hitInputMods] };
    }
    if (
      supportsOutputModality &&
      form.modalitiesOutput.length === 0 &&
      hitOutputMods &&
      hitOutputMods.length > 0
    ) {
      next = { ...next, modalitiesOutput: [...hitOutputMods] };
    }
  }

  return next;
}

const NPM_PRESETS = [
  { value: "", label: "默认（内置）" },
  { value: "@ai-sdk/openai-compatible", label: "OpenAI Compatible" },
  { value: "@ai-sdk/openai", label: "OpenAI" },
  { value: "@ai-sdk/anthropic", label: "Anthropic" },
  { value: "@ai-sdk/google", label: "Google" },
];

type AppSelectOption = {
  value: string;
  label: string;
  sub?: string;
};

/** 与 ClaudeEnv / Skills 共用的 app-select 下拉，替代原生 <select>。 */
function AppSelect({
  id,
  labelId,
  value,
  options,
  onChange,
  disabled = false,
  placeholder = "请选择",
  emptyText = "暂无选项",
}: {
  id?: string;
  labelId?: string;
  value: string;
  options: AppSelectOption[];
  onChange: (value: string) => void;
  disabled?: boolean;
  placeholder?: string;
  emptyText?: string;
}) {
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);
  const selected = options.find((o) => o.value === value) ?? null;

  useEffect(() => {
    if (!open) return;
    const onPointerDown = (e: MouseEvent) => {
      if (rootRef.current && !rootRef.current.contains(e.target as Node)) {
        setOpen(false);
      }
    };
    document.addEventListener("mousedown", onPointerDown);
    return () => document.removeEventListener("mousedown", onPointerDown);
  }, [open]);

  // capture：先于页面级 Escape 关闭弹窗，仅收起下拉
  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      e.preventDefault();
      e.stopPropagation();
      setOpen(false);
    };
    document.addEventListener("keydown", onKey, true);
    return () => document.removeEventListener("keydown", onKey, true);
  }, [open]);

  useEffect(() => {
    if (disabled) setOpen(false);
  }, [disabled]);

  return (
    <div
      className={`app-select ${open ? "open" : ""} ${disabled ? "disabled" : ""}`}
      ref={rootRef}
    >
      <button
        type="button"
        id={id}
        className="app-select-trigger form-input"
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-labelledby={labelId}
        disabled={disabled}
        onClick={() => {
          if (!disabled) setOpen((v) => !v);
        }}
      >
        <span className={`app-select-value ${selected ? "" : "placeholder"}`}>
          {selected?.label ?? placeholder}
        </span>
        <span className="app-select-chevron" aria-hidden>
          <IconChevron open={open} />
        </span>
      </button>
      {open && (
        <div className="app-select-menu" role="listbox" aria-labelledby={labelId}>
          {options.length === 0 ? (
            <div className="app-select-empty">{emptyText}</div>
          ) : (
            options.map((o) => {
              const isSelected = o.value === value;
              return (
                <button
                  key={o.value === "" ? "__empty__" : o.value}
                  type="button"
                  role="option"
                  aria-selected={isSelected}
                  className={`app-select-option ${isSelected ? "selected" : ""}`}
                  disabled={disabled}
                  onClick={() => {
                    onChange(o.value);
                    setOpen(false);
                  }}
                >
                  <span className="app-select-option-title">{o.label}</span>
                  {o.sub ? <span className="app-select-option-sub">{o.sub}</span> : null}
                </button>
              );
            })
          )}
        </div>
      )}
    </div>
  );
}

const NPM_SELECT_OPTIONS: AppSelectOption[] = [
  ...NPM_PRESETS.map((p) => ({
    value: p.value,
    label: p.label,
    sub: p.value || undefined,
  })),
  { value: "__custom__", label: "自定义…" },
];

/** Pi / Oh-My-Pi 原生支持的三种 HTTP API 格式。 */
const PI_API_OPTIONS: AppSelectOption[] = [
  {
    value: "openai-completions",
    label: "OpenAI Chat Completions",
    sub: "/v1/chat/completions",
  },
  {
    value: "openai-responses",
    label: "OpenAI Responses",
    sub: "/v1/responses",
  },
  {
    value: "anthropic-messages",
    label: "Anthropic Messages",
    sub: "/v1/messages",
  },
];

function piApiLabel(api: string): string {
  return PI_API_OPTIONS.find((option) => option.value === api)?.label ?? api;
}

const THINKING_TYPE_OPTIONS: AppSelectOption[] = [
  { value: "", label: "（不设置）" },
  { value: "enabled", label: "enabled" },
  { value: "disabled", label: "disabled" },
];

/** AI 供应商（OpenAI / 通用类型）面向 OpenAI 兼容端点的 Base URL。 */
function aiProviderOpenaiBaseUrl(p: AiProvider): string {
  return p.providerType === "universal" ? p.openaiBaseUrl : p.baseUrl;
}

type ProviderForm = {
  id: string;
  previousId: string | null;
  name: string;
  npm: string;
  api: string;
  baseUrl: string;
  timeout: string;
  chunkTimeout: string;
  whitelist: string;
  blacklist: string;
  apiKey: string;
  apiKeyTouched: boolean;
  isNew: boolean;
};

type ModelForm = {
  providerId: string;
  id: string;
  previousId: string | null;
  name: string;
  limitContext: string;
  limitInput: string;
  limitOutput: string;
  modalitiesInput: string[];
  modalitiesOutput: string[];
  reasoning: boolean | null;
  toolCall: boolean | null;
  attachment: boolean | null;
  thinkingType: string;
  thinkingBudgetTokens: string;
  reasoningEffort: string;
  textVerbosity: string;
  variants: AgentVariantView[];
  extraOptionsRaw: string;
  isNew: boolean;
  showAdvanced: boolean;
};

function emptyProviderForm(): ProviderForm {
  return {
    id: "",
    previousId: null,
    name: "",
    npm: "@ai-sdk/openai-compatible",
    api: "openai-completions",
    baseUrl: "",
    timeout: "",
    chunkTimeout: "",
    whitelist: "",
    blacklist: "",
    apiKey: "",
    apiKeyTouched: false,
    isNew: true,
  };
}

function emptyModelForm(providerId: string): ModelForm {
  return {
    providerId,
    id: "",
    previousId: null,
    name: "",
    limitContext: "",
    limitInput: "",
    limitOutput: "",
    modalitiesInput: ["text"],
    modalitiesOutput: ["text"],
    reasoning: null,
    toolCall: null,
    attachment: null,
    thinkingType: "",
    thinkingBudgetTokens: "",
    reasoningEffort: "",
    textVerbosity: "",
    variants: [],
    extraOptionsRaw: "",
    isNew: true,
    showAdvanced: false,
  };
}

function numToInput(n: number | null | undefined): string {
  if (n == null || !Number.isFinite(n)) return "";
  return String(n);
}

function parseOptionalNumber(raw: string): number | null {
  const t = raw.trim();
  if (!t) return null;
  const n = Number(t);
  return Number.isFinite(n) ? n : null;
}

function toggleModality(list: string[], value: string): string[] {
  return list.includes(value) ? list.filter((x) => x !== value) : [...list, value];
}

/** 将通用目录的输入能力收敛到 Pi/Oh-My-Pi 的 models.json schema。 */
function normalizePiInputModalities(input: ReadonlyArray<string>): string[] {
  const hasText = input.includes("text");
  const hasImage = input.includes("image");
  if (!hasText && !hasImage) return input.length > 0 ? ["text"] : [];
  return hasImage ? ["text", "image"] : ["text"];
}

function inputModalitiesForAgent(
  agent: ModelConfigAgentId,
  input: ReadonlyArray<string>,
): string[] {
  return agent === "opencode" ? [...input] : normalizePiInputModalities(input);
}

function toggleInputModality(
  list: string[],
  value: string,
  agent: ModelConfigAgentId,
): string[] {
  const next = toggleModality(list, value);
  if (agent === "opencode") return next;
  // Pi 的 schema 只允许 text 或 text + image；取消 text 时同时取消 image，
  // 让「未设置 input」仍保持合法，而不会产生 image-only 数组。
  if (value === "text" && !next.includes("text")) return [];
  if (value === "image" && next.includes("image") && !next.includes("text")) {
    return ["text", "image"];
  }
  return next;
}

/* ===== Agent 切换 ===== */

const AGENT_TABS: { id: ModelConfigAgentId; label: string; notInstalledHint: string }[] = [
  {
    id: "opencode",
    label: "OpenCode",
    notInstalledHint:
      "请先安装 OpenCode CLI 或 App（例如 opencode 命令），安装后重新打开本页即可管理供应商与模型配置。",
  },
  {
    id: "pi",
    label: "Pi",
    notInstalledHint:
      "请先安装 Pi CLI（例如 npm install -g @earendil-works/pi-coding-agent，或 pi.dev/install.sh），安装后重新打开本页即可管理供应商与模型配置。",
  },
  {
    id: "oh-my-pi",
    label: "Oh-My-Pi",
    notInstalledHint:
      "请先安装 Oh-My-Pi（例如 brew install can1357/tap/omp，或 omp.sh/install），安装后重新打开本页即可管理供应商与模型配置。",
  },
];

function agentLabel(id: ModelConfigAgentId): string {
  return AGENT_TABS.find((t) => t.id === id)?.label ?? id;
}

/** API Key 写入的 auth.json 路径提示（按 agent）。 */
function authFileHint(id: ModelConfigAgentId): string {
  switch (id) {
    case "pi":
      return "~/.pi/agent/auth.json";
    case "oh-my-pi":
      return "~/.omp/agent/auth.json";
    default:
      return "~/.local/share/opencode/auth.json";
  }
}

/* ===== Subcomponents ===== */

function ModalityBadges({
  input,
  output,
}: {
  input: string[];
  output: string[];
}) {
  const show = input.length > 0 || output.length > 0;
  if (!show) return null;
  return (
    <div className="oc-modality-row">
      {input.map((m) => (
        <span key={`in-${m}`} className={`oc-modality oc-modality-${m}`} title={`输入 · ${m}`}>
          in:{m}
        </span>
      ))}
      {output.map((m) => (
        <span key={`out-${m}`} className={`oc-modality oc-modality-${m}`} title={`输出 · ${m}`}>
          out:{m}
        </span>
      ))}
    </div>
  );
}

function ModelChip({
  providerId,
  model,
  isDefault,
  isSmall,
  onEdit,
  onDelete,
}: {
  providerId: string;
  model: AgentModelView;
  isDefault: boolean;
  isSmall: boolean;
  onEdit: () => void;
  onDelete: () => void;
}) {
  const thinking =
    model.reasoningEffort ||
    (model.thinkingType
      ? `thinking:${model.thinkingType}${
          model.thinkingBudgetTokens != null ? `/${model.thinkingBudgetTokens}` : ""
        }`
      : null);

  return (
    <div className="oc-model-chip">
      <div className="oc-model-chip-main">
        <div className="oc-model-chip-title-row">
          <span className="oc-model-chip-id">{model.name || model.id}</span>
          {model.id !== model.name && model.name ? (
            <span className="oc-model-chip-sub">{model.id}</span>
          ) : null}
          {isDefault ? <span className="oc-pill oc-pill-primary">默认</span> : null}
          {isSmall ? <span className="oc-pill">small</span> : null}
          {model.reasoning ? <span className="oc-pill oc-pill-think">思考</span> : null}
        </div>
        <div className="oc-limit">{formatLimit(model.limitContext, model.limitOutput, model.limitInput)}</div>
        <ModalityBadges input={model.modalitiesInput} output={model.modalitiesOutput} />
        {thinking ? <div className="oc-model-chip-meta">{thinking}</div> : null}
      </div>
      <div className="oc-model-chip-actions">
        <button type="button" className="btn-icon-action" data-tooltip="编辑模型" onClick={onEdit}>
          <IconEdit />
        </button>
        <button type="button" className="btn-delete" data-tooltip="删除模型" onClick={onDelete}>
          <IconTrash />
        </button>
      </div>
      <span className="oc-model-chip-ref" title={modelRef(providerId, model.id)}>
        {modelRef(providerId, model.id)}
      </span>
    </div>
  );
}

/* ===== Page ===== */

export default function ModelConfig() {
  const [agent, setAgent] = useState<ModelConfigAgentId>("opencode");
  const isOpenCode = agent === "opencode";
  const [view, setView] = useState<AgentModelConfigView | null>(null);
  const [catalog, setCatalog] = useState<ModelsDevCatalog | null>(null);
  const [catalogError, setCatalogError] = useState("");
  const [catalogLoading, setCatalogLoading] = useState(false);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [statusMsg, setStatusMsg] = useStatusMessage();

  const [providerForm, setProviderForm] = useState<ProviderForm | null>(null);
  // 供应商弹窗内的「选择 AI 供应商」：候选列表 + 当前选中 ID（"" = 未选择）
  const [aiProviders, setAiProviders] = useState<AiProvider[]>([]);
  const [formAiProviderId, setFormAiProviderId] = useState("");
  const [modelForm, setModelForm] = useState<ModelForm | null>(null);
  const [showApiKey, setShowApiKey] = useState(false);
  const [formError, setFormError] = useState("");

  const [deleteProvider, setDeleteProvider] = useState<AgentProviderView | null>(null);
  const [deleteAuthToo, setDeleteAuthToo] = useState(true);
  const [deleteModelTarget, setDeleteModelTarget] = useState<{
    providerId: string;
    model: AgentModelView;
  } | null>(null);

  const [defaultsOpen, setDefaultsOpen] = useState(false);
  const [draftModel, setDraftModel] = useState("");
  const [draftSmallModel, setDraftSmallModel] = useState("");
  const [modelSearch, setModelSearch] = useState("");

  // 模型弹窗内的「从供应商模型列表选择」：拉取当前供应商端点的模型列表
  // **临时配置专用**：当 OpenCode / Pi / Oh-My-Pi provider **不**关联 AI 供应商库、
  // 手填 baseUrl + apiKey 时调用。"已配置 AI 供应商"路径以 `customModels` 为唯一来源。
  const [providerPickOpen, setProviderPickOpen] = useState(false);
  const [providerPickQuery, setProviderPickQuery] = useState("");
  const [providerPickModels, setProviderPickModels] = useState<string[]>([]);
  const [providerPickLoading, setProviderPickLoading] = useState(false);
  const [providerPickMsg, setProviderPickMsg] = useState("");

  const providerIdRef = useRef<HTMLInputElement>(null);
  const modelIdRef = useRef<HTMLInputElement>(null);

  const providerDismiss = useOverlayDismiss(() => {
    if (!busy) {
      setProviderForm(null);
      setFormError("");
    }
  }, !busy);
  const modelDismiss = useOverlayDismiss(() => {
    if (!busy) {
      setModelForm(null);
      setFormError("");
      setProviderPickOpen(false);
    }
  }, !busy);
  const deleteProviderDismiss = useOverlayDismiss(
    () => !busy && setDeleteProvider(null),
    !busy,
  );
  const deleteModelDismiss = useOverlayDismiss(
    () => !busy && setDeleteModelTarget(null),
    !busy,
  );
  const defaultsDismiss = useOverlayDismiss(() => !busy && setDefaultsOpen(false), !busy);

  const applyView = useCallback((next: AgentModelConfigView) => {
    setView(next);
  }, []);

  const loadConfig = useCallback(async (quiet = false) => {
    if (!quiet) setLoading(true);
    try {
      const next = await invokeGetConfig(agent);
      applyView(next);
      if (!quiet) {
        if (next.installed && next.warnings.length > 0) {
          setStatusMsg(next.warnings[0]);
        }
      }
      return next;
    } catch (e) {
      setStatusMsg(`加载失败: ${e}`);
      return null;
    } finally {
      setLoading(false);
    }
  }, [agent, applyView, setStatusMsg]);

  const loadCatalog = useCallback(
    async (force = false) => {
      setCatalogLoading(true);
      setCatalogError("");
      try {
        const cat = await invokeFetchCatalog(force);
        setCatalog(cat);
      } catch (e) {
        setCatalog(null);
        setCatalogError(String(e));
      } finally {
        setCatalogLoading(false);
      }
    },
    [],
  );

  useEffect(() => {
    (async () => {
      const next = await loadConfig();
      // Models.dev catalog only matters after the agent is installed.
      if (next?.installed) {
        void loadCatalog(false);
      }
    })();
  }, [loadConfig, loadCatalog]);

  /** 切换 agent：收起所有弹窗并重新加载。 */
  const switchAgent = (next: ModelConfigAgentId) => {
    if (next === agent || busy) return;
    setProviderForm(null);
    setModelForm(null);
    setDeleteProvider(null);
    setDeleteModelTarget(null);
    setDefaultsOpen(false);
    setProviderPickOpen(false);
    setFormError("");
    setView(null);
    setAgent(next);
  };

  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.key !== "Escape" || busy) return;
      setProviderForm(null);
      setModelForm(null);
      setDeleteProvider(null);
      setDeleteModelTarget(null);
      setDefaultsOpen(false);
      setProviderPickOpen(false);
      setFormError("");
    };
    document.addEventListener("keydown", handler);
    return () => document.removeEventListener("keydown", handler);
  }, [busy]);

  const configuredModelOptions = useMemo(() => {
    if (!view) return [] as string[];
    const opts: string[] = [];
    for (const p of view.providers) {
      for (const m of p.models) {
        opts.push(modelRef(p.id, m.id));
      }
    }
    return opts;
  }, [view]);

  const filteredModelOptions = useMemo(() => {
    const q = modelSearch.trim().toLowerCase();
    if (!q) return configuredModelOptions;
    return configuredModelOptions.filter((x) => x.toLowerCase().includes(q));
  }, [configuredModelOptions, modelSearch]);

  const providerPickHits = useMemo(() => {
    const q = providerPickQuery.trim().toLowerCase();
    if (!q) return providerPickModels;
    return providerPickModels.filter((m) => m.toLowerCase().includes(q));
  }, [providerPickModels, providerPickQuery]);

  // 把过滤后的列表附上 Models.dev 目录命中信息（用于展示「目录」小角标与 ctx/out 提示）
  const providerPickHitDetails = useMemo(() => {
    const pid = modelForm?.providerId ?? "";
    return providerPickHits.map((id) => ({
      id,
      catalogHit: findCatalogModel(catalog, pid, id),
    }));
  }, [providerPickHits, catalog, modelForm?.providerId]);


/** 加载已配置的 AI 供应商（仅 OpenAI / 通用类型可对接本页面）；编辑时按 Base URL 预选。
 * 路由聚合运行时将虚拟供应商「路由聚合」置顶。 */
  const loadAiProviders = useCallback(async (presetBaseUrl?: string) => {
    setFormAiProviderId("");
    try {
      const rows = await invokeAiProviderList();
      const eligible = rows.filter(
        (p) => p.providerType === "openai" || p.providerType === "universal",
      );
      const routeAgg = await fetchRouteAggregationProvider("openai");
      const all = routeAgg ? [routeAgg, ...eligible] : eligible;
      setAiProviders(all);
      if (presetBaseUrl) {
        const match = all.find((p) => aiProviderOpenaiBaseUrl(p) === presetBaseUrl);
        setFormAiProviderId(match?.id ?? "");
      }
    } catch {
      setAiProviders([]);
    }
  }, []);

  const aiProviderOptions = useMemo<AppSelectOption[]>(
    () => [
      { value: "", label: "未选择（自行填写）" },
      ...aiProviders.map((p) => ({
        value: p.id,
        label: p.name,
        sub: isRouteAggregationProvider(p.id)
          ? `本地端点 · ${aiProviderOpenaiBaseUrl(p)}`
          : `${p.providerType === "openai" ? "OpenAI" : "通用"} · ${aiProviderOpenaiBaseUrl(p)}`,
      })),
    ],
    [aiProviders],
  );

  /** 选中已配置 AI 供应商：自动回填显示名称 / Base URL / API Key（类似 Claude 环境页）。 */
  const pickAiProvider = async (v: string) => {
    if (!providerForm) return;
    setFormAiProviderId(v);
    if (!v) return;
    const p = aiProviders.find((x) => x.id === v);
    if (!p) return;
    setProviderForm({
      ...providerForm,
      name: p.name || providerForm.name,
      baseUrl: aiProviderOpenaiBaseUrl(p),
    });
    if (p.hasApiKey) {
      try {
        const secret = await resolveProviderSecret(p);
        setProviderForm((prev) =>
          prev ? { ...prev, apiKey: secret, apiKeyTouched: true } : prev,
        );
      } catch {
        // 拉取失败则保持现值，用户可手动填写
      }
    }
  };

  const openAddProvider = () => {
    setFormError("");
    setShowApiKey(false);
    setProviderForm(emptyProviderForm());
    void loadAiProviders();
    setTimeout(() => providerIdRef.current?.focus(), 80);
  };

  const openEditProvider = async (p: AgentProviderView) => {
    setFormError("");
    setShowApiKey(false);
    const form: ProviderForm = {
      id: p.id,
      previousId: p.id,
      name: p.name ?? "",
      npm: p.npm ?? "",
      api: p.api ?? "openai-completions",
      baseUrl: p.baseUrl ?? "",
      timeout: p.timeout != null ? String(p.timeout) : "",
      chunkTimeout: p.chunkTimeout != null ? String(p.chunkTimeout) : "",
      whitelist: tagsToCsv(p.whitelist),
      blacklist: tagsToCsv(p.blacklist),
      apiKey: "",
      apiKeyTouched: false,
      isNew: false,
    };
    setProviderForm(form);
    void loadAiProviders(form.baseUrl);
    if (p.hasApiKey) {
      try {
        const secret = await invokeGetSecret(agent, p.id);
        setProviderForm((prev) =>
          prev
            ? { ...prev, apiKey: secret, apiKeyTouched: false }
            : prev,
        );
      } catch {
        // soft-fail: leave empty, user can re-enter
      }
    }
  };

  const resetProviderPick = () => {
    setProviderPickOpen(false);
    setProviderPickQuery("");
    setProviderPickModels([]);
    setProviderPickMsg("");
  };

  const openAddModel = (providerId: string) => {
    setFormError("");
    resetProviderPick();
    setModelForm(emptyModelForm(providerId));
    setTimeout(() => modelIdRef.current?.focus(), 80);
  };

  const openEditModel = (providerId: string, model: AgentModelView) => {
    setFormError("");
    resetProviderPick();
    setModelForm({
      providerId,
      id: model.id,
      previousId: model.id,
      name: model.name ?? "",
      limitContext: numToInput(model.limitContext),
      limitInput: numToInput(model.limitInput),
      limitOutput: numToInput(model.limitOutput),
      modalitiesInput: inputModalitiesForAgent(agent, model.modalitiesInput),
      modalitiesOutput: [...model.modalitiesOutput],
      reasoning: model.reasoning ?? null,
      toolCall: model.toolCall ?? null,
      attachment: model.attachment ?? null,
      thinkingType: model.thinkingType ?? "",
      thinkingBudgetTokens: model.thinkingBudgetTokens != null ? String(model.thinkingBudgetTokens) : "",
      reasoningEffort: model.reasoningEffort ?? "",
      textVerbosity: model.textVerbosity ?? "",
      variants: model.variants.map((v) => ({ ...v, extra: { ...v.extra } })),
      extraOptionsRaw:
        Object.keys(model.extraOptions).length > 0
          ? JSON.stringify(model.extraOptions, null, 2)
          : "",
      isNew: false,
      showAdvanced: Object.keys(model.extraOptions).length > 0 || model.variants.length > 0,
    });
  };

  /** 点击「从供应商模型列表选择」：从当前 provider 的 Base URL 拉取模型列表。
   *
   * **临时配置专用**：仅当 OpenCode / Pi / Oh-My-Pi provider **不**关联 AI 供应商
   * 库、手填 baseUrl + apiKey 时使用。"已配置 AI 供应商"路径以 `customModels` 为
   * 唯一来源，**不**触发远端拉取。
   */
  const loadProviderModels = async (providerId: string) => {
    setProviderPickOpen(true);
    setProviderPickMsg("");
    const p = view?.providers.find((x) => x.id === providerId);
    const baseUrl = (p?.baseUrl ?? "").trim();
    if (!baseUrl) {
      setProviderPickModels([]);
      setProviderPickMsg("该供应商未配置 Base URL，无法拉取模型列表，请手动填写模型 ID");
      return;
    }
    setProviderPickLoading(true);
    try {
      let apiKey = "";
      if (p?.hasApiKey) {
        try {
          apiKey = await invokeGetSecret(agent, p.id);
        } catch {
          // 密钥读取失败则匿名拉取，失败后用户仍可手动填写
        }
      }
      const models = await invokeFetchRemoteModels(baseUrl, apiKey || undefined);
      setProviderPickModels(models);
      setProviderPickMsg(
        models.length === 0 ? "远端未返回可用模型，仍可手动填写" : `已加载 ${models.length} 个模型`,
      );
    } catch (e) {
      setProviderPickModels([]);
      setProviderPickMsg(`拉取模型列表失败：${e instanceof Error ? e.message : String(e)}`);
    } finally {
      setProviderPickLoading(false);
    }
  };

  const applyProviderPick = (modelId: string) => {
    setModelForm((prev) => {
      if (!prev) return prev;
      // 每次从 picker 挑选都视为"明确选定"，整体重置：
      // - id 换成新的
      // - name 优先取 catalog 显示名，否则回退到 modelId（强制覆盖，避免前一次的 id 残留）
      // - limit 字段按目录强制覆盖（即使是空值也清空）
      const hit = findCatalogModel(catalog, prev.providerId, modelId);
      const name = hit?.name ?? modelId;
      const base = { ...prev, id: modelId, name };
      return applyCatalogLimits(base, catalog, { overwrite: true, agent });
    });
    setProviderPickOpen(false);
  };

  const saveProvider = async () => {
    if (!providerForm) return;
    const id = providerForm.id.trim();
    if (!id) {
      setFormError("请填写供应商 ID");
      return;
    }
    setBusy(true);
    setFormError("");
    try {
      const payload: Parameters<typeof invokeUpsertProvider>[1] = {
        id,
        previousId: providerForm.isNew ? null : providerForm.previousId,
        name: providerForm.name.trim() || null,
        baseUrl: providerForm.baseUrl.trim() || null,
      };
      if (!isOpenCode) {
        payload.api = providerForm.api;
      }
      if (isOpenCode) {
        payload.npm = providerForm.npm.trim() || null;
        payload.timeout = (() => {
          const n = parseOptionalNumber(providerForm.timeout);
          return n == null ? null : Math.round(n);
        })();
        payload.chunkTimeout = (() => {
          const n = parseOptionalNumber(providerForm.chunkTimeout);
          return n == null ? null : Math.round(n);
        })();
        payload.whitelist = parseCsvTags(providerForm.whitelist);
        payload.blacklist = parseCsvTags(providerForm.blacklist);
      }
      if (providerForm.apiKeyTouched) {
        payload.apiKey = providerForm.apiKey;
      }
      const res = await invokeUpsertProvider(agent, payload);
      if (res.view) applyView(res.view);
      else await loadConfig(true);
      setProviderForm(null);
      setStatusMsg(res.message || "供应商已保存");
    } catch (e) {
      setFormError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const saveModel = async () => {
    if (!modelForm) return;
    const id = modelForm.id.trim();
    if (!id) {
      setFormError("请填写模型 ID");
      return;
    }
    if (!modelForm.providerId.trim()) {
      setFormError("缺少供应商 ID");
      return;
    }

    let extraOptions: Record<string, unknown> | null = null;
    if (modelForm.extraOptionsRaw.trim()) {
      try {
        const parsed = JSON.parse(modelForm.extraOptionsRaw) as unknown;
        if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) {
          setFormError("高级 options 必须是 JSON 对象");
          return;
        }
        extraOptions = parsed as Record<string, unknown>;
      } catch {
        setFormError("高级 options JSON 无法解析");
        return;
      }
    }

    setBusy(true);
    setFormError("");
    try {
      const res = await invokeUpsertModel(agent, {
        providerId: modelForm.providerId,
        id,
        previousId: modelForm.isNew ? null : modelForm.previousId,
        name: modelForm.name.trim() || null,
        limitContext: parseOptionalNumber(modelForm.limitContext),
        limitInput: parseOptionalNumber(modelForm.limitInput),
        limitOutput: parseOptionalNumber(modelForm.limitOutput),
        modalitiesInput: modelForm.modalitiesInput,
        modalitiesOutput: modelForm.modalitiesOutput,
        reasoning: modelForm.reasoning,
        // 以下为 OpenCode 专属字段；Pi 家族后端会忽略
        toolCall: isOpenCode ? modelForm.toolCall : null,
        attachment: isOpenCode ? modelForm.attachment : null,
        thinkingType: isOpenCode ? modelForm.thinkingType.trim() || null : null,
        thinkingBudgetTokens: isOpenCode
          ? (() => {
              const n = parseOptionalNumber(modelForm.thinkingBudgetTokens);
              return n == null ? null : Math.max(0, Math.round(n));
            })()
          : null,
        reasoningEffort: isOpenCode ? modelForm.reasoningEffort.trim() || null : null,
        textVerbosity: isOpenCode ? modelForm.textVerbosity.trim() || null : null,
        variants: isOpenCode ? modelForm.variants : null,
        extraOptions,
        replaceExtraOptions: extraOptions != null,
      });
      if (res.view) applyView(res.view);
      else await loadConfig(true);
      setModelForm(null);
      setStatusMsg(res.message || "模型已保存");
    } catch (e) {
      setFormError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const confirmDeleteProvider = async () => {
    if (!deleteProvider) return;
    setBusy(true);
    try {
      const res = await invokeDeleteProvider(agent, deleteProvider.id, deleteAuthToo);
      if (res.view) applyView(res.view);
      else await loadConfig(true);
      setDeleteProvider(null);
      setStatusMsg(res.message || "供应商已删除");
    } catch (e) {
      setStatusMsg(`删除失败: ${e}`);
    } finally {
      setBusy(false);
    }
  };

  const confirmDeleteModel = async () => {
    if (!deleteModelTarget) return;
    setBusy(true);
    try {
      const res = await invokeDeleteModel(
        agent,
        deleteModelTarget.providerId,
        deleteModelTarget.model.id,
      );
      if (res.view) applyView(res.view);
      else await loadConfig(true);
      setDeleteModelTarget(null);
      setStatusMsg(res.message || "模型已删除");
    } catch (e) {
      setStatusMsg(`删除失败: ${e}`);
    } finally {
      setBusy(false);
    }
  };

  const openDefaults = () => {
    setDraftModel(view?.model ?? "");
    setDraftSmallModel(view?.smallModel ?? "");
    setModelSearch("");
    setDefaultsOpen(true);
  };

  const saveDefaults = async () => {
    setBusy(true);
    try {
      const res = await invokeSetDefaults(agent, {
        model: draftModel.trim(),
        // Pi 家族不支持 small model：不传即不改动
        smallModel: view?.smallModelSupported ? draftSmallModel.trim() : undefined,
      });
      if (res.view) applyView(res.view);
      else await loadConfig(true);
      setDefaultsOpen(false);
      setStatusMsg(res.message || "默认模型已更新");
    } catch (e) {
      setStatusMsg(`保存失败: ${e}`);
    } finally {
      setBusy(false);
    }
  };

  const reveal = async () => {
    try {
      const res = await invokeRevealConfig(agent);
      setStatusMsg(res.message || "已在 Finder 中显示");
    } catch (e) {
      setStatusMsg(`打开失败: ${e}`);
    }
  };

  const catalogReasoning = modelForm
    ? reasoningOptionsFor(catalog, modelForm.providerId, modelForm.id)
    : [];
  const hasEffort = catalogReasoning.some((r) => r.type === "effort");
  const hasBudget = catalogReasoning.some((r) => r.type === "budget_tokens");
  const hasToggle = catalogReasoning.some((r) => r.type === "toggle");
  const effortValues =
    catalogReasoning.find((r) => r.type === "effort")?.values ?? [...EFFORT_PRESETS];
  // 当前表单的 providerId/modelId 是否在 Models.dev 目录里命中，且至少带有限制字段
  // 该判断同时覆盖 opencode / pi / oh-my-pi 三个 tab：表单共享同一组 limit 字段
  // Pi/Oh-My-Pi 后端不写 limitInput，故仅当 context/output 至少有一个非空时才显示按钮/提示
  const catalogMatchForLimits = modelForm
    ? (() => {
        const hit = findCatalogModel(catalog, modelForm.providerId, modelForm.id);
        if (!hit) return null;
        const hasContext = hit.limitContext != null;
        const hasOutput = hit.limitOutput != null;
        const hasInput =
          agent === "opencode" && hit.limitInput != null;
        if (!hasContext && !hasOutput && !hasInput) return null;
        return hit;
      })()
    : null;
  const inputModalityOptions =
    agent === "opencode" ? MODALITY_OPTIONS : PI_MODALITY_OPTIONS;

  return (
    <>
      <div className="content-header">
        <div className="content-header-bar">
          <h1 className="content-title">模型配置</h1>
          <div className="oc-agent-tabs" role="tablist" aria-label="选择 Agent">
            {AGENT_TABS.map((t) => (
              <button
                key={t.id}
                type="button"
                role="tab"
                aria-selected={agent === t.id}
                className={`oc-agent-tab ${agent === t.id ? "active" : ""}`}
                onClick={() => switchAgent(t.id)}
                disabled={busy}
              >
                {t.label}
              </button>
            ))}
          </div>
          <div className="header-actions">
            <button
              type="button"
              className={`action-btn ${loading ? "sniffing" : ""}`}
              data-tooltip="刷新"
              onClick={() => {
                void loadConfig();
                if (view?.installed) void loadCatalog(false);
              }}
              disabled={loading || busy}
            >
              <IconRefresh />
            </button>
            <button
              type="button"
              className="action-btn"
              data-tooltip={
                view && !view.installed ? `请先安装 ${agentLabel(agent)}` : "在 Finder 中显示"
              }
              onClick={() => void reveal()}
              disabled={busy || !!(view && !view.installed)}
            >
              <IconFolderOpen />
            </button>
            <button
              type="button"
              className="action-btn"
              data-tooltip={
                view && !view.installed ? `请先安装 ${agentLabel(agent)}` : "添加供应商"
              }
              onClick={openAddProvider}
              disabled={busy || !!(view && !view.installed)}
            >
              <IconPlus />
            </button>
          </div>
        </div>
      </div>

      <div className="content-body">
        <Toast message={statusMsg} />

        {view && view.installed && (
          <div className="oc-defaults-bar">
            <div className="oc-defaults-main">
              <div className="oc-defaults-row">
                <span className="oc-defaults-label">默认模型</span>
                {view.defaultsSupported ? (
                  <code className="oc-defaults-value">{view.model || "未设置"}</code>
                ) : (
                  <span className="oc-defaults-value">
                    请在 omp 内使用 /model 或 omp config 命令管理
                  </span>
                )}
              </div>
              {view.smallModelSupported && (
                <div className="oc-defaults-row">
                  <span className="oc-defaults-label">small_model</span>
                  <code className="oc-defaults-value">{view.smallModel || "未设置"}</code>
                </div>
              )}
              <div className="oc-defaults-path" title={view.configPath}>
                {view.configPath}
                {view.isJsonc ? " · jsonc" : ""}
                {!view.configExists ? " · 尚未创建" : ""}
              </div>
            </div>
            {view.defaultsSupported && (
              <button type="button" className="btn btn-secondary" onClick={openDefaults} disabled={busy}>
                设置默认模型
              </button>
            )}
          </div>
        )}

        {agent === "pi" && view?.installed && (
          <div className="oc-warning">
            Pi 不内置 MCP：写入 ~/.pi/agent/mcp.json 后，需安装 pi-mcp-adapter 扩展才能生效。
          </div>
        )}

        {view?.installed && (
          <div className={`oc-catalog-bar ${catalogError ? "is-error" : catalog ? "is-ok" : ""}`}>
            <span>
              {catalogLoading
                ? "正在加载 Models.dev 目录…"
                : catalog
                  ? `Models.dev 已加载：${catalog.providers.length} 个供应商${
                      catalog.fromCache ? "（缓存）" : ""
                    }`
                  : catalogError
                    ? `目录离线：${catalogError}`
                    : "Models.dev 未加载"}
            </span>
            <button
              type="button"
              className="btn btn-secondary"
              onClick={() => void loadCatalog(true)}
              disabled={catalogLoading || busy}
            >
              {catalogError ? "重试" : "刷新目录"}
            </button>
          </div>
        )}

        {view?.installed && view.warnings?.length ? (
          <div className="oc-warning">
            {view.warnings.map((w) => (
              <div key={w}>{w}</div>
            ))}
          </div>
        ) : null}

        {loading && !view ? (
          <div className="empty-state">
            <div className="empty-state-text">加载中…</div>
          </div>
        ) : view && !view.installed ? (
          <div className="empty-state">
            <IconEmpty />
            <div className="empty-state-text">未检测到 {agentLabel(agent)}</div>
            <div className="empty-state-subtext">
              {AGENT_TABS.find((t) => t.id === agent)?.notInstalledHint}
            </div>
          </div>
        ) : !view || view.providers.length === 0 ? (
          <div className="empty-state">
            <IconEmpty />
            <div className="empty-state-text">暂无自定义供应商</div>
            <button type="button" className="btn btn-primary" onClick={openAddProvider}>
              添加供应商
            </button>
          </div>
        ) : (
          <>
            <div className="mcp-summary">
              共 <strong>{view.providers.length}</strong> 个供应商 ·{" "}
              <strong>{configuredModelOptions.length}</strong> 个模型
            </div>
            <div className="oc-provider-list">
              {view.providers.map((p) => (
                <div key={p.id} className="mcp-card oc-provider-card">
                  <div className="mcp-card-header">
                    <div className="mcp-card-icon">{(p.name || p.id).slice(0, 2).toUpperCase()}</div>
                    <div className="mcp-card-main">
                      <div className="mcp-card-title-row">
                        <span className="mcp-card-title">{p.name || p.id}</span>
                        <span className="mcp-type-badge">{p.id}</span>
                        {p.hasApiKey ? (
                          <span className="oc-pill oc-pill-key" title={apiKeySourceLabel(p.apiKeySource)}>
                            <IconKey /> 已配置密钥
                          </span>
                        ) : (
                          <span className="oc-pill">无密钥</span>
                        )}
                      </div>
                      <div className="oc-provider-meta">
                        {p.npm ? <span>npm: {p.npm}</span> : null}
                        {p.baseUrl ? (
                          <span className="oc-provider-url" title={p.baseUrl}>
                            {p.baseUrl}
                          </span>
                        ) : null}
                        {p.api ? (
                          <span className="oc-provider-url" title={p.api}>
                            {!isOpenCode ? piApiLabel(p.api) : p.api}
                          </span>
                        ) : null}
                      </div>
                    </div>
                    <div className="mcp-card-actions">
                      <button
                        type="button"
                        className="btn-icon-action mcp-card-action"
                        data-tooltip="添加模型"
                        onClick={() => openAddModel(p.id)}
                      >
                        <IconPlus />
                      </button>
                      <button
                        type="button"
                        className="btn-icon-action mcp-card-action"
                        data-tooltip="编辑"
                        onClick={() => void openEditProvider(p)}
                      >
                        <IconEdit />
                      </button>
                      <button
                        type="button"
                        className="btn-delete mcp-card-action"
                        data-tooltip="删除"
                        onClick={() => {
                          setDeleteAuthToo(true);
                          setDeleteProvider(p);
                        }}
                      >
                        <IconTrash />
                      </button>
                    </div>
                  </div>

                  {p.models.length === 0 ? (
                    <div className="oc-models-empty">尚无模型，点击 + 添加</div>
                  ) : (
                    <div className="oc-model-list">
                      {p.models.map((m) => {
                        const ref = modelRef(p.id, m.id);
                        return (
                          <ModelChip
                            key={m.id}
                            providerId={p.id}
                            model={m}
                            isDefault={view.model === ref}
                            isSmall={view.smallModel === ref}
                            onEdit={() => openEditModel(p.id, m)}
                            onDelete={() => setDeleteModelTarget({ providerId: p.id, model: m })}
                          />
                        );
                      })}
                    </div>
                  )}
                </div>
              ))}
            </div>
          </>
        )}
      </div>

      {/* ===== Provider modal ===== */}
      <div className={`modal-overlay ${providerForm ? "visible" : ""}`} {...providerDismiss}>
        <div className="modal modal-lg oc-modal">
          <div className="modal-header">
            <h2 className="modal-title">
              {providerForm?.isNew ? "添加供应商" : "编辑供应商"}
            </h2>
            <button
              type="button"
              className="modal-close"
              onClick={() => !busy && setProviderForm(null)}
              disabled={busy}
            >
              <IconClose />
            </button>
          </div>
          {providerForm && (
            <>
              <div className="modal-body oc-modal-body">
                {aiProviders.length > 0 && (
                  <div className="form-group">
                    <label className="form-label" id="oc-ai-provider-label" htmlFor="oc-ai-provider">
                      选择 AI 供应商（可选）
                    </label>
                    <AppSelect
                      id="oc-ai-provider"
                      labelId="oc-ai-provider-label"
                      value={formAiProviderId}
                      options={aiProviderOptions}
                      onChange={(v) => void pickAiProvider(v)}
                      disabled={busy}
                      placeholder="未选择（自行填写）"
                    />
                    <p className="oc-form-hint">
                      从已配置的 AI 供应商中选择，自动回填显示名称、Base URL 与 API Key。
                    </p>
                  </div>
                )}
                <div className="form-group">
                  <label className="form-label" htmlFor="oc-pid">
                    供应商 ID
                  </label>
                  <input
                    ref={providerIdRef}
                    id="oc-pid"
                    className="form-input"
                    placeholder="例如 my-proxy"
                    value={providerForm.id}
                    onChange={(e) =>
                      setProviderForm({ ...providerForm, id: e.target.value })
                    }
                    disabled={busy}
                    autoComplete="off"
                    spellCheck={false}
                  />
                </div>
                <div className="form-group">
                  <label className="form-label" htmlFor="oc-pname">
                    显示名称
                  </label>
                  <input
                    id="oc-pname"
                    className="form-input"
                    placeholder="可选"
                    value={providerForm.name}
                    onChange={(e) =>
                      setProviderForm({ ...providerForm, name: e.target.value })
                    }
                    disabled={busy}
                  />
                </div>
                {isOpenCode && (
                  <div className="form-group">
                    <label className="form-label" id="oc-npm-label" htmlFor="oc-npm">
                      npm 包
                    </label>
                    <AppSelect
                      id="oc-npm"
                      labelId="oc-npm-label"
                      value={
                        NPM_PRESETS.some((x) => x.value === providerForm.npm)
                          ? providerForm.npm
                          : "__custom__"
                      }
                      options={NPM_SELECT_OPTIONS}
                      onChange={(v) => {
                        // 「自定义…」只切换展示态，不改写当前 npm 值，由下方输入框编辑
                        if (v === "__custom__") return;
                        setProviderForm({ ...providerForm, npm: v });
                      }}
                      disabled={busy}
                      placeholder="选择 npm 包"
                    />
                    <input
                      className="form-input"
                      style={{ marginTop: 8 }}
                      placeholder="@ai-sdk/…"
                      value={providerForm.npm}
                      onChange={(e) =>
                        setProviderForm({ ...providerForm, npm: e.target.value })
                      }
                      disabled={busy}
                      spellCheck={false}
                    />
                  </div>
                )}
                {!isOpenCode && (
                  <div className="form-group">
                    <label className="form-label" id="oc-api-format-label" htmlFor="oc-api-format">
                      API 格式
                    </label>
                    <AppSelect
                      id="oc-api-format"
                      labelId="oc-api-format-label"
                      value={providerForm.api}
                      options={PI_API_OPTIONS}
                      onChange={(api) => setProviderForm({ ...providerForm, api })}
                      disabled={busy}
                      placeholder="请选择 API 格式"
                    />
                    <p className="oc-form-hint">
                      选择后会写入 Pi/Oh-My-Pi 的供应商配置。
                    </p>
                  </div>
                )}
                <div className="form-group">
                  <label className="form-label" htmlFor="oc-base">
                    Base URL
                  </label>
                  <input
                    id="oc-base"
                    className="form-input"
                    placeholder="https://api.example.com/v1"
                    value={providerForm.baseUrl}
                    onChange={(e) =>
                      setProviderForm({ ...providerForm, baseUrl: e.target.value })
                    }
                    disabled={busy}
                    spellCheck={false}
                  />
                </div>
                {isOpenCode && (
                  <div className="oc-form-grid">
                    <div className="form-group">
                      <label className="form-label" htmlFor="oc-timeout">
                        timeout (ms)
                      </label>
                      <input
                        id="oc-timeout"
                        className="form-input"
                        inputMode="numeric"
                        placeholder="默认"
                        value={providerForm.timeout}
                        onChange={(e) =>
                          setProviderForm({ ...providerForm, timeout: e.target.value })
                        }
                        disabled={busy}
                      />
                    </div>
                    <div className="form-group">
                      <label className="form-label" htmlFor="oc-chunk">
                        chunkTimeout (ms)
                      </label>
                      <input
                        id="oc-chunk"
                        className="form-input"
                        inputMode="numeric"
                        placeholder="默认"
                        value={providerForm.chunkTimeout}
                        onChange={(e) =>
                          setProviderForm({ ...providerForm, chunkTimeout: e.target.value })
                        }
                        disabled={busy}
                      />
                    </div>
                  </div>
                )}
                {isOpenCode && (
                  <div className="form-group">
                    <label className="form-label" htmlFor="oc-wl">
                      whitelist（逗号分隔）
                    </label>
                    <input
                      id="oc-wl"
                      className="form-input"
                      value={providerForm.whitelist}
                      onChange={(e) =>
                        setProviderForm({ ...providerForm, whitelist: e.target.value })
                      }
                      disabled={busy}
                    />
                  </div>
                )}
                {isOpenCode && (
                  <div className="form-group">
                    <label className="form-label" htmlFor="oc-bl">
                      blacklist（逗号分隔）
                    </label>
                    <input
                      id="oc-bl"
                      className="form-input"
                      value={providerForm.blacklist}
                      onChange={(e) =>
                        setProviderForm({ ...providerForm, blacklist: e.target.value })
                      }
                      disabled={busy}
                    />
                  </div>
                )}
                <div className="form-group">
                  <label className="form-label" htmlFor="oc-key">
                    API Key
                  </label>
                  <div className="form-input-with-action">
                    <input
                      id="oc-key"
                      className="form-input"
                      type={showApiKey ? "text" : "password"}
                      placeholder={
                        providerForm.isNew
                          ? `可选，写入 ${authFileHint(agent)}`
                          : "留空且不改动；清空并保存可删除密钥"
                      }
                      value={providerForm.apiKey}
                      onChange={(e) =>
                        setProviderForm({
                          ...providerForm,
                          apiKey: e.target.value,
                          apiKeyTouched: true,
                        })
                      }
                      disabled={busy}
                      autoComplete="new-password"
                      spellCheck={false}
                    />
                    <button
                      type="button"
                      className="form-input-action"
                      data-tooltip={showApiKey ? "隐藏" : "显示"}
                      onClick={() => setShowApiKey((v) => !v)}
                      disabled={busy}
                    >
                      {showApiKey ? <IconEyeOff /> : <IconEye />}
                    </button>
                  </div>
                  <p className="oc-form-hint">
                    密钥默认写入 <code>auth.json</code>，列表不会回传明文。清空输入并保存可删除密钥。
                  </p>
                  <button
                    type="button"
                    className="btn btn-secondary"
                    style={{ marginTop: 8 }}
                    onClick={() =>
                      setProviderForm({
                        ...providerForm,
                        apiKey: "",
                        apiKeyTouched: true,
                      })
                    }
                    disabled={busy}
                  >
                    清除密钥
                  </button>
                </div>
                {formError ? <div className="form-error">{formError}</div> : null}
              </div>
              <div className="modal-footer">
                <button
                  type="button"
                  className="btn btn-secondary"
                  onClick={() => setProviderForm(null)}
                  disabled={busy}
                >
                  取消
                </button>
                <button
                  type="button"
                  className="btn btn-primary"
                  onClick={() => void saveProvider()}
                  disabled={busy}
                >
                  {busy ? "保存中…" : "保存"}
                </button>
              </div>
            </>
          )}
        </div>
      </div>

      {/* ===== Model modal ===== */}
      <div className={`modal-overlay ${modelForm ? "visible" : ""}`} {...modelDismiss}>
        <div className="modal modal-lg oc-modal">
          <div className="modal-header">
            <h2 className="modal-title">{modelForm?.isNew ? "添加模型" : "编辑模型"}</h2>
            <button
              type="button"
              className="modal-close"
              onClick={() => !busy && setModelForm(null)}
              disabled={busy}
            >
              <IconClose />
            </button>
          </div>
          {modelForm && (
            <>
              <div className="modal-body oc-modal-body">
                <div className="oc-catalog-actions">
                  <button
                    type="button"
                    className="btn btn-secondary"
                    disabled={busy || providerPickLoading}
                    onClick={() => {
                      if (providerPickOpen) {
                        setProviderPickOpen(false);
                      } else {
                        void loadProviderModels(modelForm.providerId);
                      }
                    }}
                  >
                    {providerPickLoading
                      ? "加载中…"
                      : providerPickOpen
                        ? "收起列表"
                        : "从供应商模型列表选择"}
                  </button>
                </div>

                {providerPickOpen && (
                  <div className="oc-catalog-picker">
                    <input
                      className="form-input"
                      placeholder="搜索模型…"
                      value={providerPickQuery}
                      onChange={(e) => setProviderPickQuery(e.target.value)}
                      disabled={busy}
                    />
                    {providerPickMsg && (
                      <div className="oc-catalog-picker-hint">{providerPickMsg}</div>
                    )}
                    <div className="oc-catalog-hits">
                      {providerPickLoading ? (
                        <div className="app-select-empty">正在拉取模型列表…</div>
                      ) : providerPickHitDetails.length === 0 ? (
                        <div className="app-select-empty">无匹配结果</div>
                      ) : (
                        providerPickHitDetails.map(({ id, catalogHit }) => {
                          const subParts: string[] = [];
                          if (catalogHit?.name && catalogHit.name !== id) {
                            subParts.push(catalogHit.name);
                          }
                          const limitText = formatLimit(
                            catalogHit?.limitContext,
                            catalogHit?.limitOutput,
                            catalogHit?.limitInput,
                          );
                          if (limitText && limitText !== "未设置 limit") {
                            subParts.push(limitText);
                          }
                          // 模态短标签：把 input + output modalities 合并一行展示
                          if (catalogHit) {
                            const modalityText = formatModalityTags(
                              catalogHit.modalitiesInput,
                              catalogHit.modalitiesOutput,
                            );
                            if (modalityText) subParts.push(modalityText);
                          }
                          const inputMissing =
                            !!catalogHit &&
                            catalogHit.limitInput == null &&
                            catalogHit.limitContext != null;
                          return (
                            <button
                              key={id}
                              type="button"
                              className="oc-catalog-hit"
                              onClick={() => applyProviderPick(id)}
                              disabled={busy}
                            >
                              <div className="oc-catalog-hit-row">
                                <span
                                  className="oc-catalog-hit-title"
                                  title={catalogHit?.name ?? id}
                                >
                                  {id}
                                </span>
                                {catalogHit && (
                                  <span
                                    className="oc-catalog-hit-badge"
                                    title={`来自 Models.dev 目录：${catalogHit.name ?? id}`}
                                  >
                                    目录
                                  </span>
                                )}
                              </div>
                              {subParts.length > 0 && (
                                <span className="oc-catalog-hit-sub">
                                  {subParts.join(" · ")}
                                </span>
                              )}
                              {inputMissing && (
                                <span className="oc-catalog-hit-meta">
                                  input 目录未提供
                                </span>
                              )}
                            </button>
                          );
                        })
                      )}
                    </div>
                  </div>
                )}

                <div className="form-group">
                  <label className="form-label">供应商</label>
                  <code className="oc-defaults-value">{modelForm.providerId}</code>
                </div>
                <div className="form-group">
                  <label className="form-label" htmlFor="oc-mid">
                    模型 ID
                  </label>
                  <input
                    ref={modelIdRef}
                    id="oc-mid"
                    className="form-input"
                    placeholder="例如 grok-4.5"
                    value={modelForm.id}
                    onChange={(e) => {
                      const newId = e.target.value;
                      setModelForm((prev) => {
                        if (!prev) return prev;
                        const base = { ...prev, id: newId };
                        // 新建模式下：若用户尚未手动填过限制，命中目录时自动补全空字段
                        // 编辑模式下不自动覆盖，避免改动已存在的模型
                        if (prev.isNew) {
                          return applyCatalogLimits(base, catalog, {
                            overwrite: false,
                            agent,
                          });
                        }
                        return base;
                      });
                    }}
                    disabled={busy}
                    spellCheck={false}
                  />
                </div>
                <div className="form-group">
                  <label className="form-label" htmlFor="oc-mname">
                    显示名称
                  </label>
                  <input
                    id="oc-mname"
                    className="form-input"
                    value={modelForm.name}
                    onChange={(e) => setModelForm({ ...modelForm, name: e.target.value })}
                    disabled={busy}
                  />
                </div>

                <div className="form-group">
                  <div className="oc-form-label-row">
                    <label className="form-label">上下文 / 输出限制</label>
                    {catalogMatchForLimits && (
                      <button
                        type="button"
                        className="oc-link-btn"
                        disabled={busy}
                        onClick={() => {
                          setModelForm((prev) =>
                            prev
                              ? applyCatalogLimits(prev, catalog, {
                                  overwrite: true,
                                  agent,
                                })
                              : prev,
                          );
                        }}
                      >
                        从 Models.dev 补全
                      </button>
                    )}
                  </div>
                  <div className="oc-form-grid oc-form-grid-3">
                    <input
                      className="form-input"
                      inputMode="numeric"
                      placeholder="context"
                      value={modelForm.limitContext}
                      onChange={(e) =>
                        setModelForm({ ...modelForm, limitContext: e.target.value })
                      }
                      disabled={busy}
                    />
                    <input
                      className="form-input"
                      inputMode="numeric"
                      placeholder="input"
                      value={modelForm.limitInput}
                      onChange={(e) =>
                        setModelForm({ ...modelForm, limitInput: e.target.value })
                      }
                      disabled={busy}
                    />
                    <input
                      className="form-input"
                      inputMode="numeric"
                      placeholder="output"
                      value={modelForm.limitOutput}
                      onChange={(e) =>
                        setModelForm({ ...modelForm, limitOutput: e.target.value })
                      }
                      disabled={busy}
                    />
                  </div>
                  {catalogMatchForLimits && (
                    <div className="oc-form-hint">
                      Models.dev 目录已匹配到 <code>{catalogMatchForLimits.name}</code>
                      {agent === "opencode" && catalogMatchForLimits.limitInput == null
                        ? "；input 字段目录未提供，已自动跳过"
                        : ""}
                      ，{agent === "opencode" ? "上下文/输入/输出" : "上下文/输出"}
                      限制可在命中时自动或手动补全
                    </div>
                  )}
                </div>

                <div className="form-group">
                  <label className="form-label">输入模态</label>
                  <div className="oc-modality-checks">
                    {inputModalityOptions.map((m) => (
                      <label key={`in-${m}`} className="ui-check">
                        <input
                          type="checkbox"
                          className="ui-check-input"
                          checked={modelForm.modalitiesInput.includes(m)}
                          onChange={() =>
                            setModelForm({
                              ...modelForm,
                              modalitiesInput: toggleInputModality(
                                modelForm.modalitiesInput,
                                m,
                                agent,
                              ),
                            })
                          }
                          disabled={busy}
                        />
                        <CheckGlyph />
                        <span className="ui-check-label">{m}</span>
                      </label>
                    ))}
                  </div>
                </div>
                {isOpenCode && (
                  <div className="form-group">
                    <label className="form-label">输出模态</label>
                    <div className="oc-modality-checks">
                      {MODALITY_OPTIONS.map((m) => (
                        <label key={`out-${m}`} className="ui-check">
                          <input
                            type="checkbox"
                            className="ui-check-input"
                            checked={modelForm.modalitiesOutput.includes(m)}
                            onChange={() =>
                              setModelForm({
                                ...modelForm,
                                modalitiesOutput: toggleModality(modelForm.modalitiesOutput, m),
                              })
                            }
                            disabled={busy}
                          />
                          <CheckGlyph />
                          <span className="ui-check-label">{m}</span>
                        </label>
                      ))}
                    </div>
                  </div>
                )}

                <div className="form-group">
                  <label className="form-label">能力标记</label>
                  <div className="oc-modality-checks">
                    {(
                      // toolCall/attachment 为 OpenCode 专属字段；Pi 家族仅建模 reasoning
                      (isOpenCode
                        ? ([
                            ["reasoning", "reasoning"],
                            ["toolCall", "tool_call"],
                            ["attachment", "attachment"],
                          ] as const)
                        : ([["reasoning", "reasoning"]] as const))
                    ).map(([key, label]) => {
                      const val = modelForm[key];
                      return (
                        <label key={key} className="ui-check">
                          <input
                            type="checkbox"
                            className="ui-check-input"
                            checked={val === true}
                            onChange={() =>
                              setModelForm({
                                ...modelForm,
                                [key]: val === true ? null : true,
                              })
                            }
                            disabled={busy}
                          />
                          <CheckGlyph />
                          <span className="ui-check-label">{label}</span>
                        </label>
                      );
                    })}
                  </div>
                </div>

                {isOpenCode && (
                  <div className="form-group">
                    <label className="form-label">思考 / Reasoning</label>
                    {(hasEffort || !catalogReasoning.length) && (
                    <div className="form-group">
                      <label
                        className="form-label form-label-optional"
                        id="oc-effort-label"
                        htmlFor="oc-effort"
                      >
                        reasoningEffort
                      </label>
                      <AppSelect
                        id="oc-effort"
                        labelId="oc-effort-label"
                        value={modelForm.reasoningEffort}
                        options={(() => {
                          const base: AppSelectOption[] = [
                            { value: "", label: "（不设置）" },
                            ...effortValues.map((v) => ({ value: v, label: v })),
                          ];
                          // 保留目录外的既有值，避免编辑时被静默清空展示
                          if (
                            modelForm.reasoningEffort &&
                            !base.some((o) => o.value === modelForm.reasoningEffort)
                          ) {
                            base.push({
                              value: modelForm.reasoningEffort,
                              label: modelForm.reasoningEffort,
                            });
                          }
                          return base;
                        })()}
                        onChange={(v) =>
                          setModelForm({ ...modelForm, reasoningEffort: v })
                        }
                        disabled={busy}
                        placeholder="（不设置）"
                      />
                    </div>
                  )}
                  {(hasBudget || hasToggle || !catalogReasoning.length) && (
                    <div className="oc-form-grid">
                      <div className="form-group">
                        <label
                          className="form-label form-label-optional"
                          id="oc-think-type-label"
                          htmlFor="oc-think-type"
                        >
                          thinking.type
                        </label>
                        <AppSelect
                          id="oc-think-type"
                          labelId="oc-think-type-label"
                          value={modelForm.thinkingType}
                          options={(() => {
                            if (
                              modelForm.thinkingType &&
                              !THINKING_TYPE_OPTIONS.some(
                                (o) => o.value === modelForm.thinkingType,
                              )
                            ) {
                              return [
                                ...THINKING_TYPE_OPTIONS,
                                {
                                  value: modelForm.thinkingType,
                                  label: modelForm.thinkingType,
                                },
                              ];
                            }
                            return THINKING_TYPE_OPTIONS;
                          })()}
                          onChange={(v) =>
                            setModelForm({ ...modelForm, thinkingType: v })
                          }
                          disabled={busy}
                          placeholder="（不设置）"
                        />
                      </div>
                      <div className="form-group">
                        <label className="form-label form-label-optional" htmlFor="oc-budget">
                          budgetTokens
                        </label>
                        <input
                          id="oc-budget"
                          className="form-input"
                          inputMode="numeric"
                          value={modelForm.thinkingBudgetTokens}
                          onChange={(e) =>
                            setModelForm({
                              ...modelForm,
                              thinkingBudgetTokens: e.target.value,
                            })
                          }
                          disabled={busy}
                        />
                      </div>
                    </div>
                  )}
                  </div>
                )}

                <div className="form-group">
                  <button
                    type="button"
                    className="oc-advanced-toggle"
                    onClick={() =>
                      setModelForm({ ...modelForm, showAdvanced: !modelForm.showAdvanced })
                    }
                  >
                    <IconChevron open={modelForm.showAdvanced} />
                    高级 options / variants
                  </button>
                  {modelForm.showAdvanced && (
                    <div className="oc-advanced-body">
                      <label className="form-label" htmlFor="oc-extra">
                        extra options（JSON 对象）
                      </label>
                      <textarea
                        id="oc-extra"
                        className="form-input oc-textarea"
                        rows={5}
                        placeholder={'{\n  "temperature": 0.2\n}'}
                        value={modelForm.extraOptionsRaw}
                        onChange={(e) =>
                          setModelForm({ ...modelForm, extraOptionsRaw: e.target.value })
                        }
                        disabled={busy}
                        spellCheck={false}
                      />
                      <p className="oc-form-hint">未建模字段会原样写回 options，避免丢失。</p>
                    </div>
                  )}
                </div>

                {formError ? <div className="form-error">{formError}</div> : null}
              </div>
              <div className="modal-footer">
                <button
                  type="button"
                  className="btn btn-secondary"
                  onClick={() => setModelForm(null)}
                  disabled={busy}
                >
                  取消
                </button>
                <button
                  type="button"
                  className="btn btn-primary"
                  onClick={() => void saveModel()}
                  disabled={busy}
                >
                  {busy ? "保存中…" : "保存"}
                </button>
              </div>
            </>
          )}
        </div>
      </div>

      {/* ===== Defaults modal ===== */}
      <div className={`modal-overlay ${defaultsOpen ? "visible" : ""}`} {...defaultsDismiss}>
        <div className="modal oc-modal">
          <div className="modal-header">
            <h2 className="modal-title">默认模型</h2>
            <button
              type="button"
              className="modal-close"
              onClick={() => !busy && setDefaultsOpen(false)}
              disabled={busy}
            >
              <IconClose />
            </button>
          </div>
          <div className="modal-body oc-modal-body">
            <div className="form-group">
              <label className="form-label" htmlFor="oc-search-models">
                搜索已配置模型
              </label>
              <input
                id="oc-search-models"
                className="form-input"
                placeholder="provider/model"
                value={modelSearch}
                onChange={(e) => setModelSearch(e.target.value)}
                disabled={busy}
              />
            </div>
            <div className="form-group">
              <label className="form-label" htmlFor="oc-default-model">
                model
              </label>
              <input
                id="oc-default-model"
                className="form-input"
                list="oc-model-options"
                placeholder="provider/model，清空则删除"
                value={draftModel}
                onChange={(e) => setDraftModel(e.target.value)}
                disabled={busy}
                spellCheck={false}
              />
            </div>
            <div className="form-group">
              <label className="form-label" htmlFor="oc-small-model">
                small_model
              </label>
              <input
                id="oc-small-model"
                className="form-input"
                list="oc-model-options"
                placeholder="provider/model，清空则删除"
                value={draftSmallModel}
                onChange={(e) => setDraftSmallModel(e.target.value)}
                disabled={busy}
                spellCheck={false}
              />
            </div>
            <datalist id="oc-model-options">
              {filteredModelOptions.map((opt) => (
                <option key={opt} value={opt} />
              ))}
            </datalist>
            {filteredModelOptions.length > 0 && (
              <div className="oc-defaults-suggestions">
                {filteredModelOptions.slice(0, 12).map((opt) => (
                  <button
                    key={opt}
                    type="button"
                    className="oc-suggest-chip"
                    onClick={() => setDraftModel(opt)}
                    disabled={busy}
                  >
                    {opt}
                  </button>
                ))}
              </div>
            )}
          </div>
          <div className="modal-footer">
            <button
              type="button"
              className="btn btn-secondary"
              onClick={() => setDefaultsOpen(false)}
              disabled={busy}
            >
              取消
            </button>
            <button
              type="button"
              className="btn btn-primary"
              onClick={() => void saveDefaults()}
              disabled={busy}
            >
              保存
            </button>
          </div>
        </div>
      </div>

      {/* ===== Delete provider ===== */}
      <div
        className={`modal-overlay ${deleteProvider ? "visible" : ""}`}
        {...deleteProviderDismiss}
      >
        <div className="modal">
          <div className="modal-header">
            <h2 className="modal-title">删除供应商</h2>
            <button
              type="button"
              className="modal-close"
              onClick={() => !busy && setDeleteProvider(null)}
              disabled={busy}
            >
              <IconClose />
            </button>
          </div>
          <div className="modal-body">
            <p>
              确定删除供应商 <strong>{deleteProvider?.id}</strong> 及其下全部模型？
            </p>
            <label className="ui-check" style={{ marginTop: 12 }}>
              <input
                type="checkbox"
                className="ui-check-input"
                checked={deleteAuthToo}
                onChange={(e) => setDeleteAuthToo(e.target.checked)}
                disabled={busy}
              />
              <CheckGlyph />
              <span className="ui-check-label">同时删除 auth.json 中的密钥</span>
            </label>
          </div>
          <div className="modal-footer">
            <button
              type="button"
              className="btn btn-secondary"
              onClick={() => setDeleteProvider(null)}
              disabled={busy}
            >
              取消
            </button>
            <button
              type="button"
              className="btn btn-danger"
              onClick={() => void confirmDeleteProvider()}
              disabled={busy}
            >
              删除
            </button>
          </div>
        </div>
      </div>

      {/* ===== Delete model ===== */}
      <div
        className={`modal-overlay ${deleteModelTarget ? "visible" : ""}`}
        {...deleteModelDismiss}
      >
        <div className="modal">
          <div className="modal-header">
            <h2 className="modal-title">删除模型</h2>
            <button
              type="button"
              className="modal-close"
              onClick={() => !busy && setDeleteModelTarget(null)}
              disabled={busy}
            >
              <IconClose />
            </button>
          </div>
          <div className="modal-body">
            <p>
              确定删除{" "}
              <code>
                {deleteModelTarget
                  ? modelRef(deleteModelTarget.providerId, deleteModelTarget.model.id)
                  : ""}
              </code>
              ？
            </p>
          </div>
          <div className="modal-footer">
            <button
              type="button"
              className="btn btn-secondary"
              onClick={() => setDeleteModelTarget(null)}
              disabled={busy}
            >
              取消
            </button>
            <button
              type="button"
              className="btn btn-danger"
              onClick={() => void confirmDeleteModel()}
              disabled={busy}
            >
              删除
            </button>
          </div>
        </div>
      </div>
    </>
  );
}
