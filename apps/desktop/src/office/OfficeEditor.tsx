import React from "react";
import { invoke } from "@tauri-apps/api/core";
import { renderAsync } from "docx-preview";
import ExcelJS from "exceljs";
import JSZip from "jszip";
import { createUniver, LocaleType } from "@univerjs/presets";
import { UniverSheetsCorePreset } from "@univerjs/preset-sheets-core";
import sheetsZhCN from "@univerjs/preset-sheets-core/locales/zh-CN";
import { PPTXViewer } from "pptxviewjs";
import { ArrowLeft, ArrowRight, Bold, Check, Italic, Maximize2, Minus, Plus, Save, Underline } from "lucide-react";
import SelectMenu from "../components/SelectMenu";
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

async function normalizeDocxForPreview(bytes: Uint8Array) {
  const zip = await JSZip.loadAsync(bytes);
  const entry = zip.file("word/document.xml");
  if (!entry) return bytes;
  let xml = await entry.async("string");
  if (!xml.includes("<w:pgSz")) {
    const page = '<w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1440" w:right="1440" w:bottom="1440" w:left="1440" w:header="708" w:footer="708" w:gutter="0"/>';
    xml = xml.includes("<w:sectPr/>")
      ? xml.replace("<w:sectPr/>", `<w:sectPr>${page}</w:sectPr>`)
      : xml.replace("<w:sectPr>", `<w:sectPr>${page}`);
    if (xml.includes("Lan Code 新文档") && !xml.includes("<w:rPr>")) {
      xml = xml
        .replace(
          "<w:p><w:r><w:t>Lan Code 新文档</w:t></w:r></w:p>",
          '<w:p><w:pPr><w:spacing w:after="240"/></w:pPr><w:r><w:rPr><w:rFonts w:ascii="Microsoft YaHei" w:eastAsia="Microsoft YaHei"/><w:b/><w:color w:val="1E4F91"/><w:sz w:val="36"/></w:rPr><w:t>Lan Code 新文档</w:t></w:r></w:p>',
        )
        .replace(
          "<w:p><w:r><w:t>从 Lan Code Office Mode 开始编辑。</w:t></w:r></w:p>",
          '<w:p><w:pPr><w:spacing w:after="160"/></w:pPr><w:r><w:rPr><w:rFonts w:ascii="Microsoft YaHei" w:eastAsia="Microsoft YaHei"/><w:color w:val="5F6B7A"/><w:sz w:val="22"/></w:rPr><w:t>从 Lan Code Office Mode 开始编辑。</w:t></w:r></w:p>',
        );
    }
    zip.file("word/document.xml", xml);
    return new Uint8Array(await zip.generateAsync({ type: "arraybuffer" }));
  }
  return bytes;
}

