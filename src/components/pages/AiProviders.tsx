import { useState, useEffect, useCallback, useMemo, useRef } from "react";
import { useStatusMessage } from "@/lib/useStatusMessage";
import { Toast } from "@/components/Toast";
import { ModelComboBox } from "../ModelComboBox";
import { useOverlayDismiss } from "../ui";
import { Copy, Pencil, Plus, Trash2, X, Boxes, KeyRound, Download, Eye, EyeOff, Search } from "lucide-react";
import {
  invokeList,
  invokeUpsert,
  invokeDelete,
  invokeGetSecret,
  invokeFetchRemoteModels,
} from "./ai-providers/api";
import {
  MODEL_TIERS,
  PROVIDER_TYPE_OPTIONS,
  type AiProvider,
  type ProviderType,
} from "./ai-providers/types";

/* ===== Icons ===== */
const IconPlus = () => <Plus size={16} strokeWidth={2} />;
const IconTrash = () => <Trash2 size={16} strokeWidth={1.8} />;
const IconClose = () => <X size={16} strokeWidth={2} />;
const IconTrashConfirm = () => <Trash2 size={20} strokeWidth={2} />;
const IconEdit = () => <Pencil size={16} strokeWidth={1.8} />;
const IconCopy = () => <Copy size={16} strokeWidth={1.8} />;
const IconEmpty = () => <Boxes size={40} strokeWidth={1.5} />;
const IconKey = () => <KeyRound size={14} strokeWidth={1.8} />;
const IconDownload = () => <Download size={14} strokeWidth={1.8} />;
const IconSearch = () => <Search size={16} strokeWidth={1.8} />;
const IconEye = () => <Eye size={16} strokeWidth={1.8} />;
const IconEyeOff = () => <EyeOff size={16} strokeWidth={1.8} />;
const IconChevron = ({ open }: { open?: boolean }) => (
  <svg
    width="16"
    height="16"
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    strokeWidth="2"
    style={{
      transform: open ? "rotate(180deg)" : undefined,
      transition: "transform 0.15s ease",
    }}
  >
    <path d="m6 9 6 6 6-6" />
  </svg>
);

/* RemoteModelField 已由共享组件 ModelComboBox 替代。 */

/** 类型徽标：Anthropic / OpenAI 用不同色调区分。 */
function TypeBadge({ type }: { type: ProviderType }) {
  const label =
    type === "anthropic" ? "Anthropic" : type === "openai" ? "OpenAI" : "通用";
  return (
    <span
      className="ai-provider-badge"
      style={
        type !== "openai"
          ? {
              background: "color-mix(in srgb, var(--seed-primary) 12%, transparent)",
              color: "var(--seed-active-fg)",
              borderColor: "color-mix(in srgb, var(--seed-primary) 25%, transparent)",
            }
          : undefined
      }
    >
      {label}
    </span>
  );
}

