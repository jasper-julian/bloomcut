<p align="center">
  <img src="assets/icons/concat_logo_256.png" alt="BloomCut" width="120" />
</p>

<h1 align="center">BloomCut</h1>
<p align="center">为口播短视频打造的剪辑工具：按脚本校对字幕、AI 助手对话剪辑、按规则自动划重点。</p>

<p align="center">
  <a href="https://github.com/jasper-julian/bloomcut/releases/latest/download/BloomCut-Setup.exe"><img src="assets/bloomcut_download_button.svg" alt="下载 BloomCut" width="300" /></a>
</p>

## 安装

1. 点上面的按钮下载 `BloomCut-Setup.exe`（约 1.6 GB），双击运行，一路“下一步”。不需要管理员权限。
2. 如果 Windows 提示“Windows 已保护你的电脑”，点 **更多信息 → 仍要运行**。
3. 转录模型、配音模型和划重点规则已经内置，装完即可离线使用，不需要再下载任何东西。
4. 使用 AI 助手：打开 **设置 › AI 助手**，填写自己的 API 地址、密钥和模型名称，点 **测试连接**。

## 功能

- **按脚本校对字幕**：识别字幕时粘贴口播稿，字幕自动改成稿子里的字，并按稿子的标点断句。
- **AI 助手**：点标题栏的 ✦ AI 助手，用对话让大模型直接剪辑当前工程：加字幕、切片段、调整位置、撤销……支持 DeepSeek、通义千问、Kimi、豆包等任何 OpenAI 兼容接口。
- **按规则划重点**：把划重点规则（Markdown 文件）放进 设置 › AI 助手 › 规则 的文件夹，对助手说“按规则给字幕划重点”，重点词自动变色、加粗、放大，一次撤销全部撤回。
- **导出到剪映**：菜单 文件 › 导出到剪映，把当前时间线（视频、音频、字幕和划好的重点）生成剪映草稿，打开剪映就能继续精剪。特效、转场需要在剪映里重新添加。
- **导出 SRT**：校对好的字幕可以单独导出，导入其他剪辑软件使用。

## 系统要求

- Windows 10 / 11，64 位
- 支持 AVX2 的 CPU（2013 年以后的大多数 Intel / AMD 处理器）

---

<sub>BloomCut 基于开源项目 <a href="https://github.com/jub0t/Concat">Concat</a> 修改，按 <a href="LICENSE">AGPL-3.0</a> 协议发布，源代码即本仓库。第三方组件的许可见 <a href="THIRD_PARTY_NOTICES.md">THIRD_PARTY_NOTICES.md</a>。</sub>
