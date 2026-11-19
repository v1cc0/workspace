# NSH 迁移与独立 Workspace 服务计划

NSH Linux 编辑、真实中文 IME、键盘预览、浏览器经 QUIC 与隔离持久 keyring 探针已验证。首版已选 GitHub OAuth 和原生 `turso`。P0 仍待 NSH 批量更新缺陷、完整读屏、应用会话接入与 OAuth 交接验证。

English scope: Replace the current desktop GUI with a Rust Native Shell (NSH) application, and extract an independently buildable and deployable Rust server for authenticated QUIC tunneling and user login backed by TursoDB.

## 1. 目标与首版范围

交付两个独立程序：

- `workspace-desktop`：Rust NSH 桌面应用，负责本地 Markdown 编辑、预览、浏览器、配置、登录状态和本地代理。
- `workspace-server`：独立 Rust 服务，负责登录、会话、QUIC 连接鉴权及 TCP 流量转发；账号与会话持久化到 TursoDB。

首版以 Linux 桌面为验收目标。macOS、Windows 保持清晰的平台边界，待各自浏览器代理集成通过后再宣布支持。Mobile、Web、TUI 暂不作为交付目标；浏览器不能直接复用原生 UDP/QUIC 和本地文件功能。

这里的 tunnel 沿用现有语义：浏览器 → 本地 HTTP/CONNECT 代理 → QUIC → 服务端 TCP 出口。它不是系统 VPN，也不包含反向端口暴露。

首版使用 GitHub OAuth 登录。Workspace 服务端验证身份，Turso 保存 Workspace 用户和可撤销会话。独立开发 OAuth 服务已部署在 Cloudflare Pages，据用户反馈 GitHub 登录可用，Google OAuth 正在添加。Workspace 首版验收范围为 GitHub。笔记保留本地，未登录或服务不可达时仍可编辑。

## 2. 已核实的现状

| 位置 | 当前行为 | 对迁移的影响 |
| --- | --- | --- |
| `Cargo.toml`、`src/ui/mod.rs` | 实际界面是 Dioxus Desktop；`GPUI_knowledge.md` 是参考文档 | 本次迁移对象是 Dioxus；无需设计不存在的 GPUI 组件兼容层 |
| `src/main.rs` | 同一二进制通过 `--server` 切换桌面端和 QUIC 服务端 | 当前服务端构建仍带 GUI 依赖，必须真正拆包 |
| `src/server.rs` | 监听 UDP 3000，接收目标地址并转发 TCP；无用户鉴权 | 公网部署前必须增加认证、目标访问策略和资源限制 |
| `src/client.rs`、`src/backend.rs` | 本地代理监听 `127.0.0.1:8080`；QUIC 断线重连；可回退直连 | 复用传输逻辑，但把生命周期和错误状态从 GUI 中分离 |
| `src/quic_utils.rs` | 每次启动生成自签名证书；客户端跳过证书及握手签名验证 | 不能用于生产，必须改为真实服务端身份校验 |
| `src/client.rs::start_browser_window` | Linux 专用 GTK3/Wry 窗口，依赖 Dioxus 已初始化 GTK | 不能直接移入 NSH 的 GTK4 进程 |
| `src/ui/mod.rs::MarkdownWorkspace` | 读取 `notes/`，输入时直接写文件，忽略读写错误 | 提取笔记逻辑时必须补齐错误反馈和安全保存，防止静默丢失数据 |
| `src/ui/mod.rs::MarkdownRenderer` | `pulldown-cmark` 生成 HTML，依赖 DOM 注入预览 | 优先验证 NSH 原生 Markdown 展示；受控 WebView 为备选 |
| `src/config.rs` | 原子保存配置；支持 IP/端口及 CLI、环境变量覆盖；已有测试 | 保留原子写入与覆盖顺序，增加 HTTPS API URL、QUIC 主机名和旧配置迁移 |
| `src/ui/mod.rs::SettingsView` | 已有可保存的设置页，重启应用生效 | 原进度文件中的“设置页仍为空”已过时，迁移要保留现有能力 |

Workspace 登录、数据库、迁移和部署配置待实现。规划时 Git 尚无提交；现已使用配置好的作者身份分别提交计划（`85f454b`）和现有源码基线（`03c13ab`）。本地笔记、配置、agent 说明与进度文件不纳入 Git。

