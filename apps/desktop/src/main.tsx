import React from "react";
import ReactDOM from "react-dom/client";
import { createPortal } from "react-dom";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import Editor, { type OnMount } from "@monaco-editor/react";
import * as monaco from "monaco-editor";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import "@xterm/xterm/css/xterm.css";
import materialTheme from "./assets/material-icons.json";
import {
  Bot, CheckCircle2, ChevronDown, ChevronRight, CircleStop, Code2, File, FileDiff, FilePlus, Folder, FolderGit2, Image,
  FolderOpen, FolderPlus, GitBranch, KeyRound, PanelLeftClose, PanelLeftOpen, Pencil, Plus,
  Download, MessageSquare, RefreshCw, RotateCcw, Save, Search, Send, Settings, ShieldCheck, Sparkles, TerminalSquare, Trash2,
  XCircle, Zap, Sun, Moon, Monitor, Check, Menu, GitCommitHorizontal, Eye, ExternalLink,
} from "lucide-react";
import "./styles.css";

type Mode = "readOnly" | "ask" | "workspace" | "fullAccess";
type Session = { id: string; cwd: string; title?: string; status: string; updatedAt: number };
type Project = { name: string; path: string };
type ProviderProfile = {
  id: string; name: string; provider: string; baseUrl: string; model: string; apiKey: string;
  inputPricePerMillion: number; outputPricePerMillion: number;
};
type ModelCapabilities = {
  imageInput: boolean; imageOutput: boolean; audioInput: boolean; audioOutput: boolean; toolCalling: boolean;
};
type CapabilityRoute = {
  enabled: boolean; inheritMainModel: boolean; provider: string; baseUrl: string; model: string; apiKey: string;
};
type SettingsData = {
  provider: string; baseUrl: string; model: string; apiKey: string; workspace: string;
  dataDir: string; approvalMode: Mode; maxProviderRounds: number; projects: Project[];
  inputPricePerMillion: number; outputPricePerMillion: number;
  providerProfiles: ProviderProfile[];
  modelCapabilities: ModelCapabilities; visionRoute: CapabilityRoute; imageGenerationRoute: CapabilityRoute;
  speechToTextRoute: CapabilityRoute; textToSpeechRoute: CapabilityRoute;
};
type WorkspaceEntry = { path: string; name: string; isDir: boolean; depth: number };
type GitChange = { status: string; path: string };
type GitCommit = { hash: string; subject: string; author: string; relativeTime: string };
type GitOverview = { isRepository: boolean; branch: string; additions: number; deletions: number; commits: GitCommit[] };
type OpenFile = { path: string; content: string; savedContent: string; pinned: boolean };
type SettingsSection = "appearance" | "model" | "capabilities" | "workspace" | "agent" | "updates";
type ThemeMode = "system" | "light" | "dark";
type ModelMessage = { role: "system" | "user" | "assistant" | "tool"; content?: string; reasoning_content?: string };
type TokenUsage = { inputTokens: number; outputTokens: number; totalTokens: number; cachedInputTokens: number };
type CoreEvent = {
  type: string; eventId?: string; turnId?: string; toolCallId?: string; toolName?: string; error?: string; text?: string;
  arguments?: unknown; output?: unknown; usage?: TokenUsage; model?: string;
  event_id?: string; turn_id?: string; tool_call_id?: string; tool_name?: string;
};
type ToolStep = {
  id: string; toolName: string; status: "running" | "completed" | "failed" | "stale";
  arguments?: unknown; output?: unknown; error?: string;
};
type Approval = { id: string; toolName: string; reason: string; arguments: unknown };
type ChatMessage = { role: "user" | "assistant"; text: string; reasoning?: string };
type UpdateInfo = {
  currentVersion: string; latestVersion: string; available: boolean; releaseUrl: string;
  installerUrl?: string; installerName?: string; publishedAt?: string; notes?: string;
};
type FileContextMenu = { x: number; y: number; path: string; isDir: boolean };

const DEFAULT_SETTINGS: SettingsData = {
  provider: "deepseek",
  baseUrl: "https://api.deepseek.com",
  model: "deepseek-v4-pro",
  apiKey: "",
  workspace: "",
  dataDir: "",
  approvalMode: "readOnly",
  maxProviderRounds: 48,
  inputPricePerMillion: 0,
  outputPricePerMillion: 0,
  projects: [],
  providerProfiles: [],
  modelCapabilities: { imageInput: false, imageOutput: false, audioInput: false, audioOutput: false, toolCalling: false },
  visionRoute: { enabled: false, inheritMainModel: true, provider: "custom", baseUrl: "", model: "", apiKey: "" },
  imageGenerationRoute: { enabled: false, inheritMainModel: true, provider: "custom", baseUrl: "", model: "", apiKey: "" },
  speechToTextRoute: { enabled: false, inheritMainModel: true, provider: "custom", baseUrl: "", model: "", apiKey: "" },
  textToSpeechRoute: { enabled: false, inheritMainModel: true, provider: "custom", baseUrl: "", model: "", apiKey: "" },
};

const PROVIDERS = [
  { id: "deepseek", name: "DeepSeek", baseUrl: "https://api.deepseek.com", model: "deepseek-chat", inputPrice: 0.27, outputPrice: 1.10 },
  { id: "openai", name: "OpenAI", baseUrl: "https://api.openai.com/v1", model: "gpt-4.1" },
  { id: "anthropic", name: "Anthropic Claude", baseUrl: "https://api.anthropic.com/v1", model: "claude-sonnet-4-5" },
  { id: "gemini", name: "Google Gemini", baseUrl: "https://generativelanguage.googleapis.com/v1beta/openai", model: "gemini-3.5-flash" },
  { id: "mistral", name: "Mistral AI", baseUrl: "https://api.mistral.ai/v1", model: "devstral-medium-latest" },
  { id: "openrouter", name: "OpenRouter", baseUrl: "https://openrouter.ai/api/v1", model: "anthropic/claude-sonnet-4" },
  { id: "qwen", name: "阿里云百炼 / 通义千问", baseUrl: "https://dashscope.aliyuncs.com/compatible-mode/v1", model: "qwen3-coder-next" },
  { id: "dashscope-deepseek", name: "阿里云百炼 / DeepSeek", baseUrl: "https://dashscope.aliyuncs.com/compatible-mode/v1", model: "deepseek-v4-pro" },
  { id: "moonshot", name: "Moonshot / Kimi", baseUrl: "https://api.moonshot.cn/v1", model: "kimi-k2-0711-preview" },
  { id: "siliconflow", name: "硅基流动", baseUrl: "https://api.siliconflow.cn/v1", model: "Pro/zai-org/GLM-4.7" },
  { id: "zhipu", name: "智谱 GLM Coding", baseUrl: "https://open.bigmodel.cn/api/coding/paas/v4", model: "glm-5.2" },
  { id: "groq", name: "Groq", baseUrl: "https://api.groq.com/openai/v1", model: "openai/gpt-oss-120b" },
  { id: "xai", name: "xAI", baseUrl: "https://api.x.ai/v1", model: "grok-4-0709" },
  { id: "ollama", name: "Ollama 本地模型", baseUrl: "http://localhost:11434/v1", model: "qwen2.5-coder:7b" },
  { id: "lmstudio", name: "LM Studio 本地模型", baseUrl: "http://localhost:1234/v1", model: "local-model" },
  { id: "custom", name: "自定义 OpenAI-compatible", baseUrl: "", model: "" },
];

const APPROVAL_MODES: { id: Mode; label: string }[] = [
  { id: "readOnly", label: "只读" }, { id: "ask", label: "每次询问" },
  { id: "workspace", label: "工作区写入" }, { id: "fullAccess", label: "完全访问" },
];

type IconDefinition = { iconPath?: string };
type IconAssociations = { file?: string; fileExtensions?: Record<string, string>; fileNames?: Record<string, string>; languageIds?: Record<string, string> };
const MATERIAL_THEME = materialTheme as typeof materialTheme & IconAssociations & { iconDefinitions: Record<string, IconDefinition>; light?: IconAssociations };
const EXTENSION_LANGUAGE_IDS: Record<string, string> = {
  bat: "bat", cmd: "bat", clj: "clojure", cljs: "clojure", coffee: "coffeescript", json: "json", jsonc: "jsonc", jsonl: "jsonl",
  c: "c", cc: "cpp", cpp: "cpp", cxx: "cpp", h: "c", hh: "cpp", hpp: "cpp", cs: "csharp", css: "css", scss: "scss", sass: "sass", less: "less",
  dart: "dart", dockerfile: "dockerfile", env: "dotenv", fs: "fsharp", fsx: "fsharp", go: "go", groovy: "groovy", hbs: "handlebars",
  html: "html", htm: "html", java: "java", js: "javascript", mjs: "javascript", cjs: "javascript", jsx: "javascriptreact", ts: "typescript", mts: "typescript", cts: "typescript", tsx: "typescriptreact",
  jl: "julia", tex: "tex", lua: "lua", md: "markdown", mdx: "markdown", m: "objective-c", mm: "objective-cpp", pl: "perl", php: "php",
  ps1: "powershell", pug: "jade", py: "python", pyw: "python", r: "r", rb: "ruby", rs: "rust", sh: "shellscript", bash: "shellscript", zsh: "shellscript",
  sql: "sql", swift: "swift", xml: "xml", yaml: "yaml", yml: "yaml", ex: "elixir", exs: "elixir", elm: "elm", gradle: "gradle",
  hs: "haskell", kt: "kotlin", kts: "kotlin", ml: "ocaml", mli: "ocaml", res: "rescript", styl: "stylus", tf: "terraform", vue: "vue",
};

function resolveMaterialIcon(path: string, light: boolean): string {
  const name = path.split(/[\\/]/).at(-1)?.toLowerCase() || "";
  const associations = light ? MATERIAL_THEME.light || MATERIAL_THEME : MATERIAL_THEME;
  let definitionId: string | undefined = associations.fileNames?.[name];
  if (!definitionId) {
    const parts = name.split(".");
    for (let index = 1; index < parts.length; index += 1) {
      const extension = parts.slice(index).join(".");
      definitionId = associations.fileExtensions?.[extension];
      if (!definitionId) {
        const languageId = EXTENSION_LANGUAGE_IDS[extension];
        definitionId = languageId ? associations.languageIds?.[languageId] : undefined;
      }
      if (definitionId) break;
    }
  }
  const definition = MATERIAL_THEME.iconDefinitions[definitionId || associations.file || MATERIAL_THEME.file || "file"];
  const filename = definition?.iconPath?.split("/").at(-1) || "file.svg";
  return `/material-icons/${filename}`;
}

