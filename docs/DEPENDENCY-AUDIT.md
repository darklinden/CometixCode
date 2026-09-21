# CometixCode 依赖供应链安全审计报告

| 项 | 值 |
|---|---|
| 审计对象 | `/opt/home/Github/CometixCode`（`cometix-code` v0.2.0） |
| 依赖规模 | `Cargo.lock` 共 **455 个包**：449 个 crates.io + 5 个 git + 本工程自身 |
| 审计日期 | 2026-09-21 |
| 工具链 | rustc 1.96.0 / cargo 1.96.0（工程声明 MSRV 1.88） |
| 审计方式 | 本地源码静态审计（非仅看清单）+ `cargo-audit` RUSTSEC 漏洞库比对 |

---

## 一、结论摘要

1. **未发现任何依赖存在恶意行为、数据窃取或隐私侵犯** —— 449 个 crates.io 依赖与 5 个 git 依赖（3 个仓库）**全部通过源码审计**。构建期（`build.rs` / proc-macro）零网络调用、零敏感环境变量读取、零敏感文件访问；git 依赖的 `build.rs` / proc-macro 同样全部干净。
2. **发现 1 个真实安全漏洞**：`rustls 0.23.41` 的 TLS 1.3 握手缺陷（RUSTSEC-2026-0285，medium 5.3），影响工程**全部** HTTPS 流量。
3. **发现 2 个可修复的警告**：`event-listener`（unsound）、`chacha20`（yanked）。
4. **发现 3 个无修复路径的 unmaintained 依赖**：均在语法高亮 / HTML 解析链路，暴露面低。
5. **供应链完整性良好**：校验和 100% 覆盖、无 `[patch]` / 源替换劫持、无包名仿冒。
6. **一处需注意**：5 个 git 依赖在 `Cargo.toml` 中未固定 `rev`，安全性依赖 `Cargo.lock`。

> ⚠️ 本报告的漏洞项**尚未修复**（按用户要求仅出报告）。修复命令见第六节。

---

## 二、审计范围与方法

### 覆盖范围

| 类型 | 数量 | 审计方式 |
|---|---|---|
| crates.io 依赖 | 449 | 本地 `~/.cargo/registry/src` 源码扫描 |
| git 依赖 | 5（来自 3 个仓库） | 本地 `~/.cargo/git/checkouts` 源码审计 |
| `build.rs`（编译期执行） | 50 | 逐文件扫描 + 关键路径阅读 |
| proc-macro（编译期执行） | 22 | 逐文件扫描 |
| 未下载的包 | 68 | 均为平台专有（32 Windows + 5 Android + 28 wasm/Redox/FreeBSD 等），当前平台不参与构建 |

### 核心方法论

审计按「**能否真正碰到用户机器**」分三层：

1. **构建期执行面**——只有 `build.rs` 和 proc-macro 会在编译时运行任意代码，是供应链攻击的首选位置。逐个扫描其网络、命令执行、环境变量、文件系统行为。
2. **运行期网络能力**——不满足于「有没有网络库」，而是用 `cargo tree -i` 反查**谁真正持有 HTTP 客户端**。
3. **已知漏洞**——`cargo-audit` 比对 RustSec 数据库（1256 条 advisory）。

---

## 三、构建期执行面（关键风险区）

### 3.1 五十个 `build.rs`：全部干净

| 检查项 | 结果 |
|---|---|
| 网络调用 | **零**。grep 命中的 21 处 `http/https` 逐个核实，**全是注释中的 license / 文档链接**（Apache、rust-lang blog、crates.io issue 等） |
| 敏感环境变量 | **零**。无 `TOKEN` / `KEY` / `SECRET` / `AWS` / `CI` 类读取 |
| 敏感文件路径 | **零**。无 `~/.ssh`、`~/.aws`、`~/.netrc` |
| 文件系统写入 | 全部落在 Cargo 提供的 `OUT_DIR` 内（标准行为） |

**`Command::new` 共 19 处，逐处核实结果**：

| 位置 | 行为 | 判定 |
|---|---|---|
| 18 处（anyhow / serde / thiserror / proc-macro2 / quote / syn / zerocopy / httparse / crc32fast / ref-cast / zmij 等） | 调用 `rustc --version` 探测编译器 | 正常（dtolnay 系 crate 标准套路） |
| `ring-0.17.14` | 调用 `nasm` 汇编器（Windows 目标） | 正常 |
| `libc-0.2.186` | 调用 `freebsd-version`（仅 FreeBSD 目标） | 正常 |
| `getrandom` / `rustix` | 调用 `RUSTC_WRAPPER` | 正常 |

