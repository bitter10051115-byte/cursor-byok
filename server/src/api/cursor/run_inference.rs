//! Implements Cursor's RunInference Bidi transport and provider streaming.
use std::{collections::HashMap, convert::Infallible, sync::Arc};

use async_stream::stream;
use axum::{
    body::{to_bytes, Body},
    extract::{Extension, State},
    http::{header, HeaderMap, HeaderValue, Request, Response, StatusCode},
};
use bytes::Bytes;
use futures_util::StreamExt;
use prost::Message;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use crate::{
    api::cursor::proxy::{self, CursorProxy, UPSTREAM_URL_HEADER},
    cursor::{
        protocol::{
            connect::{self, END_STREAM_FLAG},
            proto::aiserver::v1 as ai,
        },
        transport::{TransportHandle, TransportParent, TransportRegistry},
    },
    model::{
        ContentPart, ModelInvocation, ModelLatency, ModelRequest, ModelSpec, ProjectedContent,
        ProjectedMessage, PromptSpec, Role, ToolCallContent, ToolDefinition, ToolResultContent,
        Usage,
    },
    Error, Result,
};

#[derive(Clone, Default)]
pub struct RunInferenceState {
    inner: Arc<Mutex<HashMap<String, InferenceRun>>>,
}

#[derive(Default)]
struct InferenceRun {
    started: bool,
    upstream_bridged: bool,
    upstream_headers: Option<HeaderMap>,
    model: Option<ai::InferenceRequestedModel>,
    invocations: HashMap<String, CancellationToken>,
}

impl RunInferenceState {
    async fn begin_run(
        &self,
        request_id: &str,
        model: Option<ai::InferenceRequestedModel>,
    ) -> bool {
        let mut runs = self.inner.lock().await;
        let run = runs.entry(request_id.into()).or_default();
        let first = !run.started;
        run.started = true;
        if model.is_some() {
            run.model = model;
        }
        first
    }

    async fn model(&self, request_id: &str) -> Option<ai::InferenceRequestedModel> {
        self.inner
            .lock()
            .await
            .get(request_id)
            .and_then(|run| run.model.clone())
    }

    async fn insert_invocation(
        &self,
        request_id: &str,
        invocation_id: &str,
        token: CancellationToken,
    ) {
        self.inner
            .lock()
            .await
            .entry(request_id.into())
            .or_default()
            .invocations
            .insert(invocation_id.into(), token);
    }

    async fn remove_invocation(&self, request_id: &str, invocation_id: &str) {
        if let Some(run) = self.inner.lock().await.get_mut(request_id) {
            run.invocations.remove(invocation_id);
        }
    }

    async fn cancel_invocation(&self, request_id: &str, invocation_id: &str) {
        if let Some(token) = self
            .inner
            .lock()
            .await
            .get_mut(request_id)
            .and_then(|run| run.invocations.remove(invocation_id))
        {
            token.cancel();
        }
    }

    async fn cancel_all(&self, request_id: &str) {
        if let Some(run) = self.inner.lock().await.get_mut(request_id) {
            for token in run.invocations.drain() {
                token.1.cancel();
            }
        }
    }

    async fn store_upstream_headers(&self, request_id: &str, headers: &HeaderMap) {
        self.inner
            .lock()
            .await
            .entry(request_id.to_owned())
            .or_default()
            .upstream_headers = Some(headers.clone());
    }

    async fn upstream_headers(&self, request_id: &str) -> Option<HeaderMap> {
        self.inner
            .lock()
            .await
            .get(request_id)
            .and_then(|run| run.upstream_headers.clone())
    }

    async fn begin_upstream_bridge(&self, request_id: &str) -> bool {
        let mut runs = self.inner.lock().await;
        let run = runs.entry(request_id.to_owned()).or_default();
        if run.upstream_bridged {
            return false;
        }
        run.upstream_bridged = true;
        true
    }
}

pub async fn append(
    registry: &TransportRegistry,
    state: &RunInferenceState,
    request_id: String,
    _seqno: i64,
    message: ai::RunInferenceClientMessage,
    parent: Option<TransportParent>,
) -> Result<ai::BidiAppendResponse> {
    use ai::run_inference_client_message::Message;
    match message.message {
        Some(Message::RunRequest(request)) => {
            run_request(registry, state, &request_id, request, parent).await
        }
        Some(Message::InvokeModel(request)) => {
            invoke_model(registry, state, &request_id, request).await
        }
        Some(Message::CancelInvocation(request)) => {
            state
                .cancel_invocation(&request_id, &request.invocation_id)
                .await;
            Ok(ai::BidiAppendResponse {})
        }
        Some(Message::FinishRun(_)) => {
            state.cancel_all(&request_id).await;
            if let Some(handle) = registry.local(&request_id).await {
                finish_stream(&handle);
            }
            Ok(ai::BidiAppendResponse {})
        }
        Some(Message::ClientHeartbeat(_)) | None => Ok(ai::BidiAppendResponse {}),
    }
}

