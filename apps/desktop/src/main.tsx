import React from "react";
import ReactDOM from "react-dom/client";
import { invoke } from "@tauri-apps/api/core";
import Editor, { type OnMount } from "@monaco-editor/react";
import * as monaco from "monaco-editor";
import {
  Bot, CheckCircle2, ChevronDown, ChevronRight, CircleStop, Code2, File, FileDiff, FilePlus, Folder, FolderGit2, Image,
  FolderOpen, FolderPlus, GitBranch, History, KeyRound, PanelLeftClose, PanelLeftOpen, Pencil, Plus,
  Download, MessageSquare, RefreshCw, Save, Search, Send, Settings, ShieldCheck, Sparkles, TerminalSquare, Trash2,
  XCircle, Zap,
} from "lucide-react";
import "./styles.css";

type Mode = "readOnly" | "ask" | "workspace" | "fullAccess";
type Session = { id: string; cwd: string; title?: string; status: string };
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
type WorkspaceSearchMatch = { path: string; line: number; text: string };
type OpenFile = { path: string; content: string; savedContent: string };
type ModelMessage = { role: "system" | "user" | "assistant" | "tool"; content?: string };
type TokenUsage = { inputTokens: number; outputTokens: number; totalTokens: number; cachedInputTokens: number };
type CoreEvent = { type: string; toolName?: string; error?: string; text?: string; usage?: TokenUsage; model?: string };
type Approval = { id: string; toolName: string; reason: string; arguments: unknown };
type ChatMessage = { role: "user" | "assistant"; text: string };
type UpdateInfo = {
  currentVersion: string; latestVersion: string; available: boolean; releaseUrl: string;
  installerUrl?: string; installerName?: string; publishedAt?: string; notes?: string;
};

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
  { id: "deepseek", name: "DeepSeek", baseUrl: "https://api.deepseek.com", model: "deepseek-v4-pro" },
  { id: "openai", name: "OpenAI", baseUrl: "https://api.openai.com/v1", model: "gpt-4.1" },
  { id: "openrouter", name: "OpenRouter", baseUrl: "https://openrouter.ai/api/v1", model: "openai/gpt-4.1" },
  { id: "qwen", name: "通义千问", baseUrl: "https://dashscope.aliyuncs.com/compatible-mode/v1", model: "qwen-plus" },
  { id: "moonshot", name: "Moonshot", baseUrl: "https://api.moonshot.cn/v1", model: "moonshot-v1-32k" },
  { id: "ollama", name: "Ollama 本地模型", baseUrl: "http://localhost:11434/v1", model: "qwen2.5-coder:7b" },
  { id: "custom", name: "自定义 OpenAI-compatible", baseUrl: "", model: "" },
];