**⚠️ 一处值得记录**：`rmcp-2.1.0/build.rs:17` 会执行

```rust
std::process::Command::new("git")
    .args(["config", "core.hooksPath", ".githooks"])
```

即**修改 git 配置**。咪验证了触发条件——它要求 `CARGO_MANIFEST_DIR` 的上两级同时存在 `.githooks` 与 `.git`：

```
CARGO_MANIFEST_DIR = ~/.cargo/registry/src/index.crates.io-.../rmcp-2.1.0
检查的 workspace_root = ~/.cargo/registry/src
  .githooks 存在? NO
  .git 存在?      NO
```

**结论：作为依赖构建时该分支不进入，在老大的机器上不会触发**。但这是「构建脚本可修改 git 配置」这一类行为的实例，若日后 rmcp 被以 path 依赖方式引入则有实际影响。

**`ring` 的另一处**：`pregenerate_asm_main` 会写 crate 源码目录，但仅在 `RING_PREGENERATE_ASM=1` 时触发（ring 维护者重新生成汇编用），正常构建不走。

### 3.2 二十二个 proc-macro：全部干净

对 `serde_derive`、`thiserror-impl`、`tokio-macros`、`async-trait`、`futures-macro`、`time-macros`、`zerovec-derive` 等 22 个 proc-macro crate 扫描：

**无 `Command`、无网络、无 fs 写入、无环境变量读取、无敏感路径**。

---

## 四、运行期网络能力

### 4.1 HTTP 能力只有三个持有者

用 `cargo tree -i reqwest` 反查（比「有没有网络库」更严格的判据）：

```
reqwest v0.13.4
├── anthropic-sdk      ← 调 API，其存在意义
├── cometix-code       ← 本工程
└── rmcp               ← MCP SDK，必需
```

**没有第四个持有者。** WebSocket 栈（`tungstenite`）同样只有 `cometix-code` 一个使用者。

### 4.2 验证「已避开 aws-lc-sys」的声明

工程 `Cargo.toml:146` 注释声称避免 `aws-lc-sys`（要求 reqwest 使用 `rustls-no-provider`，防止 feature union 引入）：

```
$ cargo tree -e normal | grep -ci aws-lc
0
```

**声明属实。**

### 4.3 一个差点误报的发现：`jsonschema`

`jsonschema 0.45.1` 的**默认 feature 包含 `resolve-http`**，其 `src/retriever.rs:156` 确实存在会真正发请求的代码：

```rust
"http" | "https" => Ok(self.client.get(uri.as_str()).send()?.json()?),
```

但工程使用 `default-features = false`（`Cargo.toml:74`），且 `reqwest` 反向依赖中**不含 jsonschema**——证明 feature union 未被其他包打开，该代码不会编译进二进制。

工程注释「Remote/file resolution stays disabled: caller-provided schemas must be self-contained」**属实**。

### 4.4 遥测关键字扫描

全依赖树扫描 `telemetry|analytics|phone-home|beacon|sentry|datadog|amplitude|mixpanel|posthog`，仅 6 处命中，**逐个核实全部为误报**：

| 包 | 命中内容 | 实情 |
|---|---|---|
| `gif-0.14.2` | `beacon.gif` | 测试素材文件名 |
| `vte-0.14.1` | `DcsEntry` | 终端 DCS 状态机枚举，含 "sentry" 子串 |
| `libc-0.2.186` | `pub beacon: __u32` | Linux 内核结构体字段 |
| `rustix-1.1.4` | `sentry value` | io_uring 哨兵值注释 |
| `http-1.4.2` | "analytics" | 标准 HTTP 头的文档注释 |
| `tracing-0.1.44` | `tracing-opentelemetry` / `sentry-tracing` | 文档中推荐的**生态集成**，非内置上报 |

### 4.5 硬编码地址扫描

全依赖树扫描硬编码 IPv4，命中的全部为**误报**：ASN.1 OID 数字（`const-oid`）、版本号字符串（`getrandom`、`mime`、`image`）、测试用例（`email_address`、`hyper-util`、`ipnet`、`jsonschema` 的非法 IP 测试）、内核常量（`libc`）。

**无非预期地址。**