async fn run_request(
    registry: &TransportRegistry,
    state: &RunInferenceState,
    request_id: &str,
    request: ai::RunInferenceRunRequest,
    parent: Option<TransportParent>,
) -> Result<ai::BidiAppendResponse> {
    let handle = registry.get_or_create_for_append(request_id, true).await?;
    if !request.conversation_id.is_empty() {
        handle.set_conversation_id(&request.conversation_id)?;
    }
    if let Some(parent) = parent {
        handle.set_parent(parent)?;
    }
    if state
        .begin_run(request_id, request.requested_model.clone())
        .await
    {
        emit_run_ready(&handle, request.requested_model.as_ref());
    }
    Ok(ai::BidiAppendResponse {})
}

fn emit_run_ready(handle: &TransportHandle, requested_model: Option<&ai::InferenceRequestedModel>) {
    let message = ai::RunInferenceServerMessage {
        message: Some(ai::run_inference_server_message::Message::RunReady(
            ai::RunInferenceRunReady {
                resolved_model: requested_model.cloned(),
                prompt_model_metadata: Some(default_prompt_model_metadata()),
                accepts_client_heartbeat: true,
                accepts_agent_events: true,
                accepts_aborted_finish: true,
                ..Default::default()
            },
        )),
    };
    emit_server(handle, &message);
}

fn default_prompt_model_metadata() -> ai::RunInferencePromptModelMetadata {
    let mode = |mode_id: &str| ai::RunInferenceAgentModeConfig {
        mode_id: mode_id.into(),
        ..Default::default()
    };
    ai::RunInferencePromptModelMetadata {
        vendor: "unknown".into(),
        prompt_version: "latest".into(),
        persona: "anonymous".into(),
        supports_assistant_message_prefill: true,
        enable_line_numbers: true,
        use_sparse_read_line_numbers: true,
        self_identity_name: "Cursor".into(),
        dsv3_is_thinking: Some(false),
        agent_mode_configs: vec![
            mode("agent"),
            mode("chat"),
            mode("plan"),
            mode("debug"),
            mode("triage"),
            mode("multitask"),
        ],
        use_new_plan_mode_prompts: Some(false),
        ask_question_config: Some(ai::RunInferenceAskQuestionConfig {
            tool_name: "AskQuestion".into(),
            auto_reject_first: false,
        }),
        enable_semantic_search: Some(false),
        ..Default::default()
    }
}

async fn invoke_model(
    registry: &TransportRegistry,
    state: &RunInferenceState,
    request_id: &str,
    request: ai::RunInferenceInvokeModel,
) -> Result<ai::BidiAppendResponse> {
    if request.invocation_id.is_empty() {
        return Err(Error::Protocol(
            "RunInference invoke_model requires invocation_id".into(),
        ));
    }
    let fallback_model = state.model(request_id).await;
    let invocation = build_invocation(request_id, &request, fallback_model.as_ref())?;
    let handle = registry.get_or_create_for_append(request_id, false).await?;
    let token = CancellationToken::new();
    state
        .insert_invocation(request_id, &request.invocation_id, token.clone())
        .await;
    let registry = registry.clone();
    let state = state.clone();
    let invocation_id = request.invocation_id.clone();
    tokio::spawn(async move {
        run_invocation(registry, handle, state, invocation_id, invocation, token).await;
    });
    Ok(ai::BidiAppendResponse {})
}

fn build_invocation(
    request_id: &str,
    request: &ai::RunInferenceInvokeModel,
    fallback_model: Option<&ai::InferenceRequestedModel>,
) -> Result<ModelInvocation> {
    let decompressed;
    let stream_request = if let Some(request) = request.request.as_ref() {
        request
    } else {
        decompressed = decompress_stream_request(request)?;
        &decompressed
    };
    let model = model_spec(
        stream_request.requested_model.as_ref(),
        stream_request.model_id.as_deref(),
        fallback_model,
    )?;
    let history = convert_messages(&stream_request.messages)?;
    let tools = convert_tools(&stream_request.tools);
    let conversation_id = stream_request
        .conversation_id
        .clone()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| request_id.to_string());
    Ok(ModelInvocation {
        call_id: request.invocation_id.clone(),
        run_id: request_id.into(),
        conversation_id,
        provider_call_index: 0,
        request: ModelRequest {
            prompt: PromptSpec {
                instructions: String::new(),
                tools,
            },
            model,
            history,
        },
    })
}

fn decompress_stream_request(
    request: &ai::RunInferenceInvokeModel,
) -> Result<ai::InferenceStreamRequest> {
    use std::io::Read;

    let compressed = request
        .compressed_request
        .as_ref()
        .ok_or_else(|| Error::Protocol("RunInference invoke_model has no request".into()))?;
    let data = match ai::run_inference_request_encoding::Encoding::try_from(compressed.encoding) {
        Ok(ai::run_inference_request_encoding::Encoding::Zstd) => {
            zstd::stream::decode_all(compressed.data.as_slice()).map_err(|error| {
                Error::Protocol(format!("cannot zstd-decode invoke_model: {error}"))
            })?
        }
        Ok(ai::run_inference_request_encoding::Encoding::Gzip) => {
            let mut decoder = flate2::read::GzDecoder::new(compressed.data.as_slice());
            let mut data = Vec::new();
            decoder.read_to_end(&mut data).map_err(|error| {
                Error::Protocol(format!("cannot gzip-decode invoke_model: {error}"))
            })?;
            data
        }
        _ => {
            return Err(Error::Protocol(format!(
                "unsupported invoke_model encoding: {}",
                compressed.encoding
            )))
        }
    };
    ai::InferenceStreamRequest::decode(data.as_slice())
        .map_err(|error| Error::Protocol(format!("cannot decode invoke_model request: {error}")))
}