最初验证使用 NSH `0.5.1`，当时缺少 WebKitGTK 6。当前已安装 GTK4 `4.24.1` 与 WebKitGTK `2.54.2`，桌面 crate 已升级到官方 NSH `0.6.0`，并用匹配的 CLI 重跑验证。具体结果见第 13 节。

## 3. 架构与依赖方向

采用一个 Cargo workspace，三个职责明确的 crate；共享内容仅限 API 数据结构与 tunnel 协议，不额外引入通用 service/core 框架。

| 单元 | 职责与依赖 |
| --- | --- |
| `crates/workspace-desktop` | NSH 页面、应用自有 `ui` 层、笔记、系统凭据存储、API 客户端、本地代理、QUIC 客户端及平台浏览器集成；依赖 `workspace-protocol` |
| `crates/workspace-server` | Tokio + Axum 登录 API、OAuth、Turso 访问、QUIC 接入、出口策略、健康检查；依赖 `workspace-protocol`，不依赖桌面 crate |
| `crates/workspace-protocol` | 版本化请求/响应、有限长度的认证与目标地址帧、错误码；不依赖 NSH、GTK、Turso 或业务会话实现 |

桌面端有两个明确的网络通道：

1. HTTPS API：登录、查询当前用户、注销、获取 tunnel endpoint。
2. QUIC：完成会话鉴权后，承载浏览器的 TCP 流；保留现有多路流方式。

服务端在一个独立进程中承载 API 和 QUIC。首版部署一台支持 UDP 的 Linux VPS，HTTPS 反向代理将 API 请求送到仅绑定 loopback 的 Axum；QUIC UDP 端口直接暴露。NSH 的开发服务器只用于开发，不承担这些生产职责。

Workspace 服务端配置数据库文件路径。桌面端只持有 Workspace 会话凭据。

建议的关键文件：

```text
Cargo.toml                         # Cargo workspace
Cargo.lock
crates/
  workspace-protocol/src/lib.rs
  workspace-server/
    Cargo.toml
    src/{main,auth,db,tunnel}.rs
    migrations/
  workspace-desktop/
    Cargo.toml
    nsh.json
    DESIGN.md
    theme.json
    src/{lib,main,config,notes,session,tunnel,proxy,browser}.rs
    src/ui/                        # 应用自有的 styled NSH controls
    src/pages/                     # Editor、Browser、Settings、Account
```

目录在实际有实现时才创建。协议 crate 在 P1 的真实共享代码抽取时创建；不要为预想的复用增加其他 crate。

## 4. 首先解决的 NSH 可行性问题

### 浏览器必须真实经过代理

NSH 官方 `WebView` 文档列出导航、HTML、脚本、请求头等能力，但没有公开列出代理配置。本计划不假定存在 `proxy` prop，也不把 `HTTP_PROXY` 环境变量视为 WebKit 代理的保证。

P0 按以下顺序验证：

1. 检查选定 NSH 版本的实际依赖源码与 WebView API，确认是否支持对 WebView 的网络 session 设置代理。
2. 若公开 API 已满足，直接使用；否则验证应用自有的 Linux native component，在 GTK4/WebKitGTK 6 网络 session 上配置 `127.0.0.1` HTTP 代理，并由 NSH 注册和布局。具体 WebKit 方法以实际 crate 为准。
3. 使用受控 HTTP/HTTPS 测试站点与服务端计数，证明主页面、重定向、子资源、WebSocket（若支持）走相同路径；使用仅允许服务端访问的目标证明桌面端无法绕过 tunnel。检查目标域名在哪里解析，并记录 DNS、WebRTC、HTTP/3 的实际路由边界；普通 CONNECT 代理不等同于全设备隐私保护。
4. 断开 QUIC 后，strict 模式必须停止请求，不能透明直连。只有用户明确选择 direct fallback 时才允许直连，并持续显示当前路径。

GTK3 与 GTK4 不在同一进程混用。若应用自有 native component 在限定的验证阶段仍不可行，后备方案是临时独立的旧浏览器进程，使用其自己的事件循环和指定代理；该方案必须记录为过渡措施，不能声称已完成 NSH 浏览器迁移。不得用普通系统浏览器打开 URL 来替代受代理保护的浏览器。

