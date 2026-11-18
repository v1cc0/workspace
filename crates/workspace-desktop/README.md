# Workspace NSH desktop — P0

Rust NSH 0.6.0 的 Linux 编辑与原生 Markdown 预览验证入口。草稿仅存内存，关闭窗口后丢弃；尚未接入现有 `notes/` 或登录。原桌面仍从仓库根目录运行。

Linux 需要 GTK4、NSH 对应的 GStreamer 开发库和 WebKitGTK 6（`webkitgtk-6.0`）。使用 fcitx5 的机器还需要 `fcitx5-gtk`；该 GTK 模块的进程环境名为 `GTK_IM_MODULE=fcitx`。

从仓库根目录执行：

```sh
nsh check crates/workspace-desktop
nsh test crates/workspace-desktop
GTK_IM_MODULE=fcitx nsh test crates/workspace-desktop --target linux
# 不使用开发服务器，直接启动原生编辑窗口：
GTK_IM_MODULE=fcitx cargo run --manifest-path crates/workspace-desktop/Cargo.toml
# 需要 Linux 显示会话的真实浏览器路由测试：
GTK_IM_MODULE=fcitx cargo test --manifest-path crates/workspace-desktop/Cargo.toml \
  --test browser_proxy native_browser -- --ignored --nocapture
```

`src/browser.rs` 提供应用自有的 GTK4/WebKit native component。调用方先用实际代理地址建立临时网络 session，再注册 `workspace.browser` 并传入 `src` URL。当前编辑窗口未添加浏览器页面，也未启动代理；独立测试夹具使用现有 `src/backend.rs` 和受控本机 QUIC 出口，验证 HTTP/HTTPS、重定向、CSS/图片/脚本/fetch、WS/WSS、loopback 目标，以及 QUIC/代理关闭后不直连。测试证书仅在该夹具的临时 session 中信任，不改变系统或生产 TLS 验证。

当前 crate 保留官方模板独立的 `[workspace]`，P1 拆包时再接入根 workspace。用 NSH CLI 0.6.0 运行上述命令，和 Cargo 锁定的 NSH crates 对齐。

本机已通过真实 fcitx 中文组合输入、撤销/重做、选择和 GTK 键盘预览滚动检查。Tab 到预览，End 滚动，Shift+Tab 返回编辑器。已核实 GTK 运行时减弱动画设置传到 NSH；完整屏幕阅读器体验仍待验收。官方 0.6.0 的 headless 批量输入仍会报 `a kept row is gone`，现有内容断言通过并不代表缺陷已修复。设计约束与验证范围见 `DESIGN.md`；完整迁移关卡见根目录 `NSH_MIGRATION_PLAN.md`。