fn model_spec(
    requested: Option<&ai::InferenceRequestedModel>,
    model_id: Option<&str>,
    fallback: Option<&ai::InferenceRequestedModel>,
) -> Result<ModelSpec> {
    let requested = requested.or(fallback);
    let model_id = requested
        .map(|model| model.model_id.as_str())
        .filter(|model| !model.is_empty())
        .or(model_id)
        .filter(|model| !model.is_empty())
        .ok_or_else(|| Error::Protocol("RunInference request has no model".into()))?;
    let mut spec = ModelSpec::new(model_id);
    if let Some(requested) = requested {
        for parameter in &requested.parameters {
            match parameter.id.as_str() {
                "effort" | "reasoning" => {
                    let effort = parameter.value.trim();
                    spec.reasoning.effort =
                        (effort != "none" && !effort.is_empty()).then(|| effort.to_string());
                    spec.reasoning.enabled |= spec.reasoning.effort.is_some();
                }
                "thinking" => spec.reasoning.enabled |= parameter.value == "true",
                "fast" => {
                    if parameter.value == "true" {
                        spec.latency = ModelLatency::Fast;
                    }
                }
                "context" => {
                    if let Some(tokens) = crate::model::parse_token_count(&parameter.value) {
                        spec.context_window_tokens = Some(tokens);
                    }
                }
                _ => {}
            }
        }
    }
    Ok(spec)
}

fn convert_messages(messages: &[ai::InferenceCoreMessage]) -> Result<Vec<ProjectedMessage>> {
    let mut output = Vec::with_capacity(messages.len());
    for (index, message) in messages.iter().enumerate() {
        let role = match message.role {
            1 => Role::User,
            2 => Role::Assistant,
            3 => Role::Tool,
            4 => Role::System,
            _ => Role::System,
        };
        let message_id = message
            .model_provider_message_id
            .clone()
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| format!("cursor-inference-{index}"));
        if role == Role::Tool {
            let Some(ai::inference_core_message::Content::ToolContent(tool_content)) =
                message.content.as_ref()
            else {
                continue;
            };
            for (part_index, part) in tool_content.parts.iter().enumerate() {
                output.push(ProjectedMessage {
                    message_id: format!("{message_id}-tool-{part_index}"),
                    role: Role::Tool,
                    content: ProjectedContent::ToolResult(tool_result(part)?),
                });
            }
            continue;
        }
        if role == Role::Assistant {
            let thinking = message
                .reasoning_parts
                .iter()
                .filter(|part| !part.is_redacted)
                .map(|part| part.text.as_str())
                .collect::<String>();
            let calls = message
                .tool_calls
                .iter()
                .enumerate()
                .map(|(index, call)| ToolCallContent {
                    index,
                    call_id: call.tool_call_id.clone(),
                    name: call.tool_name.clone(),
                    arguments: tool_arguments(call),
                })
                .collect();
            let text = message_text(message)?;
            output.push(ProjectedMessage {
                message_id,
                role,
                content: ProjectedContent::Assistant {
                    text,
                    thinking,
                    replay_state: None,
                    calls,
                },
            });
            continue;
        }
        let parts = message_parts(message)?;
        output.push(ProjectedMessage {
            message_id,
            role,
            content: ProjectedContent::Parts(parts),
        });
    }
    Ok(output)
}

fn message_text(message: &ai::InferenceCoreMessage) -> Result<String> {
    match message.content.as_ref() {
        Some(ai::inference_core_message::Content::Text(text)) => Ok(text.clone()),
        Some(ai::inference_core_message::Content::Parts(parts)) => {
            let mut text = String::new();
            for part in &parts.parts {
                if let Some(ai::inference_content_part::Part::Text(part)) = part.part.as_ref() {
                    text.push_str(&part.text);
                }
            }
            Ok(text)
        }
        Some(ai::inference_core_message::Content::ToolContent(_)) | None => Ok(String::new()),
    }
}

fn message_parts(message: &ai::InferenceCoreMessage) -> Result<Vec<ContentPart>> {
    let mut parts = match message.content.as_ref() {
        Some(ai::inference_core_message::Content::Text(text)) => {
            vec![ContentPart::Text { text: text.clone() }]
        }
        Some(ai::inference_core_message::Content::Parts(parts)) => parts
            .parts
            .iter()
            .filter_map(content_part)
            .collect::<Result<Vec<_>>>()?,
        Some(ai::inference_core_message::Content::ToolContent(_)) | None => Vec::new(),
    };
    if parts.is_empty() {
        parts.push(ContentPart::Text {
            text: String::new(),
        });
    }
    Ok(parts)
}

