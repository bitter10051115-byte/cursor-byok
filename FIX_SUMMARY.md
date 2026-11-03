# Cursor BYOK 兼容性修复汇总

本文汇总 Cursor 3.27.22 走本地 Cursor BYOK 代理时出现的三类问题、根因、修复方式与具体改动点，以及验证和打包结果。

## 问题现象

按出现顺序，用户侧依次遇到：

1. **代理请求超时**：Cursor 通过 Cursor BYOK 发起请求时长时间无响应，本地代理收不到/解不了 Cursor 发来的新协议消息。
2. **prompt metadata 报错**：请求能建立后报 `The server sent no prompt metadata for "2ac720b73432b262". The backend may predate prompt-metadata delivery; retry later or disable agent_host_local_loop.`
3. **偶发失败与重连**：`Request ID: 22852d28-...` 场景下报：
   ```text
   {"error":"ERROR_UNSPECIFIED"}
   [internal] provider error: OpenAI Responses 400 Bad Request: {
     "error": {
       "message": "The request is invalid: the request was rejected by an internal MaaS component. ...",
       "type": "invalid_request_error", "param": "", "code": "400001"
     }
   }
   ```
   该问题“时好时坏”，会触发 Cursor 重连。

## 根因

### 1. Cursor 3.27.22 切换了 RunInference 协议

Cursor 新版本不再向本地代理发送旧的 `agent.v1.AgentClientMessage`，而是发送 `aiserver.v1.RunInferenceClientMessage`（BidiAppend 载荷中的 field 1 是普通 UUID，而不是旧的 RunRequest 结构）。本地代理此前只实现了旧协议：

- 无法解包新协议，`BidiAppend` 解码失败，请求直接超时；
- 缺少 `RunInferenceSSE` / `RunInferencePoll` 服务端点；
- 新增的 `invoke_model` 消息带 gzip/zstd 压缩载荷，需要解压后才能得到真正的 `InferenceStreamRequest`。

### 2. RunReady 缺少 prompt metadata

Cursor 的 agent host local loop 要求 `RunReady` 携带 `prompt_model_metadata`。旧实现只发 `RunReady` 空壳，Cursor 因此报 “server sent no prompt metadata”。

### 3. MaaS 网关偶发 400001

数据库 `llm_calls` / `llm_call_requests` 中当时成功与失败的请求体经逐字节/JSON 比较完全一致（例如成功 `7a37bbea-...-1-8psb` 与失败 `94e32227-...-0-qbiu` 均为同一份 115952 字节 body），因此当时判断为上游网关对完全相同的请求偶发返回 `HTTP 400 code 400001`，并先加入了有限重试。

原来 OpenAI Responses provider 对首次 HTTP 响应不做重试，RunInference 路径拿到错误后直接以 `InvocationEnd`（code 13）抛给 Cursor，Cursor 表现为 `ERROR_UNSPECIFIED` 并重连。

2026-11-02 再次复核时发现，`deepseek-v4-flash` 的 400001 已从偶发变成稳定复现。上游 MaaS 对 Cursor 传来的工具 schema 包装格式更严格：

- 失败请求中的工具参数是 `"parameters": {"jsonSchema": {...}}`；
- 同一请求删除 `tools` 后返回 200；
- 保留全部工具、仅把 `parameters` 解包成标准 JSON Schema 后返回 200。

因此请求体本身确实存在兼容性问题，重试只能缓解偶发场景，不能解决这种稳定拒绝。

### 4. Cursor 图片数据解码错误

2026-11-02 15:08 再次出现 `Request ID: fcab6ca4-...` 的 400001。数据库中的失败请求带有 1 个 `input_image`，但图片地址只有：

```text
data:image/jpeg;base64,
```

原始 BidiAppend 载荷中，Cursor 的 `InferenceImagePart.data` 实际是完整的 `data:image/jpeg;base64,...`，而 `run_inference.rs` 把它当作裸 base64 解码，解码失败后又用 `unwrap_or_default()` 变成空字节，最终向上游发送了空图片。