### Markdown 编辑与预览

先验证 NSH 原生多行输入、中文 IME、选择、撤销、长文本滚动，以及原生 Markdown/TextView 能否保留现有的表格、删除线、链接和代码块。精确 API 以所选版本源码为准。

若原生预览不能满足现有格式，复用 `pulldown-cmark`，在独立受控 WebView 中展示净化后的 HTML，禁用脚本及不必要的远程加载，不暴露浏览器页面桥接或会话凭据。编辑区仍由 NSH 原生控件实现。不要用一次 DOM/CSS 容器包装整个旧 UI 来宣称迁移完成。

## 5. 登录与会话设计

### 首版 GitHub OAuth 与 Workspace 会话

Workspace 采用开发网关的服务端接入方式。GitHub 将用户重定向到网关的 `/api/callback`，随后网关将用户送回 Workspace。把 Workspace 的完整 HTTPS 回调 URL 加入网关的 `ALLOWED_REDIRECT_URIS`。GitHub OAuth client secret 保存在网关。

1. 桌面端生成随机 verifier，将 S256 challenge 发给服务端，取得短期 `flow_id`、登录 URL 和轮询间隔。
2. 系统浏览器访问登录 URL。Workspace 服务端为本次 OAuth 流程生成并持有独立的 PKCE verifier 和一次性 `state`，绑定浏览器和登录流程，再跳转至网关的 `GET /api/login`。首版请求 `read:user`。
3. Workspace 回调校验 `state`、浏览器绑定和有效期，并在票据签发后 60 秒内向固定网关地址发送 `POST /api/exchange`，提交 `code`、网关用的 `code_verifier` 和原始 `redirect_uri`。网关调用 GitHub `/user`，响应中的 `user` 可以为 `null`。服务端收到有效的数值 `user.id` 后才创建或读取 Turso 用户并标记流程可兑换。资料缺失时提示重试。Workspace 只保存自身的用户和会话。
4. 桌面端按间隔携带自己的 verifier 轮询兑换。Workspace 校验 challenge，在事务中一次性消费流程并创建会话，通过兑换响应把会话凭据交给桌面端。
5. 返回高熵随机 opaque session token；数据库只存 token hash。首版会话固定 7 天过期，过期后重新登录，暂不引入 refresh-token 体系。

Workspace 登录流程有效期先设为 5 分钟。重复回调、重复兑换、错误 verifier、超时和取消必须有确定结果。只允许配置好的 OAuth 回调地址和提供方 URL，限制登录、轮询、回调的请求频率与载荷大小。

桌面端的会话保存在系统 keyring；token 只在会话模块中访问，不进入 `config.toml`、笔记、网页或日志。Linux 没有可用 Secret Service 时只使用内存会话，退出后重新登录。清除本地凭据时立即停止重连并关闭 QUIC；注销 API 失败要明确告知用户远端会话尚未撤销。

### 初版 API 合约

下表记录待实现的 Workspace 接口。

| 接口 | 用途与关键约束 |
| --- | --- |
| `POST /v1/auth/flows` | 创建 verifier 绑定的短期登录流程；返回服务端登录 URL、有效期和轮询间隔 |
| `GET /v1/auth/github/start` | 发起服务端 OAuth 跳转，校验对应流程 |
| `GET /v1/auth/github/callback` | 校验网关回调并从服务端兑换票据，将已验证身份的流程标记为可兑换 |
| `POST /v1/auth/flows/{id}/exchange` | verifier 兑换；pending、成功、过期、已消费状态明确 |
| `GET /v1/me` | Bearer session 验证，返回当前 Workspace 用户 |
| `POST /v1/auth/logout` | 撤销本次会话并关闭其活动 QUIC 连接；重复调用保持幂等 |
| `GET /v1/tunnel/config` | 返回允许的 endpoint、TLS server name、协议版本；需要有效会话 |
| `GET /health/live`、`GET /health/ready` | 进程存活，以及数据库/schema/监听器是否可用；不泄露配置 |

统一结构化错误码；认证失败为 401、无权限为 403、限流为 429、Turso 不可用为 503。客户端区分重新登录与暂时重试，不在 401 后无限重连。

