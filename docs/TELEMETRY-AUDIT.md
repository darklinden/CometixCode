# CometixCode 遥测与网络出站审计报告

| 项 | 值 |
|---|---|
| 审计对象 | `/opt/home/Github/CometixCode` |
| 版本 | `cometix-code` v0.2.0（AGPL-3.0-only，Rust edition 2024） |
| 上游 | `git@github.com:darklinden/CometixCode.git`，3 commits（HEAD `d7f123e`） |
| 审计日期 | 2026-09-21 |
| 审计方式 | 静态源码审查（grep 全量枚举 + 关键路径逐行阅读），未运行二进制 |

---

## 一、结论摘要

1. **本工程不包含向 Anthropic 上报使用数据的遥测出口。** 遥测事件的*生成层*完整移植（83 个 `tengu_*` 事件名、14 个文件中的 18 处 `log_event` 调用点），但*出口层*被显式移除：事件只进入进程内内存队列，无 sink、不落盘、不出网。
2. **官方 Claude Code 的全部第三方遥测通道均未移植**——无 Statsig/GrowthBook 远程拉取、无 Sentry、无 Datadog、无 OpenTelemetry/OTLP 导出器。错误上报退化为本地 JSONL 文件。
3. **存在两处自动发起的外呼**（非上报性质，拉取公开数据）：插件安装量统计 JSON 与官方插件市场 GCS 快照。**这两处缺少 `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC` 的运行时门控**，是本次审计发现的主要偏差。
4. **凭据类网络出口被编译期常量整体锁死**：`OAUTH_CREDENTIAL_SIDE_EFFECTS_ENABLED = false`，覆盖 OAuth token 交换、keychain 写入、浏览器拉起、文件上传等 20 余处，默认 fail-closed。自动更新同样默认关闭且实现为空。

---

## 二、项目性质

CometixCode 是用 Rust 重写的终端 AI 编程助手，自我定位为 Anthropic Claude Code CLI 的 **1:1 复刻**（逐文件移植，而非重新设计架构）。

- **技术路线**：原版是 TypeScript + React + Ink；本工程换成 Rust + `iocraft`（经 `CometixTUI` fork 提供 Ink 对应原语：行级 diff、事件冒泡、SIGCONT 自愈、IME 光标、bracketed paste、grid 布局）。
- **自研依赖**：`anthropic-sdk-rs`（对应 `@anthropic-ai/sdk`）、`marked-rs`（对应 `marked`）、`nucleo`（来自 Helix 的模糊匹配）。
- **声明**：README 明确标注 "Unofficial project, not affiliated with Anthropic"。
- **规模**：`src/services/` 单目录约 8.5 万行；顶层模块含 `services/`、`tools/`、`components/`、`screens/`、`commands/`、`hooks/`、`utils/`、`context/`、`state/`。

---

## 三、遥测架构：生成层完整，出口层缺失

### 3.1 事件的唯一后端是内存队列

`src/services/analytics/mod.rs:9-22` 是整个工程唯一的 analytics 后端：

```rust
/// This port has no attached analytics backend. Preserve the source pre-sink
/// queue instead of sending telemetry to an invented endpoint.
static EVENT_QUEUE: std::sync::Mutex<Vec<(String, serde_json::Value)>> = ...;

pub fn log_event(event_name: &str, mut metadata: serde_json::Value) {
    ...
    EVENT_QUEUE.lock()...push((event_name.to_owned(), metadata));
}
```

事件入队后没有任何 drain、flush 或网络发送路径。进程退出即消失。

### 3.2 官方 sink 初始化被显式跳过

`src/utils/sinks.rs:6-7`：

```rust
// CC: initializeAnalyticsSink(). Telemetry backends remain excluded under
// PORTING.md; this source location does not enable a substitute endpoint.
```

### 3.3 事件生成面（保留完整）