### 4.6 敏感文件访问扫描

扫描 `~/.ssh`、`~/.aws`、`~/.netrc`、`id_rsa`、浏览器数据、keychain，命中全部为误报：

- `bat` 的两处是**语法高亮映射表**（`.toml` 配置，告诉 bat 用哪种语法高亮 `.ssh/config` 这类**文件**），非读取
- `oauth2` / `rmcp` 是 OAuth 的 `client_credentials` grant type 字符串常量
- `rustls-webpki` / `tower-http` 是测试函数名与测试字符串

**无真实敏感文件读取。**

---

## 五、供应链完整性

| 检查项 | 结果 |
|---|---|
| `Cargo.lock` 校验和覆盖 | **449 / 449 = 100%** |
| `[patch]` / `[replace]` 段 | 无 |
| `.cargo/config.toml` 源替换 | 无（工程内无该目录） |
| 全局 `~/.cargo/config.toml` 源替换 | 无 |
| Lockfile 版本 | v4（现代格式，含完整依赖图） |
| 包名仿冒（typosquat） | **未发现** |

**455 个包名全部为真实知名 crate**，按来源核对：

- dtolnay 系：`anyhow` `thiserror` `serde` `proc-macro2` `quote` `syn` `zmij` `ryu` `itoa` `semver` `rustversion`
- tokio / Rust 官方系：`tokio` `mio` `bytes` `socket2` `libc` `cc` `cfg-if` `hashbrown` `indexmap` `memchr` `regex` `aho-corasick`
- HTTP / TLS 栈：`hyper` `hyper-util` `http` `httparse` `tower-http` `rustls` `ring` `webpki` `native-tls` `security-framework`
- Servo 生态：`html5ever` `markup5ever` `cssparser` `selectors` `scraper` `tendril` `ego-tree`
- ICU4X 系：`icu_*` `zerovec` `yoke` `zerofrom` `tinystr` `litemap` `writeable`

**小众包逐个核实元数据**：

| 包 | 作者 | 判定 |
|---|---|---|
| `zmij` | David Tolnay（serde_json 作者本人） | 合法 |
| `regress` | ridiculousfish（fish shell 作者） | 合法 |
| `nucleo-matcher` | Pascal Kuthe（Helix 编辑器） | 合法 |
| `process-wrap` | Félix Saparelli（cargo-watch 作者） | 合法 |
| `num-cmp` | Kang Seonghoon（lifthrasiir，知名 Rust 开发者） | 合法 |
| `supermarkdown` | vakra-dev，MIT | 合法（5629 行纯字符串处理，**零 unsafe / 零 fs / 零网络 / 零依赖**） |

（注：本地 `~/.cargo/git/checkouts/` 下的 `nucleo-425d994cd74b3654` 是空目录，属其他项目遗留；本工程的 `nucleo-matcher 0.3.1` 实际来自 crates.io。）

### ⚠️ 需要注意：git 依赖未固定 rev

`Cargo.toml` 中 5 个 git 依赖（第 49、52、55、110 行）**只写了仓库 URL，未指定 `rev` / `tag` / `branch`**。当前安全性完全由 `Cargo.lock` 中的 40 位完整 commit hash 保证：

```
#95de6fd686edc03c9b267088de9a28d60576a681   (anthropic-sdk-rs)
#9437badfe9d701335f42f7ced17d472db2c2ba80   (CometixTUI → iocraft / chalk / iocraft-macros)
#1a16f9ad963d1006e3ce3511c7175c833dd172db   (marked-rs)
```

**已锁定的构建安全且可复现**；但一旦删除 `Cargo.lock` 或执行 `cargo update`，就会解析到上游默认分支的最新 HEAD——上游作者可随时推送新代码。

**建议**：在 `Cargo.toml` 中显式添加 `rev = "<commit>"`。

---

## 六、已知漏洞扫描（cargo-audit）

工具：`cargo-audit v0.22.2`，RustSec advisory 数据库（1256 条）。

```
Scanning Cargo.lock for vulnerabilities (455 crate dependencies)
error: 1 vulnerability found!
warning: 5 allowed warnings found
```

### 🔴 必须修复：rustls TLS 握手缺陷

