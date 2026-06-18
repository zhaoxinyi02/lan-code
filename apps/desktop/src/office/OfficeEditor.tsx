import React from "react";
import { invoke } from "@tauri-apps/api/core";
import { renderAsync } from "docx-preview";
import ExcelJS from "exceljs";
import { createUniver, LocaleType } from "@univerjs/presets";
import { UniverSheetsCorePreset } from "@univerjs/preset-sheets-core";
import sheetsZhCN from "@univerjs/preset-sheets-core/locales/zh-CN";
import { PPTXViewer } from "pptxviewjs";
import { ArrowLeft, ArrowRight, Bold, Check, Italic, Save, Underline } from "lucide-react";
import "@univerjs/preset-sheets-core/lib/index.css";

type OfficeDocument = {
  path: string;
  name: string;
  kind: string;
  text: string;
  sections: Array<{ id: string; title: string; kind: string; index: number; text: string; children: OfficeDocument["sections"] }>;
  wordCount: number;
  objectCount: number;
  warnings: string[];
};

type OfficeBinary = { path: string; mime: string; base64: string };
type Props = {
  document: OfficeDocument;
  onSaved: (document: OfficeDocument) => void;
  onStatus: (message: string) => void;
};

function decodeBase64(value: string) {
  const binary = atob(value);
  const bytes = new Uint8Array(binary.length);
  for (let index = 0; index < binary.length; index += 1) bytes[index] = binary.charCodeAt(index);
  return bytes;
}

function encodeBase64(value: ArrayBuffer | Uint8Array) {
  const bytes = value instanceof Uint8Array ? value : new Uint8Array(value);
  let binary = "";
  const chunk = 0x8000;
  for (let index = 0; index < bytes.length; index += chunk) {
    binary += String.fromCharCode(...bytes.subarray(index, index + chunk));
  }
  return btoa(binary);
}

type TextStyle = { fontFamily?: string; fontSizePt?: number; bold?: boolean; italic?: boolean; underline?: boolean };

function RichTextToolbar({ target, onPersist }: { target: React.RefObject<HTMLDivElement | null>; onPersist: (text: string, style: TextStyle) => Promise<void> }) {
  const [font, setFont] = React.useState("PingFang SC");
  const [size, setSize] = React.useState("14");
  const selectedText = React.useRef("");
  const capture = () => {
    const selection = window.getSelection()?.toString().trim();
    if (selection) selectedText.current = selection;
  };
  const apply = React.useCallback((command: string, value?: string) => {
    target.current?.focus();
    document.execCommand(command, false, value);
  }, [target]);
  return <div className="office-format-toolbar">
    <select aria-label="字体" value={font} onPointerDown={capture} onChange={(event) => { setFont(event.target.value); apply("fontName", event.target.value); void onPersist(selectedText.current, { fontFamily: event.target.value }); }}>
      <option>PingFang SC</option><option>Microsoft YaHei</option><option>SimSun</option><option>Arial</option><option>Georgia</option>
    </select>
    <select aria-label="字号" value={size} onPointerDown={capture} onChange={(event) => { setSize(event.target.value); apply("fontSize", event.target.value === "12" ? "2" : event.target.value === "14" ? "3" : event.target.value === "18" ? "4" : "5"); void onPersist(selectedText.current, { fontSizePt: Number(event.target.value) }); }}>
      <option>12</option><option>14</option><option>18</option><option>24</option><option>32</option>
    </select>
    <button title="加粗" onPointerDown={capture} onClick={() => { apply("bold"); void onPersist(selectedText.current, { bold: true }); }}><Bold size={14} /></button>
    <button title="斜体" onPointerDown={capture} onClick={() => { apply("italic"); void onPersist(selectedText.current, { italic: true }); }}><Italic size={14} /></button>
    <button title="下划线" onPointerDown={capture} onClick={() => { apply("underline"); void onPersist(selectedText.current, { underline: true }); }}><Underline size={14} /></button>
    <span className="office-format-hint">选中文字后调整，自动备份并写回 DOCX</span>
  </div>;
}

