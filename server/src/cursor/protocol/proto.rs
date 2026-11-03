//! Includes generated Cursor protobuf types.

pub mod agent {
    #[allow(clippy::large_enum_variant)]
    pub mod v1 {
        include!(concat!(env!("OUT_DIR"), "/agent.v1.rs"));
    }
}

pub mod aiserver {
    pub mod v1 {
        use std::collections::HashMap;

        #[derive(Clone, PartialEq, ::prost::Message)]
        pub struct BidiRequestId {
            #[prost(string, tag = "1")]
            pub request_id: String,
        }

        #[derive(Clone, PartialEq, ::prost::Message)]
        pub struct BidiAppendRequest {
            #[prost(string, tag = "1")]
            pub data: String,
            #[prost(message, optional, tag = "2")]
            pub request_id: Option<BidiRequestId>,
            #[prost(int64, tag = "3")]
            pub append_seqno: i64,
            #[prost(bytes = "vec", tag = "4")]
            pub data_binary: Vec<u8>,
        }

        /// Commit message generation request. Only the fields the local
        /// generator consumes are decoded; credentials and heavyweight context
        /// fields are intentionally left to prost's unknown-field skipping.
        #[derive(Clone, PartialEq, ::prost::Message)]
        pub struct WriteGitCommitMessageRequest {
            #[prost(string, repeated, tag = "1")]
            pub diffs: Vec<String>,
            #[prost(string, repeated, tag = "2")]
            pub previous_commit_messages: Vec<String>,
            #[prost(message, optional, tag = "3")]
            pub explicit_context: Option<ExplicitContext>,
        }

        #[derive(Clone, PartialEq, ::prost::Message)]
        pub struct ExplicitContext {
            #[prost(string, tag = "1")]
            pub context: String,
            #[prost(string, optional, tag = "2")]
            pub repo_context: Option<String>,
        }

        #[derive(Clone, PartialEq, ::prost::Message)]
        pub struct WriteGitCommitMessageResponse {
            #[prost(string, tag = "1")]
            pub commit_message: String,
        }

        #[derive(Clone, Copy, PartialEq, ::prost::Message)]
        pub struct BidiAppendResponse {}

        /// `NetworkService/IsConnected` reply. The Cursor extension probes this
        /// ~10s after any slow request starts; a non-OK result is treated as
        /// "network disconnected" and aborts in-flight work (e.g. commit message
        /// generation) even while the model is still streaming, so it always
        /// answers as connected.
        #[derive(Clone, Copy, PartialEq, ::prost::Message)]
        pub struct IsConnectedResponse {}

        #[derive(Clone, PartialEq, ::prost::Message)]
        pub struct CustomErrorDetails {
            #[prost(string, tag = "1")]
            pub title: String,
            #[prost(string, tag = "2")]
            pub detail: String,
            #[prost(bool, optional, tag = "3")]
            pub allow_command_links_potentially_unsafe_please_only_use_for_handwritten_trusted_markdown:
                Option<bool>,
            #[prost(bool, optional, tag = "4")]
            pub is_retryable: Option<bool>,
            #[prost(bool, optional, tag = "5")]
            pub show_request_id: Option<bool>,
            #[prost(bool, optional, tag = "6")]
            pub should_show_immediate_error: Option<bool>,
        }

        #[derive(Clone, PartialEq, ::prost::Message)]
        pub struct ErrorDetails {
            #[prost(enumeration = "error_details::Error", tag = "1")]
            pub error: i32,
            #[prost(message, optional, tag = "2")]
            pub details: Option<CustomErrorDetails>,
            #[prost(bool, optional, tag = "3")]
            pub is_expected: Option<bool>,
        }