## 6. TursoDB 的职责与表结构

首版使用 Turso Database 新引擎的原生 Rust crate `turso`。crates.io 索引确认当前最新稳定版为 `0.8.3`。Workspace 建库与接入待实现。单节点部署方案由 `workspace-server` 使用 `Builder::new_local` 打开持久磁盘上的数据库文件，作为认证状态的唯一来源。

| 表 | 最少字段与约束 |
| --- | --- |
| `users` | `id`、唯一 `github_user_id`、`display_name`、`created_at`、可空 `disabled_at` |
| `sessions` | `id`、`user_id` 外键、唯一 `token_hash`、`created_at`、`expires_at`、可空 `revoked_at` |
| `login_flows` | `id`、`code_challenge`、OAuth state/browser binding 的 hash、状态、可空 `user_id`、`expires_at`、`consumed_at` |
| `schema_migrations` | 已应用版本与应用时间 |

使用参数化 SQL、索引和外键约束，每个数据库连接启用外键检查。唯一身份的建号冲突必须可重试。兑换时在同一事务中执行条件更新并创建会话，只有条件更新命中一行时才创建会话。

数据库迁移是服务启动前显式执行的命令；先验 schema 版本，失败不启动对外服务。定期删除过期登录流程和超出保留期的会话。Turso 只保存认证元数据，不保存网页正文、逐包流量或本地 Markdown。

## 7. 认证后的 QUIC 协议

保留 Quinn 与 TCP 双向转发思路，增加显式版本和认证阶段，并集中定义目标长度与超时等边界。

1. 客户端连接配置中的真实主机名，验证 QUIC 服务端证书及签名；生产使用有效证书，开发使用显式信任的测试 CA。删除生产路径中的 `SkipServerVerification`。
2. TLS 建立后，第一条双向控制流发送有限长度的协议版本和 Workspace session token。首版禁用认证 0-RTT；认证完成前不接受目标连接。
3. 服务端查询权威 Turso session 与用户状态，失败则拒绝；成功后把连接绑定到 `session_id` / `user_id` 并返回认证成功。客户端收到成功后才将 tunnel 标记为可用。
4. 后续双向数据流沿用“目标地址 → ACK → 字节转发”模型；对目标解析、端口、长度、连接/空闲时间及半关闭行为制定一致规则。
5. 服务端按会话跟踪活动连接；注销立即关闭本进程内相关连接，并通过到期定时器和不超过 60 秒的重新验证处理数据库撤销或用户禁用。重新验证失败时关闭连接，不延长授权。

Turso 查询发生在连接鉴权和周期复核，不在每个转发字节上。数据库故障时拒绝新连接，既有连接最多运行到下一次复核；不会因此触发桌面端的自动直连。

公网出口必须防止成为开放代理或访问服务器内网：

- 默认只允许公开 HTTP/HTTPS 目标及配置允许的端口；私网、loopback、link-local、云 metadata、IPv4-mapped IPv6 等目标默认拒绝。额外端口按服务配置明确允许。
- 解析所有 DNS 结果后检查地址，直接连接已验证的 IP，避免检查后再次按 hostname 解析；混合公开/私网响应也拒绝。
- 限制未鉴权握手数、每账号连接数、每连接并发流、建连超时和空闲连接；错误要返回明确协议状态。
- TLS 只保护客户端到服务端；HTTP 出口仍是明文，HTTPS CONNECT 保持目标站点 TLS，不进行中间人解密。

恢复策略区分网络故障、认证过期、服务拒绝和数据库故障。网络故障采用有上限的指数退避与随机抖动，成功后重置；认证失败停下并提示登录。应用退出、注销和切换服务地址时显式取消旧任务并释放代理监听端口。

## 8. NSH 界面与数据迁移约束

采用 Rust 模式，使用生成模板的 `nsh.json`、root、runner 和 manifest 接线；不引入 TSX/QuickJS。所有 NSH crate 与 CLI 来自同一最新稳定版本，锁入 `Cargo.lock`，不混用 main 与 release。

先建立 `DESIGN.md`、浅色/深色语义主题、motion tokens 和应用自有 `src/ui` 控件，再写页面。页面使用统一 Button/Input/Tabs/Menu/Dialog 等样式与语义；颜色不散落到页面。控件集合随实际页面需求增长。