| 指标 | 数量 |
|---|---|
| 唯一 `tengu_*` 事件名 | 83 |
| `log_event(` 调用点（不含定义处） | 18，分布于 14 个文件 |
| 插件遥测专有模块 | `src/utils/telemetry/plugin_telemetry.rs`（202 行） |

事件生成的分布（`log_event(` 调用点所在文件）：

```
src/utils/messages.rs                              src/commands/branch/branch.rs
src/utils/plugins/fetch_telemetry.rs               src/commands/plugin/add_marketplace.rs
src/utils/plugins/hint_recommendation.rs           src/commands/plugin/manage_marketplaces.rs
src/utils/plugins/official_marketplace_gcs.rs      src/hooks/use_manage_plugins.rs
src/utils/plugins/plugin_installation_helpers.rs   src/services/mcp/use_manage_mcp_connections.rs
src/utils/permissions/yolo_classifier.rs           src/utils/telemetry/plugin_telemetry.rs
src/utils/permissions/permission_setup.rs          src/utils/hooks/exec_agent_hook.rs
```

**隐私保护在事件层是实现了的**，例如 `plugin_telemetry.rs:69-80` 对非官方插件做名称脱敏（`plugin_name_redacted: "third-party"`）并只发送 SHA-256 短哈希（`plugin_id_hash`）。但因为不存在出口，这些脱敏逻辑当前无实际网络意义。

### 3.4 官方遥测通道的移植状态

| 官方 Claude Code 通道 | 本工程状态 | 证据 |
|---|---|---|
| Statsig / GrowthBook 远程拉取 | **未实现**。仅有本地磁盘缓存 + env/config 覆盖 | `src/services/analytics/growthbook.rs:1-5`（"Network initialization and exposure logging are owned by the future analytics runtime"） |
| GrowthBook 特性开关下发 | **未实现**，改为硬编码开关表 | `src/utils/feature_flags.rs:1-6`（"intentionally does not implement cloud delivery, cache parsing, refresh windows, or config override ingestion"） |
| Sentry / Datadog | **未实现**，全库无引用 | 全量 grep 无命中 |
| OpenTelemetry / OTLP 导出 | **未实现**。`OTEL_*` 仅作为环境变量转发白名单存在 | `src/utils/managed_env.rs:126,148-156` |
| 错误上报 | **降级为本地文件** | `src/utils/error_log_sink.rs:26-28`（写 `~/.claude/.../errors/<date>.jsonl`），全文件无 HTTP 调用 |

### 3.5 隐私开关（实现完整，但当前空转）

`src/utils/privacy_level.rs:13-21` 实现三级隐私：

| 环境变量 | 级别 |
|---|---|
| `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC` | `EssentialTraffic`（最严格） |
| `DISABLE_TELEMETRY` | `NoTelemetry` |
| （均未设置） | `Default` |

`src/services/analytics/config.rs:6-18` 的 `is_analytics_disabled()` 额外覆盖 Bedrock/Vertex/Foundry provider 与 `NODE_ENV=test`。

---

## 四、网络出站面清点

全库 20 个模块引用 `reqwest`，26 处 `.send()` 调用点。按性质分类如下。

### 4.1 功能性出站（用户主动触发，属预期行为）