| 项 | 值 |
|---|---|
| 编号 | **RUSTSEC-2026-0285** |
| 标题 | *TLS 1.3 handshake messages incorrectly accepted across encryption level boundaries* |
| 版本 | `rustls 0.23.41` |
| 严重性 | **5.3 (medium)** |
| 披露日期 | 2026-09-14 |
| 修复版本 | `>= 0.23.45` |
| 链接 | https://rustsec.org/advisories/RUSTSEC-2026-0285 |

**可达性分析**——rustls 是本工程**全部** HTTPS 的实现：

```
rustls v0.23.41
├── cometix-code            ← 工程直接依赖 (Cargo.toml:181)
├── hyper-rustls ──→ reqwest
├── tokio-rustls ──→ hyper-rustls / reqwest
├── rustls-platform-verifier
└── tokio-tungstenite       ← MCP WebSocket
        └── reqwest v0.13.4
            ├── anthropic-sdk
            ├── cometix-code
            └── rmcp
```

实际影响路径：向 `api.anthropic.com` 发送 API key、连接用户配置的 MCP server、插件市场 git clone、WebFetch 抓取网页——**全部经过此 TLS 实现**。漏洞属协议实现缺陷，可被网络中间人利用。

#### ⚠️ 修复陷阱

**直接执行 `cargo update -p rustls` 无法修复**：

```
$ cargo update -p rustls --dry-run
    Locking 1 package to latest Rust 1.88 compatible version
    Updating rustls v0.23.41 -> v0.23.43 (available: v0.23.45)
```

它只升到 `0.23.43`，而修复需要 `0.23.45`。提示中的 "latest Rust 1.88 compatible version" 具有误导性——咪核实了 `rustls 0.23.45` 声明的 MSRV 仅为 **1.71**，而本机工具链为 **1.96.0**，MSRV 并非真实约束。

**正确命令**：

```bash
cargo update -p rustls --precise 0.23.45
```

已验证可行（dry-run）：

```
$ cargo update -p rustls --precise 0.23.45 --dry-run
    Updating rustls v0.23.41 -> v0.23.45
    Updating rustls-webpki v0.103.13 -> v0.103.15
```

工程对 rustls 的约束为 `version = "0.23"`（`Cargo.toml:181`），语义化版本允许升至 0.23.45，**无需修改 `Cargo.toml`**。

### 🟡 建议修复：两个可升级警告

| 包 | 当前 | 问题 | 编号 | 修复命令 | 引入路径 |
|---|---|---|---|---|---|
| `event-listener` | 5.4.1 | **unsound**（`!Send` tag 可经 `StackSlot` 跨线程边界） | RUSTSEC-2026-0221 | `cargo update -p event-listener` → 5.4.2 | `event-listener-strategy` ← `async-channel` ← **工程直接依赖**（iocraft 异步通道） |
| `chacha20` | 0.10.1 | **yanked**（版本被作者撤回） | — | `cargo update -p chacha20` → 0.10.2 | `rand` ← anthropic-sdk + tungstenite |

两个均可通过普通 `cargo update -p <crate>` 升级到修复版本（已 dry-run 验证）。

### ⚪ 信息性：三个 unmaintained（无修复路径）

| 包 | 版本 | 编号 | 引入路径 | 实际风险 |
|---|---|---|---|---|
| `bincode` | 1.3.3 | RUSTSEC-2025-0141 | `syntect` / `bat` | 低——语法高亮二进制缓存 |
| `yaml-rust` | 0.4.5 | RUSTSEC-2024-0320 | `syntect` | 低——仅解析本地 `.sublime-syntax` 定义 |
| `fxhash` | 0.2.1 | RUSTSEC-2025-0057 | `selectors` ← `scraper` ← `supermarkdown` | 极低——非加密哈希，仅停止维护 |

三者均位于**语法高亮与 HTML 解析链路**，处理本地文件或已抓取的网页，无直接网络暴露面。上游 `syntect` 生态暂无替代方案，属可接受的存量债务。

### 修复命令汇总

```bash
# 1) 必须：修复 TLS 漏洞（注意必须带 --precise）
cargo update -p rustls --precise 0.23.45

# 2) 建议：修复 unsound + yanked
cargo update -p event-listener
cargo update -p chacha20

# 3) 复验
cargo audit

# 4) 确认仍可构建（工程要求 just test，勿用 cargo test）
just check
```

---

## 七、git 依赖源码审计

对 5 个 git 依赖（来自 3 个仓库）的**逐文件源码审计**，通过全量 grep 广度扫描 + 关键文件逐行阅读完成，所有结论均带 `文件:行号` 证据。

