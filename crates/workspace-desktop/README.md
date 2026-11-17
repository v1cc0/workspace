# Workspace NSH desktop — P0

Rust NSH 0.5.1 的 Linux 编辑与原生 Markdown 预览验证入口。草稿仅存内存，关闭窗口后丢弃；尚未接入现有 `notes/`、登录或 tunnel。现有桌面仍从仓库根目录运行。

从仓库根目录执行：

```sh
nsh check crates/workspace-desktop
nsh test crates/workspace-desktop
nsh run crates/workspace-desktop --target linux
# 不使用开发服务器，直接启动原生窗口：
cargo run --manifest-path crates/workspace-desktop/Cargo.toml
```

当前 crate 保留官方模板独立的 `[workspace]`，P1 拆包时再接入根 workspace。NSH CLI 与所有 NSH crates 均为 0.5.1。Linux 需要 GTK4 及 NSH 对应的 GStreamer 开发库；此验证窗口不需要 WebKitGTK，也不代表浏览器代理验证通过。

中文文字输入的自动测试不能代替真实 IME 组合输入检查。设计约束与验证范围见 `DESIGN.md`；完整迁移关卡见根目录 `NSH_MIGRATION_PLAN.md`。