| 目标主机 | 用途 | 代码位置 |
|---|---|---|
| `api.anthropic.com`（+ `api-staging.anthropic.com`） | 模型 API；`/api/web/domain_info`（WebFetch 前置校验）；`/mcp-registry/v0/servers` | `src/constants/oauth.rs:67`、`src/tools/web_fetch_tool/utils.rs:489`、`src/services/mcp/official_registry.rs:12` |
| `platform.claude.com`（+ `platform.staging.ant.dev`） | OAuth 授权 / token / 成功页 | `src/constants/oauth.rs:68-76` |
| `claude.com/cai/oauth/authorize`、`claude.ai` | Claude.ai 侧 OAuth 与客户端元数据 | `src/constants/oauth.rs:38,69-70` |
| `mcp-proxy.anthropic.com`（+ staging） | MCP 代理 | `src/constants/oauth.rs:79` |
| `claude.fedstart.com`、`claude-staging.fedstart.com`、`beacon.claude-ai.staging.ant.dev` | 白名单内的自定义 OAuth base URL（`CLAUDE_CODE_CUSTOM_OAUTH_URL`，非白名单值会直接 bail） | `src/constants/oauth.rs:137-141,166-170` |
| `bedrock{,-runtime}.{region}.amazonaws.com` | AWS Bedrock provider | `src/utils/model/bedrock.rs:31`、`src/services/token_estimation.rs:98` |
| `{region}-aiplatform.googleapis.com` | GCP Vertex provider | `src/services/token_estimation.rs:139-141` |
| `{resource}.services.ai.azure.com/anthropic/` | Azure Foundry provider | `src/services/api/client.rs:610-612` |
| 用户配置的任意 URL | MCP server、HTTP hooks、插件市场 git remote、WebFetch 目标 | `src/services/mcp/client.rs`、`src/utils/hooks/exec_http_hook.rs:285` 等 |

### 4.2 自动发起的外呼（非上报，但无人值守）⚠️

| 目标主机 | 用途 | 代码位置 | 运行时隐私门控 |
|---|---|---|---|
| `raw.githubusercontent.com` | 拉取官方插件安装量统计 JSON（`anthropics/claude-plugins-official/.../plugin-installs.json`） | `src/utils/plugins/install_counts.rs:22,227-242` | **缺失**（仅测试中出现该环境变量：`install_counts.rs:699`） |
| `downloads.claude.ai` | 拉取官方插件市场 GCS 快照 | `src/utils/plugins/official_marketplace_gcs.rs:8-9` | **缺失** |
| `api.anthropic.com/mcp-registry/...` | 拉取官方 MCP registry | `src/services/mcp/official_registry.rs:12,56-59` | 有（`:51` 检查 `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC`） |

前两项**发送的是公开数据拉取请求，不携带用户数据**；但它们是进程自主发起的，且未接入非必要流量开关。这是与官方行为的一处可观测偏差。

> 注意：`src/utils/plugins/fetch_telemetry.rs:26-37` 的 `KNOWN_PUBLIC_HOSTS` 列表（`github.com`、`gitlab.com`、`storage.googleapis.com` 等）**是遥测字段的脱敏分类表，不是外呼目标清单**——非清单内主机会被归类为 `"other"` 后才写入事件元数据。因为事件无出口，该表当前纯属死代码。

### 4.3 被编译期常量锁死的出口（默认 fail-closed）

`src/constants/oauth.rs:43`：

```rust
/// No CC counterpart. This is the sole user-authorized L2 product switch for
/// OAuth network, process/listener, credential-storage, revocation, and
/// credential-cache side effects. It is intentionally compile-time only.
pub const OAUTH_CREDENTIAL_SIDE_EFFECTS_ENABLED: bool = false;
```

该常量在 20+ 处把关，覆盖：

- `src/services/oauth/client.rs:110` — OAuth token 交换
- `src/utils/auth.rs:674` — 凭据读写
- `src/utils/browser.rs:49` — 拉起系统浏览器
- `src/utils/secure_storage/{plain_text,mac_os_keychain}_storage.rs` — 凭据持久化
- `src/services/mcp/xaa.rs`（5 处）、`src/services/mcp/auth.rs:386,416`、`src/services/mcp/oauth_port.rs:53,63` — MCP OAuth
- `src/tools/brief_tool/upload.rs:274` — 文件上传到 `/api/oauth/file_upload`
- `src/commands/logout/logout.rs:43` — 凭据吊销

配套的稳定错误信息与类型：`OAuthCredentialSideEffectsUnavailable`（`oauth.rs:49-62`）。

### 4.4 自动更新：默认关闭且实现为空

`src/utils/auto_updater.rs:93-97`：

```rust
pub fn auto_updater_network_enabled() -> bool {
    std::env::var("COMETIX_AUTO_UPDATER_NETWORK")
        .ok().is_some_and(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes"))
}
```

