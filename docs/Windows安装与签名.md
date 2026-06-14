# Windows 安装、SmartScreen 与代码签名

## 当前现象

Lan Code 当前发布的 Windows EXE/MSI 没有使用受信任的代码签名证书，因此首次运行时
Microsoft Defender SmartScreen 可能显示：

- “Windows 已保护你的电脑”
- “发布者未知”
- “Microsoft Defender SmartScreen 阻止了无法识别的应用启动”

这不表示安装包一定包含恶意代码。它表示 Windows 无法确认该安装包由一个已验证的
发布者签名，并且该文件尚未积累足够的下载信誉。

## 当前交付措施

在正式签名前，每次发布必须：

1. 只通过项目 GitHub Release 页面分发。
2. 由 GitHub Actions 从公开提交重新构建。
3. 保留 Release 资产的 SHA-256 摘要。
4. CI 必须通过测试、Clippy 和生产构建。
5. 不通过网盘、聊天附件等无法验证来源的渠道重新打包。

用户可以在 SmartScreen 页面点击“更多信息”，核对应用名称后选择“仍要运行”。
这只是未签名阶段的临时方式，不应作为长期方案。

## 根本解决方案

正式解决 SmartScreen 需要购买并配置 Windows 代码签名证书：

- 普通代码签名证书：成本较低，但新证书仍需要逐步积累 SmartScreen 信誉。
- EV 代码签名证书：验证更严格、成本更高，通常能更快建立可信发布者体验。

证书私钥必须保存在安全硬件、云签名服务或 GitHub Actions 支持的安全签名服务中，
不能提交到 Git 仓库。

发布流水线后续需要加入：

1. 构建未签名 EXE/MSI。
2. 使用 `signtool` 对主程序、卸载程序、EXE 安装器和 MSI 签名。
3. 使用可信时间戳服务器。
4. 使用 `signtool verify /pa` 验证签名。
5. 验证通过后才能上传 Release。

## 当前状态

- [x] GitHub Actions 自动构建 Windows EXE、MSI 和便携 ZIP。
- [x] 发布资产由公开提交可重复构建。
- [ ] 购买代码签名证书。
- [ ] 配置安全的签名私钥或云签名服务。
- [ ] 在 Release 工作流中加入签名与验证。
- [ ] 在应用内加入自动更新及更新包签名验证。
