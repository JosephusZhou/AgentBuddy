import { useState, useEffect, useCallback, useMemo, useRef } from "react";
import { useStatusMessage } from "@/lib/useStatusMessage";
import { Toast } from "@/components/Toast";
import { ModelComboBox } from "../ModelComboBox";
import { useOverlayDismiss } from "../ui";
import { Copy, Pencil, Plus, Trash2, X, Boxes, Download, Eye, EyeOff, Search, GripVertical, CheckSquare, Square } from "lucide-react";
import {
  invokeList,
  invokeUpsert,
  invokeDelete,
  invokeGetSecrets,
  invokeFetchRemoteModels,
  invokeReorder,
} from "./ai-providers/api";
import {
  MODEL_TIERS,
  PROVIDER_TYPE_OPTIONS,
  type AiProvider,
  type ProviderType,
  type CustomModel,
} from "./ai-providers/types";

/* ===== Icons ===== */
const IconPlus = () => <Plus size={16} strokeWidth={2} />;
const IconTrash = () => <Trash2 size={16} strokeWidth={1.8} />;
const IconClose = () => <X size={16} strokeWidth={2} />;
const IconTrashConfirm = () => <Trash2 size={20} strokeWidth={2} />;
const IconEdit = () => <Pencil size={16} strokeWidth={1.8} />;
const IconCopy = () => <Copy size={16} strokeWidth={1.8} />;
const IconCopyModel = () => <Copy size={13} strokeWidth={1.8} />;
const IconEmpty = () => <Boxes size={40} strokeWidth={1.5} />;
const IconDownload = () => <Download size={14} strokeWidth={1.8} />;
const IconSearch = () => <Search size={16} strokeWidth={1.8} />;
const IconEye = () => <Eye size={16} strokeWidth={1.8} />;
const IconEyeOff = () => <EyeOff size={16} strokeWidth={1.8} />;
const IconGrip = () => <GripVertical size={16} strokeWidth={1.8} />;
const IconCheckSquare = () => <CheckSquare size={16} strokeWidth={1.8} />;
const IconSquare = () => <Square size={16} strokeWidth={1.8} />;
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

/* ModelComboBox 是唯一的模型输入控件；下拉候选直接来自 `formCustomModels`。
   自定义模型列表的录入方式有两种：
   1. 下方"手动添加"输入框逐条录入
   2. "从端点拉取"按钮 → 一次性拉取 + 多选 → 应用后批量写入
   两种方式最终都落到 formCustomModels 并由 invokeUpsert 写入 DB。运行时
   （Claude / Codex / 路由聚合）永远只读 customModels，不再向供应商端点发请求。 */

/** 类型徽标：Anthropic / OpenAI / 通用 用不同色调区分。 */
function TypeBadge({ type }: { type: ProviderType }) {
  let label: string;
  if (type === "anthropic") label = "Anthropic";
  else if (type === "openai") label = "OpenAI";
  else label = "通用";

  const accentStyle =
    type === "openai"
      ? undefined
      : {
          background: "color-mix(in srgb, var(--seed-primary) 12%, transparent)",
          color: "var(--seed-active-fg)",
          borderColor: "color-mix(in srgb, var(--seed-primary) 25%, transparent)",
        };

  return (
    <span className="ai-provider-badge" style={accentStyle}>
      {label}
    </span>
  );
}