function DocxEditor({ binary, onStyle }: { binary: OfficeBinary; onStyle: (text: string, style: TextStyle) => Promise<void> }) {
  const host = React.useRef<HTMLDivElement>(null);
  const styles = React.useRef<HTMLDivElement>(null);
  React.useEffect(() => {
    if (!host.current) return;
    host.current.replaceChildren();
    void renderAsync(decodeBase64(binary.base64), host.current, styles.current || undefined, {
      className: "lancode-docx",
      inWrapper: true,
      breakPages: true,
      ignoreLastRenderedPageBreak: false,
      renderHeaders: true,
      renderFooters: true,
      renderFootnotes: true,
      renderEndnotes: true,
      renderComments: true,
      useBase64URL: true,
    });
  }, [binary.base64]);
  return <div className="office-native-editor office-docx-editor">
    <RichTextToolbar target={host} onPersist={onStyle} />
    <div ref={styles} />
    <div ref={host} className="office-docx-canvas" />
  </div>;
}

function excelToUniver(workbook: ExcelJS.Workbook) {
  const sheets: Record<string, unknown> = {};
  const order: string[] = [];
  workbook.worksheets.forEach((worksheet, sheetIndex) => {
    const id = `sheet-${sheetIndex + 1}`;
    order.push(id);
    const cellData: Record<number, Record<number, unknown>> = {};
    worksheet.eachRow({ includeEmpty: true }, (row, rowNumber) => {
      const rowData: Record<number, unknown> = {};
      row.eachCell({ includeEmpty: true }, (cell, columnNumber) => {
        const value = cell.value;
        const data: Record<string, unknown> = {};
        if (value && typeof value === "object" && "formula" in value) {
          data.f = String(value.formula);
          data.v = value.result ?? "";
        } else if (value instanceof Date) {
          data.v = value.toLocaleString();
        } else if (value && typeof value === "object" && "text" in value) {
          data.v = String(value.text);
        } else {
          data.v = value ?? "";
        }
        if (typeof data.v === "number") data.t = 2;
        rowData[columnNumber - 1] = data;
      });
      cellData[rowNumber - 1] = rowData;
    });
    sheets[id] = {
      id,
      name: worksheet.name,
      rowCount: Math.max(worksheet.rowCount + 30, 100),
      columnCount: Math.max(worksheet.columnCount + 10, 26),
      defaultColumnWidth: 96,
      defaultRowHeight: 24,
      cellData,
    };
  });
  return {
    id: `lancode-${crypto.randomUUID()}`,
    name: "Lan Code Workbook",
    appVersion: "0.2.8",
    locale: LocaleType.ZH_CN,
    styles: {},
    sheetOrder: order,
    sheets,
  };
}

function SheetEditor({ binary, onSave }: { binary: OfficeBinary; onSave: (base64: string) => Promise<void> }) {
  const host = React.useRef<HTMLDivElement>(null);
  const api = React.useRef<ReturnType<typeof createUniver>["univerAPI"] | null>(null);
  const source = React.useRef<ExcelJS.Workbook | null>(null);
  const [dirty, setDirty] = React.useState(false);
  React.useEffect(() => {
    let disposed = false;
    let univer: ReturnType<typeof createUniver>["univer"] | undefined;
    void (async () => {
      const workbook = new ExcelJS.Workbook();
      await workbook.xlsx.load(decodeBase64(binary.base64) as never);
      if (disposed || !host.current) return;
      source.current = workbook;
      const created = createUniver({
        locale: LocaleType.ZH_CN,
        locales: { [LocaleType.ZH_CN]: sheetsZhCN },
        presets: [UniverSheetsCorePreset({ container: host.current })],
      });
      univer = created.univer;
      api.current = created.univerAPI;
      created.univerAPI.createWorkbook(excelToUniver(workbook) as never);
      created.univerAPI.addEvent(created.univerAPI.Event.CommandExecuted, () => setDirty(true));
    })();
    return () => {
      disposed = true;
      api.current = null;
      univer?.dispose();
    };
  }, [binary.base64]);

  const save = async () => {
    const snapshot = api.current?.getActiveWorkbook()?.save();
    const workbook = source.current;
    if (!snapshot || !workbook) return;
    for (const sheetId of snapshot.sheetOrder || []) {
      const sheet = snapshot.sheets?.[sheetId];
      if (!sheet) continue;
      const worksheet = workbook.getWorksheet(sheet.name) || workbook.addWorksheet(sheet.name);
      Object.entries(sheet.cellData || {}).forEach(([rowIndex, row]) => {
        Object.entries(row || {}).forEach(([columnIndex, cell]) => {
          const sourceCell = worksheet.getCell(Number(rowIndex) + 1, Number(columnIndex) + 1);
          const data = cell as { v?: unknown; f?: string };
          sourceCell.value = data.f ? { formula: data.f, result: data.v as string | number } : data.v as ExcelJS.CellValue;
        });
      });
    }
    const output = await workbook.xlsx.writeBuffer();
    await onSave(encodeBase64(output));
    setDirty(false);
  };

  return <div className="office-native-editor office-sheet-editor">
    <div className="office-editor-actions"><span>Univer 表格编辑器</span><button disabled={!dirty} onClick={() => void save()}><Save size={14} /> 保存表格</button></div>
    <div ref={host} className="office-univer-host" />
  </div>;
}