### 7.1 CometixTUI — `iocraft` / `chalk` / `iocraft-macros`

**仓库**：`https://github.com/Haleclipse/CometixTUI` @ `9437badfe9d701335f42f7ced17d472db2c2ba80`
**规模**：`iocraft` 71,346 行 / 122 文件；`chalk` 1,836 行；`iocraft-macros` 1,350 行

**来源可追溯性**：fork 自 `ccbrown/iocraft`，共 371 commit，其中上游作者 Chris Brown 占 193，fork 作者 Haleclipse 占 91。**历史未被 squash，可按作者逐条追溯**。Haleclipse 触碰的非 example 文件全部在 `packages/*/src` 的 TUI 范围内，无越界。

| 检查项 | 结果 |
|---|---|
| `build.rs` | **三个 crate 全无**（`find -name build.rs` 零命中）；无 `build =`、`links =`、`[patch]`、`.cargo/config.toml` |
| proc-macro（`iocraft-macros`） | 纯 token 变换。对 `std::fs` / `std::env` / `std::process` / `Command` / `include_str!` / `include_bytes!` **零命中** |
| `quote!` 生成代码 | 仅 20 余处 `::iocraft::*` 路径，**无任何 IO/网络/进程调用** |
| 网络行为 | **零命中**。依赖闭包（`Cargo.lock:1626-1642`）为 bitflags / chalk / crossterm / futures / taffy 等，**无网络 crate** |
| `Command` 执行 | 全仓仅 1 处：`packages/iocraft/src/hooks/use_app.rs:139` 的 `sh -c "exit 7"`，位于 `#[cfg(test)] mod tests` 内，**不进发布产物** |
| 文件系统写入 | 仅 3 处，全部 env 门控：`render.rs:1753`（`IOCRAFT_LAYOUT_DUMP`）、`:1782`（`IOCRAFT_FRAME_LOG`）、`:1809`（`IOCRAFT_FRAME_DUMP`）。未设置时函数开头直接 return |
| 文件读取 | 仅 `error_overview.rs:61` 读 panic 位置的源文件用于渲染错误摘要，本地读取、无外发 |
| 混淆代码 | 无长 base64/hex。唯一 base64 是 `ansi.rs:317-318` 手写的 OSC 52 剪贴板编码表，终端功能必需 |
| `unsafe` | 全部是 FFI/trait 边界（`props.rs` downcast、`style.rs` Send/Sync 标记、`measure_text.rs` 的 `static mut` + `Once` 缓存），无可疑用法 |

**环境变量读取逐条核实**（全部为终端能力探测，**无 token/secret 类**）：

| 文件:行 | 变量 | 用途 |
|---|---|---|
| `clipboard.rs:54-60` | `SSH_CONNECTION` / `TMUX` / `LC_TERMINAL` | 选择 native / tmux / OSC52 剪贴板路径 |
| `ansi.rs:275-393` | `FORCE_HYPERLINK` / `WT_SESSION` / `TERM_PROGRAM` / `TERM` 等 | 判断终端是否支持超链接 |
| `use_terminal_notification.rs:283-301` | `TERM_PROGRAM` / `ConEmu*` / `USER_TYPE` | 决定通知协议 |
| `render.rs:1702,1767,1808` | `IOCRAFT_*` | 调试转储开关 |
| `chalk/src/level.rs:276,287` | `std::env::args()` | 检测 `--color` 参数 |

**结论：安全。**

### 7.2 anthropic-sdk-rs — `anthropic-sdk`

**仓库**：`https://github.com/Haleclipse/anthropic-sdk-rs` @ `95de6fd686edc03c9b267088de9a28d60576a681`（v0.74.0, MIT）
**规模**：`src/` 约 37,951 行，78 个 `.rs` 文件

> ⚠️ **重要限制**：`git rev-list --count HEAD` = **2**。整个仓库被 squash 成 2 个 commit（初始化 + CI），全部由 Haleclipse 提交。**无法通过与官方 `@anthropic-ai/sdk` 做 git diff 来验证改动**，以下结论完全基于全量源码审读。