function App() {
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
  const [workspaceQuery, setWorkspaceQuery] = React.useState("");
  const [workspaceMatches, setWorkspaceMatches] = React.useState<WorkspaceSearchMatch[]>([]);
  const [activeFile, setActiveFile] = React.useState("");
  const [openFiles, setOpenFiles] = React.useState<OpenFile[]>([]);
  const [collapsedDirs, setCollapsedDirs] = React.useState<Set<string>>(new Set());
  const [codeStatus, setCodeStatus] = React.useState("");
  const [diffText, setDiffText] = React.useState("");
  const editorRef = React.useRef<Parameters<OnMount>[0] | null>(null);
  const completionTimer = React.useRef<number | undefined>(undefined);
  const activeDocument = openFiles.find((file) => file.path === activeFile);

  const refreshSessions = React.useCallback(async () => {
    const rows = await invoke<Session[]>("list_sessions");
    setSessions(rows.reverse());
  }, []);

  React.useEffect(() => {
    Promise.all([invoke<SettingsData>("get_settings"), invoke<Session[]>("list_sessions")])
      .then(([loaded, rows]) => {
        setSettings(loaded);
        setDraft(loaded);
        setSessions(rows.reverse());
        if (!loaded.apiKey) setSettingsOpen(true);
      })
      .catch((error) => setFatal(String(error)));
  }, []);

  React.useEffect(() => {
    if (workbench !== "code" || !settings.workspace) return;
    invoke<WorkspaceEntry[]>("list_workspace_files").then(setWorkspaceFiles).catch((error) => setCodeStatus(String(error)));
  }, [workbench, settings.workspace]);

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
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  });

  React.useEffect(() => {
    if (!activeId) return;
    invoke<ModelMessage[]>("session_messages", { sessionId: activeId })
      .then((rows) => setMessages(rows
        .filter((row) => (row.role === "user" || row.role === "assistant") && row.content)
        .map((row) => ({ role: row.role as "user" | "assistant", text: row.content! }))))
      .catch((error) => setFatal(String(error)));
  }, [activeId]);

  React.useEffect(() => {
    const timer = window.setInterval(() => {
      if (activeId) invoke<CoreEvent[]>("session_events", { sessionId: activeId }).then(setEvents).catch(() => {});
      invoke<Approval[]>("pending_approvals").then(setApprovals).catch(() => {});
      refreshSessions().catch(() => {});
    }, 700);
    return () => window.clearInterval(timer);
  }, [activeId, refreshSessions]);

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
    }));
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
    const next = { ...settings, workspace: project.path };
    await invoke("save_settings", { settings: next });
    setSettings(next);
    setDraft(next);
    setActiveId(undefined);
    setMessages([]);
  }

  async function removeProject(project: Project) {
    const projects = settings.projects.filter((item) => item.path !== project.path);
    const workspace = settings.workspace === project.path ? (projects[0]?.path || settings.workspace) : settings.workspace;
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

  async function openWorkspaceFile(path: string, line?: number) {
    const existing = openFiles.find((file) => file.path === path);
    if (existing) {
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
      setOpenFiles((items) => [...items, { path: file.path, content: file.content, savedContent: file.content }]);
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
    setDiffText(await invoke<string>("workspace_git_diff"));
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

  async function searchWorkspace() {
    if (workspaceQuery.trim().length < 2) {
      setWorkspaceMatches([]);
      return;
    }
    try {
      const matches = await invoke<WorkspaceSearchMatch[]>("search_workspace", { query: workspaceQuery });
      setWorkspaceMatches(matches);
      setCodeStatus(`找到 ${matches.length} 条结果${matches.length === 200 ? "，已达到显示上限" : ""}`);
    } catch (error) {
      setCodeStatus(`搜索失败：${String(error)}`);
    }
  }

  async function createWorkspaceEntry(isDir: boolean) {
    const path = window.prompt(isDir ? "输入新文件夹相对路径" : "输入新文件相对路径")?.trim();
    if (!path) return;
    try {
      await invoke("create_workspace_entry", { path, isDir });
      await refreshWorkspaceFiles();
      if (!isDir) await openWorkspaceFile(path);
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

  async function requestCompletion() {
    const editor = editorRef.current;
    if (!editor || !activeDocument) return;
    const model = editor.getModel();
    const position = editor.getPosition();
    if (!model || !position) return;
    setCodeStatus("正在生成代码补全...");
    try {
      const offset = model.getOffsetAt(position);
      const completion = await invoke<string>("inline_completion", {
        path: activeFile, prefix: activeDocument.content.slice(0, offset), suffix: activeDocument.content.slice(offset),
      });
      editor.executeEdits("lan-code-completion", [{ range: new monaco.Range(position.lineNumber, position.column, position.lineNumber, position.column), text: completion }]);
      setCodeStatus("已插入 AI 补全");
    } catch (error) {
      setCodeStatus(`补全失败：${String(error)}`);
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
    if (!settings.apiKey) {
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
  const recentTools = events.filter((event) =>
    ["toolStarted", "toolCompleted", "toolFailed"].includes(event.type)).slice(-8).reverse();
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

  return (
    <div className={`app-shell ${sidebarOpen ? "" : "sidebar-collapsed"}`}>
      {sidebarOpen && <aside className="sidebar">
        <div className="brand"><img src="/lan-code-logo.png" alt="Lan Code" /><strong>Lan Code</strong>
          <button title="收起侧栏" className="icon-button" onClick={() => setSidebarOpen(false)}><PanelLeftClose size={17} /></button>
        </div>
        <div className="mode-switch"><button className={workbench === "agent" ? "active" : ""} onClick={() => setWorkbench("agent")}><MessageSquare size={14} /> Agent</button><button className={workbench === "code" ? "active" : ""} onClick={() => setWorkbench("code")}><Code2 size={14} /> Code</button></div>
        {workbench === "code" ? <>
          <div className="section-label code-label"><span>项目文件</span><div>
            <button title="新建文件" onClick={() => void createWorkspaceEntry(false)}><FilePlus size={13} /></button>
            <button title="新建文件夹" onClick={() => void createWorkspaceEntry(true)}><FolderPlus size={13} /></button>
            <button title="刷新" onClick={() => void refreshWorkspaceFiles()}><RefreshCw size={13} /></button>
          </div></div>
          <div className="workspace-search"><Search size={13} /><input value={workspaceQuery} onChange={(event) => setWorkspaceQuery(event.target.value)} onKeyDown={(event) => { if (event.key === "Enter") void searchWorkspace(); }} placeholder="全文搜索，按 Enter" />{workspaceQuery && <button title="清空" onClick={() => { setWorkspaceQuery(""); setWorkspaceMatches([]); }}>×</button>}</div>
          {workspaceMatches.length > 0 && <div className="search-results">{workspaceMatches.map((match, index) => <button key={`${match.path}:${match.line}:${index}`} title={match.text} onClick={() => void openWorkspaceFile(match.path, match.line)}><strong>{match.path}:{match.line}</strong><span>{match.text}</span></button>)}</div>}
          <div className="file-tree">{workspaceMatches.length === 0 && visibleWorkspaceFiles.map((entry) => <button key={entry.path} title={entry.path} style={{ paddingLeft: `${8 + entry.depth * 13}px` }} className={activeFile === entry.path ? "active" : ""} onClick={() => {
            if (entry.isDir) setCollapsedDirs((items) => {
              const next = new Set(items);
              if (next.has(entry.path)) next.delete(entry.path); else next.add(entry.path);
              return next;
            });
            else void openWorkspaceFile(entry.path);
          }}>{entry.isDir ? collapsedDirs.has(entry.path) ? <ChevronRight size={13} /> : <ChevronDown size={13} /> : <span className="tree-spacer" />} {entry.isDir ? collapsedDirs.has(entry.path) ? <Folder size={14} /> : <FolderOpen size={14} /> : <File size={14} />}<span>{entry.name}</span></button>)}</div>
          <button className="settings-button" onClick={() => { setDraft(settings); setSettingsOpen(true); }}><Settings size={16} /> 设置</button>
        </> : <>
        <button className="new-chat" onClick={() => void newSession()}><Plus size={16} /> 新对话</button>
        <nav>
          <button className={searchOpen ? "active" : ""} onClick={() => setSearchOpen(!searchOpen)}><Search size={16} /> 搜索</button>
          <button onClick={() => { setSearchOpen(true); setQuery(""); }}><History size={16} /> 历史</button>
          <button onClick={() => void addProject()}><FolderPlus size={16} /> 添加项目</button>
        </nav>
        {searchOpen && <input className="session-search" autoFocus placeholder="搜索会话或路径" value={query} onChange={(e) => setQuery(e.target.value)} />}
        <div className="section-label">项目</div>
        <div className="sessions project-list">{settings.projects.map((project) => (
          <div className="session-row" key={project.path}>
            <button className={project.path === settings.workspace ? "active" : ""} onClick={() => void selectProject(project)}>
              <FolderGit2 size={15} /><span>{project.name}</span>
            </button>
            <button title="移除项目" className="row-action" onClick={() => void removeProject(project)}><Trash2 size={14} /></button>
          </div>
        ))}</div>
        <div className="section-label">最近对话</div>
        <div className="sessions">{filtered.map((session) => (
          <div className="session-row" key={session.id}>
            <button title="双击可重命名" className={session.id === activeId ? "active" : ""} onClick={() => setActiveId(session.id)} onDoubleClick={() => void renameSession(session)}>
              <Code2 size={15} /><span>{session.title || "未命名对话"}</span><i className={`status ${session.status}`} />
            </button>
            <button title="删除对话" className="row-action" onClick={() => void removeSession(session.id)}><Trash2 size={14} /></button>
          </div>
        ))}</div>
        <button className="settings-button" onClick={() => { setDraft(settings); setSettingsOpen(true); }}><Settings size={16} /> 设置</button>
        </>}
      </aside>}

      {workbench === "agent" ? <main>
        <header>
          <div className="title-row">
            {!sidebarOpen && <button title="展开侧栏" className="icon-button" onClick={() => setSidebarOpen(true)}><PanelLeftOpen size={17} /></button>}
            <div><h1>{sessions.find((item) => item.id === activeId)?.title || "开始新的编码任务"}</h1><span className="subtle">{settings.workspace || "尚未选择工作区"}</span></div>
          </div>
          <div className="header-actions">
            <button className="pill" onClick={() => { setDraft(settings); void chooseWorkspace().then(() => setSettingsOpen(true)); }}><GitBranch size={15} /> 工作区 <ChevronDown size={14} /></button>
            <button className="pill" onClick={() => { setDraft(settings); setSettingsOpen(true); }}><ShieldCheck size={15} /> {settings.approvalMode}</button>
          </div>
        </header>
        {fatal && <div className="error-banner"><XCircle size={16} />{fatal}<button onClick={() => setFatal("")}>关闭</button></div>}
        <section className="conversation">
          {messages.length === 0 ? <div className="welcome">
            <img src="/lan-code-logo.png" alt="" /><h2>{settings.apiKey ? "把想法交给 Lan Code" : "先完成首次配置"}</h2>
            <p>{settings.apiKey ? "它会先理解仓库，再修改代码、运行检查并审阅差异。" : "配置 API Key 和工作区后即可开始真实编码任务。"}</p>
            <div className="suggestions">
              {!settings.apiKey && <button onClick={() => setSettingsOpen(true)}><KeyRound size={18} /> 配置 API</button>}
              <button onClick={() => setPrompt("阅读当前项目并解释架构")}><Bot size={18} /> 理解项目</button>
              <button onClick={() => setPrompt("检查当前 Git diff 并指出风险")}><FileDiff size={18} /> 审阅改动</button>
              <button onClick={() => setPrompt("运行测试并修复失败项")}><CheckCircle2 size={18} /> 修复测试</button>
              <button onClick={() => void understandImage()}><Image size={18} /> 图片理解</button>
              <button onClick={() => void createImage()}><Sparkles size={18} /> 生成图片</button>
            </div>
          </div> : <div className="messages">
            {messages.map((message, index) => <article key={index} className={message.role}><div className="message-label">{message.role === "user" ? "你" : "Lan Code"}</div><p>{message.text}</p></article>)}
            {busy && <article className="assistant thinking"><Sparkles size={16} /> 正在分析并执行...</article>}
          </div>}
        </section>
        <div className="composer-wrap"><div className="composer">
          <textarea value={prompt} onChange={(e) => setPrompt(e.target.value)} onKeyDown={(e) => { if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); void send(); } }} placeholder={settings.apiKey ? "描述你想完成的编码任务" : "请先在设置中配置 API"} />
          <div className="composer-footer"><div>
            <button title="选择工作区" className="mini" onClick={() => { setDraft(settings); void chooseWorkspace().then(() => setSettingsOpen(true)); }}><Plus size={15} /></button>
            <button className="mini" onClick={() => { setDraft(settings); setSettingsOpen(true); }}><TerminalSquare size={15} /> {settings.model}</button>
          </div>{busy ? <button className="send stop" onClick={interrupt}><CircleStop size={17} /></button> : <button className="send" disabled={!settings.apiKey} onClick={send}><Send size={17} /></button>}</div>
        </div></div>
      </main> : <main className="code-main">
        <header>
          <div className="title-row">{!sidebarOpen && <button title="展开侧栏" className="icon-button" onClick={() => setSidebarOpen(true)}><PanelLeftOpen size={17} /></button>}<div><h1>{activeFile || "Code 工作台"}</h1><span className="subtle">{activeDocument && activeDocument.content !== activeDocument.savedContent ? "有未保存修改" : settings.workspace}</span></div></div>
          <div className="header-actions"><button className="pill" onClick={() => void requestCompletion()} disabled={!activeFile}><Sparkles size={15} /> AI 补全</button><button className="pill" onClick={() => void renameWorkspaceEntry()} disabled={!activeFile}><Pencil size={15} /> 重命名</button><button className="pill danger" onClick={() => void deleteWorkspaceEntry()} disabled={!activeFile}><Trash2 size={15} /> 删除</button><button className="pill" onClick={() => void saveWorkspaceFile()} disabled={!activeDocument || activeDocument.content === activeDocument.savedContent}><Save size={15} /> 保存</button><button className="pill" onClick={() => invoke<string>("workspace_git_diff").then(setDiffText)}><FileDiff size={15} /> 查看改动</button></div>
        </header>
        {codeStatus && <div className="code-status">{codeStatus}</div>}
        <div className="editor-tabs">{openFiles.length ? openFiles.map((file) => <button key={file.path} className={file.path === activeFile ? "active" : ""} onClick={() => setActiveFile(file.path)}><File size={13} /><span>{file.path.split(/[\\/]/).at(-1)}</span>{file.content !== file.savedContent && <i />}<b title="关闭" onClick={(event) => { event.stopPropagation(); closeWorkspaceFile(file.path); }}>×</b></button>) : <span>从左侧项目树打开文件</span>}</div>
        <div className="editor-host">{activeDocument ? <Editor
          path={activeFile}
          value={activeDocument.content}
          onChange={(value) => setOpenFiles((items) => items.map((file) => file.path === activeFile ? { ...file, content: value || "" } : file))}
          onMount={(editor) => {
            editorRef.current = editor;
            editor.focus();
            monaco.languages.registerInlineCompletionsProvider("*", {
              provideInlineCompletions: async (model, position) => {
                window.clearTimeout(completionTimer.current);
                await new Promise<void>((resolve) => {
                  completionTimer.current = window.setTimeout(resolve, 700);
                });
                const offset = model.getOffsetAt(position);
                const value = model.getValue();
                try {
                  const completion = await invoke<string>("inline_completion", {
                    path: activeFile, prefix: value.slice(0, offset), suffix: value.slice(offset),
                  });
                  return { items: [{ insertText: completion, range: new monaco.Range(position.lineNumber, position.column, position.lineNumber, position.column) }] };
                } catch {
                  return { items: [] };
                }
              },
              disposeInlineCompletions: () => {},
            });
          }}
          theme="vs"
          options={{ automaticLayout: true, minimap: { enabled: true }, fontSize: 13, tabSize: 2, wordWrap: "off", inlineSuggest: { enabled: true } }}
        /> : <div className="code-welcome"><Code2 size={46} /><h2>Lan Code 工作台</h2><p>浏览项目、编辑代码、查看改动，并让同一个 Agent 理解当前仓库。</p></div>}</div>
      </main>}

      <aside className={`inspector ${workbench === "code" ? "code-inspector" : ""}`}>
        {workbench === "code" ? <>
          <h3>AI 助手</h3>
          <div className="code-chat">{messages.length === 0 ? <div className="empty-small">针对当前项目提问，Agent 会使用同一套工具、权限和会话。</div> : messages.slice(-8).map((message, index) => <article key={index} className={message.role}><div className="message-label">{message.role === "user" ? "你" : "Lan Code"}</div><p>{message.text}</p></article>)}</div>
          <div className="code-chat-composer"><textarea value={prompt} onChange={(event) => setPrompt(event.target.value)} placeholder="询问代码或要求修改项目" /><button onClick={() => void send()} disabled={busy || !prompt.trim()}><Send size={15} /></button></div>
          <div className="divider" /><h3>Git 改动</h3><pre className="diff-preview">{diffText || "点击顶部“查看改动”加载 Git diff。"}</pre>
        </> : <>
        <h3>环境信息</h3>
        <button className="info-row clickable" onClick={() => { setDraft(settings); void chooseWorkspace().then(() => setSettingsOpen(true)); }}><FolderGit2 size={16} /><span>工作区</span><strong>{settings.workspace ? "已选择" : "未配置"}</strong></button>
        <button className="info-row clickable" onClick={() => { setDraft(settings); setSettingsOpen(true); }}><KeyRound size={16} /><span>模型</span><strong>{settings.apiKey ? settings.model : "未配置"}</strong></button>
        <button className="info-row clickable" onClick={() => { setDraft(settings); setSettingsOpen(true); }}><ShieldCheck size={16} /><span>权限</span><strong>{settings.approvalMode}</strong></button>
        <div className="divider" /><h3>当前对话用量</h3>
        <div className="usage-grid"><span>输入 Token<strong>{usage.inputTokens.toLocaleString()}</strong></span><span>输出 Token<strong>{usage.outputTokens.toLocaleString()}</strong></span><span>预估费用<strong>${estimatedCost.toFixed(4)}</strong></span></div>
        <div className="divider" /><h3>工具进度</h3>
        {recentTools.length === 0 ? <div className="empty-small">发送任务后在这里查看工具执行过程</div> : recentTools.map((event, index) => (
          <div className={`progress-item ${event.type === "toolCompleted" ? "done" : event.type === "toolFailed" ? "failed" : ""}`} key={index}>
            {event.type === "toolFailed" ? <XCircle size={15} /> : <CheckCircle2 size={15} />} {event.toolName}
          </div>
        ))}
        </>}
      </aside>

      {approvals.length > 0 && <div className="approval-bar"><div><strong>需要你的批准</strong><span>{approvals[0].toolName}：{approvals[0].reason}</span></div><button onClick={() => decide(approvals[0].id, "deny")}>拒绝</button><button className="primary-inline" onClick={() => decide(approvals[0].id, "allowOnce")}>允许一次</button></div>}

      {settingsOpen && <div className="modal-backdrop"><div className="modal settings-modal">
        <div className="modal-title"><div><h2>Lan Code 设置</h2><p>默认保存在 ~/.lancode，也可以选择其他数据目录。</p></div><button className="icon-button" onClick={() => setSettingsOpen(false)}>×</button></div>
        <div className="form-grid">
          <label>模型服务<select value={draft.provider} onChange={(e) => applyProvider(e.target.value)}>
            {PROVIDERS.map((provider) => <option value={provider.id} key={provider.id}>{provider.name}</option>)}
          </select></label>
          <label>单任务最大执行轮次<input type="number" min="4" max="256" value={draft.maxProviderRounds} onChange={(e) => setDraft({ ...draft, maxProviderRounds: Number(e.target.value) })} /></label>
          <label>API 地址<input value={draft.baseUrl} onChange={(e) => setDraft({ ...draft, baseUrl: e.target.value })} /></label>
          <label>模型名称<input value={draft.model} onChange={(e) => setDraft({ ...draft, model: e.target.value })} /></label>
          <label className="span-2">API Key<input type="password" placeholder="sk-..." value={draft.apiKey} onChange={(e) => setDraft({ ...draft, apiKey: e.target.value })} /></label>
          <label className="span-2">工作区<div className="input-action"><input value={draft.workspace} onChange={(e) => setDraft({ ...draft, workspace: e.target.value })} /><button onClick={chooseWorkspace}>选择文件夹</button></div></label>
          <label className="span-2">数据保存目录<div className="input-action"><input value={draft.dataDir} onChange={(e) => setDraft({ ...draft, dataDir: e.target.value })} /><button onClick={chooseDataDir}>选择目录</button></div></label>
          <label>权限模式<select value={draft.approvalMode} onChange={(e) => setDraft({ ...draft, approvalMode: e.target.value as Mode })}><option value="readOnly">只读</option><option value="ask">每次询问</option><option value="workspace">工作区写入</option><option value="fullAccess">完全访问</option></select></label>
          <label>输入价格（美元/百万 Token）<input type="number" min="0" step="0.01" value={draft.inputPricePerMillion} onChange={(e) => setDraft({ ...draft, inputPricePerMillion: Number(e.target.value) })} /></label>
          <label>输出价格（美元/百万 Token）<input type="number" min="0" step="0.01" value={draft.outputPricePerMillion} onChange={(e) => setDraft({ ...draft, outputPricePerMillion: Number(e.target.value) })} /></label>
        </div>
        <div className="profile-section">
          <div className="profile-heading"><strong>Provider 配置档案</strong><button onClick={saveProviderProfile}>保存当前配置</button></div>
          {draft.providerProfiles.length === 0 ? <span>可保存多套模型地址、Key 和计费参数，切换时无需重复填写。</span> : draft.providerProfiles.map((profile) => (
            <div className="profile-row" key={profile.id}><button onClick={() => applyProviderProfile(profile)}><strong>{profile.name}</strong><span>{profile.model}</span></button><button onClick={() => removeProviderProfile(profile)}><Trash2 size={14} /></button></div>
          ))}
        </div>
        <div className="capability-section">
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
        </div>
        <div className="settings-note">API Key 会明文保存在所选数据目录的 settings.json 中，请不要把该文件提交到 Git 或分享给他人。</div>
        <div className="update-card">
          <div><strong><Zap size={15} /> 软件更新</strong><span>{updateStatus || "仅从 Lan Code 官方 GitHub Release 检查和下载更新。"}</span></div>
          <div>
            <button onClick={checkUpdates}><RefreshCw size={14} /> 检查更新</button>
            {updateInfo?.available && !downloadedUpdate && <button onClick={downloadUpdate}><Download size={14} /> 下载 v{updateInfo.latestVersion}</button>}
            {downloadedUpdate && <button className="primary-inline" onClick={installUpdate}>退出并安装</button>}
          </div>
        </div>
        {settingsStatus && <div className={settingsStatus.includes("失败") ? "settings-status failed" : "settings-status"}>{settingsStatus}</div>}
        <div className="modal-actions"><button onClick={testProvider}>测试 API</button><button className="primary-inline" onClick={saveSettings}>保存并启用</button></div>
      </div></div>}
    </div>
  );
}

ReactDOM.createRoot(document.getElementById("root")!).render(<App />);