首版页面保持现有产品结构：

- Editor：Explorer、编辑区、预览和可恢复的折叠状态；文件列表采用虚拟列表与稳定路径身份。
- Browser：地址输入、现有快捷入口、代理浏览窗口/区域、加载和失败状态。
- Settings：API 地址、tunnel 目标、strict/direct fallback；保留配置校验与运行时覆盖规则。原 WebKit/GTK3 兼容选项在确认淘汰旧浏览器后再移除，不能照搬禁用 sandbox 的全局设置。
- Account：登录、取消/重试、已登录身份、注销，以及过期/服务不可达状态。

应用根拥有 session、tunnel 和 proxy 生命周期，页面只拥有页面状态。NSH signals 只在 engine 线程更新；Tokio 任务通过值/通道交接状态，不从网络线程直接修改 UI signal。组件切换不能重启 tunnel 或丢弃编辑草稿。

笔记服务从组件中提取，保留现有 `notes/` 路径语义和空文件行为；读取失败不冒充空文件，保存使用同目录临时文件与原子替换，失败保留草稿并显示错误。若使用后台合并保存，每个文件按 revision 串行，切换文件/退出时处理尚未落盘的数据，防止旧请求覆盖新输入。

配置先兼容读取原 TOML，保留原文件与未知/无法迁移值的可恢复副本；新增字段有明确默认值。旧 IP/端口不能自动被当成已信任的生产 HTTPS 主机，首次连接需设置 API URL 和受信 TLS 名称。新安装默认 strict；现有明确保存的 direct fallback 选择予以保留并持续提示，登录过期或鉴权失败不得被回退机制绕过。

每页编码前写 motion inventory；检查键盘、焦点、屏幕阅读器、中文输入、最小窗口、长文本、放大字体、浅色/深色及 reduced motion。行为测试与真实窗口截图分别记录证据。

## 9. 实施顺序与验收关卡

按下列顺序做小步迁移；每阶段独立可验证。P0 是开始整批重写前的技术关卡。

| 阶段 | 工作 | 通过条件 |
| --- | --- | --- |
| P0：建立基线与可行性验证 | 配置提交身份后保存当前源码基线；记录现有测试与行为；收敛登录方式；验证 NSH 编辑/预览、GTK4 代理路径、系统 keyring、Turso 引擎与事务 | 真实 NSH 窗口可编辑中文；HTTP/HTTPS 受控流量走代理且 strict 不泄漏；选定登录方式和数据库 SDK；形成可执行的 browser 方案 |
| P1：拆出独立服务与协议 | 建立 Cargo workspace；移动服务器、协议、桌面代理职责；保留现有 Dioxus 桌面入口以便对照 | `workspace-server` 可在无 GTK/WebKit 的环境单独 build/test/start；既有代理功能通过端到端验证；新协议客户端/服务端配套测试 |
| P2：Turso 与登录 API | 建表迁移、OAuth、短期登录流程、opaque sessions、注销、健康检查 | 登录→重启服务→验证会话→注销链路成立；并发兑换最多一个成功；过期/replay/错误 verifier/数据库不可用不产生授权 |
| P3：收紧 QUIC 与代理 | TLS 身份校验、首流鉴权、撤销、目标策略、限流、超时、重连与取消；先用 Rust 测试客户端验证 | 匿名/无效会话不能建目标流；错误证书被拒绝；注销关闭已有流；私网和 DNS rebinding 用例被拒绝；strict 无直连 |
| P4：NSH 基础与账号接入 | 生成 Rust NSH app，建立主题/控件/状态桥，接通账号和 tunnel 状态 | 登录/退出/过期/断网有真实状态；切页不重复启动任务；浅色/深色、键盘及生命周期检查通过 |
| P5：逐页迁移 | 提取安全的 notes 服务，迁移 Editor、预览、Browser、Settings；执行旧配置迁移 | 现有功能对照表全部通过；笔记失败保存不丢稿；快捷入口传入正确 URL；真实浏览器经鉴权 tunnel 访问 |
| P6：替换发布入口与部署 | 默认入口切换 NSH；删除已无调用的 Dioxus、GTK3/Wry 和旧 CSS；完善独立服务构建、部署、备份/恢复说明 | NSH 桌面包在干净 Linux 环境运行；服务端无 GUI 依赖；云端登录+tunnel 实测；回退与 Turso 恢复演练通过 |