function FileTypeIcon({ path, size = 16, light = false }: { path: string; size?: number; light?: boolean }) {
  return <img className="material-file-icon" src={resolveMaterialIcon(path, light)} width={size} height={size} alt="" />;
}

function Dropdown<T extends string>({ value, options, icon, title, onChange }: {
  value: T; options: { id: T; label: string }[]; icon: React.ReactNode; title: string; onChange: (value: T) => void;
}) {
  const [open, setOpen] = React.useState(false);
  const [menuStyle, setMenuStyle] = React.useState<React.CSSProperties>({});
  const host = React.useRef<HTMLDivElement>(null);
  React.useEffect(() => {
    const close = (event: PointerEvent) => {
      if (!host.current?.contains(event.target as Node)) setOpen(false);
    };
    window.addEventListener("pointerdown", close);
    return () => window.removeEventListener("pointerdown", close);
  }, []);
  const selected = options.find((option) => option.id === value);
  const toggle = () => {
    if (!open && host.current) {
      const rect = host.current.getBoundingClientRect();
      setMenuStyle({ top: rect.bottom + 6, right: Math.max(8, window.innerWidth - rect.right), minWidth: Math.max(190, rect.width) });
    }
    setOpen((current) => !current);
  };
  return <div className="dropdown" ref={host}>
    <button className={`dropdown-trigger ${open ? "open" : ""}`} title={title} onClick={toggle}>
      {icon}<span>{selected?.label || "未选择"}</span><ChevronDown size={13} />
    </button>
    {open && createPortal(<div className="dropdown-menu dropdown-portal" style={menuStyle} onPointerDown={(event) => event.stopPropagation()}>{options.map((option) => <button key={option.id} className={option.id === value ? "active" : ""} onClick={() => { onChange(option.id); setOpen(false); }}>
      <span>{option.label}</span>{option.id === value && <Check size={13} />}
    </button>)}</div>, document.body)}
  </div>;
}

const TOOL_LABELS: Record<string, string> = {
  list_files: "浏览文件", read_file: "读取文件", search_text: "搜索代码", replace_text: "修改文件",
  apply_edits: "应用修改", create_file: "创建文件", run_command: "运行命令", git_status: "检查 Git 状态",
  git_diff: "查看 Git 改动", analyze_image: "分析图片", generate_image: "生成图片", echo: "处理信息",
};

function summarizeValue(value: unknown, limit = 150) {
  if (value === undefined || value === null) return "";
  const text = typeof value === "string" ? value : JSON.stringify(value);
  return text.length > limit ? `${text.slice(0, limit)}…` : text;
}

function toolDisplayName(event: CoreEvent) {
  const label = TOOL_LABELS[event.toolName || ""] || event.toolName || "工具调用";
  const args = event.arguments as Record<string, unknown> | undefined;
  let detail = args?.path || args?.query || args?.command || args?.pattern || args?.program;
  if (event.toolName === "apply_edits" && Array.isArray(args?.files)) detail = `${args.files.length} 个文件`;
  if (!detail || typeof detail !== "string") return label;
  const compact = detail.replaceAll("\\", "/").split("/").at(-1) || detail;
  return `${label} · ${compact.length > 30 ? `${compact.slice(0, 30)}…` : compact}`;
}

function toolStepTitle(step: ToolStep) {
  return toolDisplayName({ type: "toolStarted", toolName: step.toolName, arguments: step.arguments });
}

function toolStepDetail(step: ToolStep) {
  if (step.status === "failed") return step.error || "执行失败";
  if (step.status === "stale") return "本轮任务已经结束，但没有收到该工具的完成结果";
  if (step.status === "completed") return summarizeValue(step.output, 100) || "操作已完成";
  const args = step.arguments as Record<string, unknown> | undefined;
  if (step.toolName === "run_command" && args?.program) {
    return [String(args.program), ...(Array.isArray(args.args) ? args.args.map(String) : [])].join(" ");
  }
  if (step.toolName === "apply_edits" && Array.isArray(args?.files)) return `准备修改 ${args.files.length} 个文件`;
  return summarizeValue(step.arguments, 100) || "正在执行";
}

function ToolStepCard({ step }: { step: ToolStep }) {
  const statusLabel = step.status === "running" ? "执行中" : step.status === "completed" ? "已完成" : step.status === "failed" ? "失败" : "已中止";
  return <details className={`tool-step ${step.status}`} open={step.status === "running" || step.status === "failed"}>
    <summary>
      {step.status === "running" ? <RefreshCw size={13} /> : step.status === "completed" ? <CheckCircle2 size={13} /> : <XCircle size={13} />}
      <span><strong>{toolStepTitle(step)}</strong><small>{toolStepDetail(step)}</small></span>
      <i>{statusLabel}</i><ChevronRight size={12} />
    </summary>
    <pre>{summarizeValue(step.status === "completed" ? step.output : step.status === "failed" ? step.error : step.arguments, 1000)}</pre>
  </details>;
}

function Markdown({ children }: { children: string }) {
  return <div className="markdown"><ReactMarkdown remarkPlugins={[remarkGfm]} components={{
    a: ({ href, children: label }) => <a href={href} target="_blank" rel="noreferrer"><ExternalLink size={11} />{label}</a>,
  }}>{children}</ReactMarkdown></div>;
}

function MessageBody({ message, changes, overview, latest }: { message: ChatMessage; changes: GitChange[]; overview?: GitOverview; latest: boolean }) {
  return <>{message.reasoning && <details className="reasoning-card"><summary><Eye size={13} /><span>查看分析与执行思路</span><ChevronRight size={12} /></summary><Markdown>{message.reasoning}</Markdown></details>}
    <Markdown>{message.text}</Markdown>
    {latest && changes.length > 0 && <details className="change-summary" open><summary><FileDiff size={13} /><strong>本次工作区改动</strong><span className="diff-add">+{overview?.additions || 0}</span><span className="diff-remove">-{overview?.deletions || 0}</span><ChevronRight size={12} /></summary>
      <div>{changes.slice(0, 12).map((change) => <span key={`${change.status}:${change.path}`}><i className={change.status.includes("D") ? "removed" : change.status === "??" ? "added" : "modified"}>{change.status.trim() || "M"}</i><FileTypeIcon path={change.path} size={13} /><b>{change.path}</b></span>)}</div>
    </details>}
  </>;
}

const normalizePath = (path: string) => path.replaceAll("\\", "/").replace(/\/+$/, "").toLowerCase();
const normalizeCoreEvent = (event: CoreEvent): CoreEvent => ({
  ...event,
  eventId: event.eventId || event.event_id,
  turnId: event.turnId || event.turn_id,
  toolCallId: event.toolCallId || event.tool_call_id,
  toolName: event.toolName || event.tool_name,
});
const storedSize = (key: string, fallback: number) => {
  const value = Number(localStorage.getItem(key));
  return Number.isFinite(value) && value > 0 ? value : fallback;
};

function IntegratedTerminal({ workspace, visible, dark, onClose }: { workspace: string; visible: boolean; dark: boolean; onClose: () => void }) {
  const host = React.useRef<HTMLDivElement>(null);
  const terminal = React.useRef<Terminal | null>(null);
  const fit = React.useRef<FitAddon | null>(null);

  const start = React.useCallback(async () => {
    const instance = terminal.current;
    if (!instance) return;
    fit.current?.fit();
    try {
      await invoke("terminal_start", { cols: instance.cols, rows: instance.rows });
      instance.focus();
    } catch (error) {
      instance.writeln(`\r\n\x1b[31m${String(error)}\x1b[0m`);
    }
  }, []);

  React.useEffect(() => {
    if (!host.current) return;
    const instance = new Terminal({
      cursorBlink: true, convertEol: true, fontFamily: "Cascadia Mono, Consolas, monospace", fontSize: 12,
      scrollback: 10000, theme: { background: "#17191c", foreground: "#d9dce1", cursor: "#d9dce1", selectionBackground: "#4d78a866" },
    });
    const fitAddon = new FitAddon();
    terminal.current = instance;
    fit.current = fitAddon;
    instance.loadAddon(fitAddon);
    instance.open(host.current);
    const data = instance.onData((value) => { void invoke("terminal_write", { data: value }); });
    const resize = instance.onResize((size) => { void invoke("terminal_resize", size).catch(() => {}); });
    let stopOutput = () => {};
    let stopExit = () => {};
    void Promise.all([
      listen<string>("terminal-output", (event) => instance.write(event.payload)),
      listen("terminal-exit", () => instance.writeln("\r\n\x1b[90m[终端进程已退出]\x1b[0m")),
    ]).then(([output, exit]) => { stopOutput = output; stopExit = exit; return start(); });
    const observer = new ResizeObserver(() => requestAnimationFrame(() => fitAddon.fit()));
    observer.observe(host.current);
    return () => {
      observer.disconnect(); stopOutput(); stopExit(); data.dispose(); resize.dispose(); instance.dispose();
      terminal.current = null; fit.current = null;
    };
  }, [start]);

  React.useEffect(() => {
    if (visible) requestAnimationFrame(() => { fit.current?.fit(); terminal.current?.focus(); });
  }, [visible]);

  React.useEffect(() => {
    if (!terminal.current) return;
    terminal.current.options.theme = dark
      ? { background: "#17191c", foreground: "#d9dce1", cursor: "#d9dce1", selectionBackground: "#4d78a866" }
      : { background: "#f7f7f5", foreground: "#282b30", cursor: "#282b30", selectionBackground: "#3979b933" };
  }, [dark]);

  async function restart() {
    terminal.current?.clear();
    await invoke("terminal_stop").catch(() => {});
    await start();
  }

  return <section className={`terminal-panel ${visible ? "" : "terminal-hidden"}`}><div className="terminal-title"><strong>PowerShell · {workspace}</strong><button onClick={() => terminal.current?.clear()}>清空</button><button onClick={() => void restart()}>重启</button><button title="隐藏终端" onClick={onClose}>×</button></div><div className="terminal-host" ref={host} /></section>;
}

