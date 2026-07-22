// 主题注册表：全应用主题的唯一事实源。
// 新增/删减主题只需改这里 + index.css 中对应的 [data-theme] 令牌块。
// 每个 id 必须在 index.css 里有一组完整的 --seed-* 令牌，且是合法 slug（^[a-z0-9-]+$），
// 因为它会被直接写入 <html data-theme> 与后端 config.json。

export type ThemeCategory = "light" | "dark";

export interface ThemeDef {
  /** 稳定标识：写入 data-theme 与 config.json，切勿随意改动（会使已保存配置失效）。 */
  id: string;
  /** UI 显示名。 */
  label: string;
  /** UI 描述文案。 */
  description: string;
  /** 明/暗归类，仅用于偏好设置里的分组展示。 */
  category: ThemeCategory;
}

// 顺序即偏好设置中的展示顺序。Qoder 两套置顶作为默认主题。
export const THEMES = [
  // —— 品牌 / 默认 ——
  {
    id: "qoder-light",
    label: "Qoder Light",
    description: "明亮清爽的界面风格，适合日间使用",
    category: "light",
  },
  {
    id: "qoder-dark",
    label: "Qoder Dark",
    description: "沉稳专注的界面风格，适合夜间使用",
    category: "dark",
  },
  {
    id: "claude",
    label: "Claude",
    description: "温暖米色搭配珊瑚色强调，还原 claude.ai 质感",
    category: "light",
  },
  // —— 浅色 ——
  {
    id: "one-light",
    label: "Atom One Light",
    description: "Atom 经典浅色，柔和克制",
    category: "light",
  },
  {
    id: "github-light",
    label: "GitHub Light",
    description: "GitHub 官方浅色，干净通用",
    category: "light",
  },
  {
    id: "solarized-light",
    label: "Solarized Light",
    description: "低对比暖调，久看不累",
    category: "light",
  },
  {
    id: "gruvbox-light",
    label: "Gruvbox Light",
    description: "复古暖棕，颗粒质感",
    category: "light",
  },
  {
    id: "ayu-light",
    label: "Ayu Light",
    description: "清爽明快，橙色点缀",
    category: "light",
  },
  {
    id: "catppuccin-latte",
    label: "Catppuccin Latte",
    description: "柔和粉彩浅色，温润细腻",
    category: "light",
  },
  // —— 深色 ——
  {
    id: "dracula",
    label: "Dracula",
    description: "标志性紫粉配色，辨识度极高",
    category: "dark",
  },
  {
    id: "one-dark",
    label: "Atom One Dark",
    description: "最流行的深色主题之一，均衡耐看",
    category: "dark",
  },
  {
    id: "monokai",
    label: "Monokai",
    description: "经典高饱和深色，鲜明活泼",
    category: "dark",
  },
  {
    id: "solarized-dark",
    label: "Solarized Dark",
    description: "低对比冷青，护眼沉稳",
    category: "dark",
  },
  {
    id: "nord",
    label: "Nord",
    description: "北欧冷蓝灰，安静克制",
    category: "dark",
  },
  {
    id: "gruvbox-dark",
    label: "Gruvbox Dark",
    description: "复古暖色深色，温厚扎实",
    category: "dark",
  },
  {
    id: "tokyo-night",
    label: "Tokyo Night",
    description: "深夜霓虹蓝紫，现代感强",
    category: "dark",
  },
  {
    id: "night-owl",
    label: "Night Owl",
    description: "为夜间编码调校的深蓝配色",
    category: "dark",
  },
  {
    id: "github-dark",
    label: "GitHub Dark",
    description: "GitHub 官方深色，干净通用",
    category: "dark",
  },
  {
    id: "palenight",
    label: "Material Palenight",
    description: "Material 柔紫深色，优雅耐看",
    category: "dark",
  },
  {
    id: "cobalt2",
    label: "Cobalt2",
    description: "深蓝底黄橙点缀，对比鲜明",
    category: "dark",
  },
  {
    id: "ayu-dark",
    label: "Ayu Dark",
    description: "近黑深色，暖橙强调",
    category: "dark",
  },
  {
    id: "synthwave-84",
    label: "SynthWave '84",
    description: "蒸汽波霓虹紫粉，复古赛博",
    category: "dark",
  },
  {
    id: "catppuccin-mocha",
    label: "Catppuccin Mocha",
    description: "柔和粉彩深色，温润细腻",
    category: "dark",
  },
] as const satisfies readonly ThemeDef[];

export type Theme = (typeof THEMES)[number]["id"];

export const DEFAULT_THEME: Theme = "qoder-light";

const THEME_IDS: ReadonlySet<string> = new Set(THEMES.map((t) => t.id));

export interface AppConfig {
  theme: Theme;
}

/** 未知/非法值一律回退默认主题，保证 data-theme 永远有效。 */
function normalizeTheme(value: unknown): Theme {
  return typeof value === "string" && THEME_IDS.has(value)
    ? (value as Theme)
    : DEFAULT_THEME;
}

/** 存储值是否已是合法注册主题（用于判断启动时是否需要还原回写）。 */
function isKnownTheme(value: unknown): value is Theme {
  return typeof value === "string" && THEME_IDS.has(value);
}

export function applyTheme(theme: Theme): void {
  document.documentElement.setAttribute("data-theme", theme);
}

export async function loadAppConfig(): Promise<AppConfig> {
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    const config = await (invoke("get_app_config") as Promise<{ theme?: string }>);
    if (isKnownTheme(config?.theme)) {
      return { theme: config.theme };
    }
    // 配置文件里是未知/遗留主题（如旧的 light/dark 或人工乱改）：
    // 还原成默认主题并持久化回写，避免每次启动都残留非法值。
    return { theme: await saveTheme(DEFAULT_THEME) };
  } catch {
    // Browser / non-tauri preview: fall back to default.
    return { theme: DEFAULT_THEME };
  }
}

export async function saveTheme(theme: Theme): Promise<Theme> {
  const next = normalizeTheme(theme);
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    const config = await (invoke("set_theme", { theme: next }) as Promise<{ theme?: string }>);
    return normalizeTheme(config?.theme);
  } catch {
    return next;
  }
}