最初用抓包里的截断图片做对照时，字符串格式仍然返回 400001，一度误判为 MaaS 需要对象格式。后续用 Cursor 新请求中的完整 145 KB 截图复测，确认标准字符串 `image_url` 可以由 `deepseek-v4-flash` 正常识别；对象格式反而会让模型丢掉图片内容。因此最终修复只处理图片解码，不改变官方 Responses 图片信封。

## 修复方式与改动点

### 协议与路由（解决超时 + prompt metadata）

- `server/src/cursor/protocol/proto.rs`
  - 新增 `RunInferenceClientMessage`、`RunInferenceServerMessage`、`RunInferenceRunRequest`、`RunInferenceInvokeModel`、`RunInferenceCompressedRequest`、`RunInferenceRunReady`、`RunInferencePromptModelMetadata`、`RunInferenceInvocationResponse/End`、`InferenceStreamRequest/Response` 等一整套 aiserver.v1 RunInference 消息定义。
- `server/src/api/cursor/run_inference.rs`（新增文件）
  - `append`：分发 RunInference 的 `RunRequest` / `InvokeModel` / `CancelInvocation` / `FinishRun` / `ClientHeartbeat`；
  - `emit_run_ready`：`RunReady` 中填充 `prompt_model_metadata`（vendor、prompt_version、persona、agent_mode_configs、ask_question_config 等），修复 prompt metadata 报错；
  - `decompress_stream_request`：支持 gzip/zstd 解压 `invoke_model` 载荷；
  - `convert_messages` / `convert_tools`：把 `InferenceCoreMessage` 转成内部 `ProjectedMessage` / `ToolDefinition`；
  - `run_invocation`：把 provider 流事件转换成 `InvocationResponse`（text/thinking/tool call/usage），错误收敛为 `InvocationEnd`；
  - `sse` / `poll`：提供 `RunInferenceSSE`（text/event-stream）与 `RunInferencePoll`（application/proto）两个服务端点。
- `server/src/api/cursor/bidi.rs`
  - `DecodedMessage` 改为 `Agent` / `Inference` 双协议；
  - `decode` 先试 `AgentClientMessage`，再试 `RunInferenceClientMessage`；
  - `append` 按消息类型分别走 `append_agent` 或 `run_inference::append`；
  - 增加 Cursor 3.27.22 真实抓包载荷的解析测试。
- `server/src/api/cursor/handlers.rs`
  - 注册 `/aiserver.v1.InferenceService/RunInferenceSSE` 与 `/aiserver.v1.InferenceService/RunInferencePoll` 路由；
  - 注入 `RunInferenceState` 并在 `bidi_handler` 中传入。
- `server/src/api/cursor/mod.rs`
  - 导出 `run_inference` 模块。
- `server/src/local_app/proxy.rs`
  - 将两个 RunInference 路径加入本地直连白名单，避免被转发到远端。
- `server/src/cursor/services/analytics.rs`
  - 在 Statsig bootstrap 配置中强制关闭 `run_inference_websocket_client`，让 Cursor 走本地 SSE/Bidi 链路，而不是尝试连上游 websocket。
- `server/Cargo.toml`、`Cargo.lock`
  - `flate2` 从 dev-dependencies 提升为普通依赖，新增 `zstd`，用于解压 gzip/zstd 的 `invoke_model` 载荷。

### MaaS 400001 瞬时错误重试（解决偶发失败/重连）

- `server/src/provider/attempt.rs`
  - 新增 `send_with_retry`：对首次 HTTP 响应做有限重试；
  - 只重试 `HTTP 400` 且错误体包含 `400001` 或 `rejected by an internal MaaS component` 的响应；
  - 最多 3 次尝试（2 次重试），退避 200ms / 400ms，退避期间响应取消；
  - 普通 4xx（鉴权失败、未知模型、普通格式错误等）仍立即失败，不改变原有语义；
  - 新增单元测试：400001 先失败后重试成功；普通 400 不重试。
- `server/src/provider/openai_responses.rs`
  - 首次请求由 `send_once` 改为 `send_with_retry`，只作用于 OpenAI Responses provider（Cursor RunInference 实际走的路径）；
  - 重试发生在同一次 recorder 调用内，不会新增 `llm_calls` 记录；
  - 命中重试时日志输出 `retrying transient MaaS rejection`，便于确认是否命中。

