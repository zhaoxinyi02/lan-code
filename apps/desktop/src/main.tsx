import React from "react";
import ReactDOM from "react-dom/client";
import { createPortal } from "react-dom";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
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
  ArrowLeft, ArrowRight, Minus, Square, X,
} from "lucide-react";
import "./styles.css";

type Mode = "readOnly" | "ask" | "workspace" | "fullAccess";
type Session = { id: string; cwd: string; title?: string; status: string; updatedAt: number };
type Project = { name: string; path: string };
type ProviderProfile = {
  id: string; name: string; enabled: boolean; provider: string; baseUrl: string; model: string; apiKey: string;
  inputPricePerMillion: number; outputPricePerMillion: number; contextWindow: number; maxOutputTokens: number;
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
type GitOverview = {
  isRepository: boolean; branch: string; additions: number; deletions: number;
  changedFiles: number; stagedFiles: number; unstagedFiles: number; untrackedFiles: number; commits: GitCommit[];
};
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
  arguments?: unknown; output?: unknown; error?: string; count?: number;
};
type Approval = { id: string; toolName: string; reason: string; arguments: unknown };
type ChatMessage = { role: "user" | "assistant"; text: string; reasoning?: string };
type UpdateInfo = {
  currentVersion: string; latestVersion: string; available: boolean; releaseUrl: string;
  installerUrl?: string; installerName?: string; publishedAt?: string; notes?: string;
};
type FileContextMenu = { x: number; y: number; path: string; isDir: boolean };
type AppDialog = { kind: "confirm" | "input"; title: string; message: string; value?: string; danger?: boolean };
type TerminalKind = "powershell" | "cmd" | "wsl";
type TerminalTab = { id: string; title: string; shell: TerminalKind };
type TerminalPayload = { id: string; data: string };
type ContextMeterProps = { used: number; limit: number; compact?: boolean };

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

const CURRENT_VERSION = "0.2.4";
const DEFAULT_UPDATE_INFO: UpdateInfo = {
  currentVersion: CURRENT_VERSION,
  latestVersion: "",
  available: false,
  releaseUrl: "",
};

const DEFAULT_CONTEXT_WINDOW = 128_000;
const DEFAULT_MAX_OUTPUT_TOKENS = 8_192;

const PROVIDERS = [
  { id: "deepseek", name: "DeepSeek", baseUrl: "https://api.deepseek.com", model: "deepseek-chat", inputPrice: 0.27, outputPrice: 1.10 },
  { id: "openai", name: "OpenAI", baseUrl: "https://api.openai.com/v1", model: "gpt-4.1" },
  { id: "anthropic", name: "Anthropic Claude", baseUrl: "https://api.anthropic.com/v1", model: "claude-sonnet-4-5" },
  { id: "gemini", name: "Google Gemini", baseUrl: "https://generativelanguage.googleapis.com/v1beta/openai", model: "gemini-3.5-flash" },
  { id: "mistral", name: "Mistral AI", baseUrl: "https://api.mistral.ai/v1", model: "devstral-medium-latest" },
  { id: "openrouter", name: "OpenRouter", baseUrl: "https://openrouter.ai/api/v1", model: "anthropic/claude-sonnet-4" },
  { id: "qwen", name: "阿里云百炼 / 通义千问", baseUrl: "https://dashscope.aliyuncs.com/compatible-mode/v1", model: "qwen3-coder-next" },
  { id: "dashscope-deepseek", name: "阿里云百炼 / DeepSeek", baseUrl: "https://dashscope.aliyuncs.com/compatible-mode/v1", model: "deepseek-v4-pro" },
  { id: "volcengine", name: "火山引擎方舟", baseUrl: "https://ark.cn-beijing.volces.com/api/v3", model: "" },
  { id: "volcengine-coding", name: "火山引擎方舟 / Coding Plan", baseUrl: "https://ark.cn-beijing.volces.com/api/coding/v3", model: "ark-code-latest" },
  { id: "baidu", name: "百度千帆", baseUrl: "https://qianfan.baidubce.com/v2", model: "" },
  { id: "hunyuan", name: "腾讯混元", baseUrl: "https://api.hunyuan.cloud.tencent.com/v1", model: "" },
  { id: "moonshot", name: "Moonshot / Kimi", baseUrl: "https://api.moonshot.cn/v1", model: "kimi-k2-0711-preview" },
  { id: "minimax", name: "MiniMax", baseUrl: "https://api.minimax.io/v1", model: "" },
  { id: "stepfun", name: "阶跃星辰 StepFun", baseUrl: "https://api.stepfun.com/v1", model: "" },
  { id: "lingyi", name: "零一万物 Yi", baseUrl: "https://api.lingyiwanwu.com/v1", model: "" },
  { id: "modelscope", name: "魔搭 ModelScope", baseUrl: "https://api-inference.modelscope.cn/v1", model: "" },
  { id: "siliconflow", name: "硅基流动", baseUrl: "https://api.siliconflow.cn/v1", model: "Pro/zai-org/GLM-4.7" },
  { id: "zhipu", name: "智谱 GLM", baseUrl: "https://open.bigmodel.cn/api/paas/v4", model: "glm-4.5" },
  { id: "together", name: "Together AI", baseUrl: "https://api.together.xyz/v1", model: "" },
  { id: "fireworks", name: "Fireworks AI", baseUrl: "https://api.fireworks.ai/inference/v1", model: "" },
  { id: "cerebras", name: "Cerebras", baseUrl: "https://api.cerebras.ai/v1", model: "" },
  { id: "perplexity", name: "Perplexity", baseUrl: "https://api.perplexity.ai", model: "" },
  { id: "github-models", name: "GitHub Models", baseUrl: "https://models.inference.ai.azure.com", model: "" },
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

const TASK_TEMPLATES = [
  { id: "understand", title: "理解项目", description: "梳理目录、架构、启动方式和风险点", prompt: "阅读当前项目并解释架构、技术栈、启动方式和关键风险点。", icon: Bot },
  { id: "review", title: "审阅改动", description: "检查 Git diff，给出风险和修复建议", prompt: "检查当前 Git diff，指出潜在 bug、风险、遗漏测试，并给出修复建议。", icon: FileDiff },
  { id: "fix-tests", title: "修复测试", description: "运行测试，定位失败并修复", prompt: "运行项目测试，定位失败原因并修复，最后重新验证。", icon: CheckCircle2 },
  { id: "implement", title: "实现功能", description: "按软件工程方式拆解、修改、验证", prompt: "根据我的需求实现功能。请先理解现有代码结构，再小步修改并运行必要检查。", icon: Code2 },
  { id: "cleanup", title: "整理代码", description: "消除重复、改善命名和边界", prompt: "审查当前项目中可以安全整理的代码，消除明显重复、改善命名和模块边界，并保持行为不变。", icon: Sparkles },
  { id: "release", title: "发布检查", description: "构建、版本、文档和交付物检查", prompt: "做一次发布前检查：构建、测试、版本号、文档、安装包和 Git 状态都确认一遍。", icon: Download },
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

function gitStatusLabel(status: string): string {
  const value = status.trim();
  if (value === "??") return "新增";
  if (value.includes("D")) return "删除";
  if (value.includes("R")) return "重命名";
  if (value.includes("A")) return "新增";
  if (value.includes("M")) return "修改";
  return value || "修改";
}

function ContextMeter({ used, limit, compact = false }: ContextMeterProps) {
  const safeLimit = Math.max(1, limit || DEFAULT_CONTEXT_WINDOW);
  const percent = Math.min(100, Math.round((used / safeLimit) * 100));
  const title = `上下文 ${used.toLocaleString()} / ${safeLimit.toLocaleString()} Token（${percent}%）`;
  return <div className={`context-meter ${compact ? "compact" : ""}`} title={title} style={{ "--context-percent": `${percent}%` } as React.CSSProperties}>
    <svg viewBox="0 0 36 36" aria-hidden="true"><circle cx="18" cy="18" r="15.5" pathLength="100" /><circle cx="18" cy="18" r="15.5" pathLength="100" /></svg>
    {!compact && <span>{percent}%</span>}
  </div>;
}

function Dropdown<T extends string>({ value, options, icon, title, onChange, upward = false }: {
  value: T; options: { id: T; label: string }[]; icon: React.ReactNode; title: string; onChange: (value: T) => void; upward?: boolean;
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
      setMenuStyle(upward
        ? { top: "auto", right: "auto", bottom: window.innerHeight - rect.top + 6, left: Math.max(8, rect.left), minWidth: Math.max(240, rect.width) }
        : { bottom: "auto", left: "auto", top: rect.bottom + 6, right: Math.max(8, window.innerWidth - rect.right), minWidth: Math.max(190, rect.width) });
    }
    setOpen((current) => !current);
  };
  return <div className="dropdown" ref={host}>
    <button className={`dropdown-trigger ${open ? "open" : ""}`} title={title} onClick={toggle}>
      {icon}<span>{selected?.label || "未选择"}</span><ChevronDown size={13} />
    </button>
    {open && createPortal(<div className="dropdown-menu dropdown-portal" style={menuStyle} onPointerDown={(event) => event.stopPropagation()}>{options.length === 0 && <div className="dropdown-empty">没有已启用的模型配置</div>}{options.map((option) => <button key={option.id} className={option.id === value ? "active" : ""} onClick={() => { onChange(option.id); setOpen(false); }}>
      <span>{option.label}</span>{option.id === value && <Check size={13} />}
    </button>)}</div>, document.body)}
  </div>;
}

function ModelSwitcher<T extends string>({ value, options, onChange }: {
  value: T; options: { id: T; label: string; provider?: string; model?: string; meta?: string }[]; onChange: (value: T) => void;
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
      setMenuStyle({ bottom: window.innerHeight - rect.top + 7, left: Math.max(8, rect.left), minWidth: Math.max(310, rect.width) });
    }
    setOpen((current) => !current);
  };
  return <div className="dropdown model-switcher" ref={host}>
    <button className={`dropdown-trigger ${open ? "open" : ""}`} title="切换模型" onClick={toggle}>
      <TerminalSquare size={15} /><span>{selected?.model || selected?.label || "选择模型"}</span><ChevronDown size={13} />
    </button>
    {open && createPortal(<div className="model-menu dropdown-portal" style={menuStyle} onPointerDown={(event) => event.stopPropagation()}>
      {options.map((option) => <button key={option.id} className={option.id === value ? "active" : ""} onClick={() => { onChange(option.id); setOpen(false); }}>
        {option.id === "__manage" ? <><Settings size={15} /><span><strong>{option.label}</strong><small>添加、测试或获取模型 ID</small></span></> : <><TerminalSquare size={15} /><span><strong>{option.provider || option.label}</strong><small>{option.model || option.label}{option.meta ? ` · ${option.meta}` : ""}</small></span>{option.id === value && <Check size={14} />}</>}
      </button>)}
    </div>, document.body)}
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
  return <details className={`tool-step ${step.status}`} open={step.status === "running"}>
    <summary>
      {step.status === "running" ? <RefreshCw size={13} /> : step.status === "completed" ? <CheckCircle2 size={13} /> : <XCircle size={13} />}
      <span><strong>{toolStepTitle(step)}{(step.count || 1) > 1 ? ` × ${step.count}` : ""}</strong><small>{toolStepDetail(step)}</small></span>
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
  return <>{message.reasoning && <details className="reasoning-card"><summary><span>已处理</span><ChevronRight size={12} /></summary><Markdown>{message.reasoning}</Markdown></details>}
    <Markdown>{message.text}</Markdown>
    {latest && changes.length > 0 && <details className="change-summary" open><summary><FileDiff size={13} /><strong>本次工作区改动</strong><span className="diff-add">+{overview?.additions || 0}</span><span className="diff-remove">-{overview?.deletions || 0}</span><ChevronRight size={12} /></summary>
      <div>{changes.slice(0, 12).map((change) => <span key={`${change.status}:${change.path}`}><i className={change.status.includes("D") ? "removed" : change.status === "??" ? "added" : "modified"}>{gitStatusLabel(change.status)}</i><FileTypeIcon path={change.path} size={13} /><b>{change.path}</b></span>)}</div>
    </details>}
  </>;
}