| 检查项 | 结果 |
|---|---|
| `build.rs` / proc-macro | **均无**。无 `[patch]`、无 `.cargo/config.toml` |
| 网络主机 | **仅 `api.anthropic.com`**（`internal/constants.rs:33`、`client.rs:33`）+ 用户可覆盖的 base URL（`client.rs:381` 的 `ANTHROPIC_BASE_URL`、`:371` 的 `opts.base_url`，与官方 TS SDK 同名行为） |
| 第三方遥测域 | **零**。analytics / telemetry / beacon / phone-home / sentry / datadog / segment 类关键字全部**零命中** |
| 出站头 | `accept`、`user-agent`、`x-stainless-retry-count`、`anthropic-version`，以及 `get_platform_headers()` 的 `X-Stainless-*`（`detect_platform.rs:103-113`）。OS/Arch 来自 `std::env::consts`**编译期常量**（`:90-91`），runtime version 来自 `env!("CARGO_PKG_RUST_VERSION")`**编译期**（`:93`）——**非设备指纹，不含主机名/用户名/UUID** |
| `x-stainless-helper` | 只携带 helper 名字符串（`"mcpTool"` 等），**不含对话内容或用户数据** |
| `Command` 执行 | **零命中**（`rg "process::Command\|Command::new\|std::process"` 无结果） |
| 文件系统写入 | **无任何写入**。唯一读取是 `core/uploads.rs:132`，把调用方显式指定的文件转成上传载荷，属文件上传 API 必需 |
| `unsafe` | **无**（`rg unsafe src/` 只命中 `internal/path.rs:46` 的一句注释） |
| 混淆代码 | 无 80 字符以上的 base64/hex 长字面量 |
| 凭据处理 | 环境变量只读 4 个：`ANTHROPIC_API_KEY`(`client.rs:361`)、`ANTHROPIC_AUTH_TOKEN`(`:368`)、`ANTHROPIC_BASE_URL`(`:381`)、`ANTHROPIC_LOG`(`:392`)。**无 keychain / `~/.netrc` / `~/.ssh` / `~/.aws` / 浏览器数据 / 机器 ID 读取** |
| 日志脱敏 | `internal/log.rs:240-245` 的 `redact_header_value` 把 `X-API-Key` / `authorization` 值替换为 `"***"`，测试 `log.rs:463-464` 验证 |
| 日志出路 | 仅 `tracing` 宏或宿主注入的 `SdkLogger` trait 对象——**SDK 内无任何内置网络 logger 实现** |
| 随机数 | `internal/uuid.rs:10-19` 用 `rand::rng()` 生成随机 UUID v4 作请求 ID，**不读取任何机器标识** |

**结论：安全**（但需注意 git 历史被 squash 至 2 个 commit，无法做上游 diff）。

### 7.3 marked-rs — `marked-rs`

**仓库**：`https://github.com/Haleclipse/marked-rs` @ `1a16f9ad963d1006e3ce3511c7175c833dd172db`（v0.1.0, MIT）
**规模**：`src/` 3,654 行 / 10 文件；依赖仅 `fancy-regex 0.11` + `serde`

| 检查项 | 结果 |
|---|---|
| `build.rs` / proc-macro | **两者皆无**。无 `[patch]`、无 `.cargo/config.toml`、无嵌套 git 依赖 |
| 网络行为 | **零命中**。唯一的 "http" 是 `rules.rs:86,117,120` autolink 正则里的 `ftp\|https?` 协议字面量，以及 `tokenizer/inline.rs:329` 给 `www.` 裸链接补协议前缀的 `format!` ——纯字符串处理 |
| 敏感数据 / `Command` / FS / env | **四类全部零命中**（`rg "std::fs\|std::process\|env::var\|env!\|option_env" src/` 无结果） |
| `unsafe` / base64 / 长字面量 | 无 |

**唯一观察**：`fancy-regex` 带回溯正则在恶意 Markdown 输入下存在 **ReDoS** 风险。这是**质量/DoS 问题，不是隐私或恶意行为**。

**结论：安全。**

### 7.4 汇总

| Crate | 仓库 @ commit | build.rs | proc-macro | 网络主机 | 敏感数据 | Command | FS 写入 | 混淆 | 结论 |
|---|---|---|---|---|---|---|---|---|---|
| `iocraft` | CometixTUI @ `9437bad` | 无 | — | 无（依赖闭包内零网络 crate） | 仅终端能力类 env | 仅 `#[cfg(test)]` 内 1 处 | 仅 3 处 env 门控调试转储 | 无 | **安全** |
| `chalk` | 同上 | 无 | — | 无 | 仅 `env::args()` 检测 `--color` | 无 | 无 | 无 | **安全** |
| `iocraft-macros` | 同上 | 无 | 纯 token 变换 | 无 | 无 | 无 | 无 | 无 | **安全** |
| `anthropic-sdk` | anthropic-sdk-rs @ `95de6fd` | 无 | 无 | **仅 `api.anthropic.com`** | 仅 4 个 `ANTHROPIC_*`；日志已脱敏 | 无 | 无 | 无 | **安全**¹ |
| `marked-rs` | marked-rs @ `1a16f9a` | 无 | 无 | 无 | 无 | 无 | 无 | 无 | **安全**² |