        pub mod error_details {
            #[derive(
                Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration,
            )]
            #[repr(i32)]
            pub enum Error {
                Unspecified = 0,
                CustomMessage = 29,
                ProviderError = 57,
                Internal = 59,
            }
        }

        #[derive(Clone, PartialEq, ::prost::Message)]
        pub struct BidiPollRequest {
            #[prost(message, optional, tag = "1")]
            pub request_id: Option<BidiRequestId>,
            #[prost(bool, optional, tag = "2")]
            pub start_request: Option<bool>,
        }

        #[derive(Clone, PartialEq, ::prost::Message)]
        pub struct BidiPollResponse {
            #[prost(int64, tag = "1")]
            pub seqno: i64,
            #[prost(string, tag = "2")]
            pub data: String,
            #[prost(bool, tag = "3")]
            pub eof: bool,
        }

        #[derive(Clone, PartialEq, ::prost::Message)]
        pub struct InferenceRequestedModel {
            #[prost(string, tag = "1")]
            pub model_id: String,
            #[prost(bool, tag = "2")]
            pub max_mode: bool,
            #[prost(message, repeated, tag = "3")]
            pub parameters: Vec<InferenceModelParameterValue>,
            #[prost(bool, tag = "4")]
            pub built_in_model: bool,
            #[prost(bool, tag = "5")]
            pub is_variant_string_representation: bool,
        }

        #[derive(Clone, PartialEq, ::prost::Message)]
        pub struct InferenceModelParameterValue {
            #[prost(string, tag = "1")]
            pub id: String,
            #[prost(string, tag = "2")]
            pub value: String,
        }

        #[derive(Clone, PartialEq, ::prost::Message)]
        pub struct InferenceStreamRequest {
            #[prost(message, repeated, tag = "1")]
            pub messages: Vec<InferenceCoreMessage>,
            #[prost(message, repeated, tag = "2")]
            pub tools: Vec<InferenceAgentTool>,
            #[prost(string, optional, tag = "5")]
            pub model_id: Option<String>,
            #[prost(message, optional, tag = "7")]
            pub requested_model: Option<InferenceRequestedModel>,
            #[prost(string, optional, tag = "8")]
            pub conversation_id: Option<String>,
        }

        #[derive(Clone, PartialEq, ::prost::Message)]
        pub struct InferenceCoreMessage {
            #[prost(enumeration = "inference_message_role::Role", tag = "1")]
            pub role: i32,
            #[prost(message, repeated, tag = "4")]
            pub tool_calls: Vec<InferenceToolCall>,
            #[prost(message, repeated, tag = "7")]
            pub reasoning_parts: Vec<InferenceReasoningPart>,
            #[prost(string, optional, tag = "8")]
            pub model_provider_message_id: Option<String>,
            #[prost(oneof = "inference_core_message::Content", tags = "2,3,6")]
            pub content: Option<inference_core_message::Content>,
        }

        pub mod inference_core_message {
            #[derive(Clone, PartialEq, ::prost::Oneof)]
            pub enum Content {
                #[prost(string, tag = "2")]
                Text(String),
                #[prost(message, tag = "3")]
                Parts(super::InferenceContentParts),
                #[prost(message, tag = "6")]
                ToolContent(super::InferenceToolResultContent),
            }
        }

        pub mod inference_message_role {
            #[derive(
                Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration,
            )]
            #[repr(i32)]
            pub enum Role {
                Unspecified = 0,
                User = 1,
                Assistant = 2,
                Tool = 3,
                System = 4,
            }
        }

        #[derive(Clone, PartialEq, ::prost::Message)]
        pub struct InferenceAgentTool {
            #[prost(string, tag = "1")]
            pub name: String,
            #[prost(string, tag = "2")]
            pub description: String,
            #[prost(message, optional, tag = "3")]
            pub parameters: Option<prost_types::Struct>,
        }

        #[derive(Clone, PartialEq, ::prost::Message)]
        pub struct InferenceContentParts {
            #[prost(message, repeated, tag = "1")]
            pub parts: Vec<InferenceContentPart>,
        }

        #[derive(Clone, PartialEq, ::prost::Message)]
        pub struct InferenceContentPart {
            #[prost(oneof = "inference_content_part::Part", tags = "1,2,3")]
            pub part: Option<inference_content_part::Part>,
        }

        pub mod inference_content_part {
            #[derive(Clone, PartialEq, ::prost::Oneof)]
            pub enum Part {
                #[prost(message, tag = "1")]
                Text(super::InferenceTextPart),
                #[prost(message, tag = "2")]
                Image(super::InferenceImagePart),
                #[prost(message, tag = "3")]
                File(super::InferenceFilePart),
            }
        }

        #[derive(Clone, PartialEq, ::prost::Message)]
        pub struct InferenceTextPart {
            #[prost(string, tag = "1")]
            pub text: String,
        }

        #[derive(Clone, PartialEq, ::prost::Message)]
        pub struct InferenceImagePart {
            #[prost(string, tag = "1")]
            pub data: String,
            #[prost(string, optional, tag = "2")]
            pub mime_type: Option<String>,
        }

        #[derive(Clone, PartialEq, ::prost::Message)]
        pub struct InferenceFilePart {
            #[prost(string, tag = "1")]
            pub data: String,
            #[prost(string, tag = "2")]
            pub media_type: String,
            #[prost(string, optional, tag = "3")]
            pub filename: Option<String>,
        }

        #[derive(Clone, PartialEq, ::prost::Message)]
        pub struct InferenceToolCall {
            #[prost(string, tag = "1")]
            pub tool_call_id: String,
            #[prost(string, tag = "2")]
            pub tool_name: String,
            #[prost(message, optional, tag = "3")]
            pub args: Option<prost_types::Struct>,
            #[prost(string, optional, tag = "4")]
            pub raw_tool_call_args: Option<String>,
        }

        #[derive(Clone, PartialEq, ::prost::Message)]
        pub struct InferenceToolResultContent {
            #[prost(message, repeated, tag = "1")]
            pub parts: Vec<InferenceToolResultPart>,
        }

        #[derive(Clone, PartialEq, ::prost::Message)]
        pub struct InferenceToolResultPart {
            #[prost(string, tag = "1")]
            pub tool_call_id: String,
            #[prost(string, tag = "2")]
            pub tool_name: String,
            #[prost(message, optional, tag = "3")]
            pub result: Option<prost_types::Value>,
            #[prost(bool, tag = "4")]
            pub is_error: bool,
            #[prost(message, repeated, tag = "5")]
            pub experimental_content: Vec<InferenceContentPart>,
        }

        #[derive(Clone, PartialEq, ::prost::Message)]
        pub struct InferenceReasoningPart {
            #[prost(bool, tag = "1")]
            pub is_redacted: bool,
            #[prost(string, tag = "2")]
            pub text: String,
            #[prost(string, optional, tag = "3")]
            pub signature: Option<String>,
            #[prost(string, optional, tag = "4")]
            pub redacted_data: Option<String>,
            #[prost(string, optional, tag = "5")]
            pub model_name: Option<String>,
        }

        #[derive(Clone, PartialEq, ::prost::Message)]
        pub struct RunInferenceClientMessage {
            #[prost(oneof = "run_inference_client_message::Message", tags = "1,2,3,4,5")]
            pub message: Option<run_inference_client_message::Message>,
        }

        pub mod run_inference_client_message {
            #[derive(Clone, PartialEq, ::prost::Oneof)]
            pub enum Message {
                #[prost(message, tag = "1")]
                RunRequest(super::RunInferenceRunRequest),
                #[prost(message, tag = "2")]
                InvokeModel(super::RunInferenceInvokeModel),
                #[prost(message, tag = "3")]
                CancelInvocation(super::RunInferenceCancelInvocation),
                #[prost(message, tag = "4")]
                FinishRun(super::RunInferenceFinishRun),
                #[prost(message, tag = "5")]
                ClientHeartbeat(super::RunInferenceClientHeartbeat),
            }
        }

        #[derive(Clone, PartialEq, ::prost::Message)]
        pub struct RunInferenceRunRequest {
            #[prost(string, tag = "1")]
            pub conversation_id: String,
            #[prost(string, optional, tag = "2")]
            pub conversation_group_id: Option<String>,
            #[prost(message, optional, tag = "3")]
            pub requested_model: Option<InferenceRequestedModel>,
            #[prost(message, repeated, tag = "4")]
            pub routing_conversation: Vec<RunInferenceRoutingMessage>,
            #[prost(string, optional, tag = "5")]
            pub agent_mode: Option<String>,
            #[prost(string, optional, tag = "6")]
            pub subagent_type_name: Option<String>,
            #[prost(message, repeated, tag = "7")]
            pub selected_subagent_models: Vec<InferenceRequestedModel>,
            #[prost(message, optional, tag = "11")]
            pub agent_module: Option<RunInferenceAgentModule>,
            #[prost(string, optional, tag = "12")]
            pub previous_request_id: Option<String>,
        }

        #[derive(Clone, PartialEq, ::prost::Message)]
        pub struct RunInferenceRoutingMessage {
            #[prost(enumeration = "run_inference_routing_role::Role", tag = "1")]
            pub role: i32,
            #[prost(string, tag = "2")]
            pub text: String,
        }

        pub mod run_inference_routing_role {
            #[derive(
                Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration,
            )]
            #[repr(i32)]
            pub enum Role {
                Unspecified = 0,
                User = 1,
                Assistant = 2,
            }
        }

        #[derive(Clone, PartialEq, ::prost::Message)]
        pub struct RunInferenceAgentModule {
            #[prost(string, tag = "1")]
            pub generation: String,
            #[prost(enumeration = "run_inference_agent_module_source::Source", tag = "2")]
            pub source: i32,
            #[prost(string, tag = "3")]
            pub channel: String,
            #[prost(string, optional, tag = "4")]
            pub commit: Option<String>,
            #[prost(uint32, tag = "5")]
            pub interface_revision: u32,
            #[prost(string, repeated, tag = "6")]
            pub prompt_versions: Vec<String>,
        }

        pub mod run_inference_agent_module_source {
            #[derive(
                Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration,
            )]
            #[repr(i32)]
            pub enum Source {
                Unspecified = 0,
                Static = 1,
                Installed = 3,
            }
        }

        #[derive(Clone, PartialEq, ::prost::Message)]
        pub struct RunInferenceInvokeModel {
            #[prost(string, tag = "1")]
            pub invocation_id: String,
            #[prost(message, optional, tag = "2")]
            pub request: Option<InferenceStreamRequest>,
            #[prost(bytes = "vec", tag = "6")]
            pub config_fingerprint: Vec<u8>,
            #[prost(message, optional, tag = "7")]
            pub compressed_request: Option<RunInferenceCompressedRequest>,
        }

        #[derive(Clone, PartialEq, ::prost::Message)]
        pub struct RunInferenceCompressedRequest {
            #[prost(enumeration = "run_inference_request_encoding::Encoding", tag = "1")]
            pub encoding: i32,
            #[prost(uint32, tag = "2")]
            pub decoded_size: u32,
            #[prost(bytes = "vec", tag = "3")]
            pub data: Vec<u8>,
        }

        pub mod run_inference_request_encoding {
            #[derive(
                Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration,
            )]
            #[repr(i32)]
            pub enum Encoding {
                Unspecified = 0,
                Zstd = 1,
                Gzip = 2,
            }
        }

        #[derive(Clone, PartialEq, ::prost::Message)]
        pub struct RunInferenceCancelInvocation {
            #[prost(string, tag = "1")]
            pub invocation_id: String,
        }

        #[derive(Clone, PartialEq, ::prost::Message)]
        pub struct RunInferenceClientHeartbeat {}

        #[derive(Clone, PartialEq, ::prost::Message)]
        pub struct RunInferenceFinishRun {
            #[prost(bool, tag = "1")]
            pub aborted: bool,
            #[prost(bool, optional, tag = "2")]
            pub turn_completed: Option<bool>,
        }

        #[derive(Clone, PartialEq, ::prost::Message)]
        pub struct RunInferenceServerMessage {
            #[prost(oneof = "run_inference_server_message::Message", tags = "1,2,3,4,5")]
            pub message: Option<run_inference_server_message::Message>,
        }

        pub mod run_inference_server_message {
            #[allow(clippy::large_enum_variant)]
            #[derive(Clone, PartialEq, ::prost::Oneof)]
            pub enum Message {
                #[prost(message, tag = "1")]
                Heartbeat(super::RunInferenceHeartbeat),
                #[prost(message, tag = "2")]
                RunReady(super::RunInferenceRunReady),
                #[prost(message, tag = "3")]
                InvocationResponse(super::RunInferenceInvocationResponse),
                #[prost(message, tag = "4")]
                InvocationEnd(super::RunInferenceInvocationEnd),
                #[prost(message, tag = "5")]
                SubagentModelsResolved(super::RunInferenceSubagentModelsResolved),
            }
        }

        #[derive(Clone, PartialEq, ::prost::Message)]
        pub struct RunInferenceHeartbeat {
            #[prost(bool, tag = "1")]
            pub accepts_speculative_invocations: bool,
        }

        #[derive(Clone, PartialEq, ::prost::Message)]
        pub struct RunInferenceRunReady {
            #[prost(message, optional, tag = "1")]
            pub resolved_model: Option<InferenceRequestedModel>,
            #[prost(bool, tag = "2")]
            pub supports_self_summary: bool,
            #[prost(message, optional, tag = "4")]
            pub prompt_model_metadata: Option<RunInferencePromptModelMetadata>,
            #[prost(bool, tag = "7")]
            pub accepts_client_heartbeat: bool,
            #[prost(bool, tag = "8")]
            pub accepts_agent_events: bool,
            #[prost(bool, tag = "13")]
            pub accepts_aborted_finish: bool,
            #[prost(bool, tag = "15")]
            pub accepts_tool_call_events: bool,
            #[prost(bool, tag = "16")]
            pub accepts_client_first_call_timings: bool,
            #[prost(bytes = "vec", tag = "17")]
            pub config_fingerprint: Vec<u8>,
            #[prost(bool, tag = "18")]
            pub accepts_delta_invocations: bool,
        }

        #[derive(Clone, PartialEq, ::prost::Message)]
        pub struct RunInferencePromptModelMetadata {
            #[prost(string, tag = "1")]
            pub vendor: String,
            #[prost(string, tag = "2")]
            pub prompt_version: String,
            #[prost(bool, tag = "31")]
            pub use_dsv3_harness: bool,
            #[prost(int64, optional, tag = "32")]
            pub agent_token_limit: Option<i64>,
            #[prost(int64, optional, tag = "33")]
            pub estimated_cache_ttl_ms: Option<i64>,
            #[prost(string, tag = "35")]
            pub persona: String,
            #[prost(map = "string, message", tag = "36")]
            pub feature_flags: HashMap<String, RunInferenceFlagValue>,
            #[prost(message, optional, tag = "37")]
            pub loop_nudge: Option<RunInferenceLoopNudgeConfig>,
            #[prost(bool, tag = "40")]
            pub supports_assistant_message_prefill: bool,
            #[prost(bool, tag = "41")]
            pub enable_line_numbers: bool,
            #[prost(bool, tag = "42")]
            pub use_sparse_read_line_numbers: bool,
            #[prost(string, tag = "44")]
            pub self_identity_name: String,
            #[prost(bool, optional, tag = "45")]
            pub dsv3_is_thinking: Option<bool>,
            #[prost(message, repeated, tag = "46")]
            pub agent_mode_configs: Vec<RunInferenceAgentModeConfig>,
            #[prost(bool, optional, tag = "47")]
            pub use_new_plan_mode_prompts: Option<bool>,
            #[prost(message, optional, tag = "48")]
            pub ask_question_config: Option<RunInferenceAskQuestionConfig>,
            #[prost(bool, optional, tag = "52")]
            pub enable_semantic_search: Option<bool>,
            #[prost(bool, optional, tag = "54")]
            pub smart_mode_classifier_mode: Option<bool>,
            #[prost(bool, optional, tag = "55")]
            pub smart_mode_classifier_shadow_mode: Option<bool>,
        }

        #[derive(Clone, PartialEq, ::prost::Message)]
        pub struct RunInferenceFlagValue {
            #[prost(oneof = "run_inference_flag_value::Value", tags = "1,2,3")]
            pub value: Option<run_inference_flag_value::Value>,
        }

        pub mod run_inference_flag_value {
            #[derive(Clone, PartialEq, ::prost::Oneof)]
            pub enum Value {
                #[prost(bool, tag = "1")]
                BoolValue(bool),
                #[prost(double, tag = "2")]
                NumberValue(f64),
                #[prost(string, tag = "3")]
                StringValue(String),
            }
        }

        #[derive(Clone, PartialEq, ::prost::Message)]
        pub struct RunInferenceLoopNudgeConfig {
            #[prost(int32, optional, tag = "1")]
            pub min_repetitions: Option<i32>,
            #[prost(int32, optional, tag = "2")]
            pub min_message_length: Option<i32>,
            #[prost(bool, tag = "3")]
            pub inject_reminder: bool,
        }

        #[derive(Clone, PartialEq, ::prost::Message)]
        pub struct RunInferenceAgentModeConfig {
            #[prost(string, tag = "1")]
            pub mode_id: String,
            #[prost(bool, tag = "2")]
            pub enable_ask_question_tool: bool,
            #[prost(message, optional, tag = "3")]
            pub switch_mode_tool_config: Option<RunInferenceSwitchModeToolConfig>,
        }

        #[derive(Clone, PartialEq, ::prost::Message)]
        pub struct RunInferenceSwitchModeToolConfig {
            #[prost(string, repeated, tag = "1")]
            pub target_modes: Vec<String>,
        }

        #[derive(Clone, PartialEq, ::prost::Message)]
        pub struct RunInferenceAskQuestionConfig {
            #[prost(string, tag = "1")]
            pub tool_name: String,
            #[prost(bool, tag = "2")]
            pub auto_reject_first: bool,
        }

        #[derive(Clone, PartialEq, ::prost::Message)]
        pub struct RunInferenceInvocationResponse {
            #[prost(string, tag = "1")]
            pub invocation_id: String,
            #[prost(message, optional, tag = "2")]
            pub response: Option<InferenceStreamResponse>,
        }

        #[derive(Clone, PartialEq, ::prost::Message)]
        pub struct RunInferenceInvocationEnd {
            #[prost(string, tag = "1")]
            pub invocation_id: String,
            #[prost(message, optional, tag = "2")]
            pub error: Option<RunInferenceInvocationError>,
            #[prost(int32, optional, tag = "3")]
            pub delta_refused: Option<i32>,
        }

        #[derive(Clone, PartialEq, ::prost::Message)]
        pub struct RunInferenceInvocationError {
            #[prost(int32, tag = "1")]
            pub code: i32,
            #[prost(string, tag = "2")]
            pub message: String,
        }

        #[derive(Clone, PartialEq, ::prost::Message)]
        pub struct RunInferenceSubagentModelsResolved {
            #[prost(string, tag = "1")]
            pub resolution_id: String,
            #[prost(message, repeated, tag = "2")]
            pub models: Vec<RunInferenceResolvedSubagentModel>,
        }

        #[derive(Clone, PartialEq, ::prost::Message)]
        pub struct RunInferenceResolvedSubagentModel {
            #[prost(string, tag = "1")]
            pub requested: String,
            #[prost(string, optional, tag = "2")]
            pub resolved_slug: Option<String>,
        }

        #[derive(Clone, PartialEq, ::prost::Message)]
        pub struct InferenceStreamResponse {
            #[prost(
                oneof = "inference_stream_response::Response",
                tags = "1,2,3,4,5,7,8,9"
            )]
            pub response: Option<inference_stream_response::Response>,
        }

        pub mod inference_stream_response {
            #[derive(Clone, PartialEq, ::prost::Oneof)]
            pub enum Response {
                #[prost(message, tag = "1")]
                TextPart(super::InferenceTextStreamPart),
                #[prost(message, tag = "2")]
                ToolCallPart(super::InferenceToolCallStreamPart),
                #[prost(message, tag = "3")]
                Usage(super::InferenceUsageInfo),
                #[prost(message, tag = "4")]
                ResponseInfo(super::InferenceResponseInfo),
                #[prost(message, tag = "5")]
                ExtendedUsage(super::InferenceExtendedUsageInfo),
                #[prost(message, tag = "7")]
                InvocationId(super::InferenceInvocationIdInfo),
                #[prost(message, tag = "8")]
                Error(super::InferenceStreamError),
                #[prost(message, tag = "9")]
                ThinkingPart(super::InferenceThinkingStreamPart),
            }
        }

        #[derive(Clone, PartialEq, ::prost::Message)]
        pub struct InferenceTextStreamPart {
            #[prost(string, tag = "1")]
            pub text: String,
            #[prost(bool, tag = "2")]
            pub is_final: bool,
        }

        #[derive(Clone, PartialEq, ::prost::Message)]
        pub struct InferenceThinkingStreamPart {
            #[prost(string, tag = "1")]
            pub text: String,
            #[prost(string, optional, tag = "2")]
            pub signature: Option<String>,
            #[prost(bool, tag = "3")]
            pub is_final: bool,
        }

        #[derive(Clone, PartialEq, ::prost::Message)]
        pub struct InferenceToolCallStreamPart {
            #[prost(string, tag = "1")]
            pub tool_call_id: String,
            #[prost(string, tag = "2")]
            pub tool_name: String,
            #[prost(string, tag = "3")]
            pub args: String,
            #[prost(bool, tag = "4")]
            pub is_complete: bool,
            #[prost(int32, optional, tag = "5")]
            pub tool_index: Option<i32>,
        }

        #[derive(Clone, PartialEq, ::prost::Message)]
        pub struct InferenceUsageInfo {
            #[prost(int32, tag = "1")]
            pub prompt_tokens: i32,
            #[prost(int32, tag = "2")]
            pub completion_tokens: i32,
            #[prost(int32, optional, tag = "3")]
            pub total_tokens: Option<i32>,
        }

        #[derive(Clone, PartialEq, ::prost::Message)]
        pub struct InferenceExtendedUsageInfo {
            #[prost(int32, tag = "1")]
            pub input_tokens: i32,
            #[prost(int32, tag = "2")]
            pub output_tokens: i32,
            #[prost(int32, tag = "3")]
            pub cache_read_tokens: i32,
            #[prost(int32, tag = "4")]
            pub cache_write_tokens: i32,
            #[prost(int32, tag = "5")]
            pub max_tokens: i32,
        }

        #[derive(Clone, PartialEq, ::prost::Message)]
        pub struct InferenceInvocationIdInfo {
            #[prost(string, tag = "1")]
            pub invocation_id: String,
        }

        #[derive(Clone, PartialEq, ::prost::Message)]
        pub struct InferenceStreamError {
            #[prost(string, tag = "1")]
            pub message: String,
            #[prost(string, tag = "2")]
            pub code: String,
            #[prost(bool, tag = "3")]
            pub is_input_token_limit_error: bool,
            #[prost(bool, tag = "4")]
            pub is_output_token_limit_error: bool,
            #[prost(enumeration = "inference_stream_error_type::ErrorType", tag = "5")]
            pub error_type: i32,
        }

        pub mod inference_stream_error_type {
            #[derive(
                Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration,
            )]
            #[repr(i32)]
            pub enum ErrorType {
                Unspecified = 0,
                Unknown = 1,
                InputTokenLimit = 2,
                OutputTokenLimit = 3,
                RateLimit = 4,
                Authentication = 5,
                Permission = 6,
                Overloaded = 7,
                ContentFilter = 8,
            }
        }

        #[derive(Clone, PartialEq, ::prost::Message)]
        pub struct InferenceResponseInfo {
            #[prost(string, tag = "1")]
            pub id: String,
            #[prost(string, tag = "2")]
            pub model: String,
            #[prost(int64, tag = "3")]
            pub created_at: i64,
            #[prost(string, optional, tag = "5")]
            pub error_message: Option<String>,
        }
    }
}