### Cursor 工具 schema 解包（解决稳定 400001）

- `server/src/api/cursor/run_inference.rs`
  - `convert_tools` 在转换为内部 `ToolDefinition` 时，识别 Cursor 的 `parameters: {"jsonSchema": {...}}` 包装并解出内层标准 JSON Schema；
  - 同时兼容 `json_schema` 命名，已经是标准 schema 的请求保持不变；
  - 新增回归测试 `unwraps_cursor_tool_json_schema` 和 `leaves_standard_tool_json_schema_unchanged`。

### Cursor 图片输入修复（解决图片附件 400001）

- `server/src/api/cursor/run_inference.rs`
  - `image_content` 同时支持 Cursor 的完整 data URL 和裸 base64；
  - 从 data URL 的 header 读取 MIME 类型，优先于独立的 `mime_type` 字段；
  - 空图片安全跳过，不再发送 `data:image/jpeg;base64,`；
  - base64 非法时返回明确的协议错误，不再静默生成坏请求；
  - 新增 4 个图片解码回归测试。
- `server/src/provider/openai_responses.rs`
  - 继续使用 OpenAI Responses 官方要求的字符串 `image_url`；
  - 新增回归测试，防止重新引入对象信封。

### 测试更新

- `server/tests/connect_wire.rs`：新增 `run_inference_append_streams_run_ready_over_sse`，验证 RunReady 携带 prompt metadata、SSE 可流式返回。
- `server/tests/interrupt.rs`：适配 `DecodedMessage` 双协议枚举与 `RunInferenceState` 参数。

## 验证结果

已执行并通过：

```bash
cargo fmt --check
cargo check -p cursor-server
cargo clippy -p cursor-server --all-targets -- -D warnings
cargo test -p cursor-server provider::attempt -- --test-threads=1 --skip request_transport_failure_is_one_failed_attempt
cargo test -p cursor-server --test connect_wire   # 7 passed
cargo test -p cursor-server api::cursor::run_inference
cargo test -p cursor-server provider::openai_responses
cargo clippy -p cursor-server --all-targets -- -D warnings
```

使用后续真实请求 `fb6fa128-...` 的完整图片和工具集做上游对照：

- 标准字符串 `image_url`：`200`，模型推理中明确描述了 Cursor 截图；
- 对象 `image_url: {"url": ...}`：同样 `200`，但只收到 32 个文本 token，模型回复“需要我帮你什么”，没有看到图片；
- `qwen3.8-max` 使用字符串时返回 `image_tokens=578`，证明上游视觉通道正常。

说明：`request_transport_failure_is_one_failed_attempt` 是既有测试，断言“连接已关闭端口应得到 transport error”，但当前机器的网络层会把该连接拦截成 `502 Bad Gateway`，与本次改动无关；其余 attempt 测试均通过。

## 打包与运行状态

```bash
make build-desktop
```

打包成功，产物：

- `/Users/smzdm/Ai-Project/cursor-byok/target/release/bundle/macos/Cursor BYOK.app`
- `/Users/smzdm/Ai-Project/cursor-byok/target/release/bundle/macos/Cursor BYOK.app.tar.gz`

打包完成后已重启本地客户端，新版客户端监听：

- `127.0.0.1:54042`（本地服务）
- `127.0.0.1:54177`（Cursor BYOK 代理端点）

Cursor 新实例已重新连接。发布包中已包含 `retrying transient MaaS rejection` 字符串，可确认新版 server 代码已打进 app。

## 后续观测

如果再遇到疑似 400001，先看客户端日志：

- 若出现 `retrying transient MaaS rejection`，说明本地代理已识别并重试该瞬时错误；
- 若仍失败，可查 `/Users/smzdm/.cursor-byok-v3/cursor-byok.db` 的 `llm_calls`，对比同一次请求的成功/失败 `body_json`，并按 `http_status` 确认上游是否仍偶发 `400001`。
- 带图片的请求应检查上游 body 中 `image_url` 是否为字符串；对象格式会导致部分模型静默丢图。