/** 类型下拉：选项仅三个（Anthropic / OpenAI / 通用），复用 app-select 既有样式。 */
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

  const formDismiss = useOverlayDismiss(() => setShowForm(false), !isSaving);
  const deleteDismiss = useOverlayDismiss(() => setDeleteTarget(null), !isDeleting);

  const [formName, setFormName] = useState("");
  const [formType, setFormType] = useState<ProviderType>("anthropic");
  const [formBaseUrl, setFormBaseUrl] = useState("");
  /** 多 API Key 列表（编辑时自动载入）。 */
  const [formApiKeys, setFormApiKeys] = useState<string[]>([""]);
  const [formApiKeyVisibles, setFormApiKeyVisibles] = useState<Record<number, boolean>>({});
  const [formDefaultModel, setFormDefaultModel] = useState("");
  const [formOpenaiDefaultModel, setFormOpenaiDefaultModel] = useState("");
  const [formTierModels, setFormTierModels] = useState<Record<string, string>>({});
  const [formNotes, setFormNotes] = useState("");
  const [editingHasKey, setEditingHasKey] = useState(false);
  /** 自定义模型列表（运行时的唯一来源，由编辑页通过"手动添加"或"从端点拉取多选"录入）。 */
  const [formCustomModels, setFormCustomModels] = useState<CustomModel[]>([]);
  /** 模型选择面板是否展开（编辑页"从端点拉取"后的多选 UI）。 */
  const [showModelPicker, setShowModelPicker] = useState(false);
  /** 从端点拉取的原始模型列表（用于选择面板的候选集）。 */
  const [pickerModels, setPickerModels] = useState<string[]>([]);
  /** 选择面板中已勾选的模型集合。 */
  const [pickerSelected, setPickerSelected] = useState<Set<string>>(new Set());
  const [pickerLoading, setPickerLoading] = useState(false);
  /** 手动添加模型的输入值（除"从端点拉取"外的另一种录入方式）。 */
  const [manualModelInput, setManualModelInput] = useState("");

  const idCounter = useRef(0);
  const nameInputRef = useRef<HTMLInputElement>(null);
  const hasLoaded = useRef(false);

  // 拖拽排序状态（基于 Pointer Events）
  const [dragIndex, setDragIndex] = useState<number | null>(null);
  const [dragOverIndex, setDragOverIndex] = useState<number | null>(null);
  const [dragDeltaY, setDragDeltaY] = useState(0);
  const listRef = useRef<HTMLDivElement>(null);
  // 拖拽开始时的布局快照：命中检测只依赖快照，绝不读取已被 transform 变换的实时布局，从而消除闪烁/错位
  const dragSnap = useRef<{
    index: number;
    pointerId: number;
    startY: number;
    targetIndex: number;
    centers: number[];
    height: number;
    captureEl: HTMLElement | null;
  } | null>(null);

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
  const universalCount = useMemo(
    () => providers.filter((p) => p.providerType === "universal").length,
    [providers],
  );

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
    setFormApiKeys([""]);
    setFormApiKeyVisibles({});
    setFormDefaultModel("");
    setFormOpenaiDefaultModel("");
    setFormTierModels({});
    setFormNotes("");
    setEditingHasKey(false);
    setFormCustomModels([]);
    setShowModelPicker(false);
    setPickerModels([]);
    setPickerSelected(new Set());
    setManualModelInput("");
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
    setFormApiKeys([""]);
    setFormApiKeyVisibles({});
    setFormDefaultModel(p.defaultModel);
    setFormOpenaiDefaultModel(p.openaiDefaultModel);
    setFormTierModels({ ...p.models });
    setFormNotes(p.notes);
    setEditingHasKey(p.hasApiKey);
    setFormCustomModels(p.customModels ? [...p.customModels] : []);
    setShowModelPicker(false);
    setPickerModels([]);
    setPickerSelected(new Set());
    setManualModelInput("");
    setFormError("");
    setShowForm(true);
    // 自动载入已保存的 API Key
    if (p.hasApiKey) {
      invokeGetSecrets(p.id)
        .then((secrets) => {
          if (secrets.length > 0) {
            setFormApiKeys(secrets);
          }
        })
        .catch(() => setFormError("读取密钥失败，请手动填写 API Key"));
    }
  }, []);

  // 复制：以新建弹窗打开并回填原供应商数据；API Key 自动载入
  const openClone = useCallback((p: AiProvider) => {
    setEditingId(null);
    setFormName(`${p.name} 副本`);
    setFormType(p.providerType);
    setFormBaseUrl(p.baseUrl);
    setFormApiKeys([""]);
    setFormApiKeyVisibles({});
    setFormDefaultModel(p.defaultModel);
    setFormOpenaiDefaultModel(p.openaiDefaultModel);
    setFormTierModels({ ...p.models });
    setFormNotes(p.notes);
    setEditingHasKey(false);
    setFormCustomModels(p.customModels ? [...p.customModels] : []);
    setShowModelPicker(false);
    setPickerModels([]);
    setPickerSelected(new Set());
    setManualModelInput("");
    setFormError("");
    setShowForm(true);
    if (p.hasApiKey) {
      invokeGetSecrets(p.id)
        .then((secrets) => {
          if (secrets.length > 0) {
            setFormApiKeys(secrets);
          }
        })
        .catch(() => setFormError("原供应商密钥读取失败，请手动填写 API Key"));
    }
  }, []);

  // 拉取远端模型列表（**编辑页填表工具**：一次性导入候选模型，让用户多选后批量
  // 写入 customModels；不影响"customModels 是运行时唯一来源"的规则——拉取的
  // 结果仅在选择面板里暂存，"应用"按钮才会把它落到 formCustomModels 并由
  // invokeUpsert 写入 DB。保存后再次打开表单依然只显示 customModels。）。
  const fetchModelsForPicker = useCallback(async () => {
    const baseUrl = formBaseUrl.trim();
    if (!baseUrl) {
      setStatusMsg("请先填写 Base URL");
      return;
    }
    setPickerLoading(true);
    try {
      // 优先使用表单中的第一个 Key；编辑时表单为空则取已保存的密钥
      let apiKey = formApiKeys[0]?.trim() || "";
      if (!apiKey && editingId && editingHasKey) {
        const secrets = await invokeGetSecrets(editingId);
        apiKey = secrets[0]?.trim() || "";
      }
      const models = await invokeFetchRemoteModels(baseUrl, apiKey || undefined);
      if (models.length === 0) {
        setStatusMsg("远端未返回可用模型");
        return;
      }
      setPickerModels(models);
      // 预选已存在于自定义列表中的模型
      const existingSet = new Set(formCustomModels.map((cm) => cm.model));
      setPickerSelected(existingSet);
      setShowModelPicker(true);
      setStatusMsg(`已拉取 ${models.length} 个远端模型`);
    } catch (err) {
      setStatusMsg(`拉取模型失败：${err instanceof Error ? err.message : String(err)}`);
    } finally {
      setPickerLoading(false);
    }
  }, [formBaseUrl, formApiKeys, editingId, editingHasKey, formCustomModels, setStatusMsg]);

  // 应用选择面板中的选择到自定义模型列表（保留已有别名）。
  const applyPickerSelection = useCallback(() => {
    const existingMap = new Map(formCustomModels.map((cm) => [cm.model, cm.aliasId]));
    const newModels: CustomModel[] = [];
    pickerSelected.forEach((model) => {
      newModels.push({
        model,
        aliasId: existingMap.get(model) || "",
      });
    });
    setFormCustomModels(newModels);
    setShowModelPicker(false);
  }, [pickerSelected, formCustomModels]);

  const addManualModel = useCallback(() => {
    const model = manualModelInput.trim();
    if (!model) return;
    if (formCustomModels.some((cm) => cm.model === model)) {
      setFormError(`模型「${model}」已在自定义列表中`);
      return;
    }
    setFormCustomModels((prev) => [...prev, { model, aliasId: "" }]);
    setManualModelInput("");
    setFormError("");
  }, [manualModelInput, formCustomModels]);

  // 复制自定义模型 ID 到系统剪贴板；toast 复用页面顶部 useStatusMessage。
  // 表单保存后再次打开表单依然只显示 customModels，因此复制按钮始终读 formCustomModels 当前快照。
  const copyModelId = useCallback(
    async (modelId: string) => {
      if (!modelId) return;
      try {
        await navigator.clipboard.writeText(modelId);
        setStatusMsg(`已复制模型 ID：${modelId}`);
      } catch {
        setStatusMsg("复制失败，请手动选择文本");
      }
    },
    [setStatusMsg],
  );

  // 自定义模型列表中的模型 ID 列表（用于档位模型下拉）
  const customModelOptions = useMemo(
    () =>
      formCustomModels.flatMap((cm) =>
        cm.aliasId ? [cm.aliasId, cm.model] : [cm.model],
      ),
    [formCustomModels],
  );

  const handleSave = useCallback(async () => {
    const name = formName.trim();
    const baseUrl = formBaseUrl.trim();
    // 过滤空 Key
    const apiKeys = formApiKeys.map((k) => k.trim()).filter((k) => k);
    const defaultModel = formDefaultModel.trim();

    if (!name || !baseUrl) {
      setFormError("请填写名称和 Base URL");
      return;
    }
    if (!(baseUrl.startsWith("http://") || baseUrl.startsWith("https://"))) {
      setFormError("Base URL 必须以 http:// 或 https:// 开头");
      return;
    }
    if (!editingId && apiKeys.length === 0) {
      setFormError("新建供应商时 API Key 不能为空");
      return;
    }

    setIsSaving(true);
    setFormError("");
    try {
      const id = editingId ?? nextId();
      // 仅 Anthropic / 通用类型提交档位模型；OpenAI 不支持档位覆盖
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
        ...(apiKeys.length > 0 ? { apiKeys } : {}),
        defaultModel,
        ...(formType === "universal"
          ? { openaiDefaultModel: formOpenaiDefaultModel.trim() }
          : {}),
        models,
        customModels: formCustomModels,
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
  }, [formName, formType, formBaseUrl, formApiKeys, formDefaultModel, formOpenaiDefaultModel, formTierModels, formCustomModels, formNotes, editingId, nextId]);

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

  // 拖拽排序：放下后重排并持久化
  const handleDrop = useCallback(
    async (fromIndex: number, toIndex: number) => {
      if (fromIndex === toIndex) return;
      const reordered = [...providers];
      const [moved] = reordered.splice(fromIndex, 1);
      reordered.splice(toIndex, 0, moved);
      setProviders(reordered);
      try {
        await invokeReorder(reordered.map((p) => p.id));
      } catch (err) {
        setStatusMsg(`排序保存失败：${err instanceof Error ? err.message : String(err)}`);
        void loadProviders();
      }
    },
    [providers, loadProviders, setStatusMsg],
  );

  // 根据指针 Y 与“原始布局快照”计算拖拽项的最终落点索引（结果永远在 [0, n-1]，稳定不抖动）
  const computeTargetIndex = useCallback(
    (centers: number[], dragIndex: number, pointerY: number): number => {
      let above = 0;
      for (let i = 0; i < centers.length; i++) {
        if (i === dragIndex) continue;
        if (centers[i] < pointerY) above++;
      }
      return Math.max(0, Math.min(centers.length - 1, above));
    },
    [],
  );

  const onGripPointerDown = useCallback(
    (index: number, e: React.PointerEvent) => {
      if (isFiltering) return;
      const container = listRef.current;
      if (!container) return;
      e.preventDefault();
      const items = Array.from(container.querySelectorAll<HTMLElement>(".ai-provider-item"));
      const rects = items.map((el) => el.getBoundingClientRect());
      const centers = rects.map((r) => r.top + r.height / 2);
      const gap = rects.length >= 2 ? rects[1].top - rects[0].bottom : 12;
      const captureEl = e.currentTarget as HTMLElement;
      captureEl.setPointerCapture(e.pointerId);

      dragSnap.current = {
        index,
        pointerId: e.pointerId,
        startY: e.clientY,
        targetIndex: index,
        centers,
        height: rects[index].height + gap,
        captureEl,
      };
      setDragIndex(index);
      setDragOverIndex(index);
      setDragDeltaY(0);
    },
    [isFiltering],
  );

  const onGripPointerMove = useCallback(
    (e: React.PointerEvent) => {
      const snap = dragSnap.current;
      if (!snap || snap.pointerId !== e.pointerId) return;
      const delta = e.clientY - snap.startY;
      snap.targetIndex = computeTargetIndex(snap.centers, snap.index, e.clientY);
      setDragDeltaY(delta);
      setDragOverIndex(snap.targetIndex);
    },
    [computeTargetIndex],
  );

  const onGripPointerUp = useCallback(
    (e: React.PointerEvent) => {
      const snap = dragSnap.current;
      if (!snap || snap.pointerId !== e.pointerId) return;
      try {
        snap.captureEl?.releasePointerCapture(e.pointerId);
      } catch {
        // 忽略释放失败
      }
      const from = snap.index;
      const to = snap.targetIndex;
      dragSnap.current = null;
      setDragIndex(null);
      setDragOverIndex(null);
      setDragDeltaY(0);
      if (to !== from) {
        void handleDrop(from, to);
      }
    },
    [handleDrop],
  );

  const onGripPointerCancel = useCallback((e: React.PointerEvent) => {
    const snap = dragSnap.current;
    if (!snap) return;
    try {
      snap.captureEl?.releasePointerCapture(e.pointerId);
    } catch {
      // 忽略释放失败
    }
    dragSnap.current = null;
    setDragIndex(null);
    setDragOverIndex(null);
    setDragDeltaY(0);
  }, []);

  // 计算每个 item 在拖拽过程中的 translateY 偏移（基于快照，落点稳定）
  const getItemTransform = useCallback(
    (index: number): string => {
      const snap = dragSnap.current;
      if (dragIndex === null || snap === null) return "";
      const target = snap.targetIndex;
      if (index === dragIndex) return `translateY(${dragDeltaY}px)`;
      // 向下拖：被拖项下方、目标位置之间的 item 上移一个“项高+间距”，让出空位
      if (target > dragIndex && index > dragIndex && index <= target) {
        return `translateY(${-snap.height}px)`;
      }
      // 向上拖：目标位置到被拖项之间的 item 下移一个“项高+间距”
      if (target < dragIndex && index >= target && index < dragIndex) {
        return `translateY(${snap.height}px)`;
      }
      return "";
    },
    [dragIndex, dragDeltaY],
  );

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
              ).map((opt) => {
                return (
                  <button
                    key={opt.key}
                    type="button"
                    className={`skill-source-chip ${opt.inheritFont ? "skill-source-chip-all" : ""} ${typeFilter === opt.key ? "active" : ""}`}
                    aria-pressed={typeFilter === opt.key}
                    onClick={() => {
                      setTypeFilter(opt.key);
                    }}
                  >
                    <span className="skill-source-chip-label">{opt.label}</span>
                    <span className="skill-source-chip-count">{opt.count}</span>
                  </button>
                );
              })}
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
          <div className="ai-provider-list" ref={listRef}>
            {filteredProviders.map((p, index) => {
              const tierCount = Object.keys(p.models).length;
              const canDrag = !isFiltering;
              return (
                <div
                  key={p.id}
                  className={`ai-provider-item ${dragIndex === index ? "dragging" : ""} ${dragOverIndex === index && dragIndex !== index ? "drag-over" : ""}`}
                  style={{
                    transform: getItemTransform(index) || undefined,
                    transition: dragIndex === index ? "none" : "transform 0.15s ease",
                    zIndex: dragIndex === index ? 10 : undefined,
                    position: "relative",
                  }}
                >
                  {canDrag && (
                    <span
                      className="ai-provider-grip"
                      aria-hidden
                      onPointerDown={(e) => onGripPointerDown(index, e)}
                      onPointerMove={onGripPointerMove}
                      onPointerUp={onGripPointerUp}
                      onPointerCancel={onGripPointerCancel}
                    >
                      <IconGrip />
                    </span>
                  )}
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
              <label className="form-label">API Key</label>
              {formApiKeys.map((key, index) => (
                <div key={index} style={{ display: "flex", gap: 8, alignItems: "center", marginBottom: index < formApiKeys.length - 1 ? 8 : 0 }}>
                  <div className="form-input-with-action" style={{ flex: 1 }}>
                    <input
                      type={formApiKeyVisibles[index] ? "text" : "password"}
                      className="form-input"
                      placeholder={
                        editingId && editingHasKey && !key
                          ? "已载入密钥"
                          : "输入 API Key"
                      }
                      value={key}
                      onChange={(e) => {
                        const next = [...formApiKeys];
                        next[index] = e.target.value;
                        setFormApiKeys(next);
                      }}
                      disabled={isSaving}
                      autoComplete="new-password"
                      spellCheck={false}
                    />
                    <button
                      type="button"
                      className="form-input-action"
                      data-tooltip={formApiKeyVisibles[index] ? "隐藏" : "显示"}
                      onClick={() =>
                        setFormApiKeyVisibles((prev) => ({ ...prev, [index]: !prev[index] }))
                      }
                      disabled={isSaving}
                      tabIndex={0}
                    >
                      {formApiKeyVisibles[index] ? <IconEyeOff /> : <IconEye />}
                    </button>
                  </div>
                  {index > 0 && (
                    <button
                      type="button"
                      className="btn-delete"
                      data-tooltip="删除此 Key"
                      onClick={() => {
                        setFormApiKeys((prev) => prev.filter((_, i) => i !== index));
                      }}
                      disabled={isSaving}
                      style={{ opacity: 1 }}
                    >
                      <IconTrash />
                    </button>
                  )}
                </div>
              ))}
              <button
                type="button"
                className="btn btn-secondary"
                style={{ marginTop: 8 }}
                onClick={() => setFormApiKeys((prev) => [...prev, ""])}
                disabled={isSaving}
              >
                <IconPlus />
                添加 API Key
              </button>
            </div>
            {/* 自定义模型列表区域 */}
            <div className="form-group">
              <div className="claude-env-model-label-row">
                <label className="form-label">自定义模型列表</label>
                <button
                  type="button"
                  className="claude-env-fetch-models-btn"
                  data-tooltip="从当前 Base URL 拉取候选模型并多选"
                  onClick={() => void fetchModelsForPicker()}
                  disabled={isSaving || pickerLoading}
                >
                  <IconDownload />
                  {pickerLoading ? "拉取中…" : "从端点拉取"}
                </button>
              </div>
              {/* 远程拉取的模型选择面板（"从端点拉取"后展开的多选 UI，放在标题与已有列表之间） */}
              {showModelPicker && (
                <div className="ai-provider-model-picker">
                  <div className="ai-provider-model-picker-header">
                    <span className="form-label">选择模型</span>
                    <button
                      type="button"
                      className="modal-close"
                      onClick={() => {
                        setShowModelPicker(false);
                        setPickerModels([]);
                        setPickerSelected(new Set());
                      }}
                    >
                      <IconClose />
                    </button>
                  </div>
                  <div className="ai-provider-model-picker-list">
                    {pickerModels.map((model) => {
                      const checked = pickerSelected.has(model);
                      const toggle = () => {
                        setPickerSelected((prev) => {
                          const next = new Set(prev);
                          if (next.has(model)) {
                            next.delete(model);
                          } else {
                            next.add(model);
                          }
                          return next;
                        });
                      };
                      return (
                        <div
                          key={model}
                          className="ai-provider-model-picker-item"
                          role="checkbox"
                          aria-checked={checked}
                          tabIndex={0}
                          onClick={toggle}
                          onKeyDown={(e) => {
                            if (e.key === "Enter" || e.key === " ") {
                              e.preventDefault();
                              toggle();
                            }
                          }}
                        >
                          <span className="ai-provider-model-picker-checkbox">
                            {checked ? <IconCheckSquare /> : <IconSquare />}
                          </span>
                          <span>{model}</span>
                        </div>
                      );
                    })}
                  </div>
                  <div className="ai-provider-model-picker-footer">
                    <span className="ai-provider-model-picker-count">
                      已选 {pickerSelected.size} / {pickerModels.length}
                    </span>
                    <button
                      type="button"
                      className="btn btn-primary"
                      onClick={applyPickerSelection}
                      disabled={pickerSelected.size === 0}
                    >
                      应用
                    </button>
                  </div>
                </div>
              )}
              {formCustomModels.length === 0 ? (
                <div className="ai-provider-custom-models-empty">
                  暂无自定义模型，可点击"从端点拉取"获取供应商模型列表并多选，或在下方手动添加
                </div>
              ) : (
                <div className="ai-provider-custom-models-list">
                  {formCustomModels.map((cm, index) => (
                    <div key={cm.model} className="ai-provider-custom-model-item">
                      <div className="ai-provider-custom-model-info">
                        <div className="ai-provider-custom-model-name-row">
                          <span className="ai-provider-custom-model-name">{cm.model}</span>
                          <button
                            type="button"
                            className="btn-icon-action ai-provider-custom-model-copy"
                            data-tooltip="复制模型 ID"
                            aria-label={`复制模型 ID ${cm.model}`}
                            onClick={() => void copyModelId(cm.model)}
                            disabled={isSaving}
                          >
                            <IconCopyModel />
                          </button>
                        </div>
                        {cm.aliasId && (
                          <span className="ai-provider-custom-model-alias">别名: {cm.aliasId}</span>
                        )}
                      </div>
                      <div style={{ display: "flex", gap: 6, alignItems: "center" }}>
                        <input
                          type="text"
                          className="form-input"
                          placeholder="别名 ID"
                          value={cm.aliasId}
                          onChange={(e) => {
                            const next = [...formCustomModels];
                            next[index] = { ...cm, aliasId: e.target.value };
                            setFormCustomModels(next);
                          }}
                          disabled={isSaving}
                          style={{ width: 120, fontSize: 12 }}
                        />
                        <button
                          type="button"
                          className="btn-delete"
                          data-tooltip="删除此模型"
                          onClick={() => {
                            setFormCustomModels((prev) => prev.filter((_, i) => i !== index));
                          }}
                          disabled={isSaving}
                          style={{ opacity: 1 }}
                        >
                          <IconTrash />
                        </button>
                      </div>
                    </div>
                  ))}
                </div>
              )}
              {/* 手动添加模型（除"从端点拉取"外的另一种录入方式） */}
              <div style={{ display: "flex", gap: 8, alignItems: "center", marginTop: 8 }}>
                <input
                  type="text"
                  className="form-input"
                  placeholder="手动输入模型 ID，如 claude-sonnet-4-5"
                  value={manualModelInput}
                  onChange={(e) => setManualModelInput(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter") {
                      e.preventDefault();
                      addManualModel();
                    }
                  }}
                  disabled={isSaving}
                  style={{ flex: 1 }}
                />
                <button
                  type="button"
                  className="btn btn-secondary"
                  onClick={addManualModel}
                  disabled={isSaving || !manualModelInput.trim()}
                >
                  <IconPlus />
                  添加
                </button>
              </div>
            </div>
            {/* 默认模型选择（从自定义模型列表筛选） */}
            {formType === "universal" && (
              <div className="form-group">
                <label className="form-label" htmlFor="ai-provider-openai-default-model">
                  OpenAI 默认模型 <span className="form-label-optional">可选</span>
                </label>
                <ModelComboBox
                  id="ai-provider-openai-default-model"
                  value={formOpenaiDefaultModel}
                  onChange={setFormOpenaiDefaultModel}
                  disabled={isSaving}
                  options={customModelOptions}
                  placeholder="gpt-5"
                />
              </div>
            )}
            <div className="form-group">
              <label className="form-label" htmlFor="ai-provider-default-model">
                {formType === "universal" ? "Anthropic 默认模型" : "默认模型"}{" "}
                <span className="form-label-optional">可选</span>
              </label>
              <ModelComboBox
                id="ai-provider-default-model"
                value={formDefaultModel}
                onChange={setFormDefaultModel}
                disabled={isSaving}
                options={customModelOptions}
                placeholder={
                  formType === "openai"
                    ? "gpt-5"
                    : "claude-sonnet-4-5"
                }
              />
            </div>
            {formType !== "openai" ? (
              <div className="form-group">
                <label className="form-label">
                  档位模型 <span className="form-label-optional">可选，从自定义模型列表选择</span>
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
                      options={customModelOptions}
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
              disabled={isSaving}
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