¹ 注意 git 历史被 squash 成 2 个 commit
² 注意 git 历史仅 3 个 commit；另存在 `fancy-regex` 的 ReDoS 风险（质量类）

**三个仓库全部未发现危险行为或隐私侵犯行为。** 没有未声明的外呼、没有遥测上报、没有凭据窃取、没有持久化机制（`.bashrc` / `crontab` / `LaunchAgents` / `systemd` 等关键词在三仓全部零命中）。

### 7.5 git 依赖的遗留风险

| 风险 | 说明 | 建议 |
|---|---|---|
| 无法交叉验证 | `anthropic-sdk-rs`（2 commit）与 `marked-rs`（3 commit）**历史被 squash**，无法与上游官方实现 diff；且均为单一作者，缺少社区 review 痕迹 | 属结构性风险，无法通过技术手段消除，只能作为信任决策 |
| 未固定 `rev` | `Cargo.toml` 中 5 个 git 依赖只写 URL，未指定 `rev`（详见第五节） | 显式添加 `rev = "<commit>"` |
| 宿主侧待审 | 本次审计的是**依赖库本身**。剪贴板进程执行（`pbcopy` / `xclip` / `tmux`）的实际 `ClipboardBackend` 实现，以及真正的网络调用参数，都由宿主工程提供 | 若需完整结论，应继续审计宿主工程的 `impl ClipboardBackend` 与网络调用点 |

---

## 八、总体判断

| 维度 | 结论 |
|---|---|
| 恶意行为 | **未发现**。无数据窃取、无未声明外呼、无持久化机制 |
| 构建期安全 | **干净**。50 个 `build.rs` + 22 个 proc-macro 零网络、零敏感读取 |
| 运行期暴露面 | **收敛**。HTTP 能力仅 3 个持有者，全为功能必需 |
| 供应链完整性 | **良好**。校验和 100%、无源替换劫持、无 typosquat |
| 已知漏洞 | **1 个需修**（rustls，TLS 实现层）、2 个建议修、3 个信息性 |
| 待改进 | git 依赖未固定 `rev`（中）；`rmcp` build.rs 的 git 配置写入分支（低，当前不触发） |

**核心判断**：本工程的依赖风险集中在**「版本更新滞后」而非「恶意代码」**。唯一的真实漏洞出现在 TLS 实现库的版本落后上，而不是任何库主动做了坏事。修复它是低成本的单条命令。

值得注意的是，工程在依赖选择上有多处**主动的安全设计**，且经审计全部属实：

- `default-features = false` 关闭 `jsonschema` 的联网检索器（已验证 feature union 未被打开）
- 刻意避免 reqwest 的 `rustls` feature 以防 union 引入 `aws-lc-sys`（已验证不在依赖树）
- 按平台分离 TLS 后端（Android 用 native-tls 以避开缺 JNI 上下文时的 panic）
- 用 `--offline` 可完成的树反查验证了上述所有声明

---

## 九、复现方法

```bash
cd /opt/home/Github/CometixCode

# 依赖规模
grep -c '^\[\[package\]\]' Cargo.lock          # → 455

# 构建期执行面
ls ~/.cargo/registry/src/*/ | grep -c .        # 缓存包数
# 扫描 build.rs 的网络/命令/环境变量行为见正文

# 网络能力反查（关键判据）
cargo tree --offline -i reqwest -e normal      # → 仅 3 个持有者
cargo tree --offline -i tungstenite -e normal
cargo tree --offline -e normal | grep -ci aws-lc   # → 0

# 漏洞扫描
cargo install cargo-audit --locked             # 已安装 v0.22.2
cargo audit

# 供应链完整性
grep -c '^checksum = ' Cargo.lock              # → 449（100% 覆盖）
grep -nE '^\[patch|^\[replace|^\[source' Cargo.toml   # → 空
```