fn content_part(part: &ai::InferenceContentPart) -> Option<Result<ContentPart>> {
    match part.part.as_ref()? {
        ai::inference_content_part::Part::Text(text) => Some(Ok(ContentPart::Text {
            text: text.text.clone(),
        })),
        ai::inference_content_part::Part::Image(image) => image_content(image).transpose(),
        ai::inference_content_part::Part::File(_) => None,
    }
}

fn image_content(image: &ai::InferenceImagePart) -> Result<Option<ContentPart>> {
    use base64::Engine;

    let data = image.data.trim();
    if data.is_empty() {
        return Ok(None);
    }
    let (mime_type, encoded) = if let Some(rest) = data.strip_prefix("data:") {
        let (header, encoded) = rest
            .split_once(',')
            .ok_or_else(|| Error::Protocol("Cursor image data URL has no payload".into()))?;
        let header_lower = header.to_ascii_lowercase();
        let marker = header_lower
            .find(";base64")
            .ok_or_else(|| Error::Protocol("Cursor image data URL is not base64".into()))?;
        let media_type = &header[..marker];
        (
            if media_type.is_empty() {
                image
                    .mime_type
                    .clone()
                    .unwrap_or_else(|| "image/png".into())
            } else {
                media_type.to_string()
            },
            encoded,
        )
    } else {
        (
            image
                .mime_type
                .clone()
                .unwrap_or_else(|| "image/png".into()),
            data,
        )
    };
    let encoded = encoded
        .chars()
        .filter(|character| !character.is_ascii_whitespace())
        .collect::<String>();
    let data = base64::engine::general_purpose::STANDARD
        .decode(encoded.as_bytes())
        .map_err(|error| Error::Protocol(format!("cannot decode Cursor image data: {error}")))?;
    if data.is_empty() {
        return Ok(None);
    }
    Ok(Some(ContentPart::Image { mime_type, data }))
}

fn tool_result(part: &ai::InferenceToolResultPart) -> Result<ToolResultContent> {
    let value = part
        .result
        .as_ref()
        .map(prost_value_to_json)
        .unwrap_or_default();
    let content = match &value {
        serde_json::Value::String(text) => text.clone(),
        _ => value.to_string(),
    };
    Ok(ToolResultContent {
        call_id: part.tool_call_id.clone(),
        name: part.tool_name.clone(),
        content,
        is_error: part.is_error,
        image: None,
        provider_parts: part
            .experimental_content
            .iter()
            .filter_map(content_part)
            .collect::<Result<Vec<_>>>()?,
    })
}

fn tool_arguments(call: &ai::InferenceToolCall) -> serde_json::Value {
    if let Some(raw) = call
        .raw_tool_call_args
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        if let Ok(value) = serde_json::from_str(raw) {
            return value;
        }
    }
    call.args
        .as_ref()
        .map(prost_struct_to_json)
        .unwrap_or_else(|| serde_json::json!({}))
}

fn prost_struct_to_json(value: &prost_types::Struct) -> serde_json::Value {
    serde_json::Value::Object(
        value
            .fields
            .iter()
            .map(|(key, value)| (key.clone(), prost_value_to_json(value)))
            .collect(),
    )
}

fn prost_value_to_json(value: &prost_types::Value) -> serde_json::Value {
    use prost_types::value::Kind;
    match value.kind.as_ref() {
        None | Some(Kind::NullValue(_)) => serde_json::Value::Null,
        Some(Kind::NumberValue(value)) => serde_json::Number::from_f64(*value)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null),
        Some(Kind::StringValue(value)) => serde_json::Value::String(value.clone()),
        Some(Kind::BoolValue(value)) => serde_json::Value::Bool(*value),
        Some(Kind::StructValue(value)) => prost_struct_to_json(value),
        Some(Kind::ListValue(value)) => {
            serde_json::Value::Array(value.values.iter().map(prost_value_to_json).collect())
        }
    }
}

fn convert_tools(tools: &[ai::InferenceAgentTool]) -> Vec<ToolDefinition> {
    tools
        .iter()
        .map(|tool| ToolDefinition {
            name: tool.name.clone(),
            description: tool.description.clone(),
            parameters: convert_tool_parameters(tool.parameters.as_ref()),
        })
        .collect()
}

fn convert_tool_parameters(parameters: Option<&prost_types::Struct>) -> serde_json::Value {
    let parameters = parameters
        .map(prost_struct_to_json)
        .unwrap_or_else(|| serde_json::json!({"type": "object"}));
    unwrap_cursor_json_schema(parameters)
}

fn unwrap_cursor_json_schema(parameters: serde_json::Value) -> serde_json::Value {
    let Some(object) = parameters.as_object() else {
        return parameters;
    };
    if object.len() != 1 {
        return parameters;
    }
    ["jsonSchema", "json_schema"]
        .into_iter()
        .find_map(|key| object.get(key).filter(|schema| schema.is_object()).cloned())
        .unwrap_or(parameters)
}

