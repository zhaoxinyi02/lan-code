import React from "react";
import ReactDOM from "react-dom/client";
import { invoke } from "@tauri-apps/api/core";
import {
  Bot, CheckCircle2, ChevronDown, CircleStop, Code2, FileDiff, FolderGit2,
  FolderPlus, GitBranch, History, KeyRound, PanelLeftClose, PanelLeftOpen, Plus,
  Search, Send, Settings, ShieldCheck, Sparkles, TerminalSquare, Trash2, XCircle,
} from "lucide-react";
import "./styles.css";

type Mode = "readOnly" | "ask" | "workspace" | "fullAccess";
type Session = { id: string; cwd: string; title?: string; status: string };
type Project = { name: string; path: string };
type SettingsData = {
  provider: string; baseUrl: string; model: string; apiKey: string; workspace: string;
  dataDir: string; approvalMode: Mode; maxProviderRounds: number; projects: Project[];
};
type ModelMessage = { role: "system" | "user" | "assistant" | "tool"; content?: string };
type CoreEvent = { type: string; toolName?: string; error?: string; text?: string };
type Approval = { id: string; toolName: string; reason: string; arguments: unknown };
type ChatMessage = { role: "user" | "assistant"; text: string };

const DEFAULT_SETTINGS: SettingsData = {
  provider: "deepseek",
  baseUrl: "https://api.deepseek.com",
  model: "deepseek-v4-pro",
  apiKey: "",
  workspace: "",
  dataDir: "",
  approvalMode: "readOnly",
  maxProviderRounds: 48,
  projects: [],
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
    setSettingsStatus("正在测试 API...");
    try {
      const result = await invoke<string>("test_provider", { settings: draft });
      setSettingsStatus(`连接成功：${result || "模型已响应"}`);
    } catch (error) {
      setSettingsStatus(`连接失败：${String(error)}`);
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

  return (
    <div className={`app-shell ${sidebarOpen ? "" : "sidebar-collapsed"}`}>
      {sidebarOpen && <aside className="sidebar">
        <div className="brand"><img src="/lan-code-logo.png" alt="Lan Code" /><strong>Lan Code</strong>
          <button title="收起侧栏" className="icon-button" onClick={() => setSidebarOpen(false)}><PanelLeftClose size={17} /></button>
        </div>
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
            <button className={session.id === activeId ? "active" : ""} onClick={() => setActiveId(session.id)}>
              <Code2 size={15} /><span>{session.title || "未命名对话"}</span><i className={`status ${session.status}`} />
            </button>
            <button title="删除对话" className="row-action" onClick={() => void removeSession(session.id)}><Trash2 size={14} /></button>
          </div>
        ))}</div>
        <button className="settings-button" onClick={() => { setDraft(settings); setSettingsOpen(true); }}><Settings size={16} /> 设置</button>
      </aside>}

      <main>
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
      </main>

      <aside className="inspector">
        <h3>环境信息</h3>
        <button className="info-row clickable" onClick={() => { setDraft(settings); void chooseWorkspace().then(() => setSettingsOpen(true)); }}><FolderGit2 size={16} /><span>工作区</span><strong>{settings.workspace ? "已选择" : "未配置"}</strong></button>
        <button className="info-row clickable" onClick={() => { setDraft(settings); setSettingsOpen(true); }}><KeyRound size={16} /><span>模型</span><strong>{settings.apiKey ? settings.model : "未配置"}</strong></button>
        <button className="info-row clickable" onClick={() => { setDraft(settings); setSettingsOpen(true); }}><ShieldCheck size={16} /><span>权限</span><strong>{settings.approvalMode}</strong></button>
        <div className="divider" /><h3>工具进度</h3>
        {recentTools.length === 0 ? <div className="empty-small">发送任务后在这里查看工具执行过程</div> : recentTools.map((event, index) => (
          <div className={`progress-item ${event.type === "toolCompleted" ? "done" : event.type === "toolFailed" ? "failed" : ""}`} key={index}>
            {event.type === "toolFailed" ? <XCircle size={15} /> : <CheckCircle2 size={15} />} {event.toolName}
          </div>
        ))}
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
        </div>
        <div className="settings-note">API Key 会明文保存在所选数据目录的 settings.json 中，请不要把该文件提交到 Git 或分享给他人。</div>
        {settingsStatus && <div className={settingsStatus.includes("失败") ? "settings-status failed" : "settings-status"}>{settingsStatus}</div>}
        <div className="modal-actions"><button onClick={testProvider}>测试 API</button><button className="primary-inline" onClick={saveSettings}>保存并启用</button></div>
      </div></div>}
    </div>
  );
}

ReactDOM.createRoot(document.getElementById("root")!).render(<App />);
