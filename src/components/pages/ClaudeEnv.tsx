import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useStatusMessage } from "@/lib/useStatusMessage";
import { Toast } from "@/components/Toast";
import { CheckGlyph, useOverlayDismiss } from "../ui";
import { ModelComboBox } from "../ModelComboBox";

import type {
  ClaudeEnvironment,
  ClaudeEnvCandidate,
  ClaudeEnvShellStatus,
} from "./claude-env/types";

import {
  invokeList,
  invokeSniff,
  invokeImport,
  invokeClone,
  invokeUpsert,
  invokeDelete,
  invokeInstallEnvAlias,
  invokeRemoveEnvAlias,
  invokeRemoveAllAliases,
  invokeShellStatus,
  invokeReveal,
  invokeOpenSettings,
  invokeGetSecret,
  invokeFetchRemoteModels,
  invokeSyncMcp,
  invokeSyncSkills,
  invokeSyncAllMcp,
} from "./claude-env/api";
import {
  invokeList as invokeProviderList,
  invokeGetSecret as invokeProviderGetSecret,
} from "./ai-providers/api";
import { MODEL_TIERS, type AiProvider, type ProviderType } from "./ai-providers/types";
import { Blocks, ChevronDown, Copy, Download, Eye, EyeOff, FileJson, Folder, FolderOpen, Pencil, Radar, Sparkles, Terminal, Trash2, X } from "lucide-react";

/* ===== Icons ===== */

const IconClone = () => (
  <Copy size={16} strokeWidth={1.8} />
);

const IconScan = () => (
  <Radar size={16} strokeWidth={1.8} />
);

const IconTerminal = () => (
  <Terminal size={16} strokeWidth={1.8} />
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

const IconFolder = () => (
  <FolderOpen size={16} strokeWidth={1.8} />
);

const IconFile = () => (
  <FileJson size={16} strokeWidth={1.8} />
);

const IconCopy = () => (
  <Copy size={16} strokeWidth={1.8} />
);

const IconEye = () => (
  <Eye size={16} strokeWidth={1.8} />
);

const IconEyeOff = () => (
  <EyeOff size={16} strokeWidth={1.8} />
);

const IconSyncSkills = () => (
  <Sparkles size={16} strokeWidth={1.8} />
);

const IconSyncMcp = () => (
  <Blocks size={16} strokeWidth={1.8} />
);

const IconDownload = () => (
  <Download size={16} strokeWidth={1.8} />
);

const IconEmpty = () => (
  <Folder size={40} strokeWidth={1.5} />
);

const IconChevron = ({ open }: { open?: boolean }) => (
  <ChevronDown size={16} strokeWidth={1.8} style={{ transform: open ? "rotate(180deg)" : undefined, transition: "transform 0.15s ease" }} />
);

/* ===== AI 供应商选择下拉（Claude 环境专用：仅 Anthropic / 通用类型） ===== */

const ProviderSelect = ({
  id,
  providers,
  value,
  onChange,
  disabled,
  allowClear,
}: {
  id?: string;
  providers: AiProvider[];
  value: AiProvider | null;
  onChange: (provider: AiProvider | null) => void;
  disabled?: boolean;
  allowClear?: boolean;
}) => {
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);

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

  const typeLabel = (t: ProviderType) =>
    t === "anthropic" ? "Anthropic" : t === "openai" ? "OpenAI" : "通用";

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
        onClick={() => !disabled && setOpen((v) => !v)}
        disabled={disabled}
      >
        <span className={`app-select-value ${value ? "" : "placeholder"}`}>
          {value ? `${value.name} (${typeLabel(value.providerType)})` : "未选择（自行填写）"}
        </span>
        <span className="app-select-chevron" aria-hidden>
          <IconChevron open={open} />
        </span>
      </button>
      {open && (
        <div className="app-select-menu" role="listbox" aria-label="AI 供应商列表">
          {allowClear && value && (
            <button
              type="button"
              role="option"
              aria-selected={false}
              className="app-select-option"
              onClick={() => {
                onChange(null);
                setOpen(false);
              }}
            >
              <span className="app-select-option-title">清除选择</span>
            </button>
          )}
          {providers.map((p) => (
            <button
              key={p.id}
              type="button"
              role="option"
              aria-selected={value?.id === p.id}
              className={`app-select-option ${value?.id === p.id ? "selected" : ""}`}
              onClick={() => {
                onChange(p);
                setOpen(false);
              }}
            >
              <span className="app-select-option-title">{p.name}</span>
              <span className="app-select-option-sub">
                {typeLabel(p.providerType)} · {p.baseUrl}
              </span>
            </button>
          ))}
        </div>
      )}
    </div>
  );
};

/* ===== Helpers ===== */

function displayPath(abs: string): string {
  return abs.replace(/^\/Users\/[^/]+/, "~").replace(/^\/home\/[^/]+/, "~");
}

function sourceLabel(source: string, isDefault: boolean): string {
  if (isDefault) return "默认";
  if (source === "imported") return "已导入";
  if (source === "managed") return "自定义";
  return source;
}

function mcpStatusLabel(env: ClaudeEnvironment): string {
  const local = env.mcpServerCount ?? 0;
  const global = env.globalMcpServerCount ?? 0;
  const status = env.mcpSyncStatus || "";
  if (env.isDefault || status === "default") {
    return global > 0 ? `MCP：全局 ${global} 个` : "MCP：全局无配置";
  }
  if (status === "in_sync") {
    return global > 0 ? `MCP：与全局一致 (${local})` : "MCP：与全局一致 (0)";
  }
  if (status === "missing") {
    return global > 0 ? `MCP：未同步 (0/${global})` : "MCP：无本地文件";
  }
  if (status === "out_of_sync") {
    return `MCP：未对齐 (${local}/${global})`;
  }
  if (status === "no_global") {
    return local > 0 ? `MCP：本地 ${local} 个（无全局文件）` : "MCP：无全局文件";
  }
  return "MCP：未知";
}

/* ===== Component ===== */