async fn run_invocation(
    registry: TransportRegistry,
    handle: TransportHandle,
    state: RunInferenceState,
    invocation_id: String,
    invocation: ModelInvocation,
    cancellation: CancellationToken,
) {
    let mut tool_state = HashMap::<usize, ToolState>::new();
    let mut error = None;
    {
        let provider = registry.conversations().dependencies().provider.clone();
        let stream = provider.stream(invocation, cancellation.clone());
        futures_util::pin_mut!(stream);
        emit_server(
            &handle,
            &invocation_response(
                &invocation_id,
                ai::InferenceStreamResponse {
                    response: Some(ai::inference_stream_response::Response::InvocationId(
                        ai::InferenceInvocationIdInfo {
                            invocation_id: invocation_id.clone(),
                        },
                    )),
                },
            ),
        );
        while let Some(event) = stream.next().await {
            match event {
                Ok(event) => {
                    for message in response_messages(&invocation_id, &event, &mut tool_state) {
                        emit_server(&handle, &message);
                    }
                }
                Err(event_error) => {
                    error = Some(event_error.to_string());
                    break;
                }
            }
        }
    }
    let end = ai::RunInferenceInvocationEnd {
        invocation_id: invocation_id.clone(),
        error: error.map(|message| ai::RunInferenceInvocationError { code: 13, message }),
        delta_refused: None,
    };
    emit_server(
        &handle,
        &ai::RunInferenceServerMessage {
            message: Some(ai::run_inference_server_message::Message::InvocationEnd(
                end,
            )),
        },
    );
    state
        .remove_invocation(handle.request_id(), &invocation_id)
        .await;
}

#[derive(Default)]
struct ToolState {
    call_id: String,
    name: String,
    args: String,
}

fn response_messages(
    invocation_id: &str,
    event: &crate::provider::ModelEvent,
    tool_state: &mut HashMap<usize, ToolState>,
) -> Vec<ai::RunInferenceServerMessage> {
    use crate::provider::ModelEvent;
    let response =
        |response: ai::InferenceStreamResponse| invocation_response(invocation_id, response);
    match event {
        ModelEvent::Start { .. } | ModelEvent::Done(_) | ModelEvent::ProviderReplayState(_) => {
            Vec::new()
        }
        ModelEvent::TextDelta(text) => {
            vec![response(ai::InferenceStreamResponse {
                response: Some(ai::inference_stream_response::Response::TextPart(
                    ai::InferenceTextStreamPart {
                        text: text.clone(),
                        is_final: false,
                    },
                )),
            })]
        }
        ModelEvent::TextEnd => {
            vec![response(ai::InferenceStreamResponse {
                response: Some(ai::inference_stream_response::Response::TextPart(
                    ai::InferenceTextStreamPart {
                        text: String::new(),
                        is_final: true,
                    },
                )),
            })]
        }
        ModelEvent::ThinkingDelta(text) => {
            vec![response(ai::InferenceStreamResponse {
                response: Some(ai::inference_stream_response::Response::ThinkingPart(
                    ai::InferenceThinkingStreamPart {
                        text: text.clone(),
                        signature: None,
                        is_final: false,
                    },
                )),
            })]
        }
        ModelEvent::ThinkingStart | ModelEvent::ThinkingEnd => {
            vec![response(ai::InferenceStreamResponse {
                response: Some(ai::inference_stream_response::Response::ThinkingPart(
                    ai::InferenceThinkingStreamPart {
                        text: String::new(),
                        signature: None,
                        is_final: matches!(event, ModelEvent::ThinkingEnd),
                    },
                )),
            })]
        }
        ModelEvent::ToolCallStart {
            index,
            call_id,
            name,
        } => {
            tool_state.insert(
                *index,
                ToolState {
                    call_id: call_id.clone(),
                    name: name.clone(),
                    args: String::new(),
                },
            );
            vec![response(tool_call_part(*index, call_id, name, "", false))]
        }
        ModelEvent::ToolCallArgumentsDelta { index, delta } => {
            let Some(state) = tool_state.get_mut(index) else {
                return Vec::new();
            };
            state.args.push_str(delta);
            vec![response(tool_call_part(
                *index,
                &state.call_id,
                &state.name,
                &state.args,
                false,
            ))]
        }
        ModelEvent::ToolCallEnd { index } => {
            let Some(state) = tool_state.remove(index) else {
                return Vec::new();
            };
            vec![response(tool_call_part(
                *index,
                &state.call_id,
                &state.name,
                &state.args,
                true,
            ))]
        }
        ModelEvent::Usage(usage) => {
            vec![response(usage_response(usage))]
        }
        ModelEvent::TextStart => Vec::new(),
    }
}

fn tool_call_part(
    index: usize,
    call_id: &str,
    name: &str,
    args: &str,
    is_complete: bool,
) -> ai::InferenceStreamResponse {
    ai::InferenceStreamResponse {
        response: Some(ai::inference_stream_response::Response::ToolCallPart(
            ai::InferenceToolCallStreamPart {
                tool_call_id: call_id.into(),
                tool_name: name.into(),
                args: args.into(),
                is_complete,
                tool_index: Some(index as i32),
            },
        )),
    }
}