/** 类型下拉：选项仅两个，复用 app-select 既有样式。 */
function TypeSelect({
  value,
  onChange,
  disabled,
}: {
  value: ProviderType;
  onChange: (value: ProviderType) => void;
  disabled?: boolean;
}) {
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);
  const selected = PROVIDER_TYPE_OPTIONS.find((o) => o.value === value);

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

  return (
    <div
      className={`app-select ${open ? "open" : ""} ${disabled ? "disabled" : ""}`}
      ref={rootRef}
    >
      <button
        type="button"
        className="app-select-trigger form-input"
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-labelledby="ai-provider-type-label"
        disabled={disabled}
        onClick={() => {
          if (!disabled) setOpen((v) => !v);
        }}
      >
        <span className="app-select-value">{selected?.label ?? "请选择"}</span>
        <span className="app-select-chevron" aria-hidden>
          <IconChevron open={open} />
        </span>
      </button>
      {open && (
        <div className="app-select-menu" role="listbox" aria-labelledby="ai-provider-type-label">
          {PROVIDER_TYPE_OPTIONS.map((o) => (
            <button
              key={o.value}
              type="button"
              role="option"
              aria-selected={o.value === value}
              className={`app-select-option ${o.value === value ? "selected" : ""}`}
              onClick={() => {
                onChange(o.value);
                setOpen(false);
              }}
            >
              <span className="app-select-option-title">{o.label}</span>
              {o.sub ? <span className="app-select-option-sub">{o.sub}</span> : null}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

/* ===== Component ===== */
export default function AiProviders() {
  const [providers, setProviders] = useState<AiProvider[]>([]);
  const [loaded, setLoaded] = useState(false);
  // 搜索（250ms 防抖）与类型筛选，交互与 Skills 管理页一致
  const [searchInput, setSearchInput] = useState("");
  const [searchQuery, setSearchQuery] = useState("");
  const [typeFilter, setTypeFilter] = useState<"all" | ProviderType>("all");
  const [showForm, setShowForm] = useState(false);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<string | null>(null);
  const [statusMsg, setStatusMsg] = useStatusMessage();
  const [formError, setFormError] = useState("");
  const [isSaving, setIsSaving] = useState(false);
  const [isDeleting, setIsDeleting] = useState(false);
  const [isLoadingKey, setIsLoadingKey] = useState(false);

  const formDismiss = useOverlayDismiss(() => setShowForm(false), !isSaving);
  const deleteDismiss = useOverlayDismiss(() => setDeleteTarget(null), !isDeleting);

  const [formName, setFormName] = useState("");
  const [formType, setFormType] = useState<ProviderType>("anthropic");
  const [formBaseUrl, setFormBaseUrl] = useState("");
  const [formApiKey, setFormApiKey] = useState("");
  const [formDefaultModel, setFormDefaultModel] = useState("");
  const [formOpenaiDefaultModel, setFormOpenaiDefaultModel] = useState("");
  const [formTierModels, setFormTierModels] = useState<Record<string, string>>({});
  const [formNotes, setFormNotes] = useState("");
  const [editingHasKey, setEditingHasKey] = useState(false);
  const [formApiKeyVisible, setFormApiKeyVisible] = useState(false);
  // 远端模型列表：拉取成功后默认模型与档位模型输入框切换为下拉选择
  const [remoteModels, setRemoteModels] = useState<string[]>([]);
  const [modelsLoading, setModelsLoading] = useState(false);

  const idCounter = useRef(0);
  const nameInputRef = useRef<HTMLInputElement>(null);
  const hasLoaded = useRef(false);

  const nextId = useCallback(() => {
    idCounter.current += 1;
    return `provider-${Date.now()}-${idCounter.current}`;
  }, []);

  const loadProviders = useCallback(async () => {
    try {
      const rows = await invokeList();
      setProviders(rows);
    } catch (err) {
      setStatusMsg(`加载 AI 供应商失败：${err instanceof Error ? err.message : String(err)}`);
      setProviders([]);
    } finally {
      setLoaded(true);
    }
  }, []);

  useEffect(() => {
    if (hasLoaded.current) return;
    hasLoaded.current = true;
    void loadProviders();
  }, [loadProviders]);

  // 搜索防抖（与 Skills 管理页一致：250ms，清空立即生效）
  useEffect(() => {
    const query = searchInput.trim().toLocaleLowerCase();
    if (!query) {
      setSearchQuery("");
      return;
    }
    const timer = window.setTimeout(() => setSearchQuery(query), 250);
    return () => window.clearTimeout(timer);
  }, [searchInput]);

  const anthropicCount = useMemo(
    () => providers.filter((p) => p.providerType === "anthropic").length,
    [providers],
  );
  const openaiCount = useMemo(
    () => providers.filter((p) => p.providerType === "openai").length,
    [providers],
  );
  const universalCount = providers.length - anthropicCount - openaiCount;

  // 类型与关键词为「与」筛选；关键词覆盖名称、Base URL、模型与备注
  const filteredProviders = useMemo(
    () =>
      providers.filter((p) => {
        if (typeFilter !== "all" && p.providerType !== typeFilter) return false;
        if (!searchQuery) return true;
        return [
          p.name,
          p.baseUrl,
          p.openaiBaseUrl,
          p.defaultModel,
          p.openaiDefaultModel,
          p.notes,
          ...Object.values(p.models),
        ].some((value) => value.toLocaleLowerCase().includes(searchQuery));
      }),
    [providers, typeFilter, searchQuery],
  );
  const isFiltering = typeFilter !== "all" || searchQuery !== "";

  // 通用类型：OpenAI Base URL 由 Anthropic Base URL 自动派生（追加 /v1）
  const derivedOpenaiBaseUrl = (() => {
    const base = formBaseUrl.trim().replace(/\/+$/, "");
    return formType === "universal" && base ? `${base}/v1` : "";
  })();

  useEffect(() => {
    if (showForm) {
      setTimeout(() => nameInputRef.current?.focus(), 100);
    }
  }, [showForm]);

  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        if (!isSaving) setShowForm(false);
        if (!isDeleting) setDeleteTarget(null);
      }
    };
    document.addEventListener("keydown", handler);
    return () => document.removeEventListener("keydown", handler);
  }, [isSaving, isDeleting]);

  const resetForm = useCallback(() => {
    setFormName("");
    setFormType("anthropic");
    setFormBaseUrl("");
    setFormApiKey("");
    setFormDefaultModel("");
    setFormOpenaiDefaultModel("");
    setFormTierModels({});
    setFormNotes("");
    setEditingHasKey(false);
    setFormApiKeyVisible(false);
    setRemoteModels([]);
    setFormError("");
  }, []);

  const openAdd = useCallback(() => {
    setEditingId(null);
    resetForm();
    setShowForm(true);
  }, [resetForm]);

  const openEdit = useCallback((p: AiProvider) => {
    setEditingId(p.id);
    setFormName(p.name);
    setFormType(p.providerType);
    setFormBaseUrl(p.baseUrl);
    setFormApiKey("");
    setFormDefaultModel(p.defaultModel);
    setFormOpenaiDefaultModel(p.openaiDefaultModel);
    setFormTierModels({ ...p.models });
    setFormNotes(p.notes);
    setEditingHasKey(p.hasApiKey);
    setFormApiKeyVisible(false);
    setRemoteModels([]);
    setFormError("");
    setShowForm(true);
  }, []);

  // 复制：以新建弹窗打开并回填原供应商数据；API Key 经 get_ai_provider_secret 取回一并回填
  const openClone = useCallback((p: AiProvider) => {
    setEditingId(null);
    setFormName(`${p.name} 副本`);
    setFormType(p.providerType);
    setFormBaseUrl(p.baseUrl);
    setFormApiKey("");
    setFormDefaultModel(p.defaultModel);
    setFormOpenaiDefaultModel(p.openaiDefaultModel);
    setFormTierModels({ ...p.models });
    setFormNotes(p.notes);
    setEditingHasKey(false);
    setFormApiKeyVisible(false);
    setRemoteModels([]);
    setFormError("");
    setShowForm(true);
    if (p.hasApiKey) {
      setIsLoadingKey(true);
      invokeGetSecret(p.id)
        .then((secret) => setFormApiKey(secret))
        .catch(() => setFormError("原供应商密钥读取失败，请手动填写 API Key"))
        .finally(() => setIsLoadingKey(false));
    }
  }, []);

  // 拉取远端模型列表：优先用表单中的 Key；编辑时表单留空则取已保存的密钥
  const fetchModels = useCallback(async () => {
    const baseUrl = formBaseUrl.trim();
    if (!baseUrl) {
      setStatusMsg("请先填写 Base URL");
      return;
    }
    setModelsLoading(true);
    try {
      let apiKey = formApiKey.trim();
      if (!apiKey && editingId && editingHasKey) {
        apiKey = (await invokeGetSecret(editingId)).trim();
      }
      const models = await invokeFetchRemoteModels(baseUrl, apiKey || undefined);
      if (models.length === 0) {
        setStatusMsg("远端未返回可用模型，仍可手动输入");
        return;
      }
      setRemoteModels(models);
      // 已有值时保留（编辑场景不被覆盖），仅在为空时预填第一个
      if (!formDefaultModel.trim()) {
        setFormDefaultModel(models[0]);
      }
      if (formType === "universal" && !formOpenaiDefaultModel.trim()) {
        setFormOpenaiDefaultModel(models[0]);
      }
      setStatusMsg(`已拉取 ${models.length} 个远端模型`);
    } catch (err) {
      setStatusMsg(`拉取模型失败：${err instanceof Error ? err.message : String(err)}`);
    } finally {
      setModelsLoading(false);
    }
  }, [formBaseUrl, formApiKey, formDefaultModel, formOpenaiDefaultModel, formType, editingId, editingHasKey, setStatusMsg]);

  const handleLoadSecret = useCallback(async () => {
    if (!editingId) return;
    setIsLoadingKey(true);
    setFormError("");
    try {
      const secret = await invokeGetSecret(editingId);
      setFormApiKey(secret);
    } catch (err) {
      setFormError(`读取密钥失败：${err instanceof Error ? err.message : String(err)}`);
    } finally {
      setIsLoadingKey(false);
    }
  }, [editingId]);

  const handleSave = useCallback(async () => {
    const name = formName.trim();
    const baseUrl = formBaseUrl.trim();
    const apiKey = formApiKey.trim();
    const defaultModel = formDefaultModel.trim();

    if (!name || !baseUrl) {
      setFormError("请填写名称和 Base URL");
      return;
    }
    if (!(baseUrl.startsWith("http://") || baseUrl.startsWith("https://"))) {
      setFormError("Base URL 必须以 http:// 或 https:// 开头");
      return;
    }
    if (!editingId && !apiKey) {
      setFormError("新建供应商时 API Key 不能为空");
      return;
    }

    setIsSaving(true);
    setFormError("");
    try {
      const id = editingId ?? nextId();
      // 仅 Anthropic 类型提交档位模型；过滤空值
      const models: Record<string, string> = {};
      if (formType !== "openai") {
        for (const tier of MODEL_TIERS) {
          const v = (formTierModels[tier.key] ?? "").trim();
          if (v) models[tier.key] = v;
        }
      }

      const result = await invokeUpsert({
        id,
        name,
        providerType: formType,
        baseUrl,
        ...(apiKey ? { apiKey } : {}),
        defaultModel,
        ...(formType === "universal"
          ? { openaiDefaultModel: formOpenaiDefaultModel.trim() }
          : {}),
        models,
        notes: formNotes.trim(),
      });
      const saved = result.provider;
      if (saved) {
        setProviders((prev) => {
          const exists = prev.some((p) => p.id === saved.id);
          if (exists) {
            return prev.map((p) => (p.id === saved.id ? saved : p));
          }
          return [...prev, saved];
        });
      }
      setShowForm(false);
      setStatusMsg(result.message || (editingId ? "供应商已更新" : "供应商已创建"));
    } catch (err) {
      setFormError(err instanceof Error ? err.message : String(err));
    } finally {
      setIsSaving(false);
    }
  }, [formName, formType, formBaseUrl, formApiKey, formDefaultModel, formOpenaiDefaultModel, formTierModels, formNotes, editingId, nextId]);

  const handleDelete = useCallback(async () => {
    if (deleteTarget === null) return;
    setIsDeleting(true);
    try {
      await invokeDelete(deleteTarget);
      setProviders((prev) => prev.filter((p) => p.id !== deleteTarget));
      setDeleteTarget(null);
      setStatusMsg("供应商已删除");
    } catch (err) {
      setStatusMsg(`删除失败：${err instanceof Error ? err.message : String(err)}`);
    } finally {
      setIsDeleting(false);
    }
  }, [deleteTarget]);

  const deleteName = providers.find((p) => p.id === deleteTarget)?.name;

  return (
    <>
      <div className="content-header">
        <div className="content-header-bar">
          <h1 className="content-title">AI供应商</h1>
          <div className="header-actions">
            <button className="action-btn" data-tooltip="添加 AI 供应商" onClick={openAdd}>
              <IconPlus />
            </button>
          </div>
        </div>
      </div>
      <div className="content-body">
        <Toast message={statusMsg} />

        {loaded && providers.length > 0 && (
          <div className="mcp-summary">
            共 <strong>{providers.length}</strong> 个供应商
            {isFiltering && (
              <>
                ，筛选出 <strong>{filteredProviders.length}</strong> 个
              </>
            )}
          </div>
        )}

        {loaded && providers.length > 0 && (
          <>
            <div className="skill-search">
              <IconSearch />
              <input
                type="search"
                value={searchInput}
                onChange={(event) => {
                  const value = event.target.value;
                  setSearchInput(value);
                  if (!value.trim()) setSearchQuery("");
                }}
                placeholder="搜索名称、Base URL、模型或备注"
                aria-label="搜索供应商"
              />
              {searchInput && (
                <button
                  type="button"
                  className="skill-search-clear"
                  data-tooltip="清空搜索"
                  aria-label="清空搜索"
                  onClick={() => {
                    setSearchInput("");
                    setSearchQuery("");
                  }}
                >
                  <IconClose />
                </button>
              )}
            </div>
            <div className="skill-source-filter" role="group" aria-label="按类型筛选" style={{ marginBottom: 14 }}>
              {(
                [
                  { key: "all", label: "全部", count: providers.length, inheritFont: true },
                  { key: "anthropic", label: "Anthropic", count: anthropicCount, inheritFont: false },
                  { key: "openai", label: "OpenAI", count: openaiCount, inheritFont: false },
                  { key: "universal", label: "通用", count: universalCount, inheritFont: true },
                ] as const
              ).map((opt) => (
                <button
                  key={opt.key}
                  type="button"
                  className={`skill-source-chip ${opt.inheritFont ? "skill-source-chip-all" : ""} ${typeFilter === opt.key ? "active" : ""}`}
                  aria-pressed={typeFilter === opt.key}
                  onClick={() => setTypeFilter(opt.key)}
                >
                  <span className="skill-source-chip-label">{opt.label}</span>
                  <span className="skill-source-chip-count">{opt.count}</span>
                </button>
              ))}
            </div>
          </>
        )}

        {!loaded ? null : providers.length === 0 ? (
          <div className="empty-state">
            <IconEmpty />
            <div className="empty-state-text">暂无 AI 供应商，点击右上角添加</div>
          </div>
        ) : filteredProviders.length === 0 ? (
          <div className="empty-state skill-filter-empty">
            <IconSearch />
            <div className="empty-state-text">没有匹配的供应商</div>
          </div>
        ) : (
          <div className="ai-provider-list">
            {filteredProviders.map((p) => {
              const tierCount = Object.keys(p.models).length;
              return (
                <div key={p.id} className="ai-provider-item">
                  <div className="ai-provider-info">
                    <div className="ai-provider-name">
                      <TypeBadge type={p.providerType} />
                      {p.name}
                    </div>
                    <div className="ai-provider-detail">
                      {p.providerType === "universal" ? `Anthropic: ${p.baseUrl}` : p.baseUrl}
                    </div>
                    {p.providerType === "universal" && p.openaiBaseUrl ? (
                      <div className="ai-provider-detail">OpenAI: {p.openaiBaseUrl}</div>
                    ) : null}
                    <div className="ai-provider-detail">
                      {p.providerType === "universal"
                        ? `OpenAI 默认: ${p.openaiDefaultModel || "未设置"} · Anthropic 默认: ${p.defaultModel || "未设置"}`
                        : `默认模型: ${p.defaultModel || "未设置"}`}
                      {p.providerType !== "openai" && tierCount > 0
                        ? ` · 档位模型 ${tierCount} 个`
                        : ""}
                      {p.hasApiKey ? " · 已配置 Key" : " · 未配置 Key"}
                    </div>
                    {p.notes ? (
                      <div className="ai-provider-detail">{p.notes}</div>
                    ) : null}
                  </div>
                  <button
                    className="btn-delete"
                    onClick={() => openClone(p)}
                    data-tooltip="复制供应商"
                    style={{ opacity: 1 }}
                  >
                    <IconCopy />
                  </button>
                  <button
                    className="btn-delete"
                    onClick={() => openEdit(p)}
                    data-tooltip="编辑供应商"
                    style={{ opacity: 1 }}
                  >
                    <IconEdit />
                  </button>
                  <button
                    className="btn-delete"
                    onClick={() => setDeleteTarget(p.id)}
                    data-tooltip="删除供应商"
                    style={{ opacity: 1 }}
                  >
                    <IconTrash />
                  </button>
                </div>
              );
            })}
          </div>
        )}
      </div>

      {/* ===== Add / Edit Modal ===== */}
      <div className={`modal-overlay ${showForm ? "visible" : ""}`} {...formDismiss}>
        <div className="modal ai-provider-modal">
          <div className="modal-header">
            <h2 className="modal-title">
              {editingId ? "编辑 AI 供应商" : "添加 AI 供应商"}
            </h2>
            <button
              className="modal-close"
              onClick={() => !isSaving && setShowForm(false)}
              disabled={isSaving}
            >
              <IconClose />
            </button>
          </div>
          <div className="modal-body ai-provider-modal-body">
            <div className="form-group">
              <label className="form-label" htmlFor="ai-provider-name">名称</label>
              <input
                ref={nameInputRef}
                type="text"
                className="form-input"
                id="ai-provider-name"
                placeholder="例如：官方 Anthropic / 自建网关"
                value={formName}
                onChange={(e) => setFormName(e.target.value)}
                disabled={isSaving}
              />
            </div>
            <div className="form-group">
              <label className="form-label" id="ai-provider-type-label">类型</label>
              <TypeSelect value={formType} onChange={setFormType} disabled={isSaving} />
            </div>
            <div className="form-group">
              <label className="form-label" htmlFor="ai-provider-base-url">
                {formType === "universal" ? "Anthropic Base URL" : "Base URL"}
              </label>
              <input
                type="url"
                className="form-input"
                id="ai-provider-base-url"
                placeholder={
                  formType === "openai"
                    ? "https://api.openai.com/v1"
                    : "https://api.anthropic.com"
                }
                value={formBaseUrl}
                onChange={(e) => {
                  setFormBaseUrl(e.target.value);
                  setRemoteModels([]);
                }}
                disabled={isSaving}
              />
              {formType === "universal" && (
                <div style={{ marginTop: 8 }}>
                  <div className="ai-provider-tier-label">OpenAI Base URL（自动派生）</div>
                  <input
                    type="text"
                    className="form-input"
                    aria-label="OpenAI Base URL（自动派生）"
                    value={derivedOpenaiBaseUrl}
                    placeholder="填写 Anthropic Base URL 后自动生成"
                    disabled
                    readOnly
                  />
                </div>
              )}
            </div>
            <div className="form-group">
              <label className="form-label" htmlFor="ai-provider-api-key">API Key</label>
              <div style={{ display: "flex", gap: 8, alignItems: "center" }}>
                <div className="form-input-with-action">
                  <input
                    type={formApiKeyVisible ? "text" : "password"}
                    className="form-input"
                    id="ai-provider-api-key"
                    placeholder={
                      editingId
                        ? editingHasKey
                          ? "留空则保持原密钥"
                          : "输入 API Key"
                        : "输入 API Key"
                    }
                    value={formApiKey}
                    onChange={(e) => setFormApiKey(e.target.value)}
                    disabled={isSaving || isLoadingKey}
                    autoComplete="new-password"
                    spellCheck={false}
                  />
                  <button
                    type="button"
                    className="form-input-action"
                    data-tooltip={formApiKeyVisible ? "隐藏 API Key" : "显示 API Key"}
                    aria-label={formApiKeyVisible ? "隐藏 API Key" : "显示 API Key"}
                    aria-pressed={formApiKeyVisible}
                    onClick={() => setFormApiKeyVisible((v) => !v)}
                    disabled={isSaving || isLoadingKey}
                    tabIndex={0}
                  >
                    {formApiKeyVisible ? <IconEyeOff /> : <IconEye />}
                  </button>
                </div>
                {editingId && editingHasKey ? (
                  <button
                    type="button"
                    className="btn btn-secondary"
                    onClick={() => void handleLoadSecret()}
                    disabled={isSaving || isLoadingKey}
                    data-tooltip="载入已保存的密钥以查看或修改"
                  >
                    <IconKey />
                    {isLoadingKey ? "载入中…" : "载入密钥"}
                  </button>
                ) : null}
              </div>
            </div>
            {formType === "universal" && (
              <div className="form-group">
                <div className="claude-env-model-label-row">
                  <label className="form-label" htmlFor="ai-provider-openai-default-model">
                    OpenAI 默认模型 <span className="form-label-optional">可选</span>
                  </label>
                  <button
                    type="button"
                    className="claude-env-fetch-models-btn"
                    data-tooltip="从当前 Base URL 拉取模型列表"
                    onClick={() => void fetchModels()}
                    disabled={isSaving || modelsLoading}
                  >
                    <IconDownload />
                    {modelsLoading ? "拉取中…" : "拉取列表"}
                  </button>
                </div>
                <ModelComboBox
                  id="ai-provider-openai-default-model"
                  value={formOpenaiDefaultModel}
                  onChange={setFormOpenaiDefaultModel}
                  disabled={isSaving}
                  options={remoteModels}
                  placeholder="gpt-5"
                />
              </div>
            )}
            <div className="form-group">
              <div className="claude-env-model-label-row">
                <label className="form-label" htmlFor="ai-provider-default-model">
                  {formType === "universal" ? "Anthropic 默认模型" : "默认模型"}{" "}
                  <span className="form-label-optional">可选</span>
                </label>
                {formType !== "universal" && (
                  <button
                    type="button"
                    className="claude-env-fetch-models-btn"
                    data-tooltip="从当前 Base URL 拉取模型列表"
                    onClick={() => void fetchModels()}
                    disabled={isSaving || modelsLoading}
                  >
                    <IconDownload />
                    {modelsLoading ? "拉取中…" : "拉取列表"}
                  </button>
                )}
              </div>
              <ModelComboBox
                id="ai-provider-default-model"
                value={formDefaultModel}
                onChange={setFormDefaultModel}
                disabled={isSaving}
                options={remoteModels}
                placeholder={formType === "openai" ? "gpt-5" : "claude-sonnet-4-5"}
              />
            </div>
            {formType !== "openai" ? (
              <div className="form-group">
                <label className="form-label">
                  档位模型 <span className="form-label-optional">可选</span>
                </label>
                {MODEL_TIERS.map((tier) => (
                  <div key={tier.key} style={{ marginBottom: 8 }}>
                    <div className="ai-provider-tier-label">{tier.label}</div>
                    <ModelComboBox
                      value={formTierModels[tier.key] ?? ""}
                      onChange={(v) =>
                        setFormTierModels((prev) => ({ ...prev, [tier.key]: v }))
                      }
                      disabled={isSaving}
                      options={remoteModels}
                      placeholder="留空跟随默认模型"
                    />
                  </div>
                ))}
              </div>
            ) : null}
            <div className="form-group">
              <label className="form-label" htmlFor="ai-provider-notes">
                备注 <span className="form-label-optional">可选</span>
              </label>
              <input
                type="text"
                className="form-input"
                id="ai-provider-notes"
                placeholder="备注信息"
                value={formNotes}
                onChange={(e) => setFormNotes(e.target.value)}
                disabled={isSaving}
              />
            </div>
            {formError && <div className="mcp-form-error">{formError}</div>}
          </div>
          <div className="modal-footer">
            <button
              className="btn btn-secondary"
              onClick={() => setShowForm(false)}
              disabled={isSaving}
            >
              取消
            </button>
            <button
              className="btn btn-primary"
              onClick={() => void handleSave()}
              disabled={isSaving || isLoadingKey}
            >
              {isSaving ? "保存中…" : "保存"}
            </button>
          </div>
        </div>
      </div>

      {/* ===== Delete Confirm Modal ===== */}
      <div
        className={`modal-overlay ${deleteTarget !== null ? "visible" : ""}`}
        {...deleteDismiss}
      >
        <div className="modal" style={{ width: 380 }}>
          <div className="modal-header">
            <h2 className="modal-title">确认删除</h2>
            <button
              className="modal-close"
              onClick={() => !isDeleting && setDeleteTarget(null)}
              disabled={isDeleting}
            >
              <IconClose />
            </button>
          </div>
          <div className="confirm-body">
            <div className="confirm-icon">
              <IconTrashConfirm />
            </div>
            <div className="confirm-text">
              确定要删除{deleteName ? `「${deleteName}」` : "此供应商"}吗？
            </div>
            <div className="confirm-subtext">
              删除后其 API Key 将一并清除，无法恢复。
            </div>
          </div>
          <div className="modal-footer">
            <button
              className="btn btn-secondary"
              onClick={() => setDeleteTarget(null)}
              disabled={isDeleting}
            >
              取消
            </button>
            <button
              className="btn btn-danger"
              onClick={() => void handleDelete()}
              disabled={isDeleting}
            >
              {isDeleting ? "删除中…" : "删除"}
            </button>
          </div>
        </div>
      </div>
    </>
  );
}
