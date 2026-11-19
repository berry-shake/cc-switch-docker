# cc-switch-server：服务器 / Docker / WSL 上的用量统计

`cc-switch-server` 是 CC Switch 的无界面版本，只保留**用量统计**：定时扫描服务器上各 AI CLI 的会话日志（Claude Code、Codex、Gemini CLI、OpenCode、Grok Build、Pi、Kimi Code 等，与桌面版一致），把用量写进 SQLite，并通过 Web 面板展示（同桌面版的「用量统计」页：趋势图、热力图、请求日志、供应商 / 模型统计、模型定价、币种换算）。

- 只读扫描会话目录，**不改**任何 CLI 配置，也不提供代理 / 供应商切换。
- 数据库和桌面版同构（`~/.cc-switch/cc-switch.db`），费用按同一套模型定价计算。

## 快速开始（Docker Compose）

```bash
git clone https://github.com/berry-shake/cc-switch-docker.git
cd cc-switch-docker
cp .env.example .env        # 修改 CC_SWITCH_WEB_PASSWORD
docker compose up -d
```

浏览器打开 `http://<服务器IP>:8765`，在登录页输入 `.env` 里的用户名 / 密码。

面板有三页（顶栏切换）：

- **用量统计**：同桌面版。
- **会话**：浏览、搜索、阅读服务器上各 CLI 的会话记录（同桌面版的会话页），可导出 Markdown（浏览器下载）、复制恢复命令。只读：没有删除、多选、在终端恢复、在文件管理器中显示。读的就是上面挂载进容器的会话目录，**能登录面板的人就能看到全部会话内容**（可能包含代码和密钥），务必设置强密码。右上角 ⋯ 里可以打开「搜索消息正文」，索引存在数据目录里。
- **应用**：每个 CLI 的累计请求数 / Token，以及「显示」开关，决定用量页筛选栏里出现哪些 CLI（只影响显示，隐藏的 CLI 照常采集、照常计入「全部」）。「只显示有记录的」一键隐藏没数据的 CLI。

### 登录

- 登录后下发 HttpOnly 会话 Cookie；勾选「保持登录 30 天」为持久 Cookie，否则关闭浏览器即失效（服务端最多认 12 小时）。
- 会话签名密钥保存在数据目录的 `web-session.key`（0600），重启服务不用重新登录；**改密码后所有已登录会话立即失效**。删掉该文件也会让所有会话失效。
- 同一 IP 15 分钟内失败 10 次后暂时拒绝登录（HTTP 429）。
- 经 HTTPS 反向代理访问时，代理需传 `X-Forwarded-Proto: https`，Cookie 才会带 `Secure`。
- 脚本仍可直接用 HTTP Basic 访问 `/api/*`（`curl -u 用户名:密码`），见下文 HTTP API。浏览器里的请求只认登录 Cookie（旧版浏览器缓存的 Basic 凭据不再生效）。

默认挂载宿主机的 `~/.claude` 和 `~/.codex`（只读）。用到其他 CLI 时，在 `docker-compose.yml` 里取消对应行的注释；只保留宿主机上确实存在的目录，不存在的路径会被 Docker 自动建成空目录（属主 root）。

查看日志：`docker compose logs -f`。首次启动会全量导入历史会话，日志里能看到 `Session usage sync imported N record(s)`。

### 镜像

GitHub Actions 会构建 `linux/amd64`、`linux/arm64` 两种架构并推送到 GHCR：

| 标签 | 来源 |
|---|---|
| `ghcr.io/berry-shake/cc-switch-docker:latest` | 默认分支（`server`）最新提交 |
| `ghcr.io/berry-shake/cc-switch-docker:server` | 同上（分支名） |
| `ghcr.io/berry-shake/cc-switch-docker:4.0.35` | 推送标签 `server-v4.0.35` |
| `ghcr.io/berry-shake/cc-switch-docker:sha-xxxxxxx` | 每次提交 |

想自己构建：`docker compose build`，或 `docker build -t cc-switch-server .`。

## 配置

全部通过环境变量配置：

| 变量 | 默认值 | 说明 |
|---|---|---|
| `CC_SWITCH_WEB_LISTEN` | 镜像内 `0.0.0.0:8765`；裸跑 `127.0.0.1:8765` | 监听地址 |
| `CC_SWITCH_WEB_USERNAME` | `admin` | 登录用户名 |
| `CC_SWITCH_WEB_PASSWORD` | 无 | 登录密码。监听非回环地址时**必须**设置 |
| `CC_SWITCH_WEB_INSECURE` | 无 | 设为 `1` 时允许在非回环地址上不设密码（前面已有带认证的反向代理时用） |
| `CC_SWITCH_SYNC_INTERVAL_SECS` | `600` | 后台扫描间隔（秒），`0` 只在启动时扫描。面板打开时还会按页面上选的间隔加扫 |
| `CC_SWITCH_CLAUDE_EXTRA_DIRS` | 无 | 额外的 Claude 配置目录（目录下有 `projects/`），`:` 分隔 |
| `CC_SWITCH_WEB_DIST` | 镜像内 `/app/web` | Web 面板静态文件目录 |
| `CC_SWITCH_LOG_LEVEL` | `info` | `error` / `warn` / `info` / `debug` / `trace` |
| `TZ` | `Asia/Shanghai` | 按天统计（「当天」、日趋势、热力图）使用的时区 |

其他工具的目录覆盖（如 `codexConfigDir`）与桌面版一样写在数据卷里的 `settings.json`，一般不需要：会话目录挂到容器里 `/root` 下的默认位置即可。

### 一台服务器多个用户

