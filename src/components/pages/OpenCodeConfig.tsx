import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useStatusMessage } from "@/lib/useStatusMessage";
import { Toast } from "@/components/Toast";
import { CheckGlyph, useOverlayDismiss } from "../ui";

import type {
  CatalogModelSummary,
  CatalogProvider,
  CatalogReasoningOption,
  ModelsDevCatalog,
  OpencodeConfigView,
  OpencodeForkSyncStatus,
  OpencodeModelView,
  OpencodeProviderView,
  OpencodeVariantView,
} from "./opencode-config/types";
import { EFFORT_PRESETS, MODALITY_OPTIONS } from "./opencode-config/types";
import {
  invokeDeleteModel,
  invokeDeleteProvider,
  invokeFetchCatalog,
  invokeGetConfig,
  invokeGetForkSyncStatus,
  invokeGetSecret,
  invokeRevealConfig,
  invokeSetDefaults,
  invokeSyncToFork,
  invokeUpsertModel,
  invokeUpsertProvider,
} from "./opencode-config/api";
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
  if (!catalog) return null;
  const p = catalog.providers.find((x) => x.id === providerId);
  return p?.models.find((m) => m.id === modelId) ?? null;
}

function reasoningOptionsFor(
  catalog: ModelsDevCatalog | null,
  providerId: string,
  modelId: string,
): CatalogReasoningOption[] {
  return findCatalogModel(catalog, providerId, modelId)?.reasoningOptions ?? [];
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

const THINKING_TYPE_OPTIONS: AppSelectOption[] = [
  { value: "", label: "（不设置）" },
  { value: "enabled", label: "enabled" },
  { value: "disabled", label: "disabled" },
];

type ProviderForm = {
  id: string;
  previousId: string | null;
  name: string;
  npm: string;
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
  variants: OpencodeVariantView[];
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
  model: OpencodeModelView;
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

export default function OpenCodeConfig() {
  const [view, setView] = useState<OpencodeConfigView | null>(null);
  const [catalog, setCatalog] = useState<ModelsDevCatalog | null>(null);
  const [catalogError, setCatalogError] = useState("");
  const [catalogLoading, setCatalogLoading] = useState(false);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [statusMsg, setStatusMsg] = useStatusMessage();

  const [providerForm, setProviderForm] = useState<ProviderForm | null>(null);
  const [modelForm, setModelForm] = useState<ModelForm | null>(null);
  const [showApiKey, setShowApiKey] = useState(false);
  const [formError, setFormError] = useState("");

  const [deleteProvider, setDeleteProvider] = useState<OpencodeProviderView | null>(null);
  const [deleteAuthToo, setDeleteAuthToo] = useState(true);
  const [deleteModelTarget, setDeleteModelTarget] = useState<{
    providerId: string;
    model: OpencodeModelView;
  } | null>(null);

  const [defaultsOpen, setDefaultsOpen] = useState(false);
  const [draftModel, setDraftModel] = useState("");
  const [draftSmallModel, setDraftSmallModel] = useState("");
  const [modelSearch, setModelSearch] = useState("");

  const [catalogPickOpen, setCatalogPickOpen] = useState(false);
  const [catalogQuery, setCatalogQuery] = useState("");
  const [catalogProviderFilter, setCatalogProviderFilter] = useState("");

  const [forkStatus, setForkStatus] = useState<OpencodeForkSyncStatus | null>(null);
  const [forkStatusLoading, setForkStatusLoading] = useState(false);
  /** 每个二开 agent 是否勾选「同步 MCP」；默认 false，仅勾选时覆盖目标 mcp。 */
  const [forkSyncMcp, setForkSyncMcp] = useState<Record<string, boolean>>({});
  /** 每个二开 agent 是否勾选「同步 skills」；默认 false，仅勾选时替换目标 skills。 */
  const [forkSyncSkills, setForkSyncSkills] = useState<Record<string, boolean>>({});

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
      setCatalogPickOpen(false);
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

  const loadForkStatus = useCallback(async () => {
    setForkStatusLoading(true);
    try {
      const status = await invokeGetForkSyncStatus();
      setForkStatus(status);
    } catch {
      // 非关键路径：同步条失败不阻塞主配置
      setForkStatus(null);
    } finally {
      setForkStatusLoading(false);
    }
  }, []);

  const applyView = useCallback((next: OpencodeConfigView) => {
    setView(next);
    // provider / mcp 源变化后刷新 fork 对齐状态（失败静默）
    void loadForkStatus();
  }, [loadForkStatus]);

  const loadConfig = useCallback(async (quiet = false) => {
    if (!quiet) setLoading(true);
    try {
      const next = await invokeGetConfig();
      applyView(next);
      if (!quiet) {
        if (next.opencodeInstalled && next.warnings.length > 0) {
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
  }, [applyView, setStatusMsg]);

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

  const handleSyncFork = useCallback(
    async (agent: string) => {
      setBusy(true);
      try {
        const syncMcp = !!forkSyncMcp[agent];
        const syncSkills = !!forkSyncSkills[agent];
        const res = await invokeSyncToFork(agent, syncMcp, syncSkills);
        setStatusMsg(res.message);
        await loadForkStatus();
      } catch (e) {
        setStatusMsg(`同步失败: ${e}`);
      } finally {
        setBusy(false);
      }
    },
    [forkSyncMcp, forkSyncSkills, loadForkStatus, setStatusMsg],
  );

  const handleSyncAllForks = useCallback(async () => {
    setBusy(true);
    try {
      // 按各目标的 MCP / skills 勾选逐个同步，避免批量按钮误覆盖未选中的目标内容。
      const targets = forkStatus?.targets.filter((t) => t.found) ?? [];
      if (targets.length === 0) {
        setStatusMsg("没有可同步的 OpenCode 二开 agent");
        return;
      }
      let okN = 0;
      let failN = 0;
      let skipN = 0;
      for (const t of targets) {
        const syncMcp = !!forkSyncMcp[t.agent];
        const syncSkills = !!forkSyncSkills[t.agent];
        try {
          const res = await invokeSyncToFork(t.agent, syncMcp, syncSkills);
          for (const item of res.results) {
            if (item.ok) {
              if (item.status === "not_installed" || item.status === "no_source") {
                skipN += 1;
              } else {
                okN += 1;
              }
            } else if (item.status === "not_installed" || item.status === "no_source") {
              skipN += 1;
            } else {
              failN += 1;
            }
          }
        } catch {
          failN += 1;
        }
      }
      const message =
        failN === 0
          ? `已同步 ${okN} 个目标${skipN > 0 ? `，跳过 ${skipN} 个` : ""}`
          : `部分失败：成功 ${okN}，失败 ${failN}，跳过 ${skipN}`;
      setStatusMsg(message);
      await loadForkStatus();
    } catch (e) {
      setStatusMsg(`批量同步失败: ${e}`);
    } finally {
      setBusy(false);
    }
  }, [forkStatus, forkSyncMcp, forkSyncSkills, loadForkStatus, setStatusMsg]);

  const forkOutOfSyncCount = useMemo(() => {
    if (!forkStatus) return 0;
    return forkStatus.targets.filter(
      (t) =>
        t.found &&
        (t.status === "out_of_sync" || t.status === "missing" || t.status === "error"),
    ).length;
  }, [forkStatus]);

  const forkInstalledTargets = useMemo(() => {
    if (!forkStatus) return [];
    return forkStatus.targets.filter((t) => t.found);
  }, [forkStatus]);

  useEffect(() => {
    (async () => {
      const next = await loadConfig();
      // Models.dev catalog only matters after OpenCode is installed.
      if (next?.opencodeInstalled) {
        void loadCatalog(false);
      }
    })();
  }, [loadConfig, loadCatalog]);

  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.key !== "Escape" || busy) return;
      setProviderForm(null);
      setModelForm(null);
      setDeleteProvider(null);
      setDeleteModelTarget(null);
      setDefaultsOpen(false);
      setCatalogPickOpen(false);
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

  const catalogHits = useMemo(() => {
    if (!catalog || !catalogPickOpen) return [] as { provider: CatalogProvider; model: CatalogModelSummary }[];
    const q = catalogQuery.trim().toLowerCase();
    const pid = catalogProviderFilter.trim();
    const hits: { provider: CatalogProvider; model: CatalogModelSummary }[] = [];
    for (const p of catalog.providers) {
      if (pid && p.id !== pid) continue;
      for (const m of p.models) {
        if (q) {
          const hay = `${p.id} ${p.name} ${m.id} ${m.name}`.toLowerCase();
          if (!hay.includes(q)) continue;
        }
        hits.push({ provider: p, model: m });
        if (hits.length >= 80) return hits;
      }
    }
    return hits;
  }, [catalog, catalogPickOpen, catalogQuery, catalogProviderFilter]);

  const openAddProvider = () => {
    setFormError("");
    setShowApiKey(false);
    setProviderForm(emptyProviderForm());
    setTimeout(() => providerIdRef.current?.focus(), 80);
  };

  const openEditProvider = async (p: OpencodeProviderView) => {
    setFormError("");
    setShowApiKey(false);
    const form: ProviderForm = {
      id: p.id,
      previousId: p.id,
      name: p.name ?? "",
      npm: p.npm ?? "",
      baseUrl: p.baseUrl ?? p.api ?? "",
      timeout: p.timeout != null ? String(p.timeout) : "",
      chunkTimeout: p.chunkTimeout != null ? String(p.chunkTimeout) : "",
      whitelist: tagsToCsv(p.whitelist),
      blacklist: tagsToCsv(p.blacklist),
      apiKey: "",
      apiKeyTouched: false,
      isNew: false,
    };
    setProviderForm(form);
    if (p.hasApiKey) {
      try {
        const secret = await invokeGetSecret(p.id);
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

  const openAddModel = (providerId: string) => {
    setFormError("");
    setCatalogPickOpen(false);
    setCatalogQuery("");
    setCatalogProviderFilter(providerId);
    setModelForm(emptyModelForm(providerId));
    setTimeout(() => modelIdRef.current?.focus(), 80);
  };

  const openEditModel = (providerId: string, model: OpencodeModelView) => {
    setFormError("");
    setCatalogPickOpen(false);
    setModelForm({
      providerId,
      id: model.id,
      previousId: model.id,
      name: model.name ?? "",
      limitContext: numToInput(model.limitContext),
      limitInput: numToInput(model.limitInput),
      limitOutput: numToInput(model.limitOutput),
      modalitiesInput: [...model.modalitiesInput],
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

  const applyCatalogPick = (provider: CatalogProvider, model: CatalogModelSummary) => {
    setModelForm((prev) => {
      if (!prev) return prev;
      return {
        ...prev,
        providerId: prev.providerId || provider.id,
        id: model.id,
        name: model.name || model.id,
        limitContext: numToInput(model.limitContext),
        limitInput: numToInput(model.limitInput),
        limitOutput: numToInput(model.limitOutput),
        modalitiesInput: model.modalitiesInput.length ? [...model.modalitiesInput] : ["text"],
        modalitiesOutput: model.modalitiesOutput.length ? [...model.modalitiesOutput] : ["text"],
        reasoning: model.reasoning,
        toolCall: model.toolCall,
        attachment: model.attachment,
      };
    });
    setCatalogPickOpen(false);
  };

  const saveProvider = async () => {
    if (!providerForm) return;
    const id = providerForm.id.trim();
    if (!id) {
      setFormError("请填写提供商 ID");
      return;
    }
    setBusy(true);
    setFormError("");
    try {
      const payload: Parameters<typeof invokeUpsertProvider>[0] = {
        id,
        previousId: providerForm.isNew ? null : providerForm.previousId,
        name: providerForm.name.trim() || null,
        npm: providerForm.npm.trim() || null,
        baseUrl: providerForm.baseUrl.trim() || null,
        timeout: (() => {
          const n = parseOptionalNumber(providerForm.timeout);
          return n == null ? null : Math.round(n);
        })(),
        chunkTimeout: (() => {
          const n = parseOptionalNumber(providerForm.chunkTimeout);
          return n == null ? null : Math.round(n);
        })(),
        whitelist: parseCsvTags(providerForm.whitelist),
        blacklist: parseCsvTags(providerForm.blacklist),
      };
      if (providerForm.apiKeyTouched) {
        payload.apiKey = providerForm.apiKey;
      }
      const res = await invokeUpsertProvider(payload);
      if (res.view) applyView(res.view);
      else await loadConfig(true);
      setProviderForm(null);
      setStatusMsg(res.message || "提供商已保存");
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
      setFormError("缺少提供商 ID");
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
      const res = await invokeUpsertModel({
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
        toolCall: modelForm.toolCall,
        attachment: modelForm.attachment,
        thinkingType: modelForm.thinkingType.trim() || null,
        thinkingBudgetTokens: (() => {
          const n = parseOptionalNumber(modelForm.thinkingBudgetTokens);
          return n == null ? null : Math.max(0, Math.round(n));
        })(),
        reasoningEffort: modelForm.reasoningEffort.trim() || null,
        textVerbosity: modelForm.textVerbosity.trim() || null,
        variants: modelForm.variants,
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
      const res = await invokeDeleteProvider(deleteProvider.id, deleteAuthToo);
      if (res.view) applyView(res.view);
      else await loadConfig(true);
      setDeleteProvider(null);
      setStatusMsg(res.message || "提供商已删除");
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
      const res = await invokeSetDefaults({
        model: draftModel.trim(),
        smallModel: draftSmallModel.trim(),
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
      const res = await invokeRevealConfig();
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

  return (
    <>
      <div className="content-header">
        <div className="content-header-bar">
          <h1 className="content-title">OpenCode 配置</h1>
          <div className="header-actions">
            <button
              type="button"
              className={`action-btn ${loading ? "sniffing" : ""}`}
              data-tooltip="刷新"
              onClick={() => {
                void loadConfig();
                if (view?.opencodeInstalled) void loadCatalog(false);
              }}
              disabled={loading || busy}
            >
              <IconRefresh />
            </button>
            <button
              type="button"
              className="action-btn"
              data-tooltip={
                view && !view.opencodeInstalled ? "请先安装 OpenCode" : "在 Finder 中显示"
              }
              onClick={() => void reveal()}
              disabled={busy || !!(view && !view.opencodeInstalled)}
            >
              <IconFolderOpen />
            </button>
            <button
              type="button"
              className="action-btn"
              data-tooltip={
                view && !view.opencodeInstalled ? "请先安装 OpenCode" : "添加提供商"
              }
              onClick={openAddProvider}
              disabled={busy || !!(view && !view.opencodeInstalled)}
            >
              <IconPlus />
            </button>
          </div>
        </div>
      </div>

      <div className="content-body">
        <Toast message={statusMsg} />

        {view && view.opencodeInstalled && (
          <div className="oc-defaults-bar">
            <div className="oc-defaults-main">
              <div className="oc-defaults-row">
                <span className="oc-defaults-label">默认模型</span>
                <code className="oc-defaults-value">{view.model || "未设置"}</code>
              </div>
              <div className="oc-defaults-row">
                <span className="oc-defaults-label">small_model</span>
                <code className="oc-defaults-value">{view.smallModel || "未设置"}</code>
              </div>
              <div className="oc-defaults-path" title={view.configPath}>
                {view.configPath}
                {view.isJsonc ? " · jsonc" : ""}
                {!view.configExists ? " · 尚未创建" : ""}
              </div>
            </div>
            <button type="button" className="btn btn-secondary" onClick={openDefaults} disabled={busy}>
              设置默认模型
            </button>
          </div>
        )}

        {view?.opencodeInstalled && (forkStatus || forkStatusLoading) && (
          <div className="oc-fork-sync-bar">
            <div className="oc-fork-sync-main">
              <div className="oc-fork-sync-title">同步到 OpenCode 二开 Agent</div>
              <div className="oc-fork-sync-desc">
                将本页维护的 <code>provider</code> 覆盖同步到同源配置（如 DevEco Code），并合并对应{" "}
                <code>auth.json</code> 密钥条目。勾选「同步 MCP」后才会覆盖目标的{" "}
                <code>mcp</code>；勾选「同步 skills」后会以 OpenCode 的最新 skills 替换目标 skills 目录。
              </div>
              {forkStatusLoading && !forkStatus ? (
                <div className="oc-fork-sync-empty">正在检测本机 fork…</div>
              ) : forkInstalledTargets.length === 0 ? (
                <div className="oc-fork-sync-empty">
                  未检测到已安装的 OpenCode 二开 agent（当前支持 JsonMcp 方言，例如 DevEco Code）
                </div>
              ) : (
                <>
                  <div className="oc-fork-sync-toolbar">
                    <button
                      type="button"
                      className="claude-env-link-btn"
                      disabled={busy || !forkStatus?.sourceExists}
                      onClick={() => void handleSyncAllForks()}
                    >
                      {forkOutOfSyncCount > 0
                        ? `同步全部（${forkOutOfSyncCount} 未对齐）`
                        : "同步全部"}
                    </button>
                  </div>
                  <ul className="oc-fork-sync-list">
                    {forkInstalledTargets.map((t) => {
                      const syncMcpChecked = !!forkSyncMcp[t.agent];
                      const syncSkillsChecked = !!forkSyncSkills[t.agent];
                      const mcpCheckId = `oc-fork-sync-mcp-${t.agent}`;
                      const skillsCheckId = `oc-fork-sync-skills-${t.agent}`;
                      return (
                        <li key={t.agent} className="oc-fork-sync-item">
                          <div className="oc-fork-sync-item-body">
                            <div className="oc-fork-sync-item-main">
                              <span className="oc-fork-sync-name">{t.displayName}</span>
                              <span
                                className={`oc-fork-sync-badge oc-fork-sync-badge-${t.status}`}
                                data-tooltip={t.message}
                              >
                                {t.status === "in_sync"
                                  ? "已对齐"
                                  : t.status === "out_of_sync"
                                    ? "未对齐"
                                    : t.status === "missing"
                                      ? "无配置文件"
                                      : t.status === "no_source"
                                        ? "无源配置"
                                        : t.status === "error"
                                          ? "错误"
                                          : t.status}
                              </span>
                              <span className="oc-fork-sync-meta">
                                provider {t.providerCount}/{t.sourceProviderCount} · mcp{" "}
                                {t.mcpCount}/{t.sourceMcpCount}
                              </span>
                              <code className="oc-fork-sync-path" title={t.configPath}>
                                {t.configPath}
                              </code>
                            </div>
                            <label className="ui-check oc-fork-sync-mcp-check" htmlFor={mcpCheckId}>
                              <input
                                id={mcpCheckId}
                                type="checkbox"
                                className="ui-check-input"
                                checked={syncMcpChecked}
                                onChange={(e) =>
                                  setForkSyncMcp((prev) => ({
                                    ...prev,
                                    [t.agent]: e.target.checked,
                                  }))
                                }
                                disabled={busy}
                              />
                              <CheckGlyph />
                              <span className="ui-check-label">
                                同步 MCP
                                {syncMcpChecked
                                  ? "（将覆盖目标 mcp）"
                                  : "（默认仅同步 provider）"}
                              </span>
                            </label>
                            <label className="ui-check oc-fork-sync-mcp-check" htmlFor={skillsCheckId}>
                              <input
                                id={skillsCheckId}
                                type="checkbox"
                                className="ui-check-input"
                                checked={syncSkillsChecked}
                                onChange={(e) =>
                                  setForkSyncSkills((prev) => ({
                                    ...prev,
                                    [t.agent]: e.target.checked,
                                  }))
                                }
                                disabled={busy}
                              />
                              <CheckGlyph />
                              <span className="ui-check-label">
                                同步 skills
                                {syncSkillsChecked
                                  ? "（将替换目标 skills）"
                                  : "（不修改目标 skills）"}
                              </span>
                            </label>
                          </div>
                          <button
                            type="button"
                            className="btn btn-secondary"
                            disabled={busy || !forkStatus?.sourceExists}
                            onClick={() => void handleSyncFork(t.agent)}
                          >
                            同步
                          </button>
                        </li>
                      );
                    })}
                  </ul>
                </>
              )}
            </div>
          </div>
        )}

        {view?.opencodeInstalled && (
          <div className={`oc-catalog-bar ${catalogError ? "is-error" : catalog ? "is-ok" : ""}`}>
            <span>
              {catalogLoading
                ? "正在加载 Models.dev 目录…"
                : catalog
                  ? `Models.dev 已加载：${catalog.providers.length} 个提供商${
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

        {view?.opencodeInstalled && view.warnings?.length ? (
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
        ) : view && !view.opencodeInstalled ? (
          <div className="empty-state">
            <IconEmpty />
            <div className="empty-state-text">未检测到 OpenCode</div>
            <div className="empty-state-subtext">
              请先安装 OpenCode CLI 或 App（例如 <code>opencode</code> 命令），安装后重新打开本页即可管理提供商与模型配置。
            </div>
          </div>
        ) : !view || view.providers.length === 0 ? (
          <div className="empty-state">
            <IconEmpty />
            <div className="empty-state-text">
              还没有自定义提供商。可添加 OpenAI Compatible 端点，或从本地配置读取已有 provider。
            </div>
            <button type="button" className="btn btn-primary" onClick={openAddProvider}>
              添加提供商
            </button>
          </div>
        ) : (
          <>
            <div className="mcp-summary">
              共 <strong>{view.providers.length}</strong> 个提供商 ·{" "}
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
                        {p.baseUrl || p.api ? (
                          <span className="oc-provider-url" title={p.baseUrl || p.api || ""}>
                            {p.baseUrl || p.api}
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
              {providerForm?.isNew ? "添加提供商" : "编辑提供商"}
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
                <div className="form-group">
                  <label className="form-label" htmlFor="oc-pid">
                    提供商 ID
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
                          ? "可选，写入 ~/.local/share/opencode/auth.json"
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
                    disabled={!catalog || busy}
                    onClick={() => setCatalogPickOpen((v) => !v)}
                  >
                    {catalogPickOpen ? "收起目录" : "从 Models.dev 选择"}
                  </button>
                  {!catalog && (
                    <span className="oc-form-hint" style={{ margin: 0 }}>
                      目录不可用时仍可手动填写
                    </span>
                  )}
                </div>

                {catalogPickOpen && (
                  <div className="oc-catalog-picker">
                    <input
                      className="form-input"
                      placeholder="搜索提供商 / 模型…"
                      value={catalogQuery}
                      onChange={(e) => setCatalogQuery(e.target.value)}
                      disabled={busy}
                    />
                    <div className="oc-catalog-hits">
                      {catalogHits.length === 0 ? (
                        <div className="app-select-empty">无匹配结果</div>
                      ) : (
                        catalogHits.map(({ provider, model }) => (
                          <button
                            key={`${provider.id}/${model.id}`}
                            type="button"
                            className="oc-catalog-hit"
                            onClick={() => applyCatalogPick(provider, model)}
                            disabled={busy}
                          >
                            <span className="oc-catalog-hit-title">
                              {provider.id}/{model.id}
                            </span>
                            <span className="oc-catalog-hit-sub">
                              {model.name} · {formatLimit(model.limitContext, model.limitOutput)}
                            </span>
                          </button>
                        ))
                      )}
                    </div>
                  </div>
                )}

                <div className="form-group">
                  <label className="form-label">提供商</label>
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
                    onChange={(e) => setModelForm({ ...modelForm, id: e.target.value })}
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
                  <label className="form-label">上下文 / 输出限制</label>
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
                </div>

                <div className="form-group">
                  <label className="form-label">输入模态</label>
                  <div className="oc-modality-checks">
                    {MODALITY_OPTIONS.map((m) => (
                      <label key={`in-${m}`} className="ui-check">
                        <input
                          type="checkbox"
                          className="ui-check-input"
                          checked={modelForm.modalitiesInput.includes(m)}
                          onChange={() =>
                            setModelForm({
                              ...modelForm,
                              modalitiesInput: toggleModality(modelForm.modalitiesInput, m),
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

                <div className="form-group">
                  <label className="form-label">能力标记</label>
                  <div className="oc-modality-checks">
                    {(
                      [
                        ["reasoning", "reasoning"],
                        ["toolCall", "tool_call"],
                        ["attachment", "attachment"],
                      ] as const
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
            <h2 className="modal-title">删除提供商</h2>
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
              确定删除提供商 <strong>{deleteProvider?.id}</strong> 及其下全部模型？
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
