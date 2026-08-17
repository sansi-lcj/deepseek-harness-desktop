# @deepseek-ai/dsh-desktop

[English](README.md) | 中文

DeepSeek Harness 的桌面壳:一个 [Tauri 2](https://v2.tauri.app) 原生窗口,负责拉起 dsh web 服务器并承载浏览器 UI(`apps/web` 前端)。

## 技术栈

- 壳:Tauri 2(Rust,edition 2024)+ tauri-plugin-single-instance
- 启动页(splash):Vite 6 + TypeScript,`@tauri-apps/api` 事件桥
- 产品 UI:复用 `apps/web`(Vite + React),由内嵌的 `dsh web` 服务器提供

## 架构

壳以受管子进程方式拥有服务器(打包后全部内嵌,不依赖本机任何环境):

1. 解析 Node.js 运行时与服务器入口:优先 `DSH_DESKTOP_NODE` / `DSH_DESKTOP_SERVER_BIN` / `DSH_DESKTOP_REPO_ROOT`(仓库开发模式),打包版回退到应用资源目录里的 `resources/node/node` 与 `resources/server/node_modules/@deepseek-ai/dsh/lib/bin.js`,最后才是 PATH 与 mise/nvm/Homebrew 常见路径。
2. 启动 `node apps/cli/lib/bin.js web --host 127.0.0.1 --port 0`,监听 stdout 中的就绪行 `dsh web: http://127.0.0.1:<port>` —— web bundle 将它作为 supervisor 就绪信号打印。
3. 轮询 TCP 端口直到可连接,随后把主窗口从内置 splash 页导航到真实 URL。
4. 退出时先通知服务器(SIGTERM,3 秒后 SIGKILL)再退出;服务器意外退出时自动重启(最多 3 次),超过后用失败详情重建 splash。

splash 渲染 `server-status` 事件(`starting` / `ready` / `exited`)。外部 http(s) 链接在默认浏览器打开;二次启动聚焦已有窗口。内嵌服务器的工作目录在用户级「应用支持」目录下,替换 `.app`(如更新时)不会影响正在运行的实例。

## 自动更新

启动后壳会检查 `tauri.conf.json` 中配置的 GitHub Releases 端点(`plugins.updater.endpoints`);发现新版本时弹原生对话框,确认后下载签名更新包、安装并自动重启。

## Release 发布(CI)

本仓库是 `deepseek-ai/deepseek-harness` 的 fork,默认分支为 `desktop-release`。`.github/workflows/sync-upstream.yml` 每天(也可手动触发)把上游 `master` 合并进 `desktop-release`;每次合并推送都会经 `.github/workflows/desktop.yml` 重新构建桌面应用,合并冲突时不推送并自动开 issue 提醒。

推送 `desktop-v*` tag 触发 `.github/workflows/desktop-release.yml`:tauri-action 在 macOS / Windows / Linux 三平台构建并发布 GitHub Release 资产与供自动更新读取的各平台 `latest.json`。macOS 任务额外经由共享 composite action 走完整签名公证链:

1. 把 Developer ID Application 证书导入独立钥匙串,解锁到任务结束,并从导入的证书中反推 `APPLE_SIGNING_IDENTITY`,保证签名身份与证书永远一致。
2. 对 `src-tauri/resources` 下暂存的所有 Mach-O 二进制(内嵌 Node 运行时与原生扩展)逐一用硬运行时(hardened runtime)加时间戳签名,并逐个校验。
3. `tauri build`(tauri-action 内)对 app 包以硬运行时签名、公证,并把票据 staple 进 app。
4. 用 `notarytool` 公证 DMG 并 staple,随后校验整条链:对 DMG 与 app 各做 `stapler validate`、对 app 做 `codesign --verify --deep --strict` 与 `spctl --assess --type exec`;校验通过的 DMG 以 `--clobber` 覆盖替换 Release 上未公证的原文件。

所需 secrets:`TAURI_SIGNING_PRIVATE_KEY`、`APPLE_CERTIFICATE`、`APPLE_CERTIFICATE_PASSWORD`、`APPLE_SIGNING_IDENTITY`、`APPLE_ID`、`APPLE_PASSWORD`、`APPLE_TEAM_ID`、`KEYCHAIN_PASSWORD`。

## 前置条件

- Node.js(^22.19 或 >=24)、pnpm
- Rust stable(通过 mise 或 rustup)
- macOS:Xcode Command Line Tools;Windows:WebView2(系统预装);Linux:webkit2gtk-4.1

## 快速开始

在仓库根目录,服务器构建产物已存在时:

```sh
pnpm install
pnpm run build
pnpm desktop:dev      # run the desktop shell (debug)
pnpm desktop:build    # bundle the macOS .app
pnpm desktop:smoke    # headless smoke: load the server page, report, exit
```

打包产物位于 `apps/desktop/src-tauri/target/release/bundle/macos/`:`DeepSeek Harness.app` 与 `DeepSeek Harness_0.1.0-rc.6_aarch64.dmg`。双击即用,自带 Node 运行时、dsh 服务器与 Web UI,目标机器无需安装任何东西。

## 环境变量覆盖

| 变量 | 含义 |
| --- | --- |
| `DSH_DESKTOP_NODE` | 用于运行服务器的 Node.js 可执行文件 |
| `DSH_DESKTOP_SERVER_BIN` | 构建产物 `bin.js` 的路径 |
| `DSH_DESKTOP_REPO_ROOT` | 持有构建产物的仓库检出目录 |
| `DSH_DESKTOP_SMOKE` | 设为 `1` 时执行加载即退出的冒烟测试 |

## 已知限制与后续工作

- 内嵌服务器是 npm 发布的 `@deepseek-ai/dsh@0.1.0-rc.6`(可用 `DSH_DESKTOP_SERVER_VERSION` 覆盖版本);开发模式(`pnpm desktop:dev`)运行的是本仓库构建产物。
- 本地构建的包未签名;CI 的 Release 构建已签名并公证(见上文)。
- 单窗口;多服务器实例场景不在范围内。
- 内嵌服务器的暂存安装改用 pnpm,沿用 workspace 的构建脚本白名单与 node-pty 补丁,打包行为与开发安装一致;未来改用单文件可执行路线(参见 `scripts/build-exe-for-python-sdk.ts`)可进一步缩小体积。