export default function ClaudeEnv() {
  const [envs, setEnvs] = useState<ClaudeEnvironment[]>([]);
  const [shell, setShell] = useState<ClaudeEnvShellStatus | null>(null);
  const [loaded, setLoaded] = useState(false);
  const [statusMsg, setStatusMsg] = useStatusMessage();
  const [busy, setBusy] = useState(false);

  // Clone modal
  const [showClone, setShowClone] = useState(false);
  const [cloneSourceId, setCloneSourceId] = useState("default");
  const [cloneSourceOpen, setCloneSourceOpen] = useState(false);
  const [cloneName, setCloneName] = useState("");
  const [cloneSlug, setCloneSlug] = useState("");
  const [cloneConfigDir, setCloneConfigDir] = useState("");
  const [cloneAlias, setCloneAlias] = useState("");
  const [cloneNotes, setCloneNotes] = useState("");
  const [cloneBaseUrl, setCloneBaseUrl] = useState("");
  const [cloneApiKey, setCloneApiKey] = useState("");
  const [showCloneApiKey, setShowCloneApiKey] = useState(false);
  const [cloneModel, setCloneModel] = useState("");
  const [cloneRemoteModels, setCloneRemoteModels] = useState<string[]>([]);
  const [cloneModelsLoading, setCloneModelsLoading] = useState(false);
  const [cloneSyncMcp, setCloneSyncMcp] = useState(true);
  const [cloneSyncSkills, setCloneSyncSkills] = useState(true);
  const [cloneSyncAgents, setCloneSyncAgents] = useState(true);
  const [cloneInstallAlias, setCloneInstallAlias] = useState(true);
  const [cloneError, setCloneError] = useState("");
  const [slugTouched, setSlugTouched] = useState(false);
  const [aliasTouched, setAliasTouched] = useState(false);
  const [dirTouched, setDirTouched] = useState(false);
  // 供应商选择 + 四档模型（可选覆盖）
  const [cloneProviders, setCloneProviders] = useState<AiProvider[]>([]);
  const [cloneSelectedProvider, setCloneSelectedProvider] = useState<AiProvider | null>(null);
  const [cloneTierModels, setCloneTierModels] = useState<Record<string, string>>({});

  // Edit modal
  const [showEdit, setShowEdit] = useState(false);
  const [editId, setEditId] = useState<string | null>(null);
  const [editName, setEditName] = useState("");
  const [editSlug, setEditSlug] = useState("");
  const [editConfigDir, setEditConfigDir] = useState("");
  const [editAlias, setEditAlias] = useState("");
  const [editNotes, setEditNotes] = useState("");
  const [editError, setEditError] = useState("");
  const [editIsDefault, setEditIsDefault] = useState(false);
  // 自定义环境的 settings.json env 字段（预填当前值，留空即删除）。
  const [editBaseUrl, setEditBaseUrl] = useState("");
  const [editApiKey, setEditApiKey] = useState("");
  const [editModel, setEditModel] = useState("");
  const [editRemoteModels, setEditRemoteModels] = useState<string[]>([]);
  const [editModelsLoading, setEditModelsLoading] = useState(false);
  const [editApiKeyVisible, setEditApiKeyVisible] = useState(false);
  // 目录变更时的迁移二次确认（Tauri WebView 不支持原生 window.confirm，用受控 modal）。
  const [showMigrateConfirm, setShowMigrateConfirm] = useState(false);
  // 记录进入编辑时的原始值，用于三态判定（仅在实际变化时下发）。
  const editEnvOriginalRef = useRef({ baseUrl: "", apiKey: "", model: "", modelTiers: {} as Record<string, string>, providerId: "" });
  // 进入编辑时的原始配置目录，用于判断是否发生"目录迁移"并弹确认。
  const editOriginalConfigDirRef = useRef("");
  // 编辑弹层的供应商选择 + 四档模型
  const [editProviders, setEditProviders] = useState<AiProvider[]>([]);
  const [editSelectedProvider, setEditSelectedProvider] = useState<AiProvider | null>(null);
  const [editTierModels, setEditTierModels] = useState<Record<string, string>>({});

  // Scan modal
  const [showScan, setShowScan] = useState(false);
  const [candidates, setCandidates] = useState<ClaudeEnvCandidate[]>([]);
  const [selectedPaths, setSelectedPaths] = useState<Set<string>>(new Set());
  const [aliasPaths, setAliasPaths] = useState<Set<string>>(new Set());

  // Delete modal
  const [deleteTarget, setDeleteTarget] = useState<ClaudeEnvironment | null>(null);
  const [deleteFiles, setDeleteFiles] = useState(false);

  // Shell preview modal
  const [showShellPreview, setShowShellPreview] = useState(false);

  const hasLoaded = useRef(false);
  const nameInputRef = useRef<HTMLInputElement>(null);
  const cloneSourceRef = useRef<HTMLDivElement>(null);

  const cloneDismiss = useOverlayDismiss(() => setShowClone(false), !busy);
  const editDismiss = useOverlayDismiss(() => setShowEdit(false), !busy);
  const scanDismiss = useOverlayDismiss(() => setShowScan(false), !busy);
  const deleteDismiss = useOverlayDismiss(() => setDeleteTarget(null), !busy);
  const shellPreviewDismiss = useOverlayDismiss(() => setShowShellPreview(false));
  const migrateConfirmDismiss = useOverlayDismiss(() => setShowMigrateConfirm(false), !busy);

  const refresh = useCallback(async () => {
    const [list, status] = await Promise.all([invokeList(), invokeShellStatus()]);
    setEnvs(list);
    setShell(status);
  }, []);

  useEffect(() => {
    if (hasLoaded.current) return;
    hasLoaded.current = true;
    (async () => {
      try {
        await refresh();
      } catch (err) {
        setStatusMsg(`加载失败：${err instanceof Error ? err.message : String(err)}`);
        setEnvs([]);
      } finally {
        setLoaded(true);
      }
    })();
  }, [refresh]);

  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.key !== "Escape" || busy) return;
      // 关闭源环境下拉
      if (cloneSourceOpen) {
        setCloneSourceOpen(false);
        return;
      }
      setShowClone(false);
      setShowEdit(false);
      setShowScan(false);
      setDeleteTarget(null);
      setShowShellPreview(false);
    };
    document.addEventListener("keydown", handler);
    return () => document.removeEventListener("keydown", handler);
  }, [busy, cloneSourceOpen]);

  useEffect(() => {
    if (!cloneSourceOpen) return;
    const onPointerDown = (e: MouseEvent) => {
      const root = cloneSourceRef.current;
      if (root && !root.contains(e.target as Node)) {
        setCloneSourceOpen(false);
      }
    };
    document.addEventListener("mousedown", onPointerDown);
    return () => document.removeEventListener("mousedown", onPointerDown);
  }, [cloneSourceOpen]);

  useEffect(() => {
    if (showClone || showEdit) {
      setTimeout(() => nameInputRef.current?.focus(), 80);
    }
    if (!showClone) setCloneSourceOpen(false);
  }, [showClone, showEdit]);

  const nonDefaultCount = useMemo(() => envs.filter((e) => !e.isDefault).length, [envs]);

  const cloneSourceEnv = useMemo(
    () => envs.find((e) => e.id === cloneSourceId) ?? null,
    [envs, cloneSourceId],
  );

  const openClone = useCallback((source?: ClaudeEnvironment) => {
    const src = source ?? envs.find((e) => e.isDefault) ?? envs[0];
    setCloneSourceId(src?.id ?? "default");
    setCloneSourceOpen(false);
    setCloneName("");
    setCloneSlug("");
    setCloneConfigDir("");
    setCloneAlias("");
    setCloneNotes("");
    setCloneBaseUrl("");
    setCloneApiKey("");
    setShowCloneApiKey(false);
    setCloneModel("");
    setCloneRemoteModels([]);
    setCloneModelsLoading(false);
    setCloneSyncMcp(true);
    setCloneSyncSkills(true);
    setCloneSyncAgents(true);
    setCloneInstallAlias(true);
    setCloneError("");
    setSlugTouched(false);
    setAliasTouched(false);
    setDirTouched(false);
    setCloneSelectedProvider(null);
    setCloneTierModels({});
    setShowClone(true);
    // 加载供应商列表
    void invokeProviderList().then((rows) => {
      // Claude 环境只展示 Anthropic / 通用供应商
      const eligible = rows.filter((p) => p.providerType === "anthropic" || p.providerType === "universal");
      setCloneProviders(eligible);
    }).catch(() => setCloneProviders([]));
  }, [envs]);

  const onCloneSlugChange = (slug: string) => {
    setCloneSlug(slug);
    setSlugTouched(true);
    const clean = slug.trim().toLowerCase();
    if (!aliasTouched && clean) setCloneAlias(`claude-${clean}`);
    if (!dirTouched && clean) setCloneConfigDir(`~/.claude-${clean}`);
  };

  const onCloneNameChange = (name: string) => {
    setCloneName(name);
    if (!slugTouched) {
      const slug = name
        .trim()
        .toLowerCase()
        .replace(/[^a-z0-9]+/g, "-")
        .replace(/^-+|-+$/g, "")
        .slice(0, 32);
      if (slug) {
        setCloneSlug(slug);
        if (!aliasTouched) setCloneAlias(`claude-${slug}`);
        if (!dirTouched) setCloneConfigDir(`~/.claude-${slug}`);
      }
    }
  };

  const fetchModels = useCallback(async (mode: "clone" | "edit") => {
    const baseUrl = mode === "clone" ? cloneBaseUrl : editBaseUrl;
    const apiKey = mode === "clone" ? cloneApiKey : editApiKey;
    const setLoading = mode === "clone" ? setCloneModelsLoading : setEditModelsLoading;
    const setModels = mode === "clone" ? setCloneRemoteModels : setEditRemoteModels;
    const setModel = mode === "clone" ? setCloneModel : setEditModel;

    setLoading(true);
    try {
      const models = await invokeFetchRemoteModels(baseUrl, apiKey || undefined);
      if (models.length === 0) {
        setStatusMsg("远端未返回可用模型，仍可手动输入");
        return;
      }
      setModels(models);
      setModel(models[0]);
      setStatusMsg(`已拉取 ${models.length} 个远端模型`);
    } catch (err) {
      setStatusMsg(`拉取模型失败：${err instanceof Error ? err.message : String(err)}`);
    } finally {
      setLoading(false);
    }
  }, [cloneApiKey, cloneBaseUrl, editApiKey, editBaseUrl, setStatusMsg]);

  const handleClone = useCallback(async () => {
    setBusy(true);
    setCloneError("");
    try {
      // 四档模型：仅传显式覆盖的档位（留空跟随主模型）
      const tiers: Record<string, string> = {};
      for (const tier of MODEL_TIERS) {
        const v = (cloneTierModels[tier.key] ?? "").trim();
        if (v) tiers[tier.key] = v;
      }
const result = await invokeClone({
sourceId: cloneSourceId,
name: cloneName.trim(),
slug: cloneSlug.trim(),
configDir: cloneConfigDir.trim(),
aliasName: cloneAlias.trim(),
notes: cloneNotes.trim() || undefined,
baseUrl: cloneBaseUrl.trim() || undefined,
apiKey: cloneApiKey.trim() || undefined,
model: cloneModel.trim() || undefined,
modelHaiku: tiers.haiku || undefined,
modelSonnet: tiers.sonnet || undefined,
modelOpus: tiers.opus || undefined,
modelFable: tiers.fable || undefined,
syncMcp: cloneSyncMcp,
syncSkills: cloneSyncSkills,
syncAgents: cloneSyncAgents,
installAlias: cloneInstallAlias,
providerId: cloneSelectedProvider?.id || undefined,
});
      if (!result.ok) {
        setCloneError(result.message);
        return;
      }
      setShowClone(false);
      setStatusMsg(result.message);
      await refresh();
    } catch (err) {
      setCloneError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  }, [
    cloneSourceId,
    cloneName,
    cloneSlug,
    cloneConfigDir,
    cloneAlias,
    cloneNotes,
    cloneBaseUrl,
    cloneApiKey,
    cloneModel,
    cloneTierModels,
    cloneSyncMcp,
    cloneSyncSkills,
    cloneSyncAgents,
    cloneInstallAlias,
    refresh,
  ]);

  const openEdit = useCallback(async (env: ClaudeEnvironment) => {
    setEditId(env.id);
    setEditName(env.name);
    setEditSlug(env.slug);
    setEditConfigDir(env.configDir);
    editOriginalConfigDirRef.current = env.configDir;
    setEditAlias(env.aliasName);
    setEditNotes(env.notes);
    setEditIsDefault(env.isDefault);
    // 预填 settings.json 当前值；默认环境也参与供应商 / BaseURL / API Key / 模型编辑。
    const baseUrl = env.baseUrl ?? "";
    const model = env.model ?? "";
    // 回填四档模型（仅含与主模型不同的档位）
    const tiersRaw = env.modelTiers ?? {};
    setEditBaseUrl(baseUrl);
    setEditModel(model);
    setEditRemoteModels([]);
    setEditModelsLoading(false);
    setEditApiKeyVisible(false);
    setEditError("");
    setEditSelectedProvider(null);
    setEditTierModels({ ...tiersRaw });
    // token 列表接口不回传：先以空值打开（避免误判为删除），再按需拉取真值填入并作为三态基准。
    setEditApiKey("");
    editEnvOriginalRef.current = { baseUrl, apiKey: "", model, modelTiers: { ...tiersRaw }, providerId: env.providerId ?? "" };
    setShowEdit(true);
    // 默认环境也需读取当前密钥（通过专用命令从 ~/.claude/settings.json 读取）
    if (env.hasApiKey) {
      try {
        const secret = await invokeGetSecret(env.id);
        setEditApiKey(secret);
        editEnvOriginalRef.current = { baseUrl, apiKey: secret, model, modelTiers: { ...tiersRaw }, providerId: env.providerId ?? "" };
      } catch {
        // 拉取失败则保持空，用户可重新输入
      }
    }
    // 加载供应商列表用于选择
    void invokeProviderList().then((rows) => {
      const eligible = rows.filter((p) => p.providerType === "anthropic" || p.providerType === "universal");
      setEditProviders(eligible);
      // 预选关联的供应商
      const linked = eligible.find((p) => p.id === env.providerId) ?? null;
      setEditSelectedProvider(linked);
    }).catch(() => setEditProviders([]));
  }, []);

  const handleEdit = useCallback(async () => {
    if (!editId) return;
    setShowMigrateConfirm(false);
    setBusy(true);
    setEditError("");
    try {
      // 三态：值未变 → undefined（不下发）；变化后为空 → ""（删除）；否则写入新值。
      const orig = editEnvOriginalRef.current;
      const diffField = (next: string, prev: string): string | undefined =>
        next === prev ? undefined : next;
      // 默认环境也支持供应商 / BaseURL / API Key / 模型编辑（settings.json env 节点）
      const envPayload = {
        baseUrl: diffField(editBaseUrl.trim(), orig.baseUrl),
        apiKey: diffField(editApiKey.trim(), orig.apiKey),
        model: diffField(editModel.trim(), orig.model),
        providerId: diffField(editSelectedProvider?.id ?? "", orig.providerId),
      };
      // 四档模型下发：只传显式变化过（相对原始值）且非空的档位
      const tierDiff = (tierKey: string) => {
        const next = (editTierModels[tierKey] ?? "").trim();
        const prev = (orig.modelTiers?.[tierKey] ?? "");
        return next === prev ? undefined : next || undefined;
      };
      const result = await invokeUpsert({
        id: editId,
        name: editName.trim(),
        slug: editSlug.trim(),
        configDir: editConfigDir.trim(),
        aliasName: editAlias.trim(),
        notes: editNotes.trim() || undefined,
        ...envPayload,
        modelHaiku: tierDiff("haiku"),
        modelSonnet: tierDiff("sonnet"),
        modelOpus: tierDiff("opus"),
        modelFable: tierDiff("fable"),
      });
      if (!result.ok) {
        setEditError(result.message);
        return;
      }
      setShowEdit(false);
      setStatusMsg(result.message);
      await refresh();
    } catch (err) {
      setEditError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  }, [
    editId,
    editName,
    editSlug,
    editConfigDir,
    editAlias,
    editNotes,
    editIsDefault,
    editBaseUrl,
    editApiKey,
    editModel,
    editTierModels,
    refresh,
  ]);

  // 保存入口：非默认环境且配置目录确实变化时，先弹迁移确认；否则直接保存。
  const onSaveClick = useCallback(() => {
    if (!editId) return;
    const nextDir = editConfigDir.trim();
    const prevDir = editOriginalConfigDirRef.current;
    if (!editIsDefault && nextDir && nextDir !== prevDir) {
      setShowMigrateConfirm(true);
      return;
    }
    void handleEdit();
  }, [editId, editConfigDir, editIsDefault, handleEdit]);

  const handleScan = useCallback(async () => {
    setBusy(true);
    try {
      const result = await invokeSniff();
      setCandidates(result.candidates);
      setSelectedPaths(new Set(result.candidates.map((c) => c.path)));
      setAliasPaths(new Set());
      setShowScan(true);
      setStatusMsg(result.message);
    } catch (err) {
      setStatusMsg(`扫描失败：${err instanceof Error ? err.message : String(err)}`);
    } finally {
      setBusy(false);
    }
  }, []);

  const handleImportSelected = useCallback(async () => {
    if (selectedPaths.size === 0) {
      setStatusMsg("请先勾选要导入的目录");
      return;
    }
    setBusy(true);
    try {
      let ok = 0;
      const errors: string[] = [];
      for (const path of selectedPaths) {
        const cand = candidates.find((c) => c.path === path);
        try {
          const result = await invokeImport({
            configDir: path,
            name: cand?.suggestedName,
            slug: cand?.suggestedSlug,
            aliasName: cand?.suggestedAlias,
            installAlias: aliasPaths.has(path),
          });
          if (result.ok) ok += 1;
          else errors.push(result.message);
        } catch (err) {
          errors.push(err instanceof Error ? err.message : String(err));
        }
      }
      setShowScan(false);
      await refresh();
      if (errors.length === 0) {
        setStatusMsg(`已导入 ${ok} 个环境`);
      } else {
        setStatusMsg(`导入完成：成功 ${ok}，失败 ${errors.length}。${errors[0]}`);
      }
    } finally {
      setBusy(false);
    }
  }, [selectedPaths, candidates, aliasPaths, refresh]);

  const handleDelete = useCallback(async () => {
    if (!deleteTarget) return;
    setBusy(true);
    try {
      const result = await invokeDelete(deleteTarget.id, deleteFiles);
      setDeleteTarget(null);
      setDeleteFiles(false);
      setStatusMsg(result.message);
      await refresh();
    } catch (err) {
      setStatusMsg(`删除失败：${err instanceof Error ? err.message : String(err)}`);
    } finally {
      setBusy(false);
    }
  }, [deleteTarget, deleteFiles, refresh]);

  const handleInstallEnvAlias = useCallback(async (env: ClaudeEnvironment) => {
    if (env.isDefault) {
      setStatusMsg("默认环境不支持写入 shell 别名，请直接运行 claude");
      return;
    }
    setBusy(true);
    try {
      const status = await invokeInstallEnvAlias(env.id);
      setShell(status);
      setStatusMsg(status.message);
      if (status.preview) setShowShellPreview(true);
      await refresh();
    } catch (err) {
      setStatusMsg(`写入别名失败：${err instanceof Error ? err.message : String(err)}`);
    } finally {
      setBusy(false);
    }
  }, [refresh]);

  const handleRemoveEnvAlias = useCallback(async (env: ClaudeEnvironment) => {
    if (env.isDefault) return;
    setBusy(true);
    try {
      const status = await invokeRemoveEnvAlias(env.id);
      setShell(status);
      setStatusMsg(status.message);
      await refresh();
    } catch (err) {
      setStatusMsg(`移除别名失败：${err instanceof Error ? err.message : String(err)}`);
    } finally {
      setBusy(false);
    }
  }, [refresh]);

  const handleRemoveAllAliases = useCallback(async () => {
    setBusy(true);
    try {
      const status = await invokeRemoveAllAliases();
      setShell(status);
      setStatusMsg(status.message);
      await refresh();
    } catch (err) {
      setStatusMsg(`清除别名失败：${err instanceof Error ? err.message : String(err)}`);
    } finally {
      setBusy(false);
    }
  }, [refresh]);

  const handleReveal = useCallback(async (id: string) => {
    try {
      const result = await invokeReveal(id);
      setStatusMsg(result.message);
    } catch (err) {
      setStatusMsg(`打开目录失败：${err instanceof Error ? err.message : String(err)}`);
    }
  }, []);

  const handleOpenSettings = useCallback(async (id: string) => {
    try {
      const result = await invokeOpenSettings(id);
      setStatusMsg(result.message);
    } catch (err) {
      setStatusMsg(`打开配置失败：${err instanceof Error ? err.message : String(err)}`);
    }
  }, []);

  const handleSyncMcp = useCallback(async (env: ClaudeEnvironment) => {
    if (env.isDefault) {
      setStatusMsg("默认环境已直接使用全局 ~/.claude.json，无需同步");
      return;
    }
    setBusy(true);
    try {
      const result = await invokeSyncMcp(env.id);
      setStatusMsg(result.message);
      await refresh();
    } catch (err) {
      setStatusMsg(`同步 MCP 失败：${err instanceof Error ? err.message : String(err)}`);
    } finally {
      setBusy(false);
    }
  }, [refresh]);

  const handleSyncSkills = useCallback(async (env: ClaudeEnvironment) => {
    if (env.isDefault) {
      setStatusMsg("默认环境无需同步 skills");
      return;
    }
    setBusy(true);
    try {
      const result = await invokeSyncSkills(env.id);
      setStatusMsg(result.message);
      await refresh();
    } catch (err) {
      setStatusMsg(`同步 skills 失败：${err instanceof Error ? err.message : String(err)}`);
    } finally {
      setBusy(false);
    }
  }, [refresh]);

  const handleSyncAllMcp = useCallback(async () => {
    setBusy(true);
    try {
      const result = await invokeSyncAllMcp();
      setStatusMsg(result.message);
      await refresh();
    } catch (err) {
      setStatusMsg(`批量同步 MCP 失败：${err instanceof Error ? err.message : String(err)}`);
    } finally {
      setBusy(false);
    }
  }, [refresh]);

  const mcpOutOfSyncCount = useMemo(
    () =>
      envs.filter(
        (e) =>
          !e.isDefault &&
          (e.mcpSyncStatus === "out_of_sync" || e.mcpSyncStatus === "missing"),
      ).length,
    [envs],
  );

  const copyPreview = useCallback(async () => {
    const text = shell?.preview || "";
    if (!text) return;
    try {
      await navigator.clipboard.writeText(text);
      setStatusMsg("已复制 shell 别名预览");
    } catch {
      setStatusMsg("复制失败，请手动选择文本");
    }
  }, [shell]);

  return (
    <>
      <div className="content-header">
        <div className="content-header-bar">
          <h1 className="content-title">Claude 环境</h1>
          <div className="header-actions">
            {/* 从右到左：复制、扫描 */}
            <button
              className={`action-btn ${busy ? "sniffing" : ""}`}
              data-tooltip={
                loaded && envs.length === 0
                  ? "请先安装 Claude Code"
                  : "扫描已有目录"
              }
              onClick={() => void handleScan()}
              disabled={busy || (loaded && envs.length === 0)}
            >
              <IconScan />
            </button>
            <button
              className="action-btn"
              data-tooltip={
                loaded && envs.length === 0
                  ? "请先安装 Claude Code"
                  : "从现有环境复制"
              }
              onClick={() => openClone()}
              disabled={busy || envs.length === 0}
            >
              <IconClone />
            </button>
          </div>
        </div>
      </div>

      <div className="content-body">
        <Toast message={statusMsg} />

        {loaded && envs.length > 0 && (
          <div className="mcp-summary claude-env-summary">
            共 <strong>{envs.length}</strong> 个环境
            {nonDefaultCount > 0 && (
              <>
                ，其中 <strong>{nonDefaultCount}</strong> 个额外环境
              </>
            )}
            {shell && (
              <span className="claude-env-shell-hint">
                {shell.blockPresent
                  ? ` · shell 别名已写入（${shell.aliases.length} 个）`
                  : " · 尚未写入 shell 别名"}
              </span>
            )}
            {(nonDefaultCount > 0 || shell?.blockPresent) && (
              <div className="claude-env-summary-actions">
                {nonDefaultCount > 0 && (
                  <button
                    type="button"
                    className="claude-env-link-btn"
                    onClick={() => void handleSyncAllMcp()}
                    disabled={busy}
                  >
                    {mcpOutOfSyncCount > 0
                      ? `同步 MCP 到自定义（${mcpOutOfSyncCount} 未对齐）`
                      : "同步 MCP 到自定义"}
                  </button>
                )}
                {shell?.blockPresent && (
                  <button
                    type="button"
                    className="claude-env-link-btn"
                    onClick={() => void handleRemoveAllAliases()}
                    disabled={busy}
                  >
                    清除全部别名块
                  </button>
                )}
              </div>
            )}
          </div>
        )}

        {loaded && envs.length > 0 && (
          <div className="claude-env-disclaimer">
            通过 <code>CLAUDE_CONFIG_DIR</code> 隔离多套 Claude Code 配置目录。复制始终包含
            settings / CLAUDE.md，skills / agents 可在新建时勾选；不含会话与登录态。默认环境的 MCP 在{" "}
            <code>~/.claude.json</code>；自定义环境在 <code>$配置目录/.claude.json</code>
            ，默认隔离，可用「同步 MCP」把全局顶层 <code>mcpServers</code> 覆盖同步过去。
          </div>
        )}

        {!loaded ? (
          <div className="empty-state">
            <div className="empty-state-text">正在加载…</div>
          </div>
        ) : envs.length === 0 ? (
          <div className="empty-state">
            <IconEmpty />
            <div className="empty-state-text">未检测到 Claude Code</div>
            <div className="empty-state-subtext">
              请先安装 Claude Code CLI（例如 <code>claude</code> 命令），安装后重新打开本页即可管理多环境配置。
            </div>
          </div>
        ) : (
          <div className="claude-env-list">
            {envs.map((env) => (
              <div
                key={env.id}
                className={`claude-env-card ${env.isDefault ? "is-default" : ""} ${
                  !env.dirExists ? "missing" : ""
                }`}
              >
                <div className="claude-env-card-main">
                  <div className="claude-env-title-row">
                    <span className="claude-env-name">{env.name}</span>
                    <span className={`claude-env-badge source-${env.isDefault ? "default" : env.source}`}>
                      {sourceLabel(env.source, env.isDefault)}
                    </span>
                    {!env.dirExists && (
                      <span className="claude-env-badge warn">目录不存在</span>
                    )}
                    {env.aliasInstalled && !env.isDefault && (
                      <span className="claude-env-badge ok">别名已写入</span>
                    )}
                    {!env.isDefault &&
                      (env.mcpSyncStatus === "out_of_sync" ||
                        env.mcpSyncStatus === "missing") && (
                        <span className="claude-env-badge warn">MCP 未对齐</span>
                      )}
                    {!env.isDefault && env.mcpSyncStatus === "in_sync" && (
                      <span className="claude-env-badge ok">MCP 已对齐</span>
                    )}
                    {!env.isDefault && env.skillsSyncStatus === "in_sync" && (
                      <span className="claude-env-badge ok">skills 已对齐</span>
                    )}
                    {!env.isDefault && env.skillsSyncStatus !== "in_sync" && (
                      <span className="claude-env-badge warn">skills 未对齐</span>
                    )}
                  </div>
                  <div className="claude-env-meta">
                    配置路径：<code>{displayPath(env.configDir)}</code>
                  </div>
                  <div className="claude-env-meta">
                    启动：
                    {env.isDefault ? (
                      <code>claude</code>
                    ) : (
                      <code>{env.aliasName}</code>
                    )}
                    {!env.isDefault && (
                      <span className="claude-env-meta-dim">
                        {" "}
                        （CLAUDE_CONFIG_DIR → 该目录）
                      </span>
                    )}
                  </div>
                  <div className="claude-env-meta">{mcpStatusLabel(env)}</div>
                  <div className="claude-env-meta">skills: {env.skillCount ?? 0}个</div>
                  <div className="claude-env-tags">
                    <span className={env.hasSettings ? "on" : "off"}>settings</span>
                    <span className={env.hasSkills ? "on" : "off"}>skills</span>
                    <span className={env.hasAgents ? "on" : "off"}>agents</span>
                  </div>
                  {env.notes ? <div className="claude-env-notes">{env.notes}</div> : null}
                </div>
                <div className="claude-env-actions">
                  {!env.isDefault && (
                    <button
                      type="button"
                      className="claude-env-action-btn"
                      data-tooltip="同步全局 skills 到此环境"
                      onClick={() => void handleSyncSkills(env)}
                      disabled={busy || !env.dirExists}
                    >
                      <IconSyncSkills />
                    </button>
                  )}
                  {!env.isDefault && (
                    <button
                      type="button"
                      className="claude-env-action-btn"
                      data-tooltip="同步全局 MCP 到此环境"
                      onClick={() => void handleSyncMcp(env)}
                      disabled={busy || !env.dirExists}
                    >
                      <IconSyncMcp />
                    </button>
                  )}
                  {!env.isDefault && (
                    env.aliasInstalled ? (
                      <button
                        type="button"
                        className="claude-env-action-btn"
                        data-tooltip={`移除别名 ${env.aliasName}`}
                        onClick={() => void handleRemoveEnvAlias(env)}
                        disabled={busy}
                      >
                        <IconTerminal />
                      </button>
                    ) : (
                      <button
                        type="button"
                        className="claude-env-action-btn"
                        data-tooltip={`写入别名 ${env.aliasName}`}
                        onClick={() => void handleInstallEnvAlias(env)}
                        disabled={busy || !env.dirExists}
                      >
                        <IconTerminal />
                      </button>
                    )
                  )}
                  <button
                    type="button"
                    className="claude-env-action-btn"
                    data-tooltip="打开 settings.json"
                    onClick={() => void handleOpenSettings(env.id)}
                    disabled={!env.dirExists}
                  >
                    <IconFile />
                  </button>
                  <button
                    type="button"
                    className="claude-env-action-btn"
                    data-tooltip="在 Finder 中打开"
                    onClick={() => void handleReveal(env.id)}
                    disabled={!env.dirExists}
                  >
                    <IconFolder />
                  </button>
                  <button
                    type="button"
                    className="claude-env-action-btn"
                    data-tooltip="以此为源复制"
                    onClick={() => openClone(env)}
                    disabled={!env.dirExists}
                  >
                    <IconCopy />
                  </button>
                  <button
                    type="button"
                    className="claude-env-action-btn"
                    data-tooltip="编辑环境信息"
                    onClick={() => void openEdit(env)}
                  >
                    <IconEdit />
                  </button>
                  {!env.isDefault && (
                    <button
                      type="button"
                      className="claude-env-action-btn danger"
                      data-tooltip="删除"
                      onClick={() => {
                        setDeleteTarget(env);
                        setDeleteFiles(false);
                      }}
                    >
                      <IconTrash />
                    </button>
                  )}
                </div>
              </div>
            ))}
          </div>
        )}
      </div>

      {/* ===== Clone Modal ===== */}
      <div
        className={`modal-overlay ${showClone ? "visible" : ""}`}
        {...cloneDismiss}
      >
        <div className="modal modal-lg claude-env-modal">
          <div className="modal-header">
            <h2 className="modal-title">从现有环境复制</h2>
            <button className="modal-close" onClick={() => !busy && setShowClone(false)} disabled={busy}>
              <IconClose />
            </button>
          </div>
          <div className="modal-body claude-env-modal-body">
            <div className="form-group">
              <label className="form-label" id="ce-source-label">源环境</label>
              <div
                className={`app-select ${cloneSourceOpen ? "open" : ""} ${busy ? "disabled" : ""}`}
                ref={cloneSourceRef}
              >
                <button
                  type="button"
                  id="ce-source"
                  className="app-select-trigger form-input"
                  aria-haspopup="listbox"
                  aria-expanded={cloneSourceOpen}
                  aria-labelledby="ce-source-label"
                  disabled={busy}
                  onClick={() => {
                    if (!busy) setCloneSourceOpen((v) => !v);
                  }}
                >
                  <span className={`app-select-value ${cloneSourceEnv ? "" : "placeholder"}`}>
                    {cloneSourceEnv
                      ? `${cloneSourceEnv.name}${cloneSourceEnv.dirExists ? "" : "（目录不存在）"}`
                      : "请选择源环境"}
                  </span>
                  <span className="app-select-chevron" aria-hidden>
                    <IconChevron open={cloneSourceOpen} />
                  </span>
                </button>
                {cloneSourceOpen && (
                  <div className="app-select-menu" role="listbox" aria-labelledby="ce-source-label">
                    {envs.length === 0 ? (
                      <div className="app-select-empty">暂无可用环境</div>
                    ) : (
                      envs.map((e) => {
                        const selected = e.id === cloneSourceId;
                        const disabled = !e.dirExists;
                        return (
                          <button
                            key={e.id}
                            type="button"
                            role="option"
                            aria-selected={selected}
                            className={`app-select-option ${selected ? "selected" : ""} ${
                              disabled ? "disabled" : ""
                            }`}
                            disabled={disabled || busy}
                            onClick={() => {
                              if (disabled || busy) return;
                              setCloneSourceId(e.id);
                              setCloneSourceOpen(false);
                            }}
                          >
                            <span className="app-select-option-title">{e.name}</span>
                            <span className="app-select-option-sub">
                              {displayPath(e.configDir)}
                              {disabled ? " · 目录不存在" : ""}
                            </span>
                          </button>
                        );
                      })
                    )}
                  </div>
                )}
              </div>
            </div>
            <div className="form-group">
              <label className="form-label" htmlFor="ce-name">显示名称</label>
              <input
                ref={nameInputRef}
                id="ce-name"
                className="form-input"
                placeholder="例如：工作账号"
                value={cloneName}
                onChange={(e) => onCloneNameChange(e.target.value)}
                disabled={busy}
              />
            </div>
            <div className="form-group">
              <label className="form-label" htmlFor="ce-slug">slug</label>
              <input
                id="ce-slug"
                className="form-input"
                placeholder="work"
                value={cloneSlug}
                onChange={(e) => onCloneSlugChange(e.target.value)}
                disabled={busy}
              />
            </div>
            <div className="form-group">
              <label className="form-label" htmlFor="ce-dir">配置目录</label>
              <input
                id="ce-dir"
                className="form-input"
                placeholder="~/.claude-work"
                value={cloneConfigDir}
                onChange={(e) => {
                  setDirTouched(true);
                  setCloneConfigDir(e.target.value);
                }}
                disabled={busy}
              />
            </div>
            <div className="form-group">
              <label className="form-label" htmlFor="ce-alias">shell 别名</label>
              <input
                id="ce-alias"
                className="form-input"
                placeholder="claude-work"
                value={cloneAlias}
                onChange={(e) => {
                  setAliasTouched(true);
                  setCloneAlias(e.target.value);
                }}
                disabled={busy}
              />
            </div>
            <div className="form-group">
              <label className="form-label" htmlFor="ce-notes">备注（可选）</label>
              <input
                id="ce-notes"
                className="form-input"
                placeholder="可选说明"
                value={cloneNotes}
                onChange={(e) => setCloneNotes(e.target.value)}
                disabled={busy}
              />
            </div>
            {/* 供应商选择：选后可一键填入 Base URL + 默认模型（位于 Base URL 之上） */}
            {cloneProviders.length > 0 && (
              <div className="form-group">
                <label className="form-label" htmlFor="ce-provider">选择供应商（可选）</label>
                <ProviderSelect
                  id="ce-provider"
                  providers={cloneProviders}
                  value={cloneSelectedProvider}
                      onChange={async (p) => {
                        setCloneSelectedProvider(p);
                        if (p) {
                          setCloneBaseUrl(p.baseUrl);
                          setCloneModel(p.defaultModel);
                          // 四档模型：直接用供应商的档位值覆盖
                          if (Object.keys(p.models).length > 0) {
                            setCloneTierModels((prev) => {
                              const next = { ...prev };
                              for (const tier of MODEL_TIERS) {
                                if (p.models[tier.key]) {
                                  next[tier.key] = p.models[tier.key];
                                } else {
                                  delete next[tier.key];
                                }
                              }
                              return next;
                            });
                          } else {
                            setCloneTierModels({});
                          }
                          // 填入 API Key
                          if (p.hasApiKey) {
                            try {
                              const secret = await invokeProviderGetSecret(p.id);
                              setCloneApiKey(secret);
                            } catch {
                              // 拉取失败则保持原值
                            }
                          } else {
                            setCloneApiKey("");
                          }
                        }
                      }}
                  disabled={busy}
                  allowClear
                />
                <div className="claude-env-form-hint">
                  选择后自动填入供应商的 Base URL、默认模型、四档模型与 API Key；也可跳过自行填写。
                </div>
              </div>
            )}
            <div className="form-group">
              <label className="form-label" htmlFor="ce-base-url">Base URL（可选）</label>
              <input
                id="ce-base-url"
                className="form-input"
                type="url"
                placeholder="留空则复用源环境 ANTHROPIC_BASE_URL"
                value={cloneBaseUrl}
                onChange={(e) => setCloneBaseUrl(e.target.value)}
                disabled={busy}
                autoComplete="off"
                spellCheck={false}
              />
            </div>
            <div className="form-group">
              <label className="form-label" htmlFor="ce-api-key">API Key（可选）</label>
              <div className="form-input-with-action">
                <input
                  id="ce-api-key"
                  className="form-input"
                  type={showCloneApiKey ? "text" : "password"}
                  placeholder="留空则复用源环境 ANTHROPIC_AUTH_TOKEN"
                  value={cloneApiKey}
                  onChange={(e) => setCloneApiKey(e.target.value)}
                  disabled={busy}
                  autoComplete="new-password"
                  spellCheck={false}
                />
                <button
                  type="button"
                  className="form-input-action"
                  data-tooltip={showCloneApiKey ? "隐藏 API Key" : "显示 API Key"}
                  aria-label={showCloneApiKey ? "隐藏 API Key" : "显示 API Key"}
                  aria-pressed={showCloneApiKey}
                  onClick={() => setShowCloneApiKey((v) => !v)}
                  disabled={busy}
                  tabIndex={0}
                >
                  {showCloneApiKey ? <IconEyeOff /> : <IconEye />}
                </button>
              </div>
            </div>
            <div className="form-group">
              <div className="claude-env-model-label-row">
                <label className="form-label" htmlFor="ce-model">默认模型（可选）</label>
                <button
                  type="button"
                  className="claude-env-fetch-models-btn"
                  data-tooltip="从当前 Base URL 拉取模型列表"
                  onClick={() => void fetchModels("clone")}
                  disabled={busy || cloneModelsLoading}
                >
                  <IconDownload />
                  {cloneModelsLoading ? "拉取中…" : "拉取列表"}
                </button>
              </div>
              <ModelComboBox
                id="ce-model"
                value={cloneModel}
                onChange={setCloneModel}
                options={cloneRemoteModels}
                disabled={busy}
                placeholder="留空则不指定；填写后写入 ANTHROPIC_MODEL；留空的档位回退到此值"
              />
              <div className="claude-env-form-hint">
                默认模型回用于各档位；若要按档覆盖，请在下面档位模型中填写。
              </div>
            </div>
{/* 四档模型覆盖 */}
<div className="form-group">
<label className="form-label">
档位模型 <span className="form-label-optional">可选 · 按档覆盖默认值</span>
</label>
{MODEL_TIERS.map((tier) => {
const tierKey = tier.key;
const current = cloneTierModels[tierKey] ?? "";
const idFor = `ce-tier-${tierKey}`;
return (
<div key={tierKey} style={{ marginBottom: 8 }}>
<div className="ai-provider-tier-label">{tier.label}</div>
<ModelComboBox
  id={idFor}
  value={current}
  onChange={(v) => setCloneTierModels(prev => ({ ...prev, [tierKey]: v }))}
  options={cloneRemoteModels}
  disabled={busy}
  placeholder="留空则跟随默认模型"
  clearLabel="跟随默认模型"
/>
</div>
);
})}
</div>
            <div className="form-group">
              <label className="ui-check" htmlFor="ce-sync-skills">
                <input
                  id="ce-sync-skills"
                  type="checkbox"
                  className="ui-check-input"
                  checked={cloneSyncSkills}
                  onChange={(e) => setCloneSyncSkills(e.target.checked)}
                  disabled={busy}
                />
                <CheckGlyph />
                <span className="ui-check-label">
                  同步 skills（复制源环境 <code>skills/</code> 目录到新环境）
                </span>
              </label>
              <label className="ui-check" htmlFor="ce-sync-agents">
                <input
                  id="ce-sync-agents"
                  type="checkbox"
                  className="ui-check-input"
                  checked={cloneSyncAgents}
                  onChange={(e) => setCloneSyncAgents(e.target.checked)}
                  disabled={busy}
                />
                <CheckGlyph />
                <span className="ui-check-label">
                  同步 agents（复制源环境 <code>agents/</code> 目录到新环境）
                </span>
              </label>
              <label className="ui-check" htmlFor="ce-sync-mcp">
                <input
                  id="ce-sync-mcp"
                  type="checkbox"
                  className="ui-check-input"
                  checked={cloneSyncMcp}
                  onChange={(e) => setCloneSyncMcp(e.target.checked)}
                  disabled={busy}
                />
                <CheckGlyph />
                <span className="ui-check-label">
                  同步全局 MCP（将 <code>~/.claude.json</code> 顶层 mcpServers 写入新环境）
                </span>
              </label>
              <label className="ui-check" htmlFor="ce-install-alias">
                <input
                  id="ce-install-alias"
                  type="checkbox"
                  className="ui-check-input"
                  checked={cloneInstallAlias}
                  onChange={(e) => setCloneInstallAlias(e.target.checked)}
                  disabled={busy}
                />
                <CheckGlyph />
                <span className="ui-check-label">
                  写入 shell 别名（把 <code>{cloneAlias.trim() || "claude-<slug>"}</code> 写入{" "}
                  <code>{shell ? displayPath((shell.shellConfigPath || shell.zshrcPath)) : "shell 配置"}</code>）
                </span>
              </label>
            </div>
            <div className="claude-env-form-hint">
              始终复制 settings.json、CLAUDE.md；skills/、agents/ 按上方勾选决定是否复制。不会复制会话与历史。
              Base URL / API Key 留空时沿用源环境；填写后会写入新环境 settings.json 的
              env.ANTHROPIC_BASE_URL / env.ANTHROPIC_AUTH_TOKEN。
              自定义模型留空则不指定，填写后同步写入 env.ANTHROPIC_MODEL 与各档 DEFAULT_*_MODEL / *_MODEL_NAME。
              勾选同步 MCP 会以全局配置覆盖新环境的 mcpServers（权威覆盖）。
              勾选写入别名会把该环境的启动别名追加进当前 shell 配置文件，需 source 或新开终端后生效。
            </div>
            {cloneError && <div className="mcp-form-error">{cloneError}</div>}
          </div>
          <div className="modal-footer claude-env-clone-footer">
            <button className="btn btn-secondary" onClick={() => setShowClone(false)} disabled={busy}>
              取消
            </button>
            <button className="btn btn-primary" onClick={() => void handleClone()} disabled={busy}>
              {busy ? "复制中…" : "创建"}
            </button>
          </div>
        </div>
      </div>

      {/* ===== Edit Modal ===== */}
      <div
        className={`modal-overlay ${showEdit ? "visible" : ""}`}
        {...editDismiss}
      >
        <div className="modal claude-env-modal">
          <div className="modal-header">
            <h2 className="modal-title">编辑环境</h2>
            <button className="modal-close" onClick={() => !busy && setShowEdit(false)} disabled={busy}>
              <IconClose />
            </button>
          </div>
          <div className="modal-body claude-env-modal-body">
            <div className="form-group">
              <label className="form-label" htmlFor="ce-edit-name">显示名称</label>
              <input
                ref={nameInputRef}
                id="ce-edit-name"
                className="form-input"
                value={editName}
                onChange={(e) => setEditName(e.target.value)}
                disabled={busy}
              />
            </div>
            {!editIsDefault && (
              <>
                <div className="form-group">
                  <label className="form-label" htmlFor="ce-edit-slug">slug</label>
                  <input
                    id="ce-edit-slug"
                    className="form-input"
                    value={editSlug}
                    onChange={(e) => setEditSlug(e.target.value)}
                    disabled={busy}
                  />
                </div>
                <div className="form-group">
                  <label className="form-label" htmlFor="ce-edit-dir">配置目录</label>
                  <input
                    id="ce-edit-dir"
                    className="form-input"
                    value={editConfigDir}
                    onChange={(e) => setEditConfigDir(e.target.value)}
                    disabled={busy}
                  />
                  <div className="claude-env-form-hint">
                    修改后会迁移整个环境目录；目标目录必须不存在或为空。
                  </div>
                </div>
                <div className="form-group">
                  <label className="form-label" htmlFor="ce-edit-alias">shell 别名</label>
                  <input
                    id="ce-edit-alias"
                    className="form-input"
                    value={editAlias}
                    onChange={(e) => setEditAlias(e.target.value)}
                    disabled={busy}
                  />
                </div>
              </>
            )}
            {/* 供应商选择（位于 Base URL 之上） */}
            {editProviders.length > 0 && (
              <div className="form-group">
                <label className="form-label" htmlFor="ce-edit-provider">选择供应商（可选）</label>
                <ProviderSelect
                  id="ce-edit-provider"
                  providers={editProviders}
                  value={editSelectedProvider}
                  onChange={async (p) => {
                    setEditSelectedProvider(p);
                    if (p) {
                      setEditBaseUrl(p.baseUrl);
                      setEditModel(p.defaultModel);
                      // 四档模型：直接用供应商的档位值覆盖
                      if (Object.keys(p.models).length > 0) {
                        setEditTierModels((prev) => {
                          const next = { ...prev };
                          for (const tier of MODEL_TIERS) {
                            if (p.models[tier.key]) {
                              next[tier.key] = p.models[tier.key];
                            } else {
                              delete next[tier.key];
                            }
                          }
                          return next;
                        });
                      } else {
                        setEditTierModels({});
                      }
                      // 填入 API Key
                      if (p.hasApiKey) {
                        try {
                          const secret = await invokeProviderGetSecret(p.id);
                          setEditApiKey(secret);
                        } catch {
                          // 拉取失败则保持原值
                        }
                      } else {
                        setEditApiKey("");
                      }
                    }
                  }}
                  disabled={busy}
                  allowClear
                />
              </div>
            )}
            <div className="form-group">
              <label className="form-label" htmlFor="ce-edit-base-url">Base URL</label>
              <input
                id="ce-edit-base-url"
                className="form-input"
                type="url"
                placeholder="留空即从 settings.json 删除 ANTHROPIC_BASE_URL"
                value={editBaseUrl}
                onChange={(e) => setEditBaseUrl(e.target.value)}
                disabled={busy}
                autoComplete="off"
                spellCheck={false}
              />
            </div>
            <div className="form-group">
              <label className="form-label" htmlFor="ce-edit-api-key">API Key</label>
              <div className="form-input-with-action">
                <input
                  id="ce-edit-api-key"
                  className="form-input"
                  type={editApiKeyVisible ? "text" : "password"}
                  placeholder="留空即从 settings.json 删除 ANTHROPIC_AUTH_TOKEN"
                  value={editApiKey}
                  onChange={(e) => setEditApiKey(e.target.value)}
                  disabled={busy}
                  autoComplete="new-password"
                  spellCheck={false}
                />
                <button
                  type="button"
                  className="form-input-action"
                  data-tooltip={editApiKeyVisible ? "隐藏 API Key" : "显示 API Key"}
                  aria-label={editApiKeyVisible ? "隐藏 API Key" : "显示 API Key"}
                  aria-pressed={editApiKeyVisible}
                  onClick={() => setEditApiKeyVisible((v) => !v)}
                  disabled={busy}
                  tabIndex={0}
                >
                  {editApiKeyVisible ? <IconEyeOff /> : <IconEye />}
                </button>
              </div>
            </div>
            <div className="form-group">
              <div className="claude-env-model-label-row">
                <label className="form-label" htmlFor="ce-edit-model">默认模型</label>
                <button
                  type="button"
                  className="claude-env-fetch-models-btn"
                  data-tooltip="从当前 Base URL 拉取模型列表"
                  onClick={() => void fetchModels("edit")}
                  disabled={busy || editModelsLoading}
                >
                  <IconDownload />
                  {editModelsLoading ? "拉取中…" : "拉取列表"}
                </button>
              </div>
              <ModelComboBox
                id="ce-edit-model"
                value={editModel}
                onChange={setEditModel}
                options={editRemoteModels}
                disabled={busy}
                placeholder="留空即从 settings.json 删除 ANTHROPIC_MODEL；留空的档位回退到此值"
              />
            </div>
            {/* 四档模型覆盖 */}
            <div className="form-group">
              <label className="form-label">
                档位模型 <span className="form-label-optional">可选 · 按档覆盖默认值</span>
              </label>
              {MODEL_TIERS.map((tier) => {
                const tierKey = tier.key;
                const current = editTierModels[tierKey] ?? "";
                const idFor = `ce-edit-tier-${tierKey}`;
                return (
                  <div key={tierKey} style={{ marginBottom: 8 }}>
                    <div className="ai-provider-tier-label">{tier.label}</div>
                    <ModelComboBox
                      id={idFor}
                      value={current}
                      onChange={(v) => setEditTierModels(prev => ({ ...prev, [tierKey]: v }))}
                      options={editRemoteModels}
                      disabled={busy}
                      placeholder="留空则跟随默认模型"
                      clearLabel="跟随默认模型"
                    />
                  </div>
                );
              })}
              <div className="claude-env-form-hint">
                默认模型与档位模型预填 settings.json 当前值，留空并保存即删除对应键。
              </div>
            </div>
            {editIsDefault && (
              <div className="claude-env-form-hint">
                默认环境路径固定为 <code>~/.claude</code>，直接运行 <code>claude</code> 使用；该配置将写入默认 settings.json 的 env 节点。
              </div>
            )}
            <div className="form-group">
              <label className="form-label" htmlFor="ce-edit-notes">备注</label>
              <input
                id="ce-edit-notes"
                className="form-input"
                value={editNotes}
                onChange={(e) => setEditNotes(e.target.value)}
                disabled={busy}
              />
            </div>
            {editError && <div className="mcp-form-error">{editError}</div>}
          </div>
          <div className="modal-footer">
            <button className="btn btn-secondary" onClick={() => setShowEdit(false)} disabled={busy}>
              取消
            </button>
            <button className="btn btn-primary" onClick={onSaveClick} disabled={busy}>
              {busy ? "保存中…" : "保存"}
            </button>
          </div>
        </div>
      </div>

      {/* ===== Scan / Import Modal ===== */}
      <div
        className={`modal-overlay ${showScan ? "visible" : ""}`}
        {...scanDismiss}
      >
        <div className="modal modal-lg">
          <div className="modal-header">
            <h2 className="modal-title">扫描到的目录</h2>
            <button className="modal-close" onClick={() => !busy && setShowScan(false)} disabled={busy}>
              <IconClose />
            </button>
          </div>
          <div className="modal-body">
            {candidates.length === 0 ? (
              <div className="empty-state-text">主目录下没有未登记的 <code>.claude-*</code> 目录</div>
            ) : (
              <div className="claude-env-scan-list">
                {candidates.map((c) => {
                  const checked = selectedPaths.has(c.path);
                  const aliasChecked = aliasPaths.has(c.path);
                  return (
                    <div key={c.path} className="claude-env-scan-item">
                      <label className="ui-check claude-env-scan-select">
                        <input
                          type="checkbox"
                          className="ui-check-input"
                          checked={checked}
                          onChange={() => {
                            setSelectedPaths((prev) => {
                              const next = new Set(prev);
                              if (next.has(c.path)) next.delete(c.path);
                              else next.add(c.path);
                              return next;
                            });
                            // 取消勾选目录时，一并撤销其别名写入意图，避免状态残留。
                            setAliasPaths((prev) => {
                              if (!prev.has(c.path)) return prev;
                              const next = new Set(prev);
                              next.delete(c.path);
                              return next;
                            });
                          }}
                          disabled={busy}
                        />
                        <CheckGlyph />
                      </label>
                      <div className="claude-env-scan-info">
                        <div className="claude-env-name">{c.suggestedName}</div>
                        <div className="claude-env-meta">
                          <code>{displayPath(c.path)}</code>
                          {" · "}
                          <code>{c.suggestedAlias}</code>
                        </div>
                        <div className="claude-env-tags">
                          <span className={c.hasSettings ? "on" : "off"}>settings</span>
                          <span className={c.hasSkills ? "on" : "off"}>skills</span>
                          <span className={c.hasAgents ? "on" : "off"}>agents</span>
                        </div>
                        <label className="ui-check claude-env-scan-alias">
                          <input
                            type="checkbox"
                            className="ui-check-input"
                            checked={aliasChecked}
                            onChange={() => {
                              setAliasPaths((prev) => {
                                const next = new Set(prev);
                                if (next.has(c.path)) next.delete(c.path);
                                else next.add(c.path);
                                return next;
                              });
                            }}
                            disabled={busy || !checked}
                          />
                          <CheckGlyph />
                          <span className="ui-check-label">
                            导入后写入 shell 别名 <code>{c.suggestedAlias}</code>
                          </span>
                        </label>
                      </div>
                    </div>
                  );
                })}
              </div>
            )}
          </div>
          <div className="modal-footer">
            <button className="btn btn-secondary" onClick={() => setShowScan(false)} disabled={busy}>
              取消
            </button>
            <button
              className="btn btn-primary"
              onClick={() => void handleImportSelected()}
              disabled={busy || candidates.length === 0 || selectedPaths.size === 0}
            >
              {busy ? "导入中…" : `导入所选（${selectedPaths.size}）`}
            </button>
          </div>
        </div>
      </div>

      {/* ===== Delete Modal ===== */}
      <div
        className={`modal-overlay ${deleteTarget ? "visible" : ""}`}
        {...deleteDismiss}
      >
        <div className="modal" style={{ width: 400 }}>
          <div className="modal-header">
            <h2 className="modal-title">确认删除</h2>
            <button
              className="modal-close"
              onClick={() => !busy && setDeleteTarget(null)}
              disabled={busy}
            >
              <IconClose />
            </button>
          </div>
          <div className="confirm-body">
            <div className="confirm-text">
              确定从列表移除{deleteTarget ? `「${deleteTarget.name}」` : "此环境"}吗？
            </div>
            <div className="confirm-subtext">
              默认仅取消登记，不会删除磁盘文件。勾选下方选项才会删除配置目录。
            </div>
            {deleteTarget?.aliasInstalled && (
              <div className="confirm-subtext">
                该环境的 shell 别名 <code>{deleteTarget.aliasName}</code> 也将从{" "}
                <code>shell 配置文件</code> 一并移除。
              </div>
            )}
            <label className="ui-check">
              <input
                type="checkbox"
                className="ui-check-input"
                checked={deleteFiles}
                onChange={(e) => setDeleteFiles(e.target.checked)}
                disabled={busy}
              />
              <CheckGlyph />
              <span className="ui-check-label">
                同时删除磁盘目录{" "}
                <code>{deleteTarget ? displayPath(deleteTarget.configDir) : ""}</code>
              </span>
            </label>
          </div>
          <div className="modal-footer">
            <button
              className="btn btn-secondary"
              onClick={() => setDeleteTarget(null)}
              disabled={busy}
            >
              取消
            </button>
            <button className="btn btn-danger" onClick={() => void handleDelete()} disabled={busy}>
              {busy ? "删除中…" : "删除"}
            </button>
          </div>
        </div>
      </div>

      {/* ===== Shell preview Modal ===== */}
      <div
        className={`modal-overlay ${showShellPreview ? "visible" : ""}`}
        {...shellPreviewDismiss}
      >
        <div className="modal modal-lg">
          <div className="modal-header">
            <h2 className="modal-title">Shell 别名</h2>
            <button className="modal-close" onClick={() => setShowShellPreview(false)}>
              <IconClose />
            </button>
          </div>
          <div className="modal-body">
            <div className="claude-env-form-hint">
              已写入 <code>{shell ? displayPath((shell.shellConfigPath || shell.zshrcPath)) : "shell 配置"}</code> 的 AgentBuddy 标记块。请在终端执行{" "}
              <code>source {shell ? displayPath((shell.shellConfigPath || shell.zshrcPath)) : "对应 rc 文件"}</code> 或新开终端后生效。
            </div>
            <pre className="claude-env-preview">{shell?.preview || "（无别名内容）"}</pre>
          </div>
          <div className="modal-footer">
            <button className="btn btn-secondary" onClick={() => void copyPreview()}>
              复制
            </button>
            <button className="btn btn-primary" onClick={() => setShowShellPreview(false)}>
              知道了
            </button>
          </div>
        </div>
      </div>

      {/* ===== 目录迁移确认 Modal ===== */}
      <div
        className={`modal-overlay ${showMigrateConfirm ? "visible" : ""}`}
        {...migrateConfirmDismiss}
      >
        <div className="modal" style={{ width: 440 }}>
          <div className="modal-header">
            <h2 className="modal-title">确认迁移目录</h2>
            <button
              className="modal-close"
              onClick={() => !busy && setShowMigrateConfirm(false)}
              disabled={busy}
            >
              <IconClose />
            </button>
          </div>
          <div className="confirm-body">
            <div className="confirm-text">将迁移整个 Claude 环境目录：</div>
            <div className="confirm-subtext">
              <code>{displayPath(editOriginalConfigDirRef.current)}</code>
              {" → "}
              <code>{displayPath(editConfigDir.trim())}</code>
            </div>
            <div className="confirm-subtext">
              目标目录必须不存在或为空；迁移成功后 shell 别名会自动指向新路径。建议先关闭正在使用该环境的 Claude Code 进程。
            </div>
          </div>
          <div className="modal-footer">
            <button
              className="btn btn-secondary"
              onClick={() => setShowMigrateConfirm(false)}
              disabled={busy}
            >
              取消
            </button>
            <button
              className="btn btn-primary"
              onClick={() => void handleEdit()}
              disabled={busy}
            >
              {busy ? "迁移中…" : "确认迁移"}
            </button>
          </div>
        </div>
      </div>
    </>
  );
}