fn usage_response(usage: &Usage) -> ai::InferenceStreamResponse {
    let count = |value: Option<u64>| value.unwrap_or_default().min(i32::MAX as u64) as i32;
    ai::InferenceStreamResponse {
        response: Some(ai::inference_stream_response::Response::ExtendedUsage(
            ai::InferenceExtendedUsageInfo {
                input_tokens: count(usage.input_tokens),
                output_tokens: count(usage.output_tokens),
                cache_read_tokens: count(usage.cache_read_tokens),
                cache_write_tokens: count(usage.cache_write_tokens),
                max_tokens: 0,
            },
        )),
    }
}

fn invocation_response(
    invocation_id: &str,
    response: ai::InferenceStreamResponse,
) -> ai::RunInferenceServerMessage {
    ai::RunInferenceServerMessage {
        message: Some(
            ai::run_inference_server_message::Message::InvocationResponse(
                ai::RunInferenceInvocationResponse {
                    invocation_id: invocation_id.into(),
                    response: Some(response),
                },
            ),
        ),
    }
}

fn emit_server(handle: &TransportHandle, message: &ai::RunInferenceServerMessage) {
    if let Ok(frame) = connect::encode_message(message) {
        handle.emit_frame(frame);
    }
}

fn finish_stream(handle: &TransportHandle) {
    handle.emit_frame(connect::encode_end_stream());
    handle.close_output();
    handle.close_transport();
}

const OFFICIAL_INFERENCE_SSE: &str = "/aiserver.v1.InferenceService/RunInferenceSSE";
const UPSTREAM_STREAM_READY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// Connects an official model turn to Cursor and copies its frames into the
/// local `RunInferenceSSE` subscription the client is already reading.
pub(crate) async fn ensure_upstream_bridge(
    registry: &TransportRegistry,
    state: &RunInferenceState,
    proxy: &CursorProxy,
    request_id: &str,
    headers: &HeaderMap,
) {
    state.store_upstream_headers(request_id, headers).await;
    let Some(handle) = registry.local(request_id).await else {
        return;
    };
    if !state.begin_upstream_bridge(request_id).await {
        return;
    }
    let ready = std::sync::Arc::new(tokio::sync::Notify::new());
    let notify_sent = ready.clone();
    let task_ready = ready.clone();
    let notified = ready.notified();
    tokio::pin!(notified);
    notified.as_mut().enable();
    let proxy = proxy.clone();
    let request_id = request_id.to_owned();
    let headers = headers.clone();
    tokio::spawn(async move {
        let _ready = NotifyOnDrop(task_ready);
        bridge_official_inference(
            proxy,
            request_id,
            headers,
            handle,
            move || notify_sent.notify_one(),
        )
        .await;
    });
    let _ = tokio::time::timeout(UPSTREAM_STREAM_READY_TIMEOUT, notified).await;
}

async fn resume_upstream_bridge(
    registry: &TransportRegistry,
    state: &RunInferenceState,
    proxy: &CursorProxy,
    request_id: &str,
) {
    if !registry.upstream(request_id).await {
        return;
    }
    let Some(headers) = state.upstream_headers(request_id).await else {
        return;
    };
    ensure_upstream_bridge(registry, state, proxy, request_id, &headers).await;
}

struct NotifyOnDrop(std::sync::Arc<tokio::sync::Notify>);

impl Drop for NotifyOnDrop {
    fn drop(&mut self) {
        self.0.notify_one();
    }
}

async fn bridge_official_inference(
    proxy: CursorProxy,
    request_id: String,
    headers: HeaderMap,
    handle: TransportHandle,
    on_request_sent: impl FnOnce() + Send + 'static,
) {
    let url = match official_inference_url(&headers) {
        Ok(url) => url,
        Err(error) => {
            tracing::error!(request_id, %error, "official inference URL rejected");
            emit_bridge_error(&handle, &error.to_string());
            return;
        }
    };
    let body = match connect::encode_message(&ai::BidiRequestId {
        request_id: request_id.clone(),
    }) {
        Ok(body) => body,
        Err(error) => {
            tracing::error!(
                request_id,
                %error,
                "official inference request encoding failed"
            );
            emit_bridge_error(&handle, &error.to_string());
            return;
        }
    };
    tracing::info!(request_id, %url, "opening official inference stream");
    let response = match proxy::post_stream(&proxy, url, headers, body, on_request_sent).await {
        Ok(response) => response,
        Err(error) => {
            tracing::error!(request_id, %error, "official inference stream failed");
            emit_bridge_error(&handle, &error.to_string());
            return;
        }
    };
    let status = response.status();
    tracing::info!(request_id, %status, "official inference stream responded");
    if !status.is_success() {
        emit_bridge_error(
            &handle,
            &format!("official inference stream returned {status}"),
        );
        return;
    }
    let disconnect = handle.disconnect_token();
    let mut stream = std::pin::pin!(response.bytes_stream());
    loop {
        tokio::select! {
            biased;
            _ = disconnect.cancelled() => break,
            chunk = stream.next() => {
                match chunk {
                    Some(Ok(chunk)) if !chunk.is_empty() => {
                        if !handle.emit_frame(chunk) {
                            break;
                        }
                    }
                    Some(Ok(_)) => {}
                    Some(Err(error)) => {
                        tracing::error!(
                            request_id,
                            %error,
                            "official inference stream interrupted"
                        );
                        emit_bridge_error(&handle, &error.to_string());
                        return;
                    }
                    None => break,
                }
            }
        }
    }
    handle.close_output();
}

