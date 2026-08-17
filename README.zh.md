# DeepSeek Harness 桌面版

[English](README.md) | 中文

[DeepSeek Harness](https://github.com/deepseek-ai/DeepSeek-Harness) 的桌面发行版:把官方 Web GUI 装进一个完全自包含的 Tauri 2 桌面应用。

双击即用。应用自带 Node.js 运行时、dsh 服务器与 Web 界面 —— 目标机器无需安装任何东西。

## 下载

从 [GitHub Releases](https://github.com/sansi-lcj/deepseek-harness-desktop/releases) 获取最新构建:

| 平台 | 安装包 |
| --- | --- |
| macOS(Intel + Apple Silicon) | `*_aarch64.dmg` / `*_x64.dmg` —— 已签名并公证 |
| Windows(x64;arm64 暂缓) | `*_x64-setup.exe` |
| Linux(x86_64 + arm64) | `*_amd64.deb` / `*_arm64.deb` |

## 特性

- 自包含:内置 Node 运行时 + dsh 服务器 + Web UI,零前置依赖
- 自动更新:启动时检查 GitHub Releases,弹窗确认后下载安装并自动重启
- 韧性:服务器意外退出自动重启(最多 3 次);运行目录位于应用支持目录,替换安装包不影响运行中的实例
- 桌面体验:单实例、原生窗口、外部链接在默认浏览器打开
- 全平台 CI:推送 `v*` tag 即自动构建、签名(macOS 含公证)并发布三平台产物

## 技术栈

- 壳:Tauri 2 + Rust(edition 2024)
- 启动页:Vite 6 + TypeScript
- 产品 UI:官方 DeepSeek Harness Web GUI(Vite + React),由内嵌 `dsh web` 服务器提供

## 开发

```sh
git clone git@github.com:sansi-lcj/deepseek-harness-desktop.git
cd deepseek-harness-desktop
pnpm install
pnpm run build
pnpm desktop:dev      # 运行桌面壳(调试模式)
pnpm desktop:build    # 本地打包
pnpm desktop:smoke    # 无头冒烟测试
```

前置条件:Node.js(^22.19 或 >=24)、pnpm、Rust stable;macOS 需要 Xcode Command Line Tools。

壳的架构、环境变量覆盖与发布流水线见 [apps/desktop/README.zh.md](apps/desktop/README.zh.md)。

## 发布与自动更新

推送 `v*` tag 会触发 `.github/workflows/desktop-release.yml` 在 macOS / Windows / Linux 三平台构建并发布 GitHub Release,同时合并各平台的 `latest.json` 供应用内更新器读取。所需 secrets:`TAURI_SIGNING_PRIVATE_KEY`、`TAURI_SIGNING_PRIVATE_KEY_PASSWORD`、`APPLE_CERTIFICATE`、`APPLE_CERTIFICATE_PASSWORD`、`APPLE_SIGNING_IDENTITY`、`APPLE_ID`、`APPLE_PASSWORD`、`APPLE_TEAM_ID`、`KEYCHAIN_PASSWORD`。

## 与上游的关系

本仓库跟踪官方 [deepseek-ai/DeepSeek-Harness](https://github.com/deepseek-ai/DeepSeek-Harness) 的 master,并新增 `apps/desktop/`。内嵌服务器使用 npm 发布的 `@deepseek-ai/dsh`(可用 `DSH_DESKTOP_SERVER_VERSION` 覆盖版本)。

## 许可证

[MIT](LICENSE)

第三方依赖及其许可证见 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。
