import React from "react";
import ReactDOM from "react-dom/client";
import { invoke } from "@tauri-apps/api/core";
import {
  Bot,
  CheckCircle2,
  ChevronDown,
  CircleStop,
  Code2,
  FileDiff,
  FolderGit2,
  GitBranch,
  History,
  PanelLeftClose,
  Plus,
  Search,
  Send,
  Settings,
  ShieldCheck,
  Sparkles,
  TerminalSquare,
} from "lucide-react";
import "./styles.css";

type Session = {
  id: string;
  cwd: string;
  title?: string;
  status: "idle" | "running" | "waitingForApproval" | "interrupted" | "failed";
};

type Message = {
  role: "user" | "assistant";
  text: string;
};

const isTauri = "__TAURI_INTERNALS__" in window;

function App() {
  const [sessions, setSessions] = React.useState<Session[]>([]);
  const [activeId, setActiveId] = React.useState<string>();
  const [messages, setMessages] = React.useState<Message[]>([]);
  const [prompt, setPrompt] = React.useState("");
  const [cwd, setCwd] = React.useState("D:\\Lan Code");
  const [mode, setMode] = React.useState("workspace");
  const [busy, setBusy] = React.useState(false);
  const [showSettings, setShowSettings] = React.useState(false);

  React.useEffect(() => {
    if (isTauri) {
      invoke<Session[]>("list_sessions").then(setSessions).catch(console.error);
    }
  }, []);

  async function newSession() {
    const session = isTauri
      ? await invoke<Session>("create_session", { cwd, title: "新对话" })
      : { id: crypto.randomUUID(), cwd, title: "新对话", status: "idle" as const };
    setSessions((items) => [session, ...items]);
    setActiveId(session.id);
    setMessages([]);
  }

  async function send() {
    const text = prompt.trim();
    if (!text || busy) return;
    let sessionId = activeId;
    if (!sessionId) {
      const session = isTauri
        ? await invoke<Session>("create_session", { cwd, title: text.slice(0, 30) })
        : { id: crypto.randomUUID(), cwd, title: text.slice(0, 30), status: "idle" as const };
      setSessions((items) => [session, ...items]);
      sessionId = session.id;
      setActiveId(session.id);
    }
    setMessages((items) => [...items, { role: "user", text }]);
    setPrompt("");
    setBusy(true);
    try {
      const result = isTauri
        ? await invoke<{ text: string }>("start_turn", { sessionId, prompt: text, mode })
        : { text: "桌面预览模式已就绪。连接 Tauri 后，这里会由 Lan Code Core 执行真实编码任务。" };
      setMessages((items) => [...items, { role: "assistant", text: result.text }]);
    } catch (error) {
      setMessages((items) => [...items, { role: "assistant", text: `执行失败：${String(error)}` }]);
    } finally {
      setBusy(false);
    }
  }

  async function interrupt() {
    if (activeId && isTauri) await invoke("interrupt_turn", { sessionId: activeId });
    setBusy(false);
  }

  return (
    <div className="app-shell">
      <aside className="sidebar">
        <div className="brand">
          <img src="/lan-code-logo.png" alt="Lan Code" />
          <strong>Lan Code</strong>
          <button className="icon-button"><PanelLeftClose size={17} /></button>
        </div>
        <button className="new-chat" onClick={newSession}><Plus size={16} /> 新对话</button>
        <nav>
          <button><Search size={16} /> 搜索</button>
          <button><History size={16} /> 历史</button>
          <button><FolderGit2 size={16} /> 工作区</button>
        </nav>
        <div className="section-label">最近对话</div>
        <div className="sessions">
          {sessions.map((session) => (
            <button
              key={session.id}
              className={session.id === activeId ? "active" : ""}
              onClick={() => setActiveId(session.id)}
            >
              <Code2 size={15} />
              <span>{session.title || "未命名对话"}</span>
            </button>
          ))}
        </div>
        <button className="settings-button" onClick={() => setShowSettings(!showSettings)}>
          <Settings size={16} /> 设置
        </button>
      </aside>

      <main>
        <header>
          <div>
            <h1>{sessions.find((item) => item.id === activeId)?.title || "开始新的编码任务"}</h1>
            <span className="subtle">{cwd}</span>
          </div>
          <div className="header-actions">
            <button className="pill"><GitBranch size={15} /> main <ChevronDown size={14} /></button>
            <button className="pill"><ShieldCheck size={15} /> {mode}</button>
          </div>
        </header>

        <section className="conversation">
          {messages.length === 0 ? (
            <div className="welcome">
              <img src="/lan-code-logo.png" alt="" />
              <h2>把想法交给 Lan Code</h2>
              <p>它会先理解仓库，再修改代码、运行检查并审阅差异。</p>
              <div className="suggestions">
                <button onClick={() => setPrompt("阅读当前项目并解释架构")}><Bot size={18} /> 理解项目</button>
                <button onClick={() => setPrompt("检查当前 Git diff 并指出风险")}><FileDiff size={18} /> 审阅改动</button>
                <button onClick={() => setPrompt("运行测试并修复失败项")}><CheckCircle2 size={18} /> 修复测试</button>
              </div>
            </div>
          ) : (
            <div className="messages">
              {messages.map((message, index) => (
                <article key={index} className={message.role}>
                  <div className="message-label">{message.role === "user" ? "你" : "Lan Code"}</div>
                  <p>{message.text}</p>
                </article>
              ))}
              {busy && <article className="assistant thinking"><Sparkles size={16} /> 正在分析工作区...</article>}
            </div>
          )}
        </section>

        <div className="composer-wrap">
          <div className="composer">
            <textarea
              value={prompt}
              onChange={(event) => setPrompt(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === "Enter" && !event.shiftKey) {
                  event.preventDefault();
                  void send();
                }
              }}
              placeholder="描述你想完成的编码任务"
            />
            <div className="composer-footer">
              <div>
                <button className="mini"><Plus size={15} /></button>
                <button className="mini"><TerminalSquare size={15} /> 本地</button>
              </div>
              {busy ? (
                <button className="send stop" onClick={interrupt}><CircleStop size={17} /></button>
              ) : (
                <button className="send" onClick={send}><Send size={17} /></button>
              )}
            </div>
          </div>
        </div>
      </main>

      <aside className="inspector">
        <h3>环境信息</h3>
        <div className="info-row"><FolderGit2 size={16} /><span>工作区</span><strong>本地</strong></div>
        <div className="info-row"><GitBranch size={16} /><span>分支</span><strong>main</strong></div>
        <div className="info-row"><ShieldCheck size={16} /><span>权限</span><strong>{mode}</strong></div>
        <div className="divider" />
        <h3>进度</h3>
        <div className="progress-item done"><CheckCircle2 size={16} /> Core 0.1</div>
        <div className="progress-item done"><CheckCircle2 size={16} /> CLI 客户端</div>
        <div className="progress-item"><div className="pulse" /> 桌面端会话</div>
      </aside>

      {showSettings && (
        <div className="modal-backdrop" onClick={() => setShowSettings(false)}>
          <div className="modal" onClick={(event) => event.stopPropagation()}>
            <h2>工作区设置</h2>
            <label>工作目录<input value={cwd} onChange={(event) => setCwd(event.target.value)} /></label>
            <label>权限模式<select value={mode} onChange={(event) => setMode(event.target.value)}>
              <option value="readOnly">只读</option>
              <option value="ask">每次询问</option>
              <option value="workspace">工作区写入</option>
              <option value="fullAccess">完全访问</option>
            </select></label>
            <button className="primary" onClick={() => setShowSettings(false)}>完成</button>
          </div>
        </div>
      )}
    </div>
  );
}

ReactDOM.createRoot(document.getElementById("root")!).render(<App />);