function buildChatMessages(rows: ModelMessage[]): ChatMessage[] {
  const result: ChatMessage[] = [];
  for (const row of rows) {
    if (row.role === "user" && row.content) {
      result.push({ role: "user", text: row.content });
      continue;
    }
    if (row.role !== "assistant" || (!row.content && !row.reasoning_content)) continue;
    const previous = result.at(-1);
    const reasoningPart = [row.reasoning_content, previous?.role === "assistant" ? previous.text : ""].filter(Boolean).join("\n\n");
    if (previous?.role === "assistant") {
      previous.reasoning = [previous.reasoning, reasoningPart].filter(Boolean).join("\n\n");
      if (row.content) previous.text = row.content;
    } else {
      result.push({ role: "assistant", text: row.content || "", reasoning: row.reasoning_content });
    }
  }
  return result;
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

function TerminalPane({ tab, workspace, visible, dark }: { tab: TerminalTab; workspace: string; visible: boolean; dark: boolean }) {
  const host = React.useRef<HTMLDivElement>(null);
  const terminal = React.useRef<Terminal | null>(null);
  const fit = React.useRef<FitAddon | null>(null);

  const start = React.useCallback(async () => {
    const instance = terminal.current;
    if (!instance) return;
    fit.current?.fit();
    try {
      await invoke("terminal_start", { id: tab.id, shell: tab.shell, cols: instance.cols, rows: instance.rows });
      instance.focus();
    } catch (error) {
      instance.writeln(`\r\n\x1b[31m${String(error)}\x1b[0m`);
    }
  }, [tab.id, tab.shell]);

  React.useEffect(() => {
    if (!host.current) return;
    const instance = new Terminal({
      cursorBlink: true, convertEol: true, fontFamily: "Cascadia Code, Cascadia Mono, JetBrains Mono, Consolas, monospace", fontSize: 12,
      scrollback: 10000, theme: dark
        ? { background: "#17191c", foreground: "#d9dce1", cursor: "#d9dce1", selectionBackground: "#4d78a866" }
        : { background: "#fbfcfd", foreground: "#243142", cursor: "#243142", selectionBackground: "#6aa6e833" },
    });
    const fitAddon = new FitAddon();
    terminal.current = instance;
    fit.current = fitAddon;
    instance.loadAddon(fitAddon);
    instance.open(host.current);
    const data = instance.onData((value) => { void invoke("terminal_write", { id: tab.id, data: value }); });
    const resize = instance.onResize((size) => { void invoke("terminal_resize", { id: tab.id, ...size }).catch(() => {}); });
    let stopOutput = () => {};
    let stopExit = () => {};
    void Promise.all([
      listen<TerminalPayload>("terminal-output", (event) => { if (event.payload.id === tab.id) instance.write(event.payload.data); }),
      listen<TerminalPayload>("terminal-exit", (event) => { if (event.payload.id === tab.id) instance.writeln("\r\n\x1b[90m[终端进程已退出]\x1b[0m"); }),
    ]).then(([output, exit]) => { stopOutput = output; stopExit = exit; return start(); });
    const observer = new ResizeObserver(() => requestAnimationFrame(() => fitAddon.fit()));
    observer.observe(host.current);
    return () => {
      observer.disconnect(); stopOutput(); stopExit(); data.dispose(); resize.dispose(); instance.dispose();
      terminal.current = null; fit.current = null;
    };
  }, [dark, start, tab.id]);

  React.useEffect(() => {
    if (visible) requestAnimationFrame(() => { fit.current?.fit(); terminal.current?.focus(); });
  }, [visible]);

  React.useEffect(() => {
    if (!terminal.current) return;
    terminal.current.options.theme = dark
      ? { background: "#17191c", foreground: "#d9dce1", cursor: "#d9dce1", selectionBackground: "#4d78a866" }
      : { background: "#fbfcfd", foreground: "#243142", cursor: "#243142", selectionBackground: "#6aa6e833" };
  }, [dark]);

  return <div className={`terminal-pane ${visible ? "active" : ""}`} data-workspace={workspace} ref={host} />;
}

function IntegratedTerminal({ workspace, visible, dark, onClose }: { workspace: string; visible: boolean; dark: boolean; onClose: () => void }) {
  const [tabs, setTabs] = React.useState<TerminalTab[]>(() => [{ id: crypto.randomUUID(), title: "PowerShell", shell: "powershell" }]);
  const [activeId, setActiveId] = React.useState(() => tabs[0].id);
  const [pickerOpen, setPickerOpen] = React.useState(false);

  React.useEffect(() => {
    if (visible && tabs.length === 0) addTerminal("powershell");
  }, [visible, tabs.length]);

  function addTerminal(shell: TerminalKind) {
    const title = shell === "cmd" ? "CMD" : shell === "wsl" ? "WSL" : "PowerShell";
    const tab = { id: crypto.randomUUID(), title, shell };
    setTabs((items) => [...items, tab]);
    setActiveId(tab.id);
    setPickerOpen(false);
  }

  async function closeTerminal(id: string) {
    await invoke("terminal_stop_one", { id }).catch(() => {});
    setTabs((items) => {
      const next = items.filter((item) => item.id !== id);
      if (!next.length) {
        onClose();
        return next;
      }
      if (activeId === id) setActiveId(next.at(-1)!.id);
      return next;
    });
  }

  return <section className={`terminal-panel ${visible ? "" : "terminal-hidden"}`}>
    <div className="terminal-title"><strong>终端 · {workspace}</strong><div className="terminal-tabs">{tabs.map((tab) => <button key={tab.id} className={tab.id === activeId ? "active" : ""} onClick={() => setActiveId(tab.id)}>{tab.title}<span onClick={(event) => { event.stopPropagation(); void closeTerminal(tab.id); }}>×</span></button>)}</div><div className="terminal-add"><button onClick={() => setPickerOpen((value) => !value)}>新建</button>{pickerOpen && <div><button onClick={() => addTerminal("powershell")}>PowerShell</button><button onClick={() => addTerminal("cmd")}>CMD</button><button onClick={() => addTerminal("wsl")}>WSL</button></div>}</div><button title="隐藏终端" onClick={onClose}>×</button></div>
    <div className="terminal-host">{tabs.map((tab) => <TerminalPane key={tab.id} tab={tab} workspace={workspace} visible={visible && tab.id === activeId} dark={dark} />)}</div>
  </section>;
}

function App() {
  const appWindow = React.useMemo(() => getCurrentWindow(), []);
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
  const [profileModels, setProfileModels] = React.useState<Record<string, string[]>>({});
  const [profileBusy, setProfileBusy] = React.useState<Record<string, string>>({});
  const [expandedProfiles, setExpandedProfiles] = React.useState<Set<string>>(new Set());
  const [searchOpen, setSearchOpen] = React.useState(false);
  const [query, setQuery] = React.useState("");
  const [sidebarOpen, setSidebarOpen] = React.useState(true);
  const [fatal, setFatal] = React.useState("");
  const [updateInfo, setUpdateInfo] = React.useState<UpdateInfo>(DEFAULT_UPDATE_INFO);
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
  const [appMenu, setAppMenu] = React.useState<"file" | "edit" | "view" | "help">();
  const [terminalOpen, setTerminalOpen] = React.useState(false);
  const [terminalStarted, setTerminalStarted] = React.useState(false);
  const [completionEnabled, setCompletionEnabled] = React.useState(() => localStorage.getItem("lan-code-completion-enabled") !== "false");
  const [inspectorOpen, setInspectorOpen] = React.useState(true);
  const [sidebarWidth, setSidebarWidth] = React.useState(() => storedSize("lan-code-sidebar-width", 238));
  const [inspectorWidth, setInspectorWidth] = React.useState(() => storedSize("lan-code-inspector-width", 252));
  const [activityHeight, setActivityHeight] = React.useState(() => storedSize("lan-code-activity-height", 260));
  const [showScrollBottom, setShowScrollBottom] = React.useState(false);
  const [fileContextMenu, setFileContextMenu] = React.useState<FileContextMenu>();
  const [appDialog, setAppDialog] = React.useState<AppDialog>();
  const [dialogValue, setDialogValue] = React.useState("");
  const [paletteOpen, setPaletteOpen] = React.useState(false);
  const dialogResolver = React.useRef<((value: string | boolean | undefined) => void) | undefined>(undefined);
  const editorRef = React.useRef<Parameters<OnMount>[0] | null>(null);
  const completionTimer = React.useRef<number | undefined>(undefined);
  const completionDisposable = React.useRef<monaco.IDisposable | null>(null);
  const completionEnabledRef = React.useRef(completionEnabled);
  const conversationRef = React.useRef<HTMLElement | null>(null);
  const activeDocument = openFiles.find((file) => file.path === activeFile);
  const providerReady = Boolean(settings.apiKey) || ["ollama", "lmstudio"].includes(settings.provider);
  const darkTheme = themeMode === "dark" || (themeMode === "system" && systemDark);
  const workingTreeDirty = Boolean(gitOverview?.isRepository && gitOverview.changedFiles > 0);

  function resolveDialog(value: string | boolean | undefined) {
    dialogResolver.current?.(value);
    dialogResolver.current = undefined;
    setAppDialog(undefined);
  }

  function askConfirm(title: string, message: string, danger = false) {
    setAppDialog({ kind: "confirm", title, message, danger });
    return new Promise<boolean>((resolve) => { dialogResolver.current = resolve as (value: string | boolean | undefined) => void; });
  }

  function askInput(title: string, message: string, value = "") {
    setDialogValue(value);
    setAppDialog({ kind: "input", title, message, value });
    return new Promise<string | undefined>((resolve) => { dialogResolver.current = resolve as (value: string | boolean | undefined) => void; });
  }

  function chooseTaskTemplate(template: (typeof TASK_TEMPLATES)[number]) {
    setPrompt(template.prompt);
    setPaletteOpen(false);
    setWorkbench("agent");
    requestAnimationFrame(() => conversationRef.current?.scrollTo({ top: conversationRef.current.scrollHeight, behavior: "smooth" }));
  }

  function startResize(panel: "sidebar" | "inspector" | "activity", event: React.PointerEvent) {
    event.preventDefault();
    const startX = event.clientX;
    const startY = event.clientY;
    const startSize = panel === "sidebar" ? sidebarWidth : panel === "inspector" ? inspectorWidth : activityHeight;
    const move = (pointer: PointerEvent) => {
      if (panel === "activity") {
        setActivityHeight(Math.min(420, Math.max(120, startSize + pointer.clientY - startY)));
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
    localStorage.setItem("lan-code-completion-enabled", String(completionEnabled));
    completionEnabledRef.current = completionEnabled;
  }, [completionEnabled]);

  React.useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "k") {
        event.preventDefault();
        setPaletteOpen((value) => !value);
      }
      if (event.key === "Escape") setPaletteOpen(false);
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);

  React.useEffect(() => {
    const disableWebviewMenu = (event: MouseEvent) => {
      event.preventDefault();
      if (!(event.target as HTMLElement).closest(".file-tree")) setFileContextMenu(undefined);
    };
    const closeMenu = (event?: Event) => {
      setFileContextMenu(undefined);
      if (event && !(event.target as HTMLElement).closest(".app-menu-host")) setAppMenu(undefined);
    };
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
        const currentProfile = {
          id: crypto.randomUUID(), name: PROVIDERS.find((item) => item.id === loaded.provider)?.name || loaded.provider,
          enabled: true, provider: loaded.provider, baseUrl: loaded.baseUrl, model: loaded.model, apiKey: loaded.apiKey,
          inputPricePerMillion: loaded.inputPricePerMillion, outputPricePerMillion: loaded.outputPricePerMillion,
          contextWindow: DEFAULT_CONTEXT_WINDOW, maxOutputTokens: DEFAULT_MAX_OUTPUT_TOKENS,
        };
        const hydratedProfiles = loaded.providerProfiles.map((profile) => ({
          ...profile,
          contextWindow: profile.contextWindow || DEFAULT_CONTEXT_WINDOW,
          maxOutputTokens: profile.maxOutputTokens || DEFAULT_MAX_OUTPUT_TOKENS,
        }));
        const profiles = hydratedProfiles.some((profile) => profile.provider === loaded.provider && profile.baseUrl === loaded.baseUrl && profile.model === loaded.model)
          ? hydratedProfiles
          : [currentProfile, ...hydratedProfiles];
        const normalized = { ...loaded, providerProfiles: profiles };
        setSettings(normalized);
        setDraft(normalized);
        setSessions(rows);
        if (!loaded.apiKey && !["ollama", "lmstudio"].includes(loaded.provider)) {
          setSettingsSection("model");
          setSettingsOpen(true);
        }
      })
      .catch((error) => setFatal(String(error)));
  }, []);

  React.useEffect(() => {
    void checkUpdates(true);
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
      .then((rows) => setMessages(buildChatMessages(rows)))
      .catch((error) => setFatal(String(error)));
  }, [activeId]);

  React.useEffect(() => {
    const latest = events.at(-1);
    if (!latest || !["toolCompleted", "toolFailed", "turnCompleted", "turnFailed"].includes(latest.type)) return;
    const timer = window.setTimeout(() => void refreshGitChanges().catch(() => undefined), 180);
    return () => window.clearTimeout(timer);
  }, [events]);

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
    if (!await askConfirm("删除对话", "确定删除这个对话及其历史记录吗？此操作不可撤销。", true)) return;
    await invoke("delete_session", { sessionId });
    if (activeId === sessionId) {
      setActiveId(undefined);
      setMessages([]);
      setEvents([]);
    }
    await refreshSessions();
  }

  async function renameSession(session: Session) {
    const title = (await askInput("重命名对话", "输入新的对话名称", session.title || "未命名对话"))?.trim();
    if (!title) return;
    await invoke("rename_session", { sessionId: session.id, title });
    await refreshSessions();
  }

  function addProviderProfile() {
    const preset = PROVIDERS[0];
    const profile: ProviderProfile = {
      id: crypto.randomUUID(), name: preset.name, enabled: true, provider: preset.id, baseUrl: preset.baseUrl, model: preset.model,
      apiKey: "", inputPricePerMillion: preset.inputPrice || 0, outputPricePerMillion: preset.outputPrice || 0,
      contextWindow: DEFAULT_CONTEXT_WINDOW, maxOutputTokens: DEFAULT_MAX_OUTPUT_TOKENS,
    };
    setDraft({ ...draft, providerProfiles: [...draft.providerProfiles, profile] });
  }

  function updateProviderProfile(id: string, patch: Partial<ProviderProfile>) {
    setDraft((value) => ({ ...value, providerProfiles: value.providerProfiles.map((item) => item.id === id ? { ...item, ...patch } : item) }));
  }

  function makeProviderProfileCurrent(profile: ProviderProfile) {
    setDraft((value) => ({
      ...value, provider: profile.provider, baseUrl: profile.baseUrl, model: profile.model, apiKey: profile.apiKey,
      inputPricePerMillion: profile.inputPricePerMillion, outputPricePerMillion: profile.outputPricePerMillion,
    }));
    setSettingsStatus(`保存后将使用“${profile.name} · ${profile.model}”。`);
  }

  function selectProfileProvider(profile: ProviderProfile, providerId: string) {
    const preset = PROVIDERS.find((item) => item.id === providerId)!;
    updateProviderProfile(profile.id, {
      provider: providerId, name: preset.name, baseUrl: preset.baseUrl || profile.baseUrl, model: preset.model || "",
      inputPricePerMillion: preset.inputPrice || 0, outputPricePerMillion: preset.outputPrice || 0,
    });
  }

  function removeProviderProfile(profile: ProviderProfile) {
    setDraft({ ...draft, providerProfiles: draft.providerProfiles.filter((item) => item.id !== profile.id) });
  }

  async function switchModelProfile(profileId: string) {
    const profile = settings.providerProfiles.find((item) => item.id === profileId);
    if (!profile) return;
    const next = {
      ...settings, provider: profile.provider, baseUrl: profile.baseUrl, model: profile.model,
      apiKey: profile.apiKey, inputPricePerMillion: profile.inputPricePerMillion,
      outputPricePerMillion: profile.outputPricePerMillion,
    };
    await invoke("save_settings", { settings: next });
    setSettings(next);
    setDraft(next);
  }

  function chooseModelProfile(profileId: string) {
    if (profileId === "__manage") {
      setSettingsSection("model");
      setSettingsOpen(true);
      return;
    }
    if (profileId !== "__current") void switchModelProfile(profileId);
  }

  function toggleProviderProfile(profile: ProviderProfile) {
    setDraft({ ...draft, providerProfiles: draft.providerProfiles.map((item) => item.id === profile.id ? { ...item, enabled: item.enabled === false } : item) });
  }

  async function testProviderProfile(profile: ProviderProfile) {
    setProfileBusy((value) => ({ ...value, [profile.id]: "正在测试..." }));
    try {
      const candidate = {
        ...draft, provider: profile.provider, baseUrl: profile.baseUrl, model: profile.model, apiKey: profile.apiKey,
        inputPricePerMillion: profile.inputPricePerMillion, outputPricePerMillion: profile.outputPricePerMillion,
      };
      const result = await invoke<{ latencyMs: number; toolCallSupported: boolean }>("test_provider", { settings: candidate });
      setProfileBusy((value) => ({ ...value, [profile.id]: `连接成功 · ${result.latencyMs}ms · 工具调用${result.toolCallSupported ? "可用" : "未通过"}` }));
    } catch (error) {
      setProfileBusy((value) => ({ ...value, [profile.id]: `测试失败：${String(error)}` }));
    }
  }

  async function fetchProviderModels(profile: ProviderProfile) {
    setProfileBusy((value) => ({ ...value, [profile.id]: "正在获取模型..." }));
    try {
      const models = await invoke<string[]>("list_provider_models", { provider: profile.provider, baseUrl: profile.baseUrl, apiKey: profile.apiKey });
      setProfileModels((value) => ({ ...value, [profile.id]: models }));
      setProfileBusy((value) => ({ ...value, [profile.id]: `已获取 ${models.length} 个模型` }));
      if (!profile.model && models[0]) updateProviderProfile(profile.id, { model: models[0] });
    } catch (error) {
      setProfileBusy((value) => ({ ...value, [profile.id]: `获取失败：${String(error)}` }));
    }
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

  async function closeWorkspaceFile(path: string) {
    const target = openFiles.find((file) => file.path === path);
    if (target?.content !== target?.savedContent && !await askConfirm("关闭未保存文件", `${path} 有未保存修改，确定关闭吗？`, true)) return;
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
    if (!await askConfirm("撤销文件改动", `确定撤销 ${change.path} 的全部未暂存改动吗？此操作不可撤销。`, true)) return;
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
    const path = (await askInput(isDir ? "新建文件夹" : "新建文件", isDir ? "输入新文件夹相对路径" : "输入新文件相对路径"))?.trim();
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
    const newPath = (await askInput("重命名文件", "输入新的相对路径", activeFile))?.trim();
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
    if (!activeFile || !await askConfirm("永久删除文件", `确定永久删除 ${activeFile} 吗？`, true)) return;
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

  async function checkUpdates(silent = false) {
    if (!silent) setUpdateStatus("正在检查 GitHub Release...");
    try {
      const info = await invoke<UpdateInfo>("check_for_updates");
      setUpdateInfo(info);
      setUpdateStatus(info.available ? `发现新版本 v${info.latestVersion}` : `当前已是最新版本 v${info.currentVersion}`);
    } catch (error) {
      setUpdateStatus(silent ? "自动检查更新失败，可稍后手动重试。" : `检查失败：${String(error)}`);
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
    if (!downloadedUpdate || !await askConfirm("安装更新", "Lan Code 将退出并启动安装程序，是否继续？")) return;
    await invoke("install_downloaded_update", { path: downloadedUpdate });
  }

  async function understandImage() {
    const question = await askInput("分析图片", "选择图片后，希望模型分析什么？", "请描述图片内容，并指出其中值得注意的信息。");
    if (question === undefined) return;
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
    const description = await askInput("生成图片", "描述要生成的图片");
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
  const groupedToolSteps = toolSteps.reduce((groups, step) => {
    const existing = groups.find((item) => item.status === step.status && toolStepTitle(item) === toolStepTitle(step));
    if (existing) {
      existing.count = (existing.count || 1) + 1;
    } else {
      groups.push({ ...step, count: 1 });
    }
    return groups;
  }, [] as ToolStep[]);
  const usage = events.filter((event) => event.type === "usageRecorded" && event.usage)
    .reduce((total, event) => ({
      inputTokens: total.inputTokens + event.usage!.inputTokens,
      outputTokens: total.outputTokens + event.usage!.outputTokens,
      totalTokens: total.totalTokens + event.usage!.totalTokens,
      cachedInputTokens: total.cachedInputTokens + event.usage!.cachedInputTokens,
    }), { inputTokens: 0, outputTokens: 0, totalTokens: 0, cachedInputTokens: 0 });
  const estimatedCost = usage.inputTokens / 1_000_000 * settings.inputPricePerMillion
    + usage.outputTokens / 1_000_000 * settings.outputPricePerMillion;
  const enabledProfiles = settings.providerProfiles.filter((profile) => profile.enabled !== false);
  const activeProfileId = enabledProfiles.find((profile) => profile.provider === settings.provider && profile.baseUrl === settings.baseUrl && profile.model === settings.model)?.id || "__current";
  const activeProfile = enabledProfiles.find((profile) => profile.id === activeProfileId);
  const currentContextWindow = activeProfile?.contextWindow || DEFAULT_CONTEXT_WINDOW;
  const currentProject = settings.projects.find((project) => project.path === settings.workspace)
    || (settings.workspace ? { name: settings.workspace.split(/[\\/]/).filter(Boolean).at(-1) || "当前项目", path: settings.workspace } : undefined);
  const currentProjectSessions = sessions.filter((session) => currentProject && normalizePath(session.cwd) === normalizePath(currentProject.path));
  const modelOptions = [
    ...(activeProfileId === "__current" ? [{ id: "__current", label: settings.model || "当前模型", provider: settings.provider, model: settings.model, meta: "当前配置" }] : []),
    ...enabledProfiles.map((profile) => ({ id: profile.id, label: `${profile.name} · ${profile.model || "未选择模型"}`, provider: profile.name, model: profile.model || "未选择模型", meta: profile.baseUrl.replace(/^https?:\/\//, "") })),
    { id: "__manage", label: "管理模型配置...", provider: "设置", model: "管理模型配置" },
  ];
  const visibleWorkspaceFiles = workspaceFiles.filter((entry) => {
    const normalized = entry.path.replaceAll("\\", "/");
    return !Array.from(collapsedDirs).some((dir) => normalized.startsWith(`${dir.replaceAll("\\", "/")}/`));
  });
  const projectSessions = (project: Project) => filtered.filter((session) => normalizePath(session.cwd) === normalizePath(project.path));
  const orphanSessions = filtered.filter((session) => !settings.projects.some((project) => normalizePath(session.cwd) === normalizePath(project.path)));
  const archivedProjects: Project[] = [];
  const updatePublishedAt = updateInfo?.publishedAt ? new Date(updateInfo.publishedAt).toLocaleString() : "自动检查中";
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
      <div className="window-titlebar" data-tauri-drag-region onDoubleClick={() => void appWindow.toggleMaximize().catch(() => undefined)}>
        <div className="titlebar-left">
          <button title={sidebarOpen ? "收起侧栏" : "展开侧栏"} className="titlebar-icon" onClick={() => setSidebarOpen((value) => !value)}>{sidebarOpen ? <PanelLeftClose size={15} /> : <PanelLeftOpen size={15} />}</button>
          <button title="后退" className="titlebar-icon" disabled><ArrowLeft size={15} /></button>
          <button title="前进" className="titlebar-icon" disabled><ArrowRight size={15} /></button>
          {(["file", "edit", "view", "help"] as const).map((menu) => <div className="app-menu-host" key={menu}>
            <button className={appMenu === menu ? "active" : ""} onClick={() => setAppMenu(appMenu === menu ? undefined : menu)}>{menu === "file" ? "文件" : menu === "edit" ? "编辑" : menu === "view" ? "视图" : "帮助"}</button>
            {appMenu === menu && <div className="app-menu-popover">
              {menu === "file" && <><button onClick={() => { setAppMenu(undefined); void newSession(); }}><MessageSquare size={14} />新建对话<kbd>Ctrl+N</kbd></button><button onClick={() => { setAppMenu(undefined); void addProject(); }}><FolderPlus size={14} />添加项目</button><i /><button onClick={() => { setAppMenu(undefined); setDraft(settings); setSettingsSection("appearance"); setSettingsOpen(true); }}><Settings size={14} />设置</button></>}
              {menu === "edit" && <><button onClick={() => { setAppMenu(undefined); setPaletteOpen(true); }}><Sparkles size={14} />快速任务<kbd>Ctrl+K</kbd></button><i /><button disabled={!activeFile} onClick={() => { setAppMenu(undefined); void saveWorkspaceFile(); }}><Save size={14} />保存文件<kbd>Ctrl+S</kbd></button><button onClick={() => { setAppMenu(undefined); void createWorkspaceEntry(false); }}><FilePlus size={14} />新建文件</button><button onClick={() => { setAppMenu(undefined); void createWorkspaceEntry(true); }}><FolderPlus size={14} />新建文件夹</button></>}
              {menu === "view" && <><button onClick={() => { setAppMenu(undefined); setWorkbench("agent"); }}><MessageSquare size={14} />Agent 工作台</button><button onClick={() => { setAppMenu(undefined); setWorkbench("code"); }}><Code2 size={14} />Code 工作台</button><i /><button onClick={() => { setAppMenu(undefined); setSidebarOpen((value) => !value); }}><PanelLeftOpen size={14} />切换侧栏</button><button onClick={() => { setAppMenu(undefined); setInspectorOpen((value) => !value); }}><Eye size={14} />切换观察面板</button></>}
              {menu === "help" && <><button onClick={() => { setAppMenu(undefined); window.open("https://github.com/zhaoxinyi02/lan-code", "_blank"); }}><ExternalLink size={14} />GitHub 仓库</button><button onClick={() => { setAppMenu(undefined); setDraft(settings); setSettingsSection("updates"); setSettingsOpen(true); }}><Download size={14} />检查更新</button></>}
            </div>}
          </div>)}
        </div>
        <div className="titlebar-drag" data-tauri-drag-region />
        <div className="window-controls">
          <button title="最小化" onClick={() => void appWindow.minimize().catch(() => undefined)}><Minus size={15} /></button>
          <button title="最大化或还原" onClick={() => void appWindow.toggleMaximize().catch(() => undefined)}><Square size={12} /></button>
          <button title="关闭" className="window-close" onClick={() => void appWindow.close().catch(() => undefined)}><X size={15} /></button>
        </div>
      </div>
      <aside className={`sidebar ${sidebarOpen ? "" : "collapsed"}`} style={{ width: sidebarWidth }}>
        <div className="brand"><img src="/lan-code-logo.png" alt="Lan Code" /><strong>Lan Code</strong>
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
          <div className="project-tree">{currentProject && <div className="project-group" key={currentProject.path}>
            <button className="project-heading active" title={currentProject.path} onClick={() => toggleProject(currentProject)}>
              {collapsedProjects.has(currentProject.path) ? <ChevronRight size={14} /> : <ChevronDown size={14} />}<FolderGit2 size={14} /><span>{currentProject.name}</span>
            </button>
            {!collapsedProjects.has(currentProject.path) && <>
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
          }} onDoubleClick={() => { if (!entry.isDir) void openWorkspaceFile(entry.path, undefined, true); }}>{entry.isDir ? collapsedDirs.has(entry.path) ? <ChevronRight size={13} /> : <ChevronDown size={13} /> : <span className="tree-spacer" />} {entry.isDir ? collapsedDirs.has(entry.path) ? <Folder size={14} /> : <FolderOpen size={14} /> : <FileTypeIcon path={entry.path} />}<span>{entry.name}</span></button>)}</div>
            </>}
          </div>}</div>
          <div className="code-left-panels" style={{ "--activity-height": `${activityHeight}px` } as React.CSSProperties}>
            <section className="code-agent-panel"><div className="git-heading"><h3>Agent 执行过程</h3></div>
              <div className="code-tool-timeline">{groupedToolSteps.length === 0 ? <div className="empty-small">工具调用会显示在这里。</div> : groupedToolSteps.slice(0, 6).map((step) => <ToolStepCard key={step.id} step={step} />)}</div>
            </section>
            <div className="horizontal-resizer inline" title="拖动调整 Agent / Git 区域高度，双击恢复默认" onPointerDown={(event) => startResize("activity", event)} onDoubleClick={() => setActivityHeight(220)} />
            <section className="code-git-panel"><div className="git-heading"><h3>Git 改动</h3><button title="刷新" onClick={() => void refreshGitChanges()}><RefreshCw size={12} /></button></div>
              <div className="git-changes">{gitOverview?.isRepository && !workingTreeDirty ? <div className="git-clean-inline">当前工作区干净</div> : gitChanges.length === 0 ? <div className="empty-small">暂无已加载改动。</div> : gitChanges.map((change) => <div className={selectedGitPath === change.path ? "active" : ""} key={`${change.status}:${change.path}`}><button title={change.path} onClick={() => void openGitChange(change.path)}><i className={change.status === "??" ? "added" : change.status.includes("D") ? "removed" : "modified"}>{gitStatusLabel(change.status)}</i><FileTypeIcon path={change.path} size={14} /><span>{change.path}</span></button><button title="撤销未暂存改动" disabled={change.status === "??"} onClick={() => void discardGitChange(change)}><RotateCcw size={11} /></button></div>)}</div>
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
      </aside>

      {workbench === "agent" ? <main className="mode-panel mode-agent">
        <header>
          <div className="title-row">
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
              {TASK_TEMPLATES.slice(0, 4).map((template) => {
                const TemplateIcon = template.icon;
                return <button key={template.id} onClick={() => setPrompt(template.prompt)}><TemplateIcon size={18} /><span><strong>{template.title}</strong><small>{template.description}</small></span></button>;
              })}
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
            <ModelSwitcher value={activeProfileId} options={modelOptions} onChange={chooseModelProfile} />
          </div><div className="send-area"><ContextMeter used={usage.totalTokens} limit={currentContextWindow} compact />{busy ? <button className="send stop" onClick={interrupt}><CircleStop size={17} /></button> : <button className="send" disabled={!providerReady} onClick={send}><Send size={17} /></button>}</div></div>
        </div></div>
      </main> : <main className="code-main mode-panel mode-code">
        <header>
          <div className="title-row"><div><h1>{activeFile || "Code 工作台"}</h1><span className="subtle">{activeDocument && activeDocument.content !== activeDocument.savedContent ? "有未保存修改" : settings.workspace}</span></div></div>
          <div className="header-actions"><button className={`completion-toggle ${completionEnabled ? "on" : ""}`} title="输入代码时自动生成建议，按 Tab 接受" onClick={() => setCompletionEnabled((value) => !value)}><Sparkles size={14} /><span>自动补全</span><i>{completionEnabled ? "开" : "关"}</i></button><button className="pill" onClick={() => void renameWorkspaceEntry()} disabled={!activeFile}><Pencil size={15} /> 重命名</button><button className="pill danger" onClick={() => void deleteWorkspaceEntry()} disabled={!activeFile}><Trash2 size={15} /> 删除</button><button className="pill" onClick={() => void saveWorkspaceFile()} disabled={!activeDocument || activeDocument.content === activeDocument.savedContent}><Save size={15} /> 保存</button><button className="pill" onClick={() => { setTerminalStarted(true); setTerminalOpen(!terminalOpen); }}><TerminalSquare size={15} /> 终端</button><button className="pill" onClick={() => setInspectorOpen((value) => !value)}><PanelLeftOpen size={15} /> {inspectorOpen ? "隐藏助手" : "显示助手"}</button></div>
        </header>
        {codeStatus && <div className="code-status">{codeStatus}</div>}
        <div className="editor-tabs">{openFiles.length ? openFiles.map((file) => <button key={file.path} title={file.path} className={`${file.path === activeFile ? "active" : ""} ${file.pinned ? "" : "preview"}`} onClick={() => setActiveFile(file.path)} onDoubleClick={() => setOpenFiles((items) => items.map((item) => item.path === file.path ? { ...item, pinned: true } : item))}><FileTypeIcon path={file.path} size={14} /><span>{file.path.split(/[\\/]/).at(-1)}</span>{file.content !== file.savedContent && <i />}<b title="关闭" onClick={(event) => { event.stopPropagation(); void closeWorkspaceFile(file.path); }}>×</b></button>) : <span>单击预览文件，双击固定标签</span>}</div>
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
                if (token.isCancellationRequested || !completionEnabledRef.current || !providerReady || model.getLineContent(position.lineNumber).trim().length < 2) return { items: [] };
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
          options={{ automaticLayout: true, minimap: { enabled: true }, fontFamily: "Cascadia Code, Cascadia Mono, JetBrains Mono, Consolas, monospace", fontLigatures: true, fontSize: 12, lineHeight: 19, tabSize: 2, wordWrap: "off", quickSuggestions: true, inlineSuggest: { enabled: completionEnabled, mode: "subwordSmart" } }}
        /> : <div className="code-welcome"><Code2 size={46} /><h2>Lan Code 工作台</h2><p>浏览项目、编辑代码、查看改动，并让同一个 Agent 理解当前仓库。</p></div>}</div>
        {terminalStarted && <IntegratedTerminal workspace={settings.workspace} visible={terminalOpen} dark={darkTheme} onClose={() => setTerminalOpen(false)} />}
      </main>}

      {inspectorOpen && <aside className={`inspector ${workbench === "code" ? "code-inspector" : ""}`} style={{ width: inspectorWidth }}>
        <div className="panel-resizer inspector-resizer" title="拖动调整助手宽度，双击恢复默认" onPointerDown={(event) => startResize("inspector", event)} onDoubleClick={() => setInspectorWidth(252)} />
        {workbench === "code" ? <>
          <div className="assistant-head"><div><h3>AI 助手</h3><p>针对当前项目提问，Agent 会使用同一套工具、权限和会话。</p></div><div><button title="新建当前项目对话" className="icon-button subtle-icon" onClick={() => void newSession()}><Plus size={14} /></button><select value={activeId || ""} onChange={(event) => setActiveId(event.target.value || undefined)}><option value="">选择当前项目对话</option>{currentProjectSessions.map((session) => <option key={session.id} value={session.id}>{session.title || "未命名对话"}</option>)}</select></div></div>
          <div className="code-chat">{messages.length === 0 ? <div className="empty-small">选择一个对话，或新建对话后向当前仓库提问。</div> : messages.slice(-8).map((message, index) => <article key={index} className={message.role}><div className="message-label">{message.role === "user" ? "你" : "Lan Code"}</div>{message.role === "assistant" ? <Markdown>{message.text}</Markdown> : <p>{message.text}</p>}</article>)}{busy && <article className="assistant streaming-answer"><div className="message-label">Lan Code</div>{streamingText ? <Markdown>{streamingText}</Markdown> : <div className="thinking"><Sparkles size={14} /> 正在工作...</div>}</article>}</div>
          <div className="code-chat-composer"><textarea value={prompt} onChange={(event) => setPrompt(event.target.value)} onKeyDown={(event) => { if (event.key === "Enter" && !event.shiftKey) { event.preventDefault(); void send(); } }} placeholder="询问代码或要求修改项目" /><div className="code-composer-footer"><div><ModelSwitcher value={activeProfileId} options={modelOptions} onChange={chooseModelProfile} /><Dropdown value={settings.approvalMode} title="切换 Agent 权限" icon={<ShieldCheck size={13} />} options={APPROVAL_MODES.map((mode) => ({ id: mode.id, label: mode.label }))} upward onChange={(mode) => void changeApprovalMode(mode)} /></div><button onClick={() => void send()} disabled={busy || !prompt.trim()}><Send size={14} /></button></div></div>
        </> : <>
        <h3>环境信息</h3>
        <div className="info-row"><FolderGit2 size={16} /><span>项目</span><strong>{settings.projects.find((item) => item.path === settings.workspace)?.name || "未配置"}</strong></div>
        <div className="info-row"><KeyRound size={16} /><span>模型</span><strong>{providerReady ? settings.model : "未配置"}</strong></div>
        <div className="info-row"><ShieldCheck size={16} /><span>权限</span><strong>{APPROVAL_MODES.find((item) => item.id === settings.approvalMode)?.label}</strong></div>
        <div className="divider" /><h3>当前对话用量</h3>
        <div className="usage-grid"><span>输入 Token<strong>{usage.inputTokens.toLocaleString()}</strong></span><span>输出 Token<strong>{usage.outputTokens.toLocaleString()}</strong></span><span className="context-usage"><em>上下文</em><ContextMeter used={usage.totalTokens} limit={currentContextWindow} compact /><strong>{usage.totalTokens.toLocaleString()} / {currentContextWindow.toLocaleString()}</strong></span><span>预估费用<strong>${estimatedCost.toFixed(4)}</strong></span></div>
        <div className="divider" /><h3>任务步骤</h3>
        {toolSteps.length > 0 && <div className="tool-stats"><span>{toolSteps.length} 步</span><b>{toolStepStats.completed} 完成</b>{toolStepStats.running > 0 && <i>{toolStepStats.running} 执行中</i>}{toolStepStats.failed + toolStepStats.stale > 0 && <em>{toolStepStats.failed + toolStepStats.stale} 异常</em>}</div>}
        {groupedToolSteps.length === 0 ? <div className="empty-small">发送任务后在这里查看文件读取、搜索、修改和命令执行过程。</div> : <div className="tool-step-list">{groupedToolSteps.slice(0, 6).map((step) => <ToolStepCard key={step.id} step={step} />)}{groupedToolSteps.length > 6 && <div className="tool-more">另有 {groupedToolSteps.length - 6} 组较早步骤已收起</div>}</div>}
        <div className="divider" /><div className="git-heading"><h3>Git 仓库</h3><button title="刷新 Git 信息" onClick={() => void refreshGitChanges()}><RefreshCw size={12} /></button></div>
        {!gitOverview?.isRepository ? <div className="empty-small">当前项目不是 Git 仓库。</div> : <div className="git-overview">
          <div className="git-repo-card">
            <div><GitBranch size={15} /><span><strong>{gitOverview.branch}</strong><small>当前分支</small></span></div>
            {workingTreeDirty ? <span className="git-dirty-count"><b>{gitOverview.changedFiles}</b><small>未提交</small></span> : <span className="git-clean-badge">干净</span>}
          </div>
          {workingTreeDirty ? <><div className="git-flow">
            <div><span>{gitOverview.stagedFiles}</span><strong>已暂存</strong></div>
            <i />
            <div><span>{gitOverview.unstagedFiles}</span><strong>未暂存</strong></div>
            <i />
            <div><span>{gitOverview.untrackedFiles}</span><strong>新增文件</strong></div>
          </div>
          <div className="git-delta-card"><span>{gitChanges.length} 个文件</span><b>+{gitOverview.additions}</b><i>-{gitOverview.deletions}</i></div></> : <div className="git-clean-card"><CheckCircle2 size={15} /><span>当前工作区干净</span></div>}
          {gitOverview.commits.length ? <div className="git-history">{gitOverview.commits.map((commit) => <div key={commit.hash}><GitCommitHorizontal size={13} /><span><strong>{commit.subject}</strong><small>{commit.hash} · {commit.author} · {commit.relativeTime}</small></span></div>)}</div> : <div className="git-history-empty">暂无提交记录</div>}
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

      {appDialog && <div className="modal-backdrop app-dialog-backdrop" onPointerDown={() => resolveDialog(appDialog.kind === "confirm" ? false : undefined)}>
        <div className="modal app-dialog" onPointerDown={(event) => event.stopPropagation()}>
          <div className="dialog-heading"><div className={appDialog.danger ? "dialog-icon danger" : "dialog-icon"}>{appDialog.danger ? <Trash2 size={18} /> : <MessageSquare size={18} />}</div><div><h2>{appDialog.title}</h2><p>{appDialog.message}</p></div></div>
          {appDialog.kind === "input" && <input autoFocus value={dialogValue} onChange={(event) => setDialogValue(event.target.value)} onKeyDown={(event) => { if (event.key === "Enter") resolveDialog(dialogValue); if (event.key === "Escape") resolveDialog(undefined); }} />}
          <div className="dialog-actions"><button onClick={() => resolveDialog(appDialog.kind === "confirm" ? false : undefined)}>取消</button><button className={appDialog.danger ? "danger-primary" : "primary-inline"} onClick={() => resolveDialog(appDialog.kind === "confirm" ? true : dialogValue)}>{appDialog.danger ? "确认删除" : "确定"}</button></div>
        </div>
      </div>}

      {paletteOpen && <div className="modal-backdrop command-backdrop" onPointerDown={() => setPaletteOpen(false)}>
        <div className="command-palette" onPointerDown={(event) => event.stopPropagation()}>
          <div className="command-title"><Sparkles size={17} /><div><strong>快速任务</strong><span>参考 Cline、Roo、OpenCode 的常用工作流，一键生成高质量任务提示。</span></div><kbd>Ctrl+K</kbd></div>
          <div className="command-list">{TASK_TEMPLATES.map((template) => {
            const TemplateIcon = template.icon;
            return <button key={template.id} onClick={() => chooseTaskTemplate(template)}>
              <TemplateIcon size={16} /><span><strong>{template.title}</strong><small>{template.description}</small></span><ChevronRight size={13} />
            </button>;
          })}</div>
        </div>
      </div>}

      {settingsOpen && <div className="settings-page-shell">
        <aside className="settings-page-nav">
          <button className="settings-back" onClick={() => setSettingsOpen(false)}><ArrowLeft size={15} /> 返回应用</button>
          <div className="settings-search"><Search size={14} /><span>搜索设置...</span></div>
          <div className="settings-category-label">Lan Code</div>
          {([["appearance", "外观"], ["model", "模型服务"], ["capabilities", "能力路由"], ["workspace", "项目与数据"], ["agent", "Agent 与权限"], ["updates", "软件更新"]] as [SettingsSection, string][]).map(([id, label]) => <button key={id} className={settingsSection === id ? "active" : ""} onClick={() => setSettingsSection(id)}>{label}</button>)}
        </aside><main className="settings-page-main"><div className="settings-page-inner">
        <div className="settings-page-title"><div><h2>Lan Code 设置</h2><p>按类别管理模型、能力、项目、Agent 和更新。</p></div><button className="primary-inline" onClick={saveSettings}>保存并启用</button></div>
        {settingsSection === "appearance" && <><div className="settings-section-title"><h3>外观</h3><p>选择浅色、深色，或跟随 Windows 系统主题。</p></div><div className="theme-options">
          {([{ id: "system", label: "跟随系统", icon: <Monitor size={18} /> }, { id: "light", label: "浅色", icon: <Sun size={18} /> }, { id: "dark", label: "深色", icon: <Moon size={18} /> }] as { id: ThemeMode; label: string; icon: React.ReactNode }[]).map((theme) => <button key={theme.id} className={themeMode === theme.id ? "active" : ""} onClick={() => setThemeMode(theme.id)}>{theme.icon}<strong>{theme.label}</strong>{themeMode === theme.id && <Check size={14} />}</button>)}
        </div><div className="settings-note">主题偏好保存在本机，代码编辑器和集成终端会同步切换。</div></>}
        {settingsSection === "model" && <><div className="settings-section-title model-title"><div><h3>模型管理</h3><p>配置并同时启用多个模型，随后可从对话输入框直接切换。</p></div><button className="primary-inline" onClick={addProviderProfile}><Plus size={14} /> 添加模型配置</button></div>
          <div className="provider-list">
            {draft.providerProfiles.map((profile) => {
              const models = profileModels[profile.id] || [];
              const isCurrent = profile.provider === draft.provider && profile.baseUrl === draft.baseUrl && profile.model === draft.model;
              const expanded = expandedProfiles.has(profile.id);
              return <section className={`provider-card ${profile.enabled === false ? "disabled" : ""}`} key={profile.id}>
                <div className="provider-card-header">
                  <label className="enable-check"><input type="checkbox" checked={profile.enabled !== false} onChange={() => toggleProviderProfile(profile)} /><span>启用</span></label>
                  <button className="provider-summary" onClick={() => setExpandedProfiles((items) => {
                    const next = new Set(items);
                    if (next.has(profile.id)) next.delete(profile.id); else next.add(profile.id);
                    return next;
                  })}>
                    {expanded ? <ChevronDown size={14} /> : <ChevronRight size={14} />}
                    <span><strong>{profile.name}</strong><small>{profile.model || "未选择模型"} · {models.length ? `已获取 ${models.length} 个模型` : "未获取模型列表"}</small></span>
                  </button>
                  {isCurrent && <span className="current-tag">当前使用</span>}
                  <button title="删除配置" className="icon-button" onClick={() => removeProviderProfile(profile)}><Trash2 size={14} /></button>
                </div>
                {expanded && <><div className="provider-fields">
                  <label>配置名称<input value={profile.name} onChange={(event) => updateProviderProfile(profile.id, { name: event.target.value })} /></label>
                  <label>供应商<select value={profile.provider} onChange={(event) => selectProfileProvider(profile, event.target.value)}>{PROVIDERS.map((provider) => <option value={provider.id} key={provider.id}>{provider.name}</option>)}</select></label>
                  <label className="span-2">模型 ID<div className="model-input"><input placeholder="可手动填写模型 ID" value={profile.model} onChange={(event) => updateProviderProfile(profile.id, { model: event.target.value })} /><select value={models.includes(profile.model) ? profile.model : ""} onChange={(event) => event.target.value && updateProviderProfile(profile.id, { model: event.target.value })}><option value="">从已获取模型中选择...</option>{models.map((model) => <option value={model} key={model}>{model}</option>)}</select><button onClick={() => void fetchProviderModels(profile)}><RefreshCw size={13} /> 获取模型</button></div></label>
                  <label className="span-2">API 地址<input value={profile.baseUrl} onChange={(event) => updateProviderProfile(profile.id, { baseUrl: event.target.value })} /></label>
                  <label className="span-2">API Key<input type="password" placeholder="sk-..." value={profile.apiKey} onChange={(event) => updateProviderProfile(profile.id, { apiKey: event.target.value })} /></label>
                  <label>输入上下文 / Token<input type="number" min="1024" step="1024" value={profile.contextWindow || DEFAULT_CONTEXT_WINDOW} onChange={(event) => updateProviderProfile(profile.id, { contextWindow: Number(event.target.value) })} /></label>
                  <label>最大输出 / Token<input type="number" min="256" step="256" value={profile.maxOutputTokens || DEFAULT_MAX_OUTPUT_TOKENS} onChange={(event) => updateProviderProfile(profile.id, { maxOutputTokens: Number(event.target.value) })} /></label>
                  <label>输入价格 / 百万 Token<input type="number" min="0" step="0.01" value={profile.inputPricePerMillion} onChange={(event) => updateProviderProfile(profile.id, { inputPricePerMillion: Number(event.target.value) })} /></label>
                  <label>输出价格 / 百万 Token<input type="number" min="0" step="0.01" value={profile.outputPricePerMillion} onChange={(event) => updateProviderProfile(profile.id, { outputPricePerMillion: Number(event.target.value) })} /></label>
                </div>
                <div className="provider-card-footer"><span className={profileBusy[profile.id]?.includes("失败") ? "failed" : ""}>{profileBusy[profile.id] || `${models.length ? `${models.length} 个已获取模型` : "尚未获取模型列表"}`}</span><button onClick={() => void testProviderProfile(profile)}>测试 API</button><button onClick={() => makeProviderProfileCurrent(profile)}>设为当前</button></div></>}
              </section>;
            })}
          </div>
          <div className="settings-note">模型列表通过供应商的兼容 `/models` 接口获取；不支持该接口时仍可手动填写模型 ID。价格因模型和渠道变化，请按实际账单填写。</div></>}
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
        {settingsSection === "workspace" && <><div className="settings-section-title"><h3>项目与数据</h3><p>项目是可切换的代码仓库；数据目录保存配置、会话和更新包。</p></div><div className="project-settings-list">
          <div className="profile-heading"><strong>项目列表</strong><button onClick={() => void addProject()}><FolderPlus size={13} /> 添加项目</button></div>
          {settings.projects.length ? settings.projects.map((project) => <div className={project.path === settings.workspace ? "project-settings-row active" : "project-settings-row"} key={project.path}>
            <button title={project.path} onClick={() => void selectProject(project)}><FolderGit2 size={14} /><span><strong>{project.name}</strong><small>{project.path}</small></span></button>
            <button title="删除项目" onClick={() => void removeProject(project)}><Trash2 size={13} /></button>
          </div>) : <div className="empty-small">暂无项目。点击“添加项目”选择一个代码仓库。</div>}
          <div className="profile-heading archived"><strong>已归档项目</strong><span>{archivedProjects.length} 个</span></div>
          {archivedProjects.length ? archivedProjects.map((project) => <div className="project-settings-row archived" key={project.path}>
            <button title={project.path}><FolderGit2 size={14} /><span><strong>{project.name}</strong><small>{project.path}</small></span></button>
          </div>) : <div className="empty-small">暂无已归档项目。</div>}
        </div><div className="form-grid">
          <label className="span-2">数据保存目录<div className="input-action"><input value={draft.dataDir} onChange={(e) => setDraft({ ...draft, dataDir: e.target.value })} /><button onClick={chooseDataDir}>选择目录</button></div></label>
        </div><div className="settings-note">默认数据目录为 ~/.lancode。API Key 会明文保存在 settings.json，请勿提交或分享该文件。</div></>}
        {settingsSection === "agent" && <><div className="settings-section-title"><h3>Agent 与权限</h3><p>控制自动执行范围和单次任务的最大迭代深度。</p></div><div className="form-grid">
          <label>权限模式<select value={draft.approvalMode} onChange={(e) => setDraft({ ...draft, approvalMode: e.target.value as Mode })}>{APPROVAL_MODES.map((mode) => <option key={mode.id} value={mode.id}>{mode.label}</option>)}</select></label>
          <label>单任务最大执行轮次<input type="number" min="4" max="256" value={draft.maxProviderRounds} onChange={(e) => setDraft({ ...draft, maxProviderRounds: Number(e.target.value) })} /></label>
        </div></>}
        {settingsSection === "updates" && <><div className="settings-section-title"><h3>软件更新</h3><p>仅从 Lan Code 官方 GitHub Release 检查并下载安装包。</p></div><div className="update-card">
          <div><strong><Zap size={15} /> 软件更新</strong><span>{updateStatus || "仅从 Lan Code 官方 GitHub Release 检查和下载更新。"}</span></div>
          <div>
            <button onClick={() => void checkUpdates()}><RefreshCw size={14} /> 检查更新</button>
            {updateInfo?.available && !downloadedUpdate && <button onClick={downloadUpdate}><Download size={14} /> 下载 v{updateInfo.latestVersion}</button>}
            {downloadedUpdate && <button className="primary-inline" onClick={installUpdate}>退出并安装</button>}
          </div>
        </div><div className="update-details">
          <span>当前版本<strong>v{updateInfo?.currentVersion || CURRENT_VERSION}</strong></span>
          <span>最新版本<strong>{updateInfo?.latestVersion ? `v${updateInfo.latestVersion}` : "自动检查中"}</strong></span>
          <span>更新时间<strong>{updatePublishedAt}</strong></span>
          <span>下载文件<strong>{downloadedUpdate || updateInfo?.installerName || "暂无"}</strong></span>
          {updateInfo?.releaseUrl && <a href={updateInfo.releaseUrl} target="_blank" rel="noreferrer"><ExternalLink size={13} /> 查看 Release</a>}
        </div></>}
        {settingsStatus && <div className={settingsStatus.includes("失败") ? "settings-status failed" : "settings-status"}>{settingsStatus}</div>}
        </div></main>
      </div>}
    </div>
  );
}

ReactDOM.createRoot(document.getElementById("root")!).render(<App />);
