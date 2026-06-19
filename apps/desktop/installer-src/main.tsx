import React from "react";
import { createRoot } from "react-dom/client";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  ArrowLeft,
  Check,
  ChevronRight,
  FolderOpen,
  Minus,
  Rocket,
  ShieldCheck,
  Sparkles,
  X,
} from "lucide-react";
import logoUrl from "../public/lan-code-logo.png";
import "./styles.css";

type Page = "welcome" | "options" | "installing" | "done" | "uninstall";
type ProgressPayload = { percent: number; label: string };
type InstallResult = { installDir: string; executable: string };
type InstalledInfo = {
  installed: boolean;
  version: string;
  installDir: string;
  action: "install" | "update" | "repair";
};

function App() {
  const [page, setPage] = React.useState<Page>("welcome");
  const [installDir, setInstallDir] = React.useState("");
  const [desktopShortcut, setDesktopShortcut] = React.useState(true);
  const [startMenuShortcut, setStartMenuShortcut] = React.useState(true);
  const [contextMenus, setContextMenus] = React.useState(true);
  const [launchAfterInstall, setLaunchAfterInstall] = React.useState(true);
  const [progress, setProgress] = React.useState(0);
  const [progressLabel, setProgressLabel] = React.useState("正在准备安装...");
  const [result, setResult] = React.useState<InstallResult>();
  const [error, setError] = React.useState("");
  const [installed, setInstalled] = React.useState<InstalledInfo>();

  React.useEffect(() => {
    void invoke<string>("installer_mode").then((mode) => {
      if (mode === "uninstall") setPage("uninstall");
    });
    void invoke<string>("default_install_dir").then(setInstallDir);
    void invoke<InstalledInfo>("installed_info").then(setInstalled);
    const unlisten = listen<ProgressPayload>("install-progress", ({ payload }) => {
      setProgress(payload.percent);
      setProgressLabel(payload.label);
    });
    return () => { void unlisten.then((fn) => fn()); };
  }, []);

  const isUpdate = installed?.installed && installed.action === "update";
  const isRepair = installed?.installed && installed.action === "repair";
  const actionLabel = isUpdate ? "更新" : isRepair ? "重新安装" : "安装";

  async function chooseDirectory() {
    const selected = await invoke<string | null>("choose_install_dir", { current: installDir });
    if (selected) setInstallDir(selected);
  }

  async function install() {
    setError("");
    setPage("installing");
    try {
      const installed = await invoke<InstallResult>("install_app", {
        installDir,
        desktopShortcut,
        startMenuShortcut,
        contextMenus,
      });
      setResult(installed);
      setProgress(100);
      setProgressLabel("Lan Code 已准备就绪");
      window.setTimeout(() => setPage("done"), 420);
    } catch (reason) {
      setError(String(reason));
      setPage("options");
    }
  }

  async function finish() {
    if (launchAfterInstall && result) {
      await invoke("launch_installed", { executable: result.executable });
    }
    await getCurrentWindow().close();
  }

  async function uninstall() {
    setError("");
    setPage("installing");
    setProgressLabel("正在移除 Lan Code...");
    try {
      await invoke("uninstall_app");
      setProgress(100);
      setProgressLabel("Lan Code 已完成卸载");
      setLaunchAfterInstall(false);
      setPage("done");
    } catch (reason) {
      setError(String(reason));
      setPage("uninstall");
    }
  }

  return <div className="installer-shell">
    <header className="installer-titlebar" data-tauri-drag-region>
      <div className="title-brand" data-tauri-drag-region><img src={logoUrl} alt="" /><span>Lan Code Setup</span></div>
      <div className="window-actions">
        <button title="最小化" onClick={() => void getCurrentWindow().minimize()}><Minus size={15} /></button>
        <button title="关闭" onClick={() => void getCurrentWindow().close()}><X size={15} /></button>
      </div>
    </header>

    <aside className="installer-visual">
      <div className="visual-halo" />
      <img className="hero-logo" src={logoUrl} alt="Lan Code" />
      <div className="hero-copy">
        <span className="eyebrow">LAN CODE 0.2.10</span>
        <h1>让想法自然<br />成为作品</h1>
        <p>Agent、Code 与 Office，在一个安静而强大的工作台里协同。</p>
      </div>
      <div className="feature-row">
        <span><Sparkles size={15} /> 多模型 Agent</span>
        <span><ShieldCheck size={15} /> 本地工作区</span>
      </div>
    </aside>

    <main className="installer-content">
      <div className={`page ${page}`} key={page}>
        {page === "welcome" && <>
          <div className="page-icon"><Rocket size={22} /></div>
          <span className="page-kicker">{installed?.installed ? "检测到已安装版本" : "欢迎使用"}</span>
          <h2>{actionLabel} Lan Code</h2>
          <p className="lead">{installed?.installed
            ? `当前已安装 ${installed.version || "旧版本"}，位置在 ${installed.installDir}。`
            : "一套面向真实项目的 AI 创作工作台。安装过程只需片刻。"}
          </p>
          <div className="benefits">
            <div><Check size={16} /><span><strong>统一工作流</strong><small>对话、代码、终端、Git 和文档协作</small></span></div>
            <div><Check size={16} /><span><strong>数据由你掌控</strong><small>项目与配置保存在你的电脑中</small></span></div>
            <div><Check size={16} /><span><strong>随时更新</strong><small>从 GitHub Release 安全获取新版本</small></span></div>
          </div>
          <button className="primary-action" onClick={() => setPage("options")}>{actionLabel}选项 <ChevronRight size={17} /></button>
        </>}

        {page === "options" && <>
          <button className="back-button" onClick={() => setPage("welcome")}><ArrowLeft size={15} /> 返回</button>
          <span className="page-kicker">安装选项</span>
          <h2>{installed?.installed ? "确认更新位置" : "选择安装位置"}</h2>
          <p className="lead compact">{installed?.installed ? "默认沿用当前安装位置，也可以迁移到新的目录。" : "你可以使用推荐位置，也可以放到自己习惯的目录。"}</p>
          <label className="path-field">
            <span>安装到</span>
            <div><input value={installDir} onChange={(event) => setInstallDir(event.target.value)} /><button title="选择文件夹" onClick={() => void chooseDirectory()}><FolderOpen size={17} /></button></div>
          </label>
          <div className="installer-options">
            <label><input type="checkbox" checked={desktopShortcut} onChange={(event) => setDesktopShortcut(event.target.checked)} /><i /><span>创建桌面快捷方式</span></label>
            <label><input type="checkbox" checked={startMenuShortcut} onChange={(event) => setStartMenuShortcut(event.target.checked)} /><i /><span>添加到开始菜单</span></label>
            <label><input type="checkbox" checked={contextMenus} onChange={(event) => setContextMenus(event.target.checked)} /><i /><span>添加“用 Lan Code 打开”到文件和文件夹右键菜单</span></label>
          </div>
          {error && <div className="installer-error">{error}</div>}
          <button className="primary-action" onClick={() => void install()}>开始{actionLabel} <ChevronRight size={17} /></button>
        </>}

        {page === "installing" && <>
          <div className="progress-orbit"><img src={logoUrl} alt="" /><svg viewBox="0 0 120 120"><circle cx="60" cy="60" r="54" /><circle className="progress-value" cx="60" cy="60" r="54" style={{ strokeDashoffset: 339.3 * (1 - progress / 100) }} /></svg></div>
          <span className="page-kicker">正在安装</span>
          <h2>{progressLabel}</h2>
          <p className="lead compact">请稍候，我们正在把 Lan Code 安放到你的电脑上。</p>
          <div className="progress-track"><i style={{ width: `${progress}%` }} /></div>
          <strong className="progress-number">{Math.round(progress)}%</strong>
        </>}

        {page === "done" && <>
          <div className="success-mark"><Check size={28} /></div>
          <span className="page-kicker">一切就绪</span>
          <h2>{launchAfterInstall ? "欢迎来到 Lan Code" : "已完成卸载"}</h2>
          <p className="lead">{launchAfterInstall ? "安装已经完成。现在可以进入工作台，让第一个想法开始生长。" : "Lan Code 已从这台电脑中移除。"}</p>
          {launchAfterInstall && <label className="launch-option"><input type="checkbox" checked={launchAfterInstall} onChange={(event) => setLaunchAfterInstall(event.target.checked)} /><i /><span>完成后启动 Lan Code</span></label>}
          <button className="primary-action" onClick={() => void finish()}>{launchAfterInstall ? "进入 Lan Code" : "关闭"} <ChevronRight size={17} /></button>
        </>}

        {page === "uninstall" && <>
          <div className="page-icon danger"><X size={22} /></div>
          <span className="page-kicker">卸载程序</span>
          <h2>移除 Lan Code？</h2>
          <p className="lead">程序文件和快捷方式会被移除。你的项目文件与 <code>.lancode</code> 数据目录不会被删除。</p>
          {error && <div className="installer-error">{error}</div>}
          <div className="dual-actions"><button className="secondary-action" onClick={() => void getCurrentWindow().close()}>取消</button><button className="primary-action danger-action" onClick={() => void uninstall()}>确认卸载</button></div>
        </>}
      </div>
    </main>
  </div>;
}

createRoot(document.getElementById("root")!).render(<App />);
