# Agent Note: desktop macOS release signing, notarization, and verification

Status: implemented

[English](2026-08-17-desktop-macos-signing-notarization.md) | 中文

## Problem

桌面发布流水线([`.github/workflows/desktop-release.yml`](../../../../.github/workflows/desktop-release.yml)加共享 composite action [`.github/actions/build-desktop`](../../../../.github/actions/build-desktop/action.yml))的每个矩阵任务都显式执行 `tauri build --target <triple>`。composite action 里有两个手写步骤仍读取原生构建的输出路径 `target/release/bundle/...`,该路径只对不带 `--target` 的构建存在;跨 target 的产物实际落在 `target/<triple>/release/bundle/` 下。于是 macOS 公证步骤 glob 了一个不存在的目录,永远无法提交 DMG,CI 的上传产物步骤也上传了空集。此外,整条 macOS 链完全没有验证:任务可以「绿」着发出去一个被 Gatekeeper 拒绝的 DMG;`APPLE_SIGNING_IDENTITY` 配置错误时,tauri CLI 的证书身份比对会在一个小时后才报出难以诊断的错误。

## Decision

composite action 为每个 macOS 矩阵任务持有同一条签名公证链,所有手写路径都从 `target` 输入推导:

1. **证书导入。** 把 `APPLE_CERTIFICATE` 解码进专用 `build.keychain`,设为默认钥匙串并解锁,把锁定超时延长到 3600 秒(`security set-keychain-settings`),按 codesign 分区列表导入;任一必需 secret 缺失时以具名 `::error::` 立即失败。随后从导入的证书本身反推签名身份(`security find-identity -v -p codesigning` 中的 `Developer ID Application` 条目),经 `$GITHUB_ENV` 写回 `APPLE_SIGNING_IDENTITY`,保证签名身份与证书逐字节一致——由此绕开 tauri CLI 在「其自行导入的 `APPLE_CERTIFICATE` 不含所配身份」时拒绝签名的行为。
2. **资源签名。** 构建前对 `src-tauri/resources` 下所有 Mach-O(暂存的 Node 运行时与原生扩展)逐一执行 `codesign --force --options runtime --timestamp` 并逐个 `codesign --verify --strict` 校验。这一步不可省略:tauri bundler 只签 app 二进制、framework 与 sidecar,不签任意 `Resources` 文件,而公证会拒绝包内任何未签名的可执行代码。
3. **App 签名与公证。** `tauri build`(由 `tauri-apps/tauri-action@v1` 调用)以硬运行时(tauri 默认)签名 app 包;由于 workflow 任务现导出 `APPLE_ID`/`APPLE_PASSWORD`/`APPLE_TEAM_ID`(`tauri-bundler` 的 `notarize_auth` 读取的环境变量名),构建内即对 app 完成公证并 staple。这三个变量同时在 tauri-action 步骤的 `env:` 中显式声明,不依赖任务级 env 的隐式继承。
4. **DMG 公证与验证。** 最后一步在 `target/${{ inputs.target }}/release/bundle` 下定位产物,用 `xcrun notarytool --wait --timeout 20m` 提交 DMG 并 staple,随后断言整条链:对 DMG 与 app 各做 `stapler validate`,对 app 做 `codesign --verify --deep --strict` 与 `spctl --assess --type exec`;全部通过后,该 DMG 才以 `gh release upload --clobber` 替换 tauri-action 上传的原文件。绿色任务从此证明产物能被 Gatekeeper 接受。

原 `NOTARY_ID`/`NOTARY_PASSWORD`/`NOTARY_TEAM_ID` 别名被移除,统一为 workflow 步骤与 tauri CLI 共同消费的 `APPLE_*` 命名,由 release workflow 的任务级 `env:` 一次性映射 secrets。CI(非发布)macOS 任务保持 `apple-signing: false`,按设计不签名,其产物上传读取同一按 target 推导的路径。

## Alternatives considered

**让 tauri-action 或 tauri CLI 端到端处理公证。** 否决:tauri-action@v1 自身不做签名公证,CLI 只公证 app 包、从不公证 DMG。仅分发 DMG 的 macOS 下载路径将完全没有公证,因此工作流里显式的 `notarytool` 提交不是冗余。

**打包后再签暂存资源。** 否决:那时资源已复制并封进 app 包,封签后的改动会使包签名失效,必须重建整条链。

**省掉 staple 后的验证以节约任务时长。** 否决:验证断言是把「公证成功」变成「产物能过 Gatekeeper」的证据;没有它们,坏链(例如 CLI 升级后不再 staple)会静默地以绿色发布。

**信任 `APPLE_SIGNING_IDENTITY` secret 与证书一致。** 否决:tauri CLI 会把其导入证书的身份与所配值比对,不一致即报错;从导入的证书推导该值,消灭了整类只会在构建一小时后才暴露的失败。

## Consequences

每个 macOS 发布任务现在做两次公证提交:一次是 `tauri build` 内的 app,一次是 workflow 步骤里的 DMG,每个 mac 任务约增加一轮公证往返(通常 5–20 分钟)。换来的是两条分发路径(直接下载的 DMG 与自动更新的 `.app.tar.gz`,后者内层 app 携带已 staple 票据)都持有已公证、已 staple 的产物,以及链上任何一环断裂时让任务失败的验证断言。手动资源签名步骤还依赖 runner 上的 `file` 命令(GitHub 托管的 macOS 镜像自带)。
