# Lan Code Office Mode

Office Mode 是 Lan Code 面向 `.docx`、`.xlsx`、`.pptx`、`.pdf` 等办公文件的 AI 工作台。

它的目标不是重新做一个 Word、Excel 或 PowerPoint，而是把 Office 文件放进 AI IDE：用户看到的是文档本身，AI 看到的是文件结构、当前上下文、Office Diff、操作历史和可回滚补丁。

## 当前已实现

- Office 文件扫描：自动识别当前项目下的 `.docx`、`.xlsx`、`.pptx`、`.pdf`、`.md`、`.csv`。
- 多文件 Tab：可以同时打开多个 Office 文件。
- 结构化读取：
  - `.docx`：读取正文段落并生成大纲。
  - `.pptx`：按幻灯片读取文本。
  - `.xlsx`：读取共享文本和工作表 XML 中的可见文本。
  - `.pdf`：先作为只读上下文接入。
- 三栏工作台：
  - 左侧：文件、大纲、变更、历史、检查。
  - 中间：文档/表格/幻灯片工作区。
  - 右侧：Office AI 助手、上下文卡片、操作计划、应用与回滚。
- Office Diff：展示结构化文字变更，而不只是显示“文件变了”。
- 安全写入：
  - AI 先生成结构化 `OfficeAction`。
  - 程序创建备份。
  - 程序对 OOXML 包做确定性修改。
  - 用户可应用或回滚。
- Markdown 导出：可将 Office 文件结构导出为 `.office.md`，方便审阅、检索和交给 Agent 继续处理。

## 设计原则

1. AI 不直接写 OOXML。

   模型只能描述操作，例如 `replace_text`、`insert_text`、`set_style`。真正修改由 Office Engine 完成。

2. 默认预览，不默认破坏。

   Office 文件比代码更容易因为格式问题损坏，所以修改流程必须经过预览、Diff 和备份。

3. 以对象和选区为中心。

   后续会继续把光标、选区、页面、表格、幻灯片对象、样式等上下文传给 AI。

4. 可以回滚。

   每次应用 Patch 前都会产生本地备份，用户可以从历史中恢复。

## 下一步

- Word 页面级渲染和真实光标选区同步。
- Excel 单元格级选择、公式检查、图表区域识别。
- PPT shape/textbox/image 对象级定位和视觉 Diff。
- Office 文件截图渲染，用于 AI 视觉理解和用户预览。
- 将 Office Engine 注册为 Core 工具，让模型在 Agent 循环中直接调用结构化 Office 操作。
- 更细粒度的样式操作：字体、字号、行距、标题层级、表格宽度、幻灯片对象位置。