fn emit_bridge_error(handle: &TransportHandle, message: &str) {
    let error = connect::ConnectStreamError {
        code: connect::ConnectCode::Unavailable,
        message: message.to_owned(),
        details: Vec::new(),
    };
    if let Ok(frame) = connect::encode_error_end_stream(&error) {
        handle.emit_frame(frame);
    }
    handle.close_output();
}

pub(crate) fn official_inference_url(headers: &HeaderMap) -> Result<String> {
    let origin = if let Some(value) = headers.get(UPSTREAM_URL_HEADER) {
        let value = value.to_str().map_err(|error| {
            Error::Protocol(format!("invalid upstream URL header: {error}"))
        })?;
        let url = reqwest::Url::parse(value)
            .map_err(|error| Error::Protocol(format!("invalid upstream URL: {error}")))?;
        let host = url.host_str().unwrap_or_default();
        if url.scheme() != "https" || !crate::local_app::proxy_host_allowed(host) {
            return Err(Error::Protocol(
                "upstream URL must target a Cursor HTTPS host".into(),
            ));
        }
        let port = url
            .port()
            .map(|port| format!(":{port}"))
            .unwrap_or_default();
        format!("https://{host}{port}")
    } else {
        proxy::cursor_upstream().to_owned()
    };
    Ok(format!(
        "{}{}",
        origin.trim_end_matches('/'),
        OFFICIAL_INFERENCE_SSE
    ))
}

pub async fn sse(
    State(registry): State<TransportRegistry>,
    Extension(proxy): Extension<CursorProxy>,
    Extension(state): Extension<RunInferenceState>,
    request: Request<Body>,
) -> Result<Response<Body>> {
    let body = to_bytes(request.into_body(), usize::MAX)
        .await
        .map_err(|error| Error::Protocol(format!("cannot read RunInferenceSSE body: {error}")))?;
    let request: ai::BidiRequestId = connect::decode_unary(&body)?;
    let handle = registry.get_or_create(&request.request_id).await?;
    resume_upstream_bridge(&registry, &state, &proxy, &request.request_id).await;
    let mut receiver = handle.subscribe();
    let body = stream! {
        let mut guard = StreamGuard::new(handle.clone());
        while let Some(frame) = receiver.recv().await {
            let terminal = frame
                .first()
                .is_some_and(|flags| flags & END_STREAM_FLAG != 0);
            yield Ok::<Bytes, Infallible>(frame);
            if terminal {
                guard.done();
                break;
            }
        }
    };
    let mut response = Response::new(Body::from_stream(body));
    *response.status_mut() = StatusCode::OK;
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/event-stream"),
    );
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    response
        .headers_mut()
        .insert("connect-protocol-version", HeaderValue::from_static("1"));
    Ok(response)
}

pub async fn poll(
    State(registry): State<TransportRegistry>,
    Extension(proxy): Extension<CursorProxy>,
    Extension(state): Extension<RunInferenceState>,
    request: Request<Body>,
) -> Result<Response<Body>> {
    let body = to_bytes(request.into_body(), usize::MAX)
        .await
        .map_err(|error| Error::Protocol(format!("cannot read RunInferencePoll body: {error}")))?;
    let request: ai::BidiPollRequest = connect::decode_unary(&body)?;
    let request_id = request
        .request_id
        .as_ref()
        .map(|id| id.request_id.as_str())
        .filter(|id| !id.is_empty())
        .ok_or_else(|| Error::Protocol("RunInferencePoll request_id is required".into()))?;
    let handle = registry.get_or_create(request_id).await?;
    resume_upstream_bridge(&registry, &state, &proxy, request_id).await;
    let mut receiver = handle.subscribe();
    let body = stream! {
        let mut guard = StreamGuard::new(handle.clone());
        let mut seqno = 1_i64;
        while let Some(frame) = receiver.recv().await {
            let terminal = frame
                .first()
                .is_some_and(|flags| flags & END_STREAM_FLAG != 0);
            let data = if terminal {
                String::new()
            } else {
                frame_payload(&frame).map(hex::encode).unwrap_or_default()
            };
            let response = ai::BidiPollResponse {
                seqno,
                data,
                eof: terminal,
            };
            match connect::encode_message(&response) {
                Ok(frame) => yield Ok::<Bytes, Infallible>(frame),
                Err(_) => break,
            }
            if terminal {
                guard.done();
                break;
            }
            seqno = seqno.saturating_add(1);
        }
    };
    let mut response = Response::new(Body::from_stream(body));
    *response.status_mut() = StatusCode::OK;
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/proto"),
    );
    response
        .headers_mut()
        .insert("connect-protocol-version", HeaderValue::from_static("1"));
    Ok(response)
}

fn frame_payload(frame: &Bytes) -> Option<Vec<u8>> {
    connect::decode_frames(frame)
        .ok()?
        .into_iter()
        .find_map(|(flags, payload)| (flags & END_STREAM_FLAG == 0).then(|| payload.to_vec()))
}

