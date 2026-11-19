# Workspace 登录服务

本地浏览器通过 `dev-oauth.pages.dev` 完成 GitHub 授权。`workspace-server` 使用原生 Turso 文件保存账号和 Workspace 会话。从仓库根目录可以用 `cargo build --release --locked -p workspace-server` 单独构建服务。先在私有数据目录建库，再启动服务。

```sh
cd crates/workspace-server
umask 077
workspace_data="${XDG_DATA_HOME:-$HOME/.local/share}/workspace"
mkdir -p "$workspace_data"
export WORKSPACE_DB_PATH="$workspace_data/auth.db"
cargo run --locked -- migrate
cargo run --locked -- serve
```

服务默认监听 `127.0.0.1:8789`。`WORKSPACE_LISTEN` 可指定本机地址和端口。HTTPS 反向代理使用固定的 `WORKSPACE_PUBLIC_ORIGIN`，例如 `https://workspace.example.org`。在网关的 `ALLOWED_REDIRECT_URIS` 中登记该来源的完整 `/v1/auth/github/callback` 地址。本机回环地址由网关允许。反向代理的访问日志应省略回调查询参数。GitHub 密钥保存在网关。

另开一个终端运行以下命令。浏览器完成授权后，命令显示当前 GitHub 用户并注销测试会话。

```sh
python3 - <<'PY'
import base64, hashlib, json, secrets, time, urllib.error, urllib.request, webbrowser

origin = "http://127.0.0.1:8789"
verifier = secrets.token_urlsafe(32)
challenge = base64.urlsafe_b64encode(hashlib.sha256(verifier.encode()).digest()).decode().rstrip("=")


def request(path, data=None, token=None):
    headers = {"Content-Type": "application/json"}
    if token:
        headers["Authorization"] = "Bearer " + token
    body = None if data is None else json.dumps(data).encode()
    try:
        with urllib.request.urlopen(urllib.request.Request(origin + path, body, headers), timeout=10) as response:
            return response.status, json.loads(response.read() or b"{}")
    except urllib.error.HTTPError as error:
        return error.code, {"error": error.reason}


status, flow = request("/v1/auth/flows", {"code_challenge": challenge})
if status != 201:
    raise SystemExit(f"创建登录流程失败，HTTP {status}")
print("请在浏览器完成 GitHub 授权", flow["login_url"], flush=True)
webbrowser.open(flow["login_url"])
deadline = time.monotonic() + flow["expires_in"]
while time.monotonic() < deadline:
    status, session = request(f"/v1/auth/flows/{flow['flow_id']}/exchange", {"verifier": verifier})
    if status == 200:
        token = session["token"]
        try:
            status, profile = request("/v1/me", token=token)
            if status != 200:
                raise SystemExit(f"读取用户失败，HTTP {status}")
            print(profile)
        finally:
            status, _ = request("/v1/auth/logout", {}, token)
            if status != 204:
                print(f"注销失败，HTTP {status}")
        break
    if status != 202:
        raise SystemExit(f"登录失败，HTTP {status}")
    time.sleep(flow["interval"])
else:
    raise SystemExit("登录已过期")
PY
```

`cargo test --locked` 使用本机 HTTP 服务器、模拟网关和真实 Turso 数据库验证流程。桌面接线和 QUIC 鉴权见迁移计划。