async function normalizePptxForPreview(bytes: Uint8Array) {
  const zip = await JSZip.loadAsync(bytes);
  const slideEntries = Object.keys(zip.files).filter((name) => /^ppt\/slides\/slide\d+\.xml$/.test(name));
  let changed = false;
  for (const name of slideEntries) {
    const entry = zip.file(name);
    if (!entry) continue;
    let xml = await entry.async("string");
    if (!xml.includes("<a:xfrm") && xml.includes("<p:txBody>")) {
      xml = xml
        .replace(
          "<p:cSld><p:spTree>",
          '<p:cSld><p:bg><p:bgPr><a:solidFill><a:srgbClr val="F7F9FC"/></a:solidFill><a:effectLst/></p:bgPr></p:bg><p:spTree>',
        )
        .replace(
          "<p:nvGrpSpPr/><p:grpSpPr/>",
          '<p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/><a:chOff x="0" y="0"/><a:chExt cx="0" cy="0"/></a:xfrm></p:grpSpPr>',
        )
        .replace(
          "<p:nvSpPr/><p:spPr/>",
          '<p:nvSpPr><p:cNvPr id="2" name="Lan Code Content"/><p:cNvSpPr txBox="1"/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm><a:off x="1371600" y="1371600"/><a:ext cx="9144000" cy="3657600"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom><a:noFill/></p:spPr>',
        )
        .replace("<a:bodyPr/>", '<a:bodyPr wrap="square" anchor="t"><a:spAutoFit/></a:bodyPr>')
        .replace(/<a:r><a:t>([^<]+)<\/a:t><\/a:r>/, '<a:r><a:rPr lang="zh-CN" sz="3200" b="1"/><a:t>$1</a:t></a:r>')
        .replace(/<a:p><a:r><a:t>(从 Lan Code[^<]+)<\/a:t><\/a:r><\/a:p>/, '<a:p><a:r><a:rPr lang="zh-CN" sz="1800"/><a:t>$1</a:t></a:r></a:p>');
      xml = xml.replace(
        "<p:sp><p:nvSpPr>",
        '<p:sp><p:nvSpPr><p:cNvPr id="3" name="Accent"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm><a:off x="1371600" y="1097280"/><a:ext cx="1219200" cy="91440"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom><a:solidFill><a:srgbClr val="2F80ED"/></a:solidFill><a:ln><a:noFill/></a:ln></p:spPr></p:sp><p:sp><p:nvSpPr>',
      );
      zip.file(name, xml);
      changed = true;
    }
  }
  return changed ? new Uint8Array(await zip.generateAsync({ type: "arraybuffer" })) : bytes;
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
    <SelectMenu compact ariaLabel="字体" value={font} options={["PingFang SC", "Microsoft YaHei", "SimSun", "Arial", "Georgia"].map((item) => ({ id: item, label: item }))} onChange={(value) => { capture(); setFont(value); apply("fontName", value); void onPersist(selectedText.current, { fontFamily: value }); }} />
    <SelectMenu compact ariaLabel="字号" value={size} options={["12", "14", "18", "24", "32"].map((item) => ({ id: item, label: item }))} onChange={(value) => { capture(); setSize(value); apply("fontSize", value === "12" ? "2" : value === "14" ? "3" : value === "18" ? "4" : "5"); void onPersist(selectedText.current, { fontSizePt: Number(value) }); }} />
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
    const target = host.current;
    if (!target) return;
    let disposed = false;
    let observer: ResizeObserver | undefined;
    target.replaceChildren();
    void (async () => {
      const bytes = await normalizeDocxForPreview(decodeBase64(binary.base64));
      await renderAsync(bytes, target, styles.current || undefined, {
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
      if (disposed) return;
      const fitPages = () => {
        const wrapper = target.querySelector<HTMLElement>(".lancode-docx-wrapper");
        const page = target.querySelector<HTMLElement>("section.lancode-docx");
        if (!wrapper || !page) return;
        const naturalWidth = page.offsetWidth || 794;
        const scale = Math.min(1, Math.max(0.58, (target.clientWidth - 36) / naturalWidth));
        wrapper.style.setProperty("--docx-fit-scale", scale.toFixed(4));
      };
      fitPages();
      observer = new ResizeObserver(fitPages);
      observer.observe(target);
    })();
    return () => {
      disposed = true;
      observer?.disconnect();
    };
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
    appVersion: "0.2.10",
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
  const stage = React.useRef<HTMLDivElement>(null);
  const viewer = React.useRef<PPTXViewer | null>(null);
  const [slide, setSlide] = React.useState(0);
  const [count, setCount] = React.useState(0);
  const [ready, setReady] = React.useState(false);
  const [zoom, setZoom] = React.useState(1);
  const zoomRef = React.useRef(1);
  const renderSequence = React.useRef(0);

  const renderSlide = React.useCallback(async (slideIndex: number, zoomLevel = zoomRef.current) => {
    const instance = viewer.current;
    const target = canvas.current;
    const container = stage.current;
    if (!instance || !target || !container) return;
    const sequence = ++renderSequence.current;
    const availableWidth = Math.max(320, container.clientWidth - 56);
    const availableHeight = Math.max(220, container.clientHeight - 56);
    const width = Math.floor(Math.min(availableWidth, availableHeight * (16 / 9)) * zoomLevel);
    const height = Math.floor(width * (9 / 16));
    target.style.width = `${width}px`;
    target.style.height = `${height}px`;
    target.width = Math.max(1, Math.floor(width * window.devicePixelRatio));
    target.height = Math.max(1, Math.floor(height * window.devicePixelRatio));
    await instance.render(target, { slideIndex, quality: "high" });
    if (sequence === renderSequence.current) setSlide(slideIndex);
  }, []);

  React.useEffect(() => {
    let disposed = false;
    let observer: ResizeObserver | undefined;
    void (async () => {
      if (!canvas.current || !stage.current) return;
      const instance = new PPTXViewer({
        canvas: canvas.current,
        slideSizeMode: "fit",
        backgroundColor: "#ffffff",
        autoChartRerenderDelayMs: 0,
      });
      viewer.current = instance;
      const bytes = await normalizePptxForPreview(decodeBase64(binary.base64));
      await instance.loadFile(bytes);
      if (disposed) return;
      setCount(instance.getSlideCount());
      await renderSlide(0, 1);
      setReady(true);
      let resizeTimer = 0;
      observer = new ResizeObserver(() => {
        window.clearTimeout(resizeTimer);
        resizeTimer = window.setTimeout(() => void renderSlide(instance.getCurrentSlideIndex()), 120);
      });
      observer.observe(stage.current);
    })();
    return () => {
      disposed = true;
      observer?.disconnect();
      viewer.current?.destroy();
      viewer.current = null;
    };
  }, [binary.base64, renderSlide]);

  const move = async (next: number) => {
    const target = Math.max(0, Math.min(count - 1, next));
    await renderSlide(target);
  };

  const changeZoom = (next: number) => {
    const value = Math.max(0.5, Math.min(2, next));
    zoomRef.current = value;
    setZoom(value);
    void renderSlide(slide, value);
  };

  return <div className="office-native-editor office-pptx-editor">
    <div className="office-slide-controls">
      <button disabled={slide === 0} onClick={() => void move(slide - 1)}><ArrowLeft size={14} /></button>
      <span>{ready ? `第 ${slide + 1} / ${count} 页` : "正在解析演示文稿..."}</span>
      <button disabled={!ready || slide >= count - 1} onClick={() => void move(slide + 1)}><ArrowRight size={14} /></button>
      <div className="office-slide-zoom">
        <button title="缩小" disabled={zoom <= 0.5} onClick={() => changeZoom(zoom - 0.1)}><Minus size={13} /></button>
        <button title="适合窗口" onClick={() => changeZoom(1)}><Maximize2 size={13} /><span>{Math.round(zoom * 100)}%</span></button>
        <button title="放大" disabled={zoom >= 2} onClick={() => changeZoom(zoom + 0.1)}><Plus size={13} /></button>
      </div>
      {ready && <i><Check size={13} /> 真实幻灯片画布</i>}
    </div>
    <div ref={stage} className="office-pptx-stage"><canvas ref={canvas} /></div>
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
        request: {
          path: document.path,
          text,
          fontFamily: style.fontFamily,
          fontSizePt: style.fontSizePt,
          bold: Boolean(style.bold),
          italic: Boolean(style.italic),
          underline: Boolean(style.underline),
        },
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