function PptxEditor({ binary }: { binary: OfficeBinary }) {
  const canvas = React.useRef<HTMLCanvasElement>(null);
  const viewer = React.useRef<PPTXViewer | null>(null);
  const [slide, setSlide] = React.useState(0);
  const [count, setCount] = React.useState(0);
  const [ready, setReady] = React.useState(false);
  React.useEffect(() => {
    let disposed = false;
    void (async () => {
      if (!canvas.current) return;
      const instance = new PPTXViewer({ canvas: canvas.current, slideSizeMode: "fit", backgroundColor: "#ffffff" });
      viewer.current = instance;
      await instance.loadFile(decodeBase64(binary.base64));
      if (disposed) return;
      setCount(instance.getSlideCount());
      await instance.render(canvas.current, { quality: "high" });
      setReady(true);
    })();
    return () => {
      disposed = true;
      viewer.current?.destroy();
      viewer.current = null;
    };
  }, [binary.base64]);
  const move = async (next: number) => {
    if (!viewer.current || !canvas.current) return;
    const target = Math.max(0, Math.min(count - 1, next));
    await viewer.current.goToSlide(target, canvas.current);
    setSlide(target);
  };
  return <div className="office-native-editor office-pptx-editor">
    <div className="office-slide-controls">
      <button disabled={slide === 0} onClick={() => void move(slide - 1)}><ArrowLeft size={14} /></button>
      <span>{ready ? `第 ${slide + 1} / ${count} 页` : "正在解析演示文稿..."}</span>
      <button disabled={!ready || slide >= count - 1} onClick={() => void move(slide + 1)}><ArrowRight size={14} /></button>
      {ready && <i><Check size={13} /> 真实幻灯片画布</i>}
    </div>
    <div className="office-pptx-stage"><canvas ref={canvas} /></div>
  </div>;
}

export default function OfficeEditor({ document, onSaved, onStatus }: Props) {
  const [binary, setBinary] = React.useState<OfficeBinary>();
  const [error, setError] = React.useState("");
  React.useEffect(() => {
    setBinary(undefined);
    setError("");
    void invoke<OfficeBinary>("office_read_binary", { path: document.path })
      .then(setBinary)
      .catch((reason) => setError(String(reason)));
  }, [document.path]);

  const save = async (base64: string) => {
    onStatus("正在保存 Office 文件并创建备份...");
    const saved = await invoke<OfficeDocument>("office_write_binary", { path: document.path, base64 });
    onSaved(saved);
    const refreshed = await invoke<OfficeBinary>("office_read_binary", { path: document.path });
    setBinary(refreshed);
    onStatus(`已保存 ${document.name}，原文件已自动备份`);
  };

  if (error) return <div className="office-render-error"><strong>Office 渲染器启动失败</strong><span>{error}</span></div>;
  if (!binary) return <div className="office-render-loading"><span /><b>正在加载原始 Office 文件和版式资源...</b></div>;
  if (document.kind === "docx") return <DocxEditor binary={binary} onStyle={async (text, style) => {
    try {
      onStatus("正在写入 DOCX 格式并创建备份...");
      const saved = await invoke<OfficeDocument>("office_style_text", {
        path: document.path,
        text,
        fontFamily: style.fontFamily,
        fontSizePt: style.fontSizePt,
        bold: Boolean(style.bold),
        italic: Boolean(style.italic),
        underline: Boolean(style.underline),
      });
      onSaved(saved);
      setBinary(await invoke<OfficeBinary>("office_read_binary", { path: document.path }));
      onStatus(`已保存 ${document.name} 的文字格式`);
    } catch (reason) {
      onStatus(`DOCX 格式保存失败：${String(reason)}`);
    }
  }} />;
  if (document.kind === "xlsx") return <SheetEditor binary={binary} onSave={save} />;
  if (document.kind === "pptx") return <PptxEditor binary={binary} />;
  return <div className="office-render-error">该文件类型暂不支持原生 Office 画布。</div>;
}