struct StreamGuard {
    handle: TransportHandle,
    done: bool,
}

impl StreamGuard {
    fn new(handle: TransportHandle) -> Self {
        Self {
            handle,
            done: false,
        }
    }

    fn done(&mut self) {
        self.done = true;
    }
}

impl Drop for StreamGuard {
    fn drop(&mut self) {
        if !self.done {
            let handle = self.handle.clone();
            tokio::spawn(async move {
                handle.disconnect().await;
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn compressed_gzip(request: &ai::InferenceStreamRequest) -> ai::RunInferenceInvokeModel {
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder
            .write_all(&request.encode_to_vec())
            .expect("encode stream request");
        let data = encoder.finish().expect("finish gzip");
        ai::RunInferenceInvokeModel {
            invocation_id: "invocation-1".into(),
            request: None,
            config_fingerprint: Vec::new(),
            compressed_request: Some(ai::RunInferenceCompressedRequest {
                encoding: ai::run_inference_request_encoding::Encoding::Gzip as i32,
                decoded_size: request.encoded_len() as u32,
                data,
            }),
        }
    }

    #[test]
    fn official_inference_url_keeps_the_cursor_origin() {
        let mut headers = HeaderMap::new();
        headers.insert(
            UPSTREAM_URL_HEADER,
            HeaderValue::from_static(
                "https://api2.cursor.sh/aiserver.v1.BidiService/BidiAppend",
            ),
        );
        assert_eq!(
            official_inference_url(&headers).unwrap(),
            "https://api2.cursor.sh/aiserver.v1.InferenceService/RunInferenceSSE"
        );

        let mut headers = HeaderMap::new();
        headers.insert(
            UPSTREAM_URL_HEADER,
            HeaderValue::from_static("https://evil.example/aiserver.v1.BidiService/BidiAppend"),
        );
        assert!(official_inference_url(&headers).is_err());
        assert_eq!(
            official_inference_url(&HeaderMap::new()).unwrap(),
            "https://api2.cursor.sh/aiserver.v1.InferenceService/RunInferenceSSE"
        );
    }

    #[test]
    fn decompresses_gzip_invoke_model_request() {
        let original = ai::InferenceStreamRequest {
            messages: vec![ai::InferenceCoreMessage {
                role: 1,
                content: Some(ai::inference_core_message::Content::Text(
                    "hello byok".into(),
                )),
                tool_calls: Vec::new(),
                reasoning_parts: Vec::new(),
                model_provider_message_id: None,
            }],
            tools: Vec::new(),
            model_id: Some("2ac720b73432b262".into()),
            requested_model: None,
            conversation_id: Some("captured-conversation".into()),
        };
        let request = compressed_gzip(&original);
        let decoded = decompress_stream_request(&request).expect("gzip request must decode");
        assert_eq!(decoded.model_id.as_deref(), Some("2ac720b73432b262"));
        assert_eq!(decoded.messages.len(), 1);
        assert!(matches!(
            decoded.messages[0].content,
            Some(ai::inference_core_message::Content::Text(ref text)) if text == "hello byok"
        ));
    }

    #[test]
    fn unwraps_cursor_tool_json_schema() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {"path": {"type": "string"}},
            "required": ["path"]
        });

        assert_eq!(
            unwrap_cursor_json_schema(serde_json::json!({"jsonSchema": schema})),
            schema
        );
        assert_eq!(
            unwrap_cursor_json_schema(serde_json::json!({"json_schema": schema})),
            schema
        );
    }

    #[test]
    fn leaves_standard_tool_json_schema_unchanged() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {"path": {"type": "string"}}
        });

        assert_eq!(unwrap_cursor_json_schema(schema.clone()), schema);
    }

    #[test]
    fn decodes_cursor_image_data_url() {
        let image = ai::InferenceImagePart {
            data: "data:image/png;base64,aGVsbG8=".into(),
            mime_type: Some("image/jpeg".into()),
        };

        assert!(matches!(
            image_content(&image).expect("image must decode"),
            Some(ContentPart::Image { mime_type, data })
                if mime_type == "image/png" && data == b"hello"
        ));
    }

    #[test]
    fn decodes_cursor_raw_base64_image() {
        let image = ai::InferenceImagePart {
            data: "aGVsbG8=".into(),
            mime_type: Some("image/webp".into()),
        };

        assert!(matches!(
            image_content(&image).expect("image must decode"),
            Some(ContentPart::Image { mime_type, data })
                if mime_type == "image/webp" && data == b"hello"
        ));
    }

    #[test]
    fn skips_empty_cursor_image() {
        let image = ai::InferenceImagePart {
            data: "data:image/jpeg;base64,".into(),
            mime_type: Some("image/jpeg".into()),
        };

        assert!(image_content(&image)
            .expect("empty image is valid")
            .is_none());
    }

    #[test]
    fn rejects_malformed_cursor_image() {
        let image = ai::InferenceImagePart {
            data: "data:image/jpeg;base64,not base64".into(),
            mime_type: Some("image/jpeg".into()),
        };

        assert!(matches!(image_content(&image), Err(Error::Protocol(_))));
    }
}