把每个用户的 `~/.claude` 挂到 `/sources/<name>/.claude`，再在 `CC_SWITCH_CLAUDE_EXTRA_DIRS` 里列出：

```yaml
    environment:
      CC_SWITCH_CLAUDE_EXTRA_DIRS: /sources/alice/.claude:/sources/bob/.claude
    volumes:
      - /home/alice/.claude:/sources/alice/.claude:ro
      - /home/bob/.claude:/sources/bob/.claude:ro
```

目前统计不区分用户 / 机器，所有目录的用量合在一起显示。

### WSL

在 WSL 发行版里装 Docker（或 Docker Desktop 开启 WSL 集成）后，在 WSL 里执行上面的 `docker compose up -d`，`${HOME}` 就是 WSL 里的 home。Windows 浏览器访问 `http://localhost:8765`。

## 不用 Docker 直接运行

```bash
pnpm install
pnpm build:web                                            # → dist-web/
cd src-tauri
cargo build --release --features web-server --bin cc-switch-server
CC_SWITCH_WEB_DIST=../dist-web ./target/release/cc-switch-server   # http://127.0.0.1:8765
```

Linux 上编译需要和桌面版一样的 GTK / WebKit 开发包（同一个 crate），参考 `Dockerfile` 里的 `apt-get install`。

前端开发：先跑起服务端，再 `pnpm dev:web`（`http://localhost:3001`，`/api` 代理到 `CC_SWITCH_SERVER_URL`，默认 `http://127.0.0.1:8765`）。

## HTTP API

| 方法 | 路径 | 说明 |
|---|---|---|
| `GET` | `/api/health` | 健康检查，不需要认证 |
| `GET` | `/api/session` | 当前会话状态（`authRequired` / `authenticated` / `username` / `version`），不需要认证 |
| `POST` | `/api/login` | `{"username","password","remember"}`，成功后下发会话 Cookie |
| `POST` | `/api/logout` | 清除会话 Cookie |
| `POST` | `/api/invoke/<command>` | 与桌面版 Tauri 命令同名同参（JSON，参数名 camelCase） |
| `GET` | `/api/events` | SSE，有新用量导入时推送 `{"event":"usage-log-recorded"}`，正文索引进度推送 `{"event":"session-index-status","payload":{...}}` |

`/api/health`、`/api/session`、`/api/login`、`/api/logout` 之外的接口需要会话 Cookie 或 HTTP Basic 认证。例如：

```bash
curl -u admin:密码 -X POST http://localhost:8765/api/invoke/get_usage_summary \
  -H 'Content-Type: application/json' \
  -d '{"startDate": 1767196800, "endDate": 1769875200}'
```

开放的命令：`get_usage_summary`、`get_usage_summary_by_app`、`get_usage_trends`、`get_usage_first_date`、`get_provider_stats`、`get_model_stats`、`get_request_logs`、`get_request_detail`、`get_session_usage_summary`、`sync_session_usage`、`get_session_usage_last_sync`、`rebuild_codex_usage`、`get_model_pricing`、`update_model_pricing`、`update_model_pricing_batch`、`delete_model_pricing`、`get_models_dev_sync_config`、`save_models_dev_sync_config`、`record_models_dev_sync_result`、`fetch_models_dev`、`get_pricing_model_source`、`set_pricing_model_source`、`fetch_exchange_rates`、`get_codex_quota_snapshot`、`get_codex_official_usage_snapshot`、`get_settings`、`save_settings`（只写入用量页的几项设置、`visibleApps` 和 `sessionContentSearchEnabled`）、`get_global_proxy_config`；会话页（只读）：`list_sessions`、`get_session_messages`、`stream_session_messages`（NDJSON，每行一个分块）、`get_session_block_content`、`get_session_image`（原始字节）、`search_session_content`、`sync_session_index`、`clear_session_index`、`get_pi_session_discovery`、`get_omp_session_discovery`。删除会话、在终端恢复、在文件管理器中显示等命令不开放。

## 实现说明

- 服务端代码在 `src-tauri/src/web_server/`，由 Cargo feature `web-server` 控制，桌面版构建不包含它。
- Web 面板入口 `src/web.html` → `src/web/main.tsx` → `WebRoot`（登录页 / 顶栏 / 页面切换），用量页、会话页复用桌面版的 `UsageDashboard`、`SessionManagerPage`；`src/lib/webBuild.ts` 的 `IS_WEB_BUILD` 在 Web 构建里藏起删除等本机操作，`@/hooks/useSettings` 在 Web 构建里换成 `src/web/useWebSettings.ts`；`vite.web.config.ts` 把 `@tauri-apps/*` 换成 `src/web/tauri-shim/`（invoke → HTTP，事件 → SSE）。
- 二进制和桌面版共用一个 crate，所以仍动态链接 GTK / WebKit 运行库（镜像里已安装，不会创建窗口），镜像因此偏大。

## 合并上游

`server` 分支基于 CC Switch 的 fork（`mod` 分支），删掉了上游桌面版的 GitHub Actions，只保留 `.github/workflows/docker.yml`。
合并上游时如果遇到这些 workflow 的「修改 / 删除」冲突，保持删除即可：

```bash
git merge <upstream-tag>
git status --short | awk '/^(UD|DU) .github\//{print $2}' | xargs -r git rm -q
```

## 已知限制

- 统计不区分来源机器 / 用户。
- OpenCode 的 SQLite 处于 WAL 模式时，只读挂载可能打不开；需要时去掉该挂载的 `read_only`。
- 「数据来源」里「记录请求用量」开关显示的是数据库里的代理配置，服务端不运行代理，所有数据都来自会话日志。