P1/P2 的中间版本只用于本地或隔离环境；在 P3 鉴权、证书与出口限制通过前不开放公网。旧客户端不会兼容新认证协议，需明确版本拒绝和配套升级，不能临时开放匿名连接来兼容。

首次 P0 任务已建立可回退基线，并完成最小 NSH 编辑、代理和凭据验证。登录方式和 Turso SDK 已选定，P1 仍以 P0 剩余关卡通过为前提。

## 10. 必要测试与交付证据

复用 `src/config.rs` 的现有测试，按风险添加少量有区分力的测试，不为每个 UI 样式写镜像测试。

- 协议/代理：分段输入、越界帧、IPv6、HTTP absolute-form 转 origin-form、CONNECT 后残留 payload、并发流、超时、半关闭、大数据双向传输及 strict/fallback 的不同结果。
- 认证/Turso：稳定 GitHub ID 关联、一次性 state 与流程、并发消费、会话 hash、过期/撤销、事务回滚、真实 schema 升级、数据库故障。
- 服务端安全边界：预鉴权不能开目标连接、错误证书、用户禁用、私网地址与 DNS 变化、资源上限、日志不含凭据。
- 笔记/配置：空文件与读失败区分、原子写失败保留旧内容、快速编辑不被旧保存覆盖、切页保留草稿、旧 TOML 兼容与非法配置不破坏原文件。
- NSH headless：登录状态切换、Account/Editor/Browser/Settings 导航、文件选择、保存失败、设置提交。真实 Linux：IME、撤销、键盘/焦点、缩放、两种主题截图及浏览器实际代理路径。
- 独立构建：在无 GUI 系统库的容器中 `cargo build/test -p workspace-server`，并检查 `cargo tree -p workspace-server` 不包含 NSH/Dioxus/GTK/Wry。
- 部署冒烟：HTTPS API、OAuth 回调、UDP 可达、服务重启后会话、注销后已有 tunnel、证书错误、Turso 故障、备份恢复。

实现后的常规验证使用 `cargo fmt --check`、各 crate 的定向 test/check/Clippy，NSH Rust 应用运行 `nsh check`、`nsh test` 和真实 Linux 窗口检查。只构建服务端时显式选择服务端 package，避免默认构建把 GUI 拉进来。

## 11. 部署、升级与回退

首版使用单节点服务和一个权威 Turso 库。服务端内存只保存活动连接映射及有上限的运行状态，持久会话保存在服务所在主机的 Turso 文件中。备份和恢复需要覆盖该持久目录。

Workspace 服务配置包含 API bind/public URL、QUIC bind/TLS server name、证书与私钥路径、Turso 文件路径、OAuth 集成参数、出口与连接限制。GitHub client secret 由处理授权码的服务单独配置。配置示例使用示例值，日志仅记录必要的用户标识、会话标识和错误类别。

HTTPS 与 QUIC 的证书分别配置和续期，TLS 代理不会自动替 QUIC 装载证书。开放 HTTPS/TCP 与 QUIC/UDP，对 API 内部端口保持 loopback；所选主机必须支持长期 UDP，纯 HTTP 函数平台不足以承载现有 tunnel。

发布前保留源码基线和旧应用构建；配置迁移前做本地备份，笔记不做破坏式转换。数据库首批迁移只做向前兼容的新增，升级前备份并验证恢复；不把会销毁会话的 schema 回滚当成常规回退。

按 P0→P6 逐步切换。出现编辑/浏览器回归时，在本地或隔离环境可回到旧桌面基线；旧匿名 tunnel 服务不能作为生产安全回退。生产服务升级有问题时回到上一份已通过鉴权测试的服务端构建，或暂时停用 tunnel，本地笔记仍可工作。

完成标准：NSH 成为实际桌面入口；独立服务器不依赖 GUI；用户可通过服务端登录并跨服务重启保持会话；只有已授权连接能使用 tunnel；配置与笔记无静默丢失；有可重复的构建、部署和验证说明。

## 12. 依据与尚待实测项