即便显式打开该开关，所有取版本函数仍直接返回空值（`:155-165` `get_latest_version` 注释 "Future: npm view ..." 后 `return None`；`:168-177` GCS 路径同样；`install_latest_native` 返回默认结构）。**自动更新在当前构建中不可能产生网络流量。**

---

## 五、容易被误判为出网、实际不会的项

| 项 | 实际情况 |
|---|---|
| `https://json.schemastore.org/claude-code-settings.json`、`https://www.schemastore.org/claude-code-keybindings.json` | 仅作为 `$schema` 字符串写入配置文件供编辑器使用，运行时不请求。`src/utils/settings/constants.rs:8`、`src/utils/settings/types.rs:1579`、`src/keybindings/template.rs:47` |
| `http://localhost:3118/callback`、`http://localhost:8205` | **入站**监听（MCP OAuth 回调 / 本地 mcp-proxy），非出站。`src/services/mcp/auth.rs:2558` |
| `https://example.com/*`、`https://example.test/*` | 测试夹具占位符 |
| 83 个 `tengu_*` 事件名 | 事件名常量，无发送端 |

---

## 六、风险与建议

| 优先级 | 问题 | 建议 |
|---|---|---|
| 中 | `install_counts.rs` 与 `official_marketplace_gcs.rs` 的自动外呼未接入 `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC` | 在两处 fetch 入口加 `privacy_level::is_essential_traffic_only()` 提前返回，与 `official_registry.rs:51` 保持一致 |
| 低 | `KNOWN_PUBLIC_HOSTS`、插件遥测脱敏等逻辑为死代码 | 保留可用（与官方 1:1 对齐是项目目标），但建议加注释说明当前无出口 |
| 低 | `plugin_telemetry.rs:118,169` 等调用点会产生内存增长 | `EVENT_QUEUE` 无上限且永不 drain；长会话下会持续增长。若无意接入 sink，可考虑加容量上限 |
| 信息 | 凭据出口全部编译期关闭 | 若后续需要 `cometix login` 可用，需将此常量改为可配置——届时应同步复核上表所有 20+ 个把关点 |
| 信息 | `anthropic_internal` feature | 默认关闭；开启后才会读取 `CLAUDE_INTERNAL_FC_OVERRIDES` 与 `growth_book_overrides`（`growthbook.rs:39-50`）。外部发行版不受影响 |

---

## 七、核对方法（可复现）

```bash
cd /opt/home/Github/CometixCode

# 1. 遥测事件生成面
grep -rho '"tengu_[a-z0-9_]*"' --include="*.rs" src | sort -u | wc -l        # → 83
grep -rln "log_event(" --include="*.rs" src | wc -l                          # → 15（含定义所在文件）

# 2. 确认无遥测出口
grep -rniE "statsig|sentry|datadog|otlp|opentelemetry" --include="*.rs" src | grep -v "^.*://" | head

# 3. 确认唯一后端是内存队列
sed -n '1,32p' src/services/analytics/mod.rs

# 4. 出站调用点清点
grep -rln "reqwest" --include="*.rs" src | wc -l                             # → 20
grep -rn "\.send()" --include="*.rs" src | wc -l                             # → 26

# 5. 确认隐私开关未被插件拉取使用
grep -rn "nonessential\|privacy_level" --include="*.rs" src/utils/plugins/
```

---

## 八、总体判断

CometixCode 在遥测上的处理是**保守而非激进**的：移植者完整保留了官方的事件生成与脱敏逻辑以便日后对齐，同时反复在源码注释中声明"不发明替代端点"（`services/analytics/mod.rs:7-8`、`utils/sinks.rs:6-7`），并用一个编译期常量物理切断了全部凭据类出口。

唯一的自主外呼集中在两个插件相关的公开数据拉取上，不携带用户信息，但缺少官方已有的非必要流量开关门控——这是本报告中唯一建议修复的实质问题。