function App() {
  const [themeMode, setThemeMode] = React.useState<ThemeMode>(() => (localStorage.getItem("lan-code-theme") as ThemeMode) || "system");
  const [systemDark, setSystemDark] = React.useState(() => window.matchMedia("(prefers-color-scheme: dark)").matches);
  const [settings, setSettings] = React.useState(DEFAULT_SETTINGS);
  const [draft, setDraft] = React.useState(DEFAULT_SETTINGS);
  const [sessions, setSessions] = React.useState<Session[]>([]);
  const [activeId, setActiveId] = React.useState<string>();
  const [messages, setMessages] = React.useState<ChatMessage[]>([]);
  const [events, setEvents] = React.useState<CoreEvent[]>([]);
  const [approvals, setApprovals] = React.useState<Approval[]>([]);
  const [prompt, setPrompt] = React.useState("");
  const [busy, setBusy] = React.useState(false);
  const [settingsOpen, setSettingsOpen] = React.useState(false);
  const [settingsSection, setSettingsSection] = React.useState<SettingsSection>("appearance");
  const [settingsStatus, setSettingsStatus] = React.useState("");
  const [searchOpen, setSearchOpen] = React.useState(false);
  const [query, setQuery] = React.useState("");
  const [sidebarOpen, setSidebarOpen] = React.useState(true);
  const [fatal, setFatal] = React.useState("");
  const [updateInfo, setUpdateInfo] = React.useState<UpdateInfo>();
  const [updateStatus, setUpdateStatus] = React.useState("");
  const [downloadedUpdate, setDownloadedUpdate] = React.useState("");
  const [workbench, setWorkbench] = React.useState<"agent" | "code">("agent");
  const [workspaceFiles, setWorkspaceFiles] = React.useState<WorkspaceEntry[]>([]);
  const [activeFile, setActiveFile] = React.useState("");
  const [openFiles, setOpenFiles] = React.useState<OpenFile[]>([]);
  const [collapsedDirs, setCollapsedDirs] = React.useState<Set<string>>(new Set());
  const [collapsedProjects, setCollapsedProjects] = React.useState<Set<string>>(new Set());
  const [codeStatus, setCodeStatus] = React.useState("");
  const [diffText, setDiffText] = React.useState("");
  const [gitChanges, setGitChanges] = React.useState<GitChange[]>([]);
  const [gitOverview, setGitOverview] = React.useState<GitOverview>();
  const [selectedGitPath, setSelectedGitPath] = React.useState("");
  const [appMenu, setAppMenu] = React.useState<"file" | "view" | "help">();
  const [terminalOpen, setTerminalOpen] = React.useState(false);
  const [terminalStarted, setTerminalStarted] = React.useState(false);
  const [inspectorOpen, setInspectorOpen] = React.useState(true);
  const [sidebarWidth, setSidebarWidth] = React.useState(() => storedSize("lan-code-sidebar-width", 238));
  const [inspectorWidth, setInspectorWidth] = React.useState(() => storedSize("lan-code-inspector-width", 252));
  const [activityHeight, setActivityHeight] = React.useState(() => storedSize("lan-code-activity-height", 260));
  const [showScrollBottom, setShowScrollBottom] = React.useState(false);
  const [fileContextMenu, setFileContextMenu] = React.useState<FileContextMenu>();
  const editorRef = React.useRef<Parameters<OnMount>[0] | null>(null);
  const completionTimer = React.useRef<number | undefined>(undefined);
  const completionDisposable = React.useRef<monaco.IDisposable | null>(null);
  const conversationRef = React.useRef<HTMLElement | null>(null);
  const activeDocument = openFiles.find((file) => file.path === activeFile);
  const providerReady = Boolean(settings.apiKey) || ["ollama", "lmstudio"].includes(settings.provider);
  const darkTheme = themeMode === "dark" || (themeMode === "system" && systemDark);

  function startResize(panel: "sidebar" | "inspector" | "activity", event: React.PointerEvent) {
    event.preventDefault();
    const startX = event.clientX;
    const startY = event.clientY;
    const startSize = panel === "sidebar" ? sidebarWidth : panel === "inspector" ? inspectorWidth : activityHeight;
    const move = (pointer: PointerEvent) => {
      if (panel === "activity") {
        setActivityHeight(Math.min(480, Math.max(150, startSize + startY - pointer.clientY)));
        return;
      }
      const delta = pointer.clientX - startX;
      const width = panel === "sidebar" ? startSize + delta : startSize - delta;
      if (panel === "sidebar") setSidebarWidth(Math.min(360, Math.max(190, width)));
      else setInspectorWidth(Math.min(440, Math.max(220, width)));
    };
    const stop = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", stop);
      document.body.classList.remove("resizing-panels");
      document.body.classList.remove("resizing-rows");
    };
    document.body.classList.add(panel === "activity" ? "resizing-rows" : "resizing-panels");
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", stop);
  }

  const refreshSessions = React.useCallback(async () => {
    const rows = await invoke<Session[]>("list_sessions");
    setSessions(rows);
  }, []);

  React.useEffect(() => {
    const media = window.matchMedia("(prefers-color-scheme: dark)");
    const change = () => setSystemDark(media.matches);
    media.addEventListener("change", change);
    return () => media.removeEventListener("change", change);
  }, []);

  React.useEffect(() => {
    localStorage.setItem("lan-code-theme", themeMode);
    document.documentElement.dataset.theme = darkTheme ? "dark" : "light";
    document.documentElement.style.colorScheme = darkTheme ? "dark" : "light";
  }, [themeMode, darkTheme]);

  React.useEffect(() => {
    localStorage.setItem("lan-code-sidebar-width", String(sidebarWidth));
    localStorage.setItem("lan-code-inspector-width", String(inspectorWidth));
    localStorage.setItem("lan-code-activity-height", String(activityHeight));
  }, [sidebarWidth, inspectorWidth, activityHeight]);

  React.useEffect(() => {
    const disableWebviewMenu = (event: MouseEvent) => {
      event.preventDefault();
      if (!(event.target as HTMLElement).closest(".file-tree")) setFileContextMenu(undefined);
    };
    const closeMenu = () => setFileContextMenu(undefined);
    window.addEventListener("contextmenu", disableWebviewMenu);
    window.addEventListener("pointerdown", closeMenu);
    window.addEventListener("blur", closeMenu);
    return () => {
      window.removeEventListener("contextmenu", disableWebviewMenu);
      window.removeEventListener("pointerdown", closeMenu);
      window.removeEventListener("blur", closeMenu);
    };
  }, []);

  React.useEffect(() => {
    Promise.all([invoke<SettingsData>("get_settings"), invoke<Session[]>("list_sessions")])
      .then(([loaded, rows]) => {
        setSettings(loaded);
        setDraft(loaded);
        setSessions(rows);
        if (!loaded.apiKey && !["ollama", "lmstudio"].includes(loaded.provider)) {
          setSettingsSection("model");
          setSettingsOpen(true);
        }
      })
      .catch((error) => setFatal(String(error)));
  }, []);

  React.useEffect(() => {
    if (workbench !== "code" || !settings.workspace) return;
    invoke<WorkspaceEntry[]>("list_workspace_files").then(setWorkspaceFiles).catch((error) => setCodeStatus(String(error)));
    invoke<GitChange[]>("workspace_git_changes").then(setGitChanges).catch(() => {});
    invoke<GitOverview>("workspace_git_overview").then(setGitOverview).catch(() => setGitOverview(undefined));
  }, [workbench, settings.workspace]);

  React.useEffect(() => {
    if (!settings.workspace) return;
    Promise.all([
      invoke<GitChange[]>("workspace_git_changes"),
      invoke<GitOverview>("workspace_git_overview"),
    ]).then(([changes, overview]) => {
      setGitChanges(changes);
      setGitOverview(overview);
    }).catch(() => setGitOverview(undefined));
  }, [activeId, busy, settings.workspace]);

  React.useEffect(() => {
    setOpenFiles([]);
    setActiveFile("");
    setCollapsedDirs(new Set());
  }, [settings.workspace]);

  React.useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      if (workbench === "code" && event.ctrlKey && event.key.toLowerCase() === "s") {
        event.preventDefault();
        void saveWorkspaceFile();
      }
      if (workbench === "code" && event.ctrlKey && event.key === "`") {
        event.preventDefault();
        setTerminalStarted(true);
        setTerminalOpen((value) => !value);
      }
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  });

  React.useEffect(() => {
    if (!activeId) return;
    invoke<ModelMessage[]>("session_messages", { sessionId: activeId })
      .then((rows) => setMessages(rows
        .filter((row) => (row.role === "user" || row.role === "assistant") && (row.content || row.reasoning_content))
        .map((row) => ({ role: row.role as "user" | "assistant", text: row.content || "", reasoning: row.reasoning_content }))))
      .catch((error) => setFatal(String(error)));
  }, [activeId]);

  React.useEffect(() => {
    const container = conversationRef.current;
    if (!container || !busy) return;
    const nearBottom = container.scrollHeight - container.scrollTop - container.clientHeight < 180;
    if (nearBottom) container.scrollTo({ top: container.scrollHeight, behavior: "smooth" });
  }, [events, messages, busy]);

  React.useEffect(() => {
    const timer = window.setInterval(() => {
      if (activeId) invoke<CoreEvent[]>("session_events", { sessionId: activeId }).then((rows) => setEvents(rows.map(normalizeCoreEvent))).catch(() => {});
      invoke<Approval[]>("pending_approvals").then(setApprovals).catch(() => {});
      refreshSessions().catch(() => {});
    }, busy ? 120 : 700);
    return () => window.clearInterval(timer);
  }, [activeId, busy, refreshSessions]);

  async function chooseWorkspace() {
    const path = await invoke<string | null>("pick_workspace");
    if (path) setDraft((value) => ({ ...value, workspace: path }));
  }

  async function chooseDataDir() {
    const path = await invoke<string | null>("pick_data_dir");
    if (path) setDraft((value) => ({ ...value, dataDir: path }));
  }

  function applyProvider(provider: string) {
    const preset = PROVIDERS.find((item) => item.id === provider)!;
    setDraft((value) => ({
      ...value, provider, baseUrl: preset.baseUrl || value.baseUrl, model: preset.model || value.model,
      inputPricePerMillion: preset.inputPrice ?? value.inputPricePerMillion,
      outputPricePerMillion: preset.outputPrice ?? value.outputPricePerMillion,
    }));
  }

  async function changeApprovalMode(approvalMode: Mode) {
    if (approvalMode !== "fullAccess") {
      await invoke("terminal_stop").catch(() => {});
      setTerminalOpen(false);
      setTerminalStarted(false);
    }
    const next = { ...settings, approvalMode };
    await invoke("save_settings", { settings: next });
    setSettings(next);
    setDraft(next);
  }

  function toggleProject(project: Project) {
    setCollapsedProjects((items) => {
      const next = new Set(items);
      if (next.has(project.path)) next.delete(project.path); else next.add(project.path);
      return next;
    });
  }

  async function addProject() {
    const path = await invoke<string | null>("pick_workspace");
    if (!path) return;
    const name = path.split(/[\\/]/).filter(Boolean).at(-1) || path;
    const projects = settings.projects.some((item) => item.path === path)
      ? settings.projects
      : [...settings.projects, { name, path }];
    const next = { ...settings, workspace: path, projects };
    await invoke("save_settings", { settings: next });
    setSettings(next);
    setDraft(next);
  }

  async function selectProject(project: Project) {
    await invoke("terminal_stop").catch(() => {});
    setTerminalOpen(false);
    setTerminalStarted(false);
    const next = { ...settings, workspace: project.path };
    await invoke("save_settings", { settings: next });
    setSettings(next);
    setDraft(next);
    setCollapsedProjects((items) => {
      const nextItems = new Set(items);
      nextItems.delete(project.path);
      return nextItems;
    });
    setActiveId(undefined);
    setMessages([]);
  }

  async function removeProject(project: Project) {
    const projects = settings.projects.filter((item) => item.path !== project.path);
    const workspace = settings.workspace === project.path ? (projects[0]?.path || "") : settings.workspace;
    const next = { ...settings, projects, workspace };
    await invoke("save_settings", { settings: next });
    setSettings(next);
    setDraft(next);
  }

  async function removeSession(sessionId: string) {
    if (!window.confirm("确定删除这个对话及其历史记录吗？此操作不可撤销。")) return;
    await invoke("delete_session", { sessionId });
    if (activeId === sessionId) {
      setActiveId(undefined);
      setMessages([]);
      setEvents([]);
    }
    await refreshSessions();
  }

  async function renameSession(session: Session) {
    const title = window.prompt("输入新的对话名称", session.title || "未命名对话")?.trim();
    if (!title) return;
    await invoke("rename_session", { sessionId: session.id, title });
    await refreshSessions();
  }

  function saveProviderProfile() {
    const name = window.prompt("为当前 Provider 配置命名", `${draft.provider} / ${draft.model}`)?.trim();
    if (!name) return;
    const profile: ProviderProfile = {
      id: crypto.randomUUID(), name, provider: draft.provider, baseUrl: draft.baseUrl, model: draft.model,
      apiKey: draft.apiKey, inputPricePerMillion: draft.inputPricePerMillion,
      outputPricePerMillion: draft.outputPricePerMillion,
    };
    setDraft({ ...draft, providerProfiles: [...draft.providerProfiles, profile] });
    setSettingsStatus(`已加入配置档案“${name}”，点击保存并启用后持久化。`);
  }

  function applyProviderProfile(profile: ProviderProfile) {
    setDraft({
      ...draft, provider: profile.provider, baseUrl: profile.baseUrl, model: profile.model,
      apiKey: profile.apiKey, inputPricePerMillion: profile.inputPricePerMillion,
      outputPricePerMillion: profile.outputPricePerMillion,
    });
    setSettingsStatus(`已载入配置档案“${profile.name}”。`);
  }

  function removeProviderProfile(profile: ProviderProfile) {
    setDraft({ ...draft, providerProfiles: draft.providerProfiles.filter((item) => item.id !== profile.id) });
  }

  async function saveSettings() {
    setSettingsStatus("正在保存...");
    try {
      await invoke("save_settings", { settings: draft });
      if (draft.approvalMode !== "fullAccess" || draft.workspace !== settings.workspace) {
        await invoke("terminal_stop").catch(() => {});
        setTerminalOpen(false);
        setTerminalStarted(false);
      }
      setSettings(draft);
      setSettingsOpen(false);
      setSettingsStatus("");
      setActiveId(undefined);
      setMessages([]);
      await refreshSessions();
    } catch (error) {
      setSettingsStatus(`保存失败：${String(error)}`);
    }
  }

  async function testProvider() {
    setSettingsStatus("正在测试文本响应和工具调用能力...");
    try {
      const result = await invoke<{ model: string; latencyMs: number; textResponse: string; toolCallSupported: boolean; capabilities: ModelCapabilities }>("test_provider", { settings: draft });
      setDraft((value) => ({ ...value, modelCapabilities: result.capabilities }));
      setSettingsStatus(`连接成功，延迟 ${result.latencyMs}ms，工具调用${result.toolCallSupported ? "可用" : "未通过"}：${result.textResponse || result.model}`);
    } catch (error) {
      setSettingsStatus(`连接失败：${String(error)}`);
    }
  }

  async function openWorkspaceFile(path: string, line?: number, pin = false) {
    const existing = openFiles.find((file) => file.path === path);
    if (existing) {
      if (pin && !existing.pinned) setOpenFiles((items) => items.map((file) => file.path === path ? { ...file, pinned: true } : file));
      setActiveFile(path);
      if (line) window.setTimeout(() => {
        editorRef.current?.revealLineInCenter(line);
        editorRef.current?.setPosition({ lineNumber: line, column: 1 });
      });
      return;
    }
    try {
      const file = await invoke<{ path: string; content: string }>("read_workspace_file", { path });
      setActiveFile(file.path);
      setOpenFiles((items) => {
        const preview = items.find((item) => !item.pinned && item.content === item.savedContent);
        const next = preview ? items.filter((item) => item.path !== preview.path) : items;
        return [...next, { path: file.path, content: file.content, savedContent: file.content, pinned: pin }];
      });
      setCodeStatus("");
      if (line) window.setTimeout(() => {
        editorRef.current?.revealLineInCenter(line);
        editorRef.current?.setPosition({ lineNumber: line, column: 1 });
      }, 50);
    } catch (error) {
      setCodeStatus(`打开失败：${String(error)}`);
    }
  }

  async function saveWorkspaceFile() {
    if (!activeDocument) return;
    await invoke("write_workspace_file", { path: activeFile, content: activeDocument.content });
    setOpenFiles((items) => items.map((file) => file.path === activeFile ? { ...file, savedContent: file.content } : file));
    setCodeStatus(`已保存 ${activeFile}`);
    await refreshGitChanges();
  }

  function closeWorkspaceFile(path: string) {
    const target = openFiles.find((file) => file.path === path);
    if (target?.content !== target?.savedContent && !window.confirm(`${path} 有未保存修改，确定关闭吗？`)) return;
    const index = openFiles.findIndex((file) => file.path === path);
    const remaining = openFiles.filter((file) => file.path !== path);
    setOpenFiles(remaining);
    if (activeFile === path) setActiveFile(remaining[Math.min(index, remaining.length - 1)]?.path || "");
  }

  async function refreshWorkspaceFiles() {
    setWorkspaceFiles(await invoke<WorkspaceEntry[]>("list_workspace_files"));
  }

  async function refreshGitChanges() {
    const [changes, overview] = await Promise.all([
      invoke<GitChange[]>("workspace_git_changes"),
      invoke<GitOverview>("workspace_git_overview"),
    ]);
    setGitChanges(changes);
    setGitOverview(overview);
    if (selectedGitPath && !changes.some((change) => change.path === selectedGitPath)) {
      setSelectedGitPath("");
      setDiffText("");
    }
  }

  async function openGitChange(path: string) {
    setSelectedGitPath(path);
    try {
      const diff = await invoke<string>("workspace_file_diff", { path });
      setDiffText(diff || "该文件没有未暂存 diff，可能是未跟踪文件或只有暂存区改动。");
    } catch (error) {
      setDiffText(`加载 diff 失败：${String(error)}`);
    }
  }

  async function discardGitChange(change: GitChange) {
    if (!window.confirm(`确定撤销 ${change.path} 的全部未暂存改动吗？此操作不可撤销。`)) return;
    try {
      await invoke("discard_workspace_changes", { path: change.path });
      const open = openFiles.find((file) => file.path === change.path);
      if (open) {
        const current = await invoke<{ path: string; content: string }>("read_workspace_file", { path: change.path });
        setOpenFiles((items) => items.map((file) => file.path === change.path ? { ...file, content: current.content, savedContent: current.content } : file));
      }
      await refreshGitChanges();
      setCodeStatus(`已撤销 ${change.path} 的未暂存改动`);
    } catch (error) {
      setCodeStatus(`撤销失败：${String(error)}`);
    }
  }

  async function createWorkspaceEntry(isDir: boolean) {
    const path = window.prompt(isDir ? "输入新文件夹相对路径" : "输入新文件相对路径")?.trim();
    if (!path) return;
    try {
      await invoke("create_workspace_entry", { path, isDir });
      await refreshWorkspaceFiles();
      if (!isDir) await openWorkspaceFile(path, undefined, true);
      setCodeStatus(`已创建 ${path}`);
    } catch (error) {
      setCodeStatus(`创建失败：${String(error)}`);
    }
  }

  async function renameWorkspaceEntry() {
    if (!activeFile) return;
    const newPath = window.prompt("输入新的相对路径", activeFile)?.trim();
    if (!newPath || newPath === activeFile) return;
    try {
      await invoke("rename_workspace_entry", { path: activeFile, newPath });
      setOpenFiles((items) => items.map((file) => file.path === activeFile ? { ...file, path: newPath } : file));
      setActiveFile(newPath);
      await refreshWorkspaceFiles();
      setCodeStatus(`已重命名为 ${newPath}`);
    } catch (error) {
      setCodeStatus(`重命名失败：${String(error)}`);
    }
  }

  async function deleteWorkspaceEntry() {
    if (!activeFile || !window.confirm(`确定永久删除 ${activeFile} 吗？`)) return;
    try {
      await invoke("delete_workspace_entry", { path: activeFile });
      const path = activeFile;
      const index = openFiles.findIndex((file) => file.path === path);
      const remaining = openFiles.filter((file) => file.path !== path);
      setOpenFiles(remaining);
      setActiveFile(remaining[Math.min(index, remaining.length - 1)]?.path || "");
      await refreshWorkspaceFiles();
      setCodeStatus(`已删除 ${path}`);
    } catch (error) {
      setCodeStatus(`删除失败：${String(error)}`);
    }
  }

  async function checkUpdates() {
    setUpdateStatus("正在检查 GitHub Release...");
    try {
      const info = await invoke<UpdateInfo>("check_for_updates");
      setUpdateInfo(info);
      setUpdateStatus(info.available ? `发现新版本 v${info.latestVersion}` : `当前已是最新版本 v${info.currentVersion}`);
    } catch (error) {
      setUpdateStatus(`检查失败：${String(error)}`);
    }
  }

  async function downloadUpdate() {
    if (!updateInfo?.installerUrl || !updateInfo.installerName) return;
    setUpdateStatus("正在下载最新安装包，请稍候...");
    try {
      const path = await invoke<string>("download_update", {
        installerUrl: updateInfo.installerUrl, installerName: updateInfo.installerName,
      });
      setDownloadedUpdate(path);
      setUpdateStatus("更新包已下载，点击“退出并安装”完成更新。");
    } catch (error) {
      setUpdateStatus(`下载失败：${String(error)}`);
    }
  }

  async function installUpdate() {
    if (!downloadedUpdate || !window.confirm("Lan Code 将退出并启动安装程序，是否继续？")) return;
    await invoke("install_downloaded_update", { path: downloadedUpdate });
  }

  async function understandImage() {
    const question = window.prompt("选择图片后，希望模型分析什么？", "请描述图片内容，并指出其中值得注意的信息。");
    if (question === null) return;
    setBusy(true);
    try {
      const result = await invoke<string>("analyze_image", { prompt: question });
      setMessages((items) => [...items, { role: "user", text: `分析图片：${question}` }, { role: "assistant", text: result }]);
    } catch (error) {
      setMessages((items) => [...items, { role: "assistant", text: `图片理解失败：${String(error)}` }]);
    } finally {
      setBusy(false);
    }
  }

  async function createImage() {
    const description = window.prompt("描述要生成的图片");
    if (!description?.trim()) return;
    setBusy(true);
    try {
      const path = await invoke<string>("generate_image", { prompt: description });
      setMessages((items) => [...items, { role: "user", text: `生成图片：${description}` }, { role: "assistant", text: `图片已生成并保存到：${path}` }]);
    } catch (error) {
      setMessages((items) => [...items, { role: "assistant", text: `图片生成失败：${String(error)}` }]);
    } finally {
      setBusy(false);
    }
  }

  async function newSession(title = "新对话") {
    if (!providerReady) {
      setSettingsSection("model");
      setSettingsOpen(true);
      setSettingsStatus("请先配置并测试 API");
      return undefined;
    }
    const session = await invoke<Session>("create_session", { cwd: settings.workspace, title });
    await refreshSessions();
    setActiveId(session.id);
    setMessages([]);
    setEvents([]);
    return session.id;
  }

  async function send() {
    const text = prompt.trim();
    if (!text || busy) return;
    try {
      let sessionId = activeId;
      if (!sessionId) sessionId = await newSession(text.slice(0, 32));
      if (!sessionId) return;
      setMessages((items) => [...items, { role: "user", text }]);
      setPrompt("");
      setBusy(true);
      const result = await invoke<{ text: string }>("start_turn", {
        sessionId, prompt: text, mode: settings.approvalMode,
      });
      setMessages((items) => [...items, { role: "assistant", text: result.text }]);
    } catch (error) {
      setMessages((items) => [...items, { role: "assistant", text: `执行失败：${String(error)}` }]);
    } finally {
      setBusy(false);
      await refreshSessions();
    }
  }

  async function interrupt() {
    if (activeId) await invoke("interrupt_turn", { sessionId: activeId });
  }

  async function decide(requestId: string, decision: "allowOnce" | "deny") {
    await invoke("resolve_approval", { requestId, decision });
    setApprovals((rows) => rows.filter((row) => row.id !== requestId));
  }

  const filtered = sessions.filter((row) =>
    !query || `${row.title || ""} ${row.cwd}`.toLowerCase().includes(query.toLowerCase()));
  let latestTurnStart = -1;
  for (let index = events.length - 1; index >= 0; index -= 1) {
    if (events[index].type === "turnStarted") {
      latestTurnStart = index;
      break;
    }
  }
  const activeTurnEvents = latestTurnStart >= 0 ? events.slice(latestTurnStart) : [];
  const streamingText = busy ? activeTurnEvents.filter((event) => event.type === "textDelta").map((event) => event.text || "").join("") : "";
  const toolStepSource = busy ? activeTurnEvents : events;
  const toolSteps = toolStepSource.reduce((steps, event) => {
    if (!["toolStarted", "toolCompleted", "toolFailed"].includes(event.type)) return steps;
    let index = event.toolCallId ? steps.findIndex((step) => step.id === event.toolCallId) : -1;
    if (index < 0 && event.type !== "toolStarted") {
      for (let candidate = steps.length - 1; candidate >= 0; candidate -= 1) {
        if (steps[candidate].status === "running" && steps[candidate].toolName === (event.toolName || "")) {
          index = candidate;
          break;
        }
      }
    }
    if (event.type === "toolStarted" || index < 0) {
      steps.push({
        id: event.toolCallId || event.eventId || `tool-${steps.length}`,
        toolName: event.toolName || "",
        status: event.type === "toolFailed" ? "failed" : event.type === "toolCompleted" ? "completed" : "running",
        arguments: event.arguments, output: event.output, error: event.error,
      });
      return steps;
    }
    steps[index] = {
      ...steps[index],
      status: event.type === "toolCompleted" ? "completed" : "failed",
      output: event.output ?? steps[index].output,
      error: event.error ?? steps[index].error,
    };
    return steps;
  }, [] as ToolStep[]).map((step) => step.status === "running" && !busy ? { ...step, status: "stale" as const } : step).slice(-12).reverse();
  const toolStepStats = toolSteps.reduce((stats, step) => {
    stats[step.status] += 1;
    return stats;
  }, { running: 0, completed: 0, failed: 0, stale: 0 });
  const usage = events.filter((event) => event.type === "usageRecorded" && event.usage)
    .reduce((total, event) => ({
      inputTokens: total.inputTokens + event.usage!.inputTokens,
      outputTokens: total.outputTokens + event.usage!.outputTokens,
      totalTokens: total.totalTokens + event.usage!.totalTokens,
      cachedInputTokens: total.cachedInputTokens + event.usage!.cachedInputTokens,
    }), { inputTokens: 0, outputTokens: 0, totalTokens: 0, cachedInputTokens: 0 });
  const estimatedCost = usage.inputTokens / 1_000_000 * settings.inputPricePerMillion
    + usage.outputTokens / 1_000_000 * settings.outputPricePerMillion;
  const visibleWorkspaceFiles = workspaceFiles.filter((entry) => {
    const normalized = entry.path.replaceAll("\\", "/");
    return !Array.from(collapsedDirs).some((dir) => normalized.startsWith(`${dir.replaceAll("\\", "/")}/`));
  });
  const projectSessions = (project: Project) => filtered.filter((session) => normalizePath(session.cwd) === normalizePath(project.path));
  const orphanSessions = filtered.filter((session) => !settings.projects.some((project) => normalizePath(session.cwd) === normalizePath(project.path)));
  const renderSession = (session: Session) => (
    <div className="session-row nested-session" key={session.id}>
      <button title="双击可重命名" className={session.id === activeId ? "active" : ""} onClick={() => setActiveId(session.id)} onDoubleClick={() => void renameSession(session)}>
        <MessageSquare className="session-icon" size={15} /><span>{session.title || "未命名对话"}</span>
        {session.status === "running" ? <i title="运行中" className="session-state running" /> : session.status === "failed" ? <span className="session-failed" title="执行失败"><XCircle className="session-state failed" size={14} /></span> : session.status === "waitingForApproval" ? <i title="等待批准" className="session-state waiting" /> : null}
      </button>
      <button title="删除对话" className="row-action" onClick={() => void removeSession(session.id)}><Trash2 size={13} /></button>
    </div>
  );

  return (
    <div className={`app-shell ${sidebarOpen ? "" : "sidebar-collapsed"} ${inspectorOpen ? "" : "inspector-collapsed"}`} style={{ "--sidebar-width": `${sidebarWidth}px`, "--inspector-width": `${inspectorWidth}px` } as React.CSSProperties}>
      <div className="app-menu-bar">
        <button className="app-menu-logo" title="Lan Code"><img src="/lan-code-logo.png" alt="" /></button>
        {(["file", "view", "help"] as const).map((menu) => <div className="app-menu-host" key={menu}>
          <button className={appMenu === menu ? "active" : ""} onClick={() => setAppMenu(appMenu === menu ? undefined : menu)}>{menu === "file" ? "文件" : menu === "view" ? "视图" : "帮助"}</button>
          {appMenu === menu && <div className="app-menu-popover">
            {menu === "file" && <><button onClick={() => { setAppMenu(undefined); void newSession(); }}><MessageSquare size={14} />新建对话<kbd>Ctrl+N</kbd></button><button onClick={() => { setAppMenu(undefined); void addProject(); }}><FolderPlus size={14} />添加项目</button><i /><button onClick={() => { setAppMenu(undefined); setDraft(settings); setSettingsSection("appearance"); setSettingsOpen(true); }}><Settings size={14} />设置</button></>}
            {menu === "view" && <><button onClick={() => { setAppMenu(undefined); setWorkbench("agent"); }}><MessageSquare size={14} />Agent 工作台</button><button onClick={() => { setAppMenu(undefined); setWorkbench("code"); }}><Code2 size={14} />Code 工作台</button><i /><button onClick={() => { setAppMenu(undefined); setSidebarOpen((value) => !value); }}><PanelLeftOpen size={14} />切换侧栏</button><button onClick={() => { setAppMenu(undefined); setInspectorOpen((value) => !value); }}><Eye size={14} />切换观察面板</button></>}
            {menu === "help" && <><button onClick={() => { setAppMenu(undefined); window.open("https://github.com/zhaoxinyi02/lan-code", "_blank"); }}><ExternalLink size={14} />GitHub 仓库</button><button onClick={() => { setAppMenu(undefined); setDraft(settings); setSettingsSection("updates"); setSettingsOpen(true); }}><Download size={14} />检查更新</button></>}
          </div>}
        </div>)}
      </div>
      {sidebarOpen && <aside className="sidebar" style={{ width: sidebarWidth }}>
        <div className="brand"><img src="/lan-code-logo.png" alt="Lan Code" /><strong>Lan Code</strong>
          <button title="收起侧栏" className="icon-button" onClick={() => setSidebarOpen(false)}><PanelLeftClose size={17} /></button>
        </div>
        <div className="mode-switch"><button className={workbench === "agent" ? "active" : ""} onClick={() => setWorkbench("agent")}><MessageSquare size={14} /> Agent</button><button className={workbench === "code" ? "active" : ""} onClick={() => setWorkbench("code")}><Code2 size={14} /> Code</button></div>
        {workbench === "code" ? <>
          <div className="code-side-controls">
            <Dropdown value={settings.workspace} title="切换当前项目" icon={<GitBranch size={15} />} options={settings.projects.map((project) => ({ id: project.path, label: project.name }))} onChange={(path) => { const project = settings.projects.find((item) => item.path === path); if (project) void selectProject(project); }} />
          </div>
          <div className="section-label code-label"><span>项目</span><div>
            <button title="新建文件" onClick={() => void createWorkspaceEntry(false)}><FilePlus size={13} /></button>
            <button title="新建文件夹" onClick={() => void createWorkspaceEntry(true)}><FolderPlus size={13} /></button>
            <button title="刷新" onClick={() => void refreshWorkspaceFiles()}><RefreshCw size={13} /></button>
          </div></div>
          <div className="project-tree">{settings.projects.map((project) => <div className="project-group" key={project.path}>
            <button className={`project-heading ${project.path === settings.workspace ? "active" : ""}`} title={project.path} onClick={() => { if (project.path !== settings.workspace) void selectProject(project); else toggleProject(project); }}>
              {collapsedProjects.has(project.path) ? <ChevronRight size={14} /> : <ChevronDown size={14} />}<FolderGit2 size={14} /><span>{project.name}</span>
            </button>
            {project.path === settings.workspace && !collapsedProjects.has(project.path) && <>
              <div className="file-tree">{visibleWorkspaceFiles.map((entry) => <button key={entry.path} title={entry.path} style={{ paddingLeft: `${16 + entry.depth * 11}px` }} className={activeFile === entry.path ? "active" : ""} onContextMenu={(event) => {
            event.preventDefault();
            event.stopPropagation();
            setActiveFile(entry.path);
            setFileContextMenu({ x: event.clientX, y: event.clientY, path: entry.path, isDir: entry.isDir });
          }} onClick={() => {
            if (entry.isDir) setCollapsedDirs((items) => {
              const next = new Set(items);
              if (next.has(entry.path)) next.delete(entry.path); else next.add(entry.path);
              return next;
            });
            else void openWorkspaceFile(entry.path);
          }} onDoubleClick={() => { if (!entry.isDir) void openWorkspaceFile(entry.path, undefined, true); }}>{entry.isDir ? collapsedDirs.has(entry.path) ? <ChevronRight size={13} /> : <ChevronDown size={13} /> : <span className="tree-spacer" />} {entry.isDir ? collapsedDirs.has(entry.path) ? <Folder size={14} /> : <FolderOpen size={14} /> : <FileTypeIcon path={entry.path} light={!darkTheme} />}<span>{entry.name}</span></button>)}</div>
            </>}
          </div>)}</div>
          <div className="horizontal-resizer" title="拖动调整区域高度，双击恢复默认" onPointerDown={(event) => startResize("activity", event)} onDoubleClick={() => setActivityHeight(260)} />
          <div className="code-left-panels" style={{ height: activityHeight }}>
            <section><div className="git-heading"><h3>Agent 执行过程</h3></div>
              <div className="code-tool-timeline">{toolSteps.length === 0 ? <div className="empty-small">工具调用会显示在这里。</div> : toolSteps.slice(0, 6).map((step) => <ToolStepCard key={step.id} step={step} />)}</div>
            </section>
            <section><div className="git-heading"><h3>Git 改动</h3><button title="刷新" onClick={() => void refreshGitChanges()}><RefreshCw size={12} /></button></div>
              <div className="git-changes">{gitChanges.length === 0 ? <div className="empty-small">暂无已加载改动。</div> : gitChanges.map((change) => <div className={selectedGitPath === change.path ? "active" : ""} key={`${change.status}:${change.path}`}><button title={change.path} onClick={() => void openGitChange(change.path)}><i>{change.status}</i><span>{change.path}</span></button><button title="撤销未暂存改动" disabled={change.status === "??"} onClick={() => void discardGitChange(change)}><RotateCcw size={11} /></button></div>)}</div>
              {selectedGitPath && <pre className="diff-preview">{diffText}</pre>}
            </section>
          </div>
          <button className="new-chat add-project-code" onClick={() => void addProject()}><FolderPlus size={14} /> 添加项目</button>
          <button className="settings-button" onClick={() => { setDraft(settings); setSettingsSection("appearance"); setSettingsOpen(true); }}><Settings size={16} /> 设置</button>
        </> : <>
        <button className="new-chat" onClick={() => void newSession()}><Plus size={16} /> 新对话</button>
        <nav>
          <button className={searchOpen ? "active" : ""} onClick={() => setSearchOpen(!searchOpen)}><Search size={16} /> 搜索</button>
          <button onClick={() => void addProject()}><FolderPlus size={16} /> 添加项目</button>
        </nav>
        {searchOpen && <input className="session-search" autoFocus placeholder="搜索会话或路径" value={query} onChange={(e) => setQuery(e.target.value)} />}
        <div className="section-label">项目</div>
        <div className="sessions project-conversations">{settings.projects.map((project) => (
          <div className="project-group" key={project.path}>
            <div className="session-row">
              <button className={project.path === settings.workspace ? "active project-heading" : "project-heading"} title={project.path} onClick={() => { if (project.path !== settings.workspace) void selectProject(project); toggleProject(project); }}>
                {collapsedProjects.has(project.path) ? <ChevronRight size={14} /> : <ChevronDown size={14} />}<FolderGit2 size={14} /><span>{project.name}</span>
              </button>
              <button title="移除项目" className="row-action" onClick={() => void removeProject(project)}><Trash2 size={13} /></button>
            </div>
            {!collapsedProjects.has(project.path) && (projectSessions(project).length ? projectSessions(project).map(renderSession) : <div className="empty-project">暂无对话</div>)}
          </div>
        ))}{orphanSessions.length > 0 && <div className="project-group"><div className="orphan-heading">未归档对话</div>{orphanSessions.map(renderSession)}</div>}</div>
        <button className="settings-button" onClick={() => { setDraft(settings); setSettingsSection("appearance"); setSettingsOpen(true); }}><Settings size={16} /> 设置</button>
        </>}
        <div className="panel-resizer sidebar-resizer" title="拖动调整侧栏宽度，双击恢复默认" onPointerDown={(event) => startResize("sidebar", event)} onDoubleClick={() => setSidebarWidth(238)} />
      </aside>}

      {workbench === "agent" ? <main className="mode-panel mode-agent">
        <header>
          <div className="title-row">
            {!sidebarOpen && <button title="展开侧栏" className="icon-button" onClick={() => setSidebarOpen(true)}><PanelLeftOpen size={17} /></button>}
            <div><h1>{sessions.find((item) => item.id === activeId)?.title || "开始新的编码任务"}</h1><span className="subtle">{settings.workspace || "尚未选择工作区"}</span></div>
          </div>
          <div className="header-actions">
            <Dropdown value={settings.workspace} title="切换当前项目" icon={<GitBranch size={15} />} options={settings.projects.map((project) => ({ id: project.path, label: project.name }))} onChange={(path) => { const project = settings.projects.find((item) => item.path === path); if (project) void selectProject(project); }} />
            <Dropdown value={settings.approvalMode} title="切换 Agent 权限" icon={<ShieldCheck size={15} />} options={APPROVAL_MODES.map((mode) => ({ id: mode.id, label: mode.label }))} onChange={(mode) => void changeApprovalMode(mode)} />
          </div>
        </header>
        {fatal && <div className="error-banner"><XCircle size={16} />{fatal}<button onClick={() => setFatal("")}>关闭</button></div>}
        <section className="conversation" ref={conversationRef} onScroll={(event) => {
          const target = event.currentTarget;
          setShowScrollBottom(target.scrollHeight - target.scrollTop - target.clientHeight > 240);
        }}>
          {messages.length === 0 ? <div className="welcome">
            <img src="/lan-code-logo.png" alt="" /><h2>{providerReady ? "把想法交给 Lan Code" : "先完成首次配置"}</h2>
            <p>{providerReady ? "它会先理解仓库，再修改代码、运行检查并审阅差异。" : "配置 API Key 和工作区后即可开始真实编码任务。"}</p>
            <div className="suggestions">
              {!providerReady && <button onClick={() => { setSettingsSection("model"); setSettingsOpen(true); }}><KeyRound size={18} /> 配置 API</button>}
              <button onClick={() => setPrompt("阅读当前项目并解释架构")}><Bot size={18} /> 理解项目</button>
              <button onClick={() => setPrompt("检查当前 Git diff 并指出风险")}><FileDiff size={18} /> 审阅改动</button>
              <button onClick={() => setPrompt("运行测试并修复失败项")}><CheckCircle2 size={18} /> 修复测试</button>
              <button onClick={() => void understandImage()}><Image size={18} /> 图片理解</button>
              <button onClick={() => void createImage()}><Sparkles size={18} /> 生成图片</button>
            </div>
          </div> : <div className="messages">
            {messages.map((message, index) => <article key={index} className={message.role}><div className="message-label">{message.role === "user" ? "你" : "Lan Code"}</div>{message.role === "assistant" ? <MessageBody message={message} changes={gitChanges} overview={gitOverview} latest={index === messages.length - 1} /> : <p>{message.text}</p>}</article>)}
            {busy && <article className="assistant streaming-answer"><div className="message-label">Lan Code</div>{streamingText ? <Markdown>{streamingText}</Markdown> : <div className="thinking"><Sparkles size={16} /> 正在分析项目并准备下一步...</div>}</article>}
          </div>}
        </section>
        {showScrollBottom && <button className="scroll-bottom" title="回到最新消息" onClick={() => conversationRef.current?.scrollTo({ top: conversationRef.current.scrollHeight, behavior: "smooth" })}><ChevronDown size={15} /> 最新消息</button>}
        <div className="composer-wrap"><div className="composer">
          <textarea value={prompt} onChange={(e) => setPrompt(e.target.value)} onKeyDown={(e) => { if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); void send(); } }} placeholder={providerReady ? "描述你想完成的编码任务" : "请先在设置中配置 API"} />
          <div className="composer-footer"><div>
            <button title="添加项目" className="mini" onClick={() => void addProject()}><Plus size={15} /></button>
            <button className="mini model-chip" onClick={() => { setDraft(settings); setSettingsSection("model"); setSettingsOpen(true); }}><TerminalSquare size={15} /><span>{settings.model}</span></button>
          </div>{busy ? <button className="send stop" onClick={interrupt}><CircleStop size={17} /></button> : <button className="send" disabled={!providerReady} onClick={send}><Send size={17} /></button>}</div>
        </div></div>
      </main> : <main className="code-main mode-panel mode-code">
        <header>
          <div className="title-row">{!sidebarOpen && <button title="展开侧栏" className="icon-button" onClick={() => setSidebarOpen(true)}><PanelLeftOpen size={17} /></button>}<div><h1>{activeFile || "Code 工作台"}</h1><span className="subtle">{activeDocument && activeDocument.content !== activeDocument.savedContent ? "有未保存修改" : settings.workspace}</span></div></div>
          <div className="header-actions"><span className="completion-hint" title="输入代码时自动生成建议，按 Tab 接受"><Sparkles size={14} /> 自动补全 · Tab 接受</span><button className="pill" onClick={() => void renameWorkspaceEntry()} disabled={!activeFile}><Pencil size={15} /> 重命名</button><button className="pill danger" onClick={() => void deleteWorkspaceEntry()} disabled={!activeFile}><Trash2 size={15} /> 删除</button><button className="pill" onClick={() => void saveWorkspaceFile()} disabled={!activeDocument || activeDocument.content === activeDocument.savedContent}><Save size={15} /> 保存</button><button className="pill" onClick={() => void refreshGitChanges()}><FileDiff size={15} /> 查看改动</button><button className="pill" onClick={() => { setTerminalStarted(true); setTerminalOpen(!terminalOpen); }}><TerminalSquare size={15} /> 终端</button><button className="pill" onClick={() => setInspectorOpen((value) => !value)}><PanelLeftOpen size={15} /> {inspectorOpen ? "隐藏助手" : "显示助手"}</button></div>
        </header>
        {codeStatus && <div className="code-status">{codeStatus}</div>}
        <div className="editor-tabs">{openFiles.length ? openFiles.map((file) => <button key={file.path} title={file.path} className={`${file.path === activeFile ? "active" : ""} ${file.pinned ? "" : "preview"}`} onClick={() => setActiveFile(file.path)} onDoubleClick={() => setOpenFiles((items) => items.map((item) => item.path === file.path ? { ...item, pinned: true } : item))}><FileTypeIcon path={file.path} size={14} light={!darkTheme} /><span>{file.path.split(/[\\/]/).at(-1)}</span>{file.content !== file.savedContent && <i />}<b title="关闭" onClick={(event) => { event.stopPropagation(); closeWorkspaceFile(file.path); }}>×</b></button>) : <span>单击预览文件，双击固定标签</span>}</div>
        <div className="editor-host">{activeDocument ? <Editor
          path={activeFile}
          value={activeDocument.content}
          onChange={(value) => setOpenFiles((items) => items.map((file) => file.path === activeFile ? { ...file, content: value || "", pinned: true } : file))}
          onMount={(editor) => {
            editorRef.current = editor;
            editor.focus();
            completionDisposable.current?.dispose();
            completionDisposable.current = monaco.languages.registerInlineCompletionsProvider("*", {
              provideInlineCompletions: async (model, position, _context, token) => {
                window.clearTimeout(completionTimer.current);
                await new Promise<void>((resolve) => {
                  completionTimer.current = window.setTimeout(resolve, 450);
                });
                if (token.isCancellationRequested || !providerReady || model.getLineContent(position.lineNumber).trim().length < 2) return { items: [] };
                const offset = model.getOffsetAt(position);
                const value = model.getValue();
                try {
                  const completion = await invoke<string>("inline_completion", {
                    path: activeFile, prefix: value.slice(0, offset), suffix: value.slice(offset),
                  });
                  if (token.isCancellationRequested || !completion.trim()) return { items: [] };
                  return { items: [{ insertText: completion, range: new monaco.Range(position.lineNumber, position.column, position.lineNumber, position.column) }] };
                } catch {
                  return { items: [] };
                }
              },
              disposeInlineCompletions: () => {},
            });
          }}
          theme={darkTheme ? "vs-dark" : "vs"}
          options={{ automaticLayout: true, minimap: { enabled: true }, fontSize: 12, tabSize: 2, wordWrap: "off", quickSuggestions: true, inlineSuggest: { enabled: true, mode: "subwordSmart" } }}
        /> : <div className="code-welcome"><Code2 size={46} /><h2>Lan Code 工作台</h2><p>浏览项目、编辑代码、查看改动，并让同一个 Agent 理解当前仓库。</p></div>}</div>
        {terminalStarted && <IntegratedTerminal workspace={settings.workspace} visible={terminalOpen} dark={darkTheme} onClose={() => setTerminalOpen(false)} />}
      </main>}

      {inspectorOpen && <aside className={`inspector ${workbench === "code" ? "code-inspector" : ""}`} style={{ width: inspectorWidth }}>
        <div className="panel-resizer inspector-resizer" title="拖动调整助手宽度，双击恢复默认" onPointerDown={(event) => startResize("inspector", event)} onDoubleClick={() => setInspectorWidth(252)} />
        {workbench === "code" ? <>
          <h3>AI 助手</h3>
          <div className="code-chat">{messages.length === 0 ? <div className="empty-small">针对当前项目提问，Agent 会使用同一套工具、权限和会话。</div> : messages.slice(-8).map((message, index) => <article key={index} className={message.role}><div className="message-label">{message.role === "user" ? "你" : "Lan Code"}</div>{message.role === "assistant" ? <Markdown>{message.text}</Markdown> : <p>{message.text}</p>}</article>)}{busy && <article className="assistant streaming-answer"><div className="message-label">Lan Code</div>{streamingText ? <Markdown>{streamingText}</Markdown> : <div className="thinking"><Sparkles size={14} /> 正在工作...</div>}</article>}</div>
          <div className="code-chat-composer"><textarea value={prompt} onChange={(event) => setPrompt(event.target.value)} onKeyDown={(event) => { if (event.key === "Enter" && !event.shiftKey) { event.preventDefault(); void send(); } }} placeholder="询问代码或要求修改项目" /><div className="code-composer-footer"><Dropdown value={settings.approvalMode} title="切换 Agent 权限" icon={<ShieldCheck size={13} />} options={APPROVAL_MODES.map((mode) => ({ id: mode.id, label: mode.label }))} onChange={(mode) => void changeApprovalMode(mode)} /><button onClick={() => void send()} disabled={busy || !prompt.trim()}><Send size={14} /></button></div></div>
        </> : <>
        <h3>环境信息</h3>
        <div className="info-row"><FolderGit2 size={16} /><span>项目</span><strong>{settings.projects.find((item) => item.path === settings.workspace)?.name || "未配置"}</strong></div>
        <div className="info-row"><KeyRound size={16} /><span>模型</span><strong>{providerReady ? settings.model : "未配置"}</strong></div>
        <div className="info-row"><ShieldCheck size={16} /><span>权限</span><strong>{APPROVAL_MODES.find((item) => item.id === settings.approvalMode)?.label}</strong></div>
        <div className="divider" /><h3>当前对话用量</h3>
        <div className="usage-grid"><span>输入 Token<strong>{usage.inputTokens.toLocaleString()}</strong></span><span>输出 Token<strong>{usage.outputTokens.toLocaleString()}</strong></span><span>预估费用<strong>${estimatedCost.toFixed(4)}</strong></span></div>
        <div className="divider" /><h3>任务步骤</h3>
        {toolSteps.length > 0 && <div className="tool-stats"><span>{toolSteps.length} 步</span><b>{toolStepStats.completed} 完成</b>{toolStepStats.running > 0 && <i>{toolStepStats.running} 执行中</i>}{toolStepStats.failed + toolStepStats.stale > 0 && <em>{toolStepStats.failed + toolStepStats.stale} 异常</em>}</div>}
        {toolSteps.length === 0 ? <div className="empty-small">发送任务后在这里查看文件读取、搜索、修改和命令执行过程。</div> : <div className="tool-step-list">{toolSteps.map((step) => <ToolStepCard key={step.id} step={step} />)}</div>}
        <div className="divider" /><div className="git-heading"><h3>Git 仓库</h3><button title="刷新 Git 信息" onClick={() => void refreshGitChanges()}><RefreshCw size={12} /></button></div>
        {!gitOverview?.isRepository ? <div className="empty-small">当前项目不是 Git 仓库。</div> : <div className="git-overview">
          <div className="git-branch-card"><GitBranch size={14} /><strong>{gitOverview.branch}</strong><span>{gitChanges.length} 个文件</span><b>+{gitOverview.additions}</b><i>-{gitOverview.deletions}</i></div>
          <div className="git-history">{gitOverview.commits.map((commit) => <div key={commit.hash}><GitCommitHorizontal size={13} /><span><strong>{commit.subject}</strong><small>{commit.hash} · {commit.author} · {commit.relativeTime}</small></span></div>)}</div>
        </div>}
        </>}
      </aside>}

      {approvals.length > 0 && <div className="approval-bar"><div><strong>需要你的批准</strong><span>{approvals[0].toolName}：{approvals[0].reason}</span></div><button onClick={() => decide(approvals[0].id, "deny")}>拒绝</button><button className="primary-inline" onClick={() => decide(approvals[0].id, "allowOnce")}>允许一次</button></div>}

      {fileContextMenu && <div className="context-menu" style={{ left: fileContextMenu.x, top: fileContextMenu.y }} onPointerDown={(event) => event.stopPropagation()}>
        {!fileContextMenu.isDir && <><button onClick={() => { void openWorkspaceFile(fileContextMenu.path); setFileContextMenu(undefined); }}>打开</button><button onClick={() => { void openWorkspaceFile(fileContextMenu.path, undefined, true); setFileContextMenu(undefined); }}>固定到标签页</button></>}
        <div />
        <button onClick={() => { void renameWorkspaceEntry(); setFileContextMenu(undefined); }}>重命名</button>
        <button className="danger" onClick={() => { void deleteWorkspaceEntry(); setFileContextMenu(undefined); }}>删除</button>
      </div>}

      {settingsOpen && <div className="modal-backdrop"><div className="modal settings-modal">
        <div className="modal-title"><div><h2>Lan Code 设置</h2><p>按类别管理模型、能力、项目、Agent 和更新。</p></div><button className="icon-button" onClick={() => setSettingsOpen(false)}>×</button></div>
        <div className="settings-layout"><nav className="settings-nav">
          {([["appearance", "外观"], ["model", "模型服务"], ["capabilities", "能力路由"], ["workspace", "项目与数据"], ["agent", "Agent 与权限"], ["updates", "软件更新"]] as [SettingsSection, string][]).map(([id, label]) => <button key={id} className={settingsSection === id ? "active" : ""} onClick={() => setSettingsSection(id)}>{label}</button>)}
        </nav><div className="settings-content">
        {settingsSection === "appearance" && <><div className="settings-section-title"><h3>外观</h3><p>选择浅色、深色，或跟随 Windows 系统主题。</p></div><div className="theme-options">
          {([{ id: "system", label: "跟随系统", icon: <Monitor size={18} /> }, { id: "light", label: "浅色", icon: <Sun size={18} /> }, { id: "dark", label: "深色", icon: <Moon size={18} /> }] as { id: ThemeMode; label: string; icon: React.ReactNode }[]).map((theme) => <button key={theme.id} className={themeMode === theme.id ? "active" : ""} onClick={() => setThemeMode(theme.id)}>{theme.icon}<strong>{theme.label}</strong>{themeMode === theme.id && <Check size={14} />}</button>)}
        </div><div className="settings-note">主题偏好保存在本机，代码编辑器和集成终端会同步切换。</div></>}
        {settingsSection === "model" && <><div className="settings-section-title"><h3>模型服务</h3><p>内置常用 OpenAI-compatible 服务，也可以完全自定义。</p></div><div className="form-grid">
          <label>模型服务<select value={draft.provider} onChange={(e) => applyProvider(e.target.value)}>{PROVIDERS.map((provider) => <option value={provider.id} key={provider.id}>{provider.name}</option>)}</select></label>
          <label>模型名称<input value={draft.model} onChange={(e) => setDraft({ ...draft, model: e.target.value })} /></label>
          <label className="span-2">API 地址<input value={draft.baseUrl} onChange={(e) => setDraft({ ...draft, baseUrl: e.target.value })} /></label>
          <label className="span-2">API Key<input type="password" placeholder="sk-..." value={draft.apiKey} onChange={(e) => setDraft({ ...draft, apiKey: e.target.value })} /></label>
          <label>输入价格（美元/百万 Token）<input type="number" min="0" step="0.01" value={draft.inputPricePerMillion} onChange={(e) => setDraft({ ...draft, inputPricePerMillion: Number(e.target.value) })} /></label>
          <label>输出价格（美元/百万 Token）<input type="number" min="0" step="0.01" value={draft.outputPricePerMillion} onChange={(e) => setDraft({ ...draft, outputPricePerMillion: Number(e.target.value) })} /></label>
        </div><div className="profile-section">
          <div className="profile-heading"><strong>Provider 配置档案</strong><button onClick={saveProviderProfile}>保存当前配置</button></div>
          {draft.providerProfiles.length === 0 ? <span>可保存多套模型地址、Key 和计费参数，切换时无需重复填写。</span> : draft.providerProfiles.map((profile) => (
            <div className="profile-row" key={profile.id}><button onClick={() => applyProviderProfile(profile)}><strong>{profile.name}</strong><span>{profile.model}</span></button><button onClick={() => removeProviderProfile(profile)}><Trash2 size={14} /></button></div>
          ))}
        </div><div className="settings-note">DeepSeek 默认价格来自官方价格页；其他服务价格会因模型和渠道变化，请按实际账单填写。</div></>}
        {settingsSection === "capabilities" && <><div className="settings-section-title"><h3>能力路由</h3><p>主模型缺少多模态能力时，将任务路由到专用模型。</p></div><div className="capability-section">
          <div className="profile-heading"><strong>模型能力路由</strong><span>测试 API 后会自动识别</span></div>
          <div className="capability-badges">
            <i className={draft.modelCapabilities.toolCalling ? "on" : ""}>工具调用</i>
            <i className={draft.modelCapabilities.imageInput ? "on" : ""}>图片理解</i>
            <i className={draft.modelCapabilities.imageOutput ? "on" : ""}>图片生成</i>
            <i className={draft.modelCapabilities.audioInput ? "on" : ""}>语音识别</i>
            <i className={draft.modelCapabilities.audioOutput ? "on" : ""}>语音输出</i>
          </div>
          <p>主模型支持某项能力时自动复用；不支持时，才使用下面的专用模型。</p>
          {([
            ["visionRoute", "图片理解", draft.modelCapabilities.imageInput],
            ["imageGenerationRoute", "图片生成", draft.modelCapabilities.imageOutput],
            ["speechToTextRoute", "语音识别", draft.modelCapabilities.audioInput],
            ["textToSpeechRoute", "语音输出", draft.modelCapabilities.audioOutput],
          ] as const).map(([key, label, inherited]) => {
            const route = draft[key];
            return <div className="route-row" key={key}>
              <label><input type="checkbox" checked={route.enabled} onChange={(event) => setDraft({ ...draft, [key]: { ...route, enabled: event.target.checked } })} /> {label}</label>
              <span>{inherited && route.inheritMainModel ? "自动使用主模型" : route.enabled ? "使用专用模型" : "未启用"}</span>
              {!inherited && route.enabled && <div className="route-fields"><input placeholder="API 地址，留空继承主模型" value={route.baseUrl} onChange={(event) => setDraft({ ...draft, [key]: { ...route, inheritMainModel: false, baseUrl: event.target.value } })} /><input placeholder="API Key，留空继承主模型" type="password" value={route.apiKey} onChange={(event) => setDraft({ ...draft, [key]: { ...route, inheritMainModel: false, apiKey: event.target.value } })} /><input placeholder="专用模型名称" value={route.model} onChange={(event) => setDraft({ ...draft, [key]: { ...route, inheritMainModel: false, model: event.target.value } })} /></div>}
            </div>;
          })}
        </div></>}
        {settingsSection === "workspace" && <><div className="settings-section-title"><h3>项目与数据</h3><p>项目是可切换的代码仓库；数据目录保存配置、会话和更新包。</p></div><div className="form-grid">
          <label className="span-2">当前项目目录<div className="input-action"><input value={draft.workspace} onChange={(e) => setDraft({ ...draft, workspace: e.target.value })} /><button onClick={chooseWorkspace}>选择文件夹</button></div></label>
          <label className="span-2">数据保存目录<div className="input-action"><input value={draft.dataDir} onChange={(e) => setDraft({ ...draft, dataDir: e.target.value })} /><button onClick={chooseDataDir}>选择目录</button></div></label>
        </div><div className="settings-note">默认数据目录为 ~/.lancode。API Key 会明文保存在 settings.json，请勿提交或分享该文件。</div></>}
        {settingsSection === "agent" && <><div className="settings-section-title"><h3>Agent 与权限</h3><p>控制自动执行范围和单次任务的最大迭代深度。</p></div><div className="form-grid">
          <label>权限模式<select value={draft.approvalMode} onChange={(e) => setDraft({ ...draft, approvalMode: e.target.value as Mode })}>{APPROVAL_MODES.map((mode) => <option key={mode.id} value={mode.id}>{mode.label}</option>)}</select></label>
          <label>单任务最大执行轮次<input type="number" min="4" max="256" value={draft.maxProviderRounds} onChange={(e) => setDraft({ ...draft, maxProviderRounds: Number(e.target.value) })} /></label>
        </div></>}
        {settingsSection === "updates" && <><div className="settings-section-title"><h3>软件更新</h3><p>仅从 Lan Code 官方 GitHub Release 检查并下载安装包。</p></div><div className="update-card">
          <div><strong><Zap size={15} /> 软件更新</strong><span>{updateStatus || "仅从 Lan Code 官方 GitHub Release 检查和下载更新。"}</span></div>
          <div>
            <button onClick={checkUpdates}><RefreshCw size={14} /> 检查更新</button>
            {updateInfo?.available && !downloadedUpdate && <button onClick={downloadUpdate}><Download size={14} /> 下载 v{updateInfo.latestVersion}</button>}
            {downloadedUpdate && <button className="primary-inline" onClick={installUpdate}>退出并安装</button>}
          </div>
        </div></>}
        {settingsStatus && <div className={settingsStatus.includes("失败") ? "settings-status failed" : "settings-status"}>{settingsStatus}</div>}
        <div className="modal-actions">{settingsSection === "model" && <button onClick={testProvider}>测试 API</button>}<button className="primary-inline" onClick={saveSettings}>保存并启用</button></div>
        </div></div>
      </div></div>}
    </div>
  );
}

ReactDOM.createRoot(document.getElementById("root")!).render(<App />);