官方参考（本轮已查看；实现时重新核对当前稳定版本）：

- [NSH Rust](https://nsh.dev/docs/rust.md)、[Coding Guides](https://nsh.dev/docs/coding-guides.md)、[Design Guides](https://nsh.dev/docs/design-guides.md)
- [NSH WebView](https://nsh.dev/docs/web-view-and-video.md)、[Native Components](https://nsh.dev/docs/native-components.md)、[Linux](https://nsh.dev/docs/linux.md)
- [NSH latest release](https://github.com/longbridge/nsh/releases/latest)
- [原生 `turso` crate 0.8.3](https://docs.rs/crate/turso/0.8.3)
- [crates.io 的 `turso` 版本索引](https://index.crates.io/tu/rs/turso)

P0 尚待解决 NSH 批量 Markdown 更新的行生命周期缺陷，实测完整读屏、应用会话回退及 OAuth 交接。浏览器 API、原生 Markdown 基本格式和受控 HTTP/HTTPS/WS 路由的实测结果见下一节。

## 13. P0 已执行结果与剩余关卡

### 已建立的基线与入口

- 计划与旧源码已分别提交，旧 Dioxus/GTK3 入口可单独回退；源码基线为 `03c13ab`。
- `crates/workspace-desktop` 来自官方 `nsh init --template rust`，保留 manifest、native module、root 与平台 runner 接线；项目 crates 和本轮验证使用的 CLI 均为稳定版 `0.6.0`。
- 该 crate 暂时保留独立 `[workspace]`，在 P1 才合并根 Cargo workspace；它不替换原桌面入口。
- 实现受控原生多行输入与原生 `TextView` Markdown 预览，已有语义主题和应用自有 `ui` 层。草稿仅在内存中；不读取或写入 `notes/`。预览链接只报告，图片以文字代替，避免验证窗口主动加载远端资源。

### 验证证据

| 检查 | 结果与边界 |
| --- | --- |
| `nsh check crates/workspace-desktop` | 通过 Rust 与 TypeScript 检查 |
| `nsh test crates/workspace-desktop` | 5 项通过：3 项 Rust 检查与 2 项 headless 编辑流程；需要显示会话的浏览器测试默认跳过，另行显式运行通过 |
| `GTK_IM_MODULE=fcitx nsh test crates/workspace-desktop --target linux` | 2 项原生流程通过；输入模块缺失警告已消除 |
| 显式运行所有 Rust 测试（含原生浏览器） | 4 项通过：URL 校验、HTTP 转发回归、无 tunnel 的 strict GET/CONNECT、真实 WebKit/QUIC 路由与断连检查 |
| 定向 fmt / Clippy | 新 NSH crate 的 `cargo fmt --check` 与 Clippy `-D warnings` 通过；旧源码其他文件的既有格式差异未混入本轮修改 |
| 原生 Linux 窗口 | `cargo run --manifest-path crates/workspace-desktop/Cargo.toml` 可启动；GTK 原生编辑 buffer 更新后预览能更新 |
| 浅色/深色与小窗口 | 原始 `.nsh/p0-light.png`、`.nsh/p0-dark.png` 可见中文、表格、删除线、代码块。新增 `.nsh/p0-ime-small.png` 是实际 800×600、150% 字号的原生窗口渲染：双栏、标题与中文输入/预览无重叠；不代表已定义产品最小窗口策略 |
| 真实中文输入与无障碍接口 | 现有 fcitx `shuangpin`：`n` → `ni` preedit，空格提交“你”，Ctrl+Z 撤销、Ctrl+Y 重做、Shift+Left 选择通过；AT-SPI 可读中文、可编辑状态和 0–1 选择范围。完整读屏体验尚未验收 |
| GTK 键盘与运行时减弱动画 | Tab 从编辑区到预览，End 滚动长文，Shift+Tab 返回；切换进程内 GTK 动画设置，NSH 收到 `false → true → false` 的减弱动画状态。不等于完整读屏验收 |
| Secret Service | `keyring 4.2.0` 探针在隔离 D-Bus、临时 XDG 目录和两个连续运行的守护进程中写入、跨进程/守护进程重启读取、删除并确认不存在；无 Secret Service 时探针仅在内存存取。用户的真实默认 collection 未解锁，应用会话未接入 |
| 原生 Turso 本地文件 | `turso 0.8.3` 临时库用 8 个连接并发兑换同一流程。`BEGIN IMMEDIATE` 事务内条件更新并插入会话，共提交 1 条会话，重新打开数据库后仍可读。唯一约束冲突后回滚，第二个流程仍未消费。每个连接启用外键检查，测试确认无效用户引用被拒绝 |
| 现有代理与配置 | 将原模块按路径编入临时隔离 Cargo harness，并复用根 lockfile 已解析的依赖，6 项测试通过：5 项原有配置测试，以及新增 strict 回归；无 tunnel 时 GET/CONNECT 均返回 503，受控目标未接到直连 |
| 完整旧桌面测试 | 受环境阻塞：缺少 `webkit2gtk-4.1` 与 `javascriptcoregtk-4.1`；隔离核心测试不能代替全程序构建 |

### 浏览器集成的确定方向

已查看实际 `nsh-core` 的 `WebProps` 和 `nsh-platform-linux` 的 `native/web.rs`：没有代理属性，网络 session 由平台内部建立。应用自己的 Linux native component 是当前可执行方向，不修改 NSH 源码、不混用 GTK3/GTK4。

应用自有 GTK4 native component 已实现于 `src/browser.rs`。`NetworkSession::new_ephemeral` 与显式 HTTP 代理设置先于 WebView 创建，`src` 保留调用方 URL，只允许 HTTP/HTTPS 导航。禁用 WebRTC 和 DNS prefetch；不把 token 或页面 bridge 暴露给网页。当前编辑窗口未添加浏览器页面，组件通过原生夹具验证。

`tests/browser_proxy.rs` 使用现有 `src/backend.rs`、本机 QUIC 与仅允许夹具目标的 TCP 出口。实测 HTTP/HTTPS、重定向、CSS、SVG、脚本、fetch、WS/WSS 全部经过 QUIC；loopback URL 也没有绕过代理。服务端关闭 QUIC 后仍保留旧连接对象，再次请求 HTTP/HTTPS 时源站新增 TCP 连接数为零；停止本地代理后同样没有直连。测试 TLS 仅信任该临时 session 中的夹具证书和主机名，不修改系统信任或生产校验。

浏览器实测暴露并修正了共享代理的 HTTP/WS absolute-form 转发缺陷：转为源站需要的 origin-form，保留正文与 Upgrade，去掉 Proxy-Connection/Proxy-Authorization，普通 HTTP 使用一请求一连接。直连回退 POST 回归同时检查编码路径/查询、二进制正文和自定义头。此验证不代表系统全流量路由或公网服务安全验收。

用户已安装 `webkitgtk-6.0 2.54.2` 与 `fcitx5-gtk`。该 GTK 输入模块使用进程级 `GTK_IM_MODULE=fcitx`，原环境的 `fcitx5` 名称不适用；本轮未更改全局环境。

### 剩余关卡

1. 官方 NSH 0.6.0 优化了保留行快路径，但独立回归和应用 headless 快速输入均仍出现 5 次 `a kept row is gone`。隔离副本为 `RegionElement::Kept` 保留行 owner，跳过已释放的行；同一复现为 0 次，NSH core 单元测试 1,772 项通过、15 项忽略。补丁未进入项目或安装目录，仍需评估快路径性能和上游修复。
2. 原生编辑/预览 Tab 往返、预览 End 滚动及动态减弱动画已验证；Orca 未安装，完整读屏仍未验收。前轮 Ctrl+Shift+Z 本机未触发重做，Ctrl+Y 可用。
3. 已验证隔离持久凭据及探针内存回退；接入真实会话模块后再验证锁定、注销、失败提示和跨应用重启。不要读取或解锁用户现有凭据来测试。
4. 已对照开发网关源码 `9ada5f4` 确认回调和兑换合约。网关用模拟的 GitHub 和 Google 响应运行 6 项测试，全部通过。Workspace 的回调登记和端到端登录待验证。原生 `turso 0.8.3` 的本地事务已通过隔离验证，Workspace 建库、迁移和会话集成待实现。

P0 尚未整体通过。P1 workspace/协议/服务拆分等待上述关卡处理。
