# Office 开源引擎与许可

Lan Code Office Mode 采用“本地文件解析 + 内嵌编辑器 + 原文件备份写回”的组合方案。所有组件都在桌面端本地运行，不要求上传文档到第三方服务器。

## 当前组件

| 组件 | 用途 | 许可 |
| --- | --- | --- |
| [docx-preview](https://github.com/VolodymyrBaydalka/docxjs) | DOCX 页面、分页、页眉页脚、批注和图片渲染 | Apache-2.0 |
| [Univer](https://github.com/dream-num/univer) | XLSX 表格画布、单元格编辑、公式和多工作表 | Apache-2.0 |
| [ExcelJS](https://github.com/exceljs/exceljs) | XLSX 原文件读取和写回 | MIT |
| [PptxViewJS](https://github.com/gptsci/pptxviewjs) | PPTX 幻灯片 Canvas 渲染 | MIT |
| [JSZip](https://github.com/Stuk/jszip) | 浏览器端 Office ZIP 包处理依赖 | MIT / GPLv3 双许可 |
| [Chart.js](https://github.com/chartjs/Chart.js) | PPTX 图表渲染依赖 | MIT |

## 设计边界

- DOCX 保留原文件作为真实数据源。基础字体、字号、粗体、斜体和下划线通过 OOXML run 属性写回，每次写入前自动备份。
- XLSX 在 Univer 中编辑，通过 ExcelJS 写回工作簿，保留未修改工作表及大部分原有结构。
- PPTX 当前以高保真浏览和翻页为主。后续对象级编辑继续沿用结构化 `OfficeAction` 和可回滚写回。
- Office 引擎按需加载，不进入 Office Mode 时不会加载 Univer、ExcelJS、docx-preview 或 PptxViewJS。

第三方组件的版权与许可归各自项目所有。发布二进制包时应继续保留仓库许可证和依赖许可信息。
