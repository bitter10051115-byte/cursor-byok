//! Accepts ordered Cursor Bidi append requests and routes them by request_id.
use prost::Message;

use crate::{
    api::cursor::run_inference::{self, RunInferenceState},
    cursor::{
        conversation::TransportCommand,
        protocol::{
            events,
            proto::{agent::v1 as agent, aiserver::v1 as ai},
        },
        transport::{TransportParent, TransportRegistry},
    },
    Error, Result,
};

pub struct DecodedAppend {
    pub request_id: String,
    pub seqno: i64,
    pub message: DecodedMessage,
}

pub enum DecodedMessage {
    Agent(Box<agent::AgentClientMessage>),
    Inference(Box<ai::RunInferenceClientMessage>),
}

impl DecodedAppend {
    pub fn model_id(&self) -> Option<&str> {
        match &self.message {
            DecodedMessage::Agent(message) => {
                let agent::agent_client_message::Message::RunRequest(request) =
                    message.message.as_ref()?
                else {
                    return None;
                };
                request
                    .requested_model
                    .as_ref()
                    .map(|model| model.model_id.as_str())
                    .filter(|model| !model.is_empty())
                    .or_else(|| {
                        request
                            .model_details
                            .as_ref()
                            .map(|model| model.model_id.as_str())
                            .filter(|model| !model.is_empty())
                    })
            }
            DecodedMessage::Inference(message) => {
                let ai::run_inference_client_message::Message::RunRequest(request) =
                    message.message.as_ref()?
                else {
                    return None;
                };
                request
                    .requested_model
                    .as_ref()
                    .map(|model| model.model_id.as_str())
                    .filter(|model| !model.is_empty())
            }
        }
    }

    pub fn conversation_id(&self) -> Option<&str> {
        match &self.message {
            DecodedMessage::Agent(message) => {
                let agent::agent_client_message::Message::RunRequest(request) =
                    message.message.as_ref()?
                else {
                    return None;
                };
                request.conversation_id.as_deref()
            }
            DecodedMessage::Inference(message) => {
                let ai::run_inference_client_message::Message::RunRequest(request) =
                    message.message.as_ref()?
                else {
                    return None;
                };
                (!request.conversation_id.is_empty()).then_some(request.conversation_id.as_str())
            }
        }
    }

    pub fn is_background_task_completion(&self) -> bool {
        match &self.message {
            DecodedMessage::Agent(message) => {
                let Some(agent::agent_client_message::Message::RunRequest(request)) =
                    message.message.as_ref()
                else {
                    return false;
                };
                matches!(
                    request
                        .action
                        .as_ref()
                        .and_then(|action| action.action.as_ref()),
                    Some(agent::conversation_action::Action::BackgroundTaskCompletionAction(_))
                )
            }
            DecodedMessage::Inference(_) => false,
        }
    }

    pub fn trace_metadata(&self) -> serde_json::Value {
        let DecodedMessage::Agent(message) = &self.message else {
            return inference_trace_metadata(self);
        };
        let Some(message) = message.message.as_ref() else {
            return serde_json::json!({
                "append_seqno": self.seqno,
                "message_type": "empty",
            });
        };
        let agent::agent_client_message::Message::RunRequest(request) = message else {
            return serde_json::json!({
                "append_seqno": self.seqno,
                "message_type": client_message_type(message),
            });
        };
        let (action_type, history_messages, history_images) = request
            .action
            .as_ref()
            .and_then(|action| action.action.as_ref())
            .map(|action| match action {
                agent::conversation_action::Action::UserMessageAction(action) => {
                    let history = action.conversation_history.as_ref();
                    (
                        "user_message",
                        history.map_or(0, |history| history.messages.len()),
                        history.map_or(0, history_image_count),
                    )
                }
                agent::conversation_action::Action::BackgroundTaskCompletionAction(_) => {
                    ("background_task_completion", 0, 0)
                }
                agent::conversation_action::Action::ExecutePlanAction(_) => ("execute_plan", 0, 0),
                agent::conversation_action::Action::SummarizeAction(_) => ("summarize", 0, 0),
                _ => ("other", 0, 0),
            })
            .unwrap_or(("none", 0, 0));
        let state = request.conversation_state.as_ref();
        serde_json::json!({
            "append_seqno": self.seqno,
            "message_type": "run_request",
            "conversation_id": request.conversation_id,
            "model_id": self.model_id(),
            "action_type": action_type,
            "conversation_history_messages": history_messages,
            "conversation_history_images": history_images,
            "root_message_count": state.map_or(0, |state| state.root_prompt_messages_json.len()),
            "turn_count": state.map_or(0, |state| state.turns.len()),
            "prefetched_blob_count": request.pre_fetched_blobs.len(),
        })
    }
}

fn inference_trace_metadata(append: &DecodedAppend) -> serde_json::Value {
    let DecodedMessage::Inference(message) = &append.message else {
        return serde_json::json!({
            "append_seqno": append.seqno,
            "message_type": "unknown",
        });
    };
    let Some(ai::run_inference_client_message::Message::RunRequest(request)) =
        message.message.as_ref()
    else {
        return serde_json::json!({
            "append_seqno": append.seqno,
            "message_type": "run_inference_message",
        });
    };
    serde_json::json!({
        "append_seqno": append.seqno,
        "message_type": "run_inference_run_request",
        "conversation_id": request.conversation_id,
        "model_id": request.requested_model.as_ref().map(|model| model.model_id.as_str()),
        "routing_messages": request.routing_conversation.len(),
        "agent_module": request.agent_module.as_ref().map(|module| module.generation.as_str()),
    })
}

fn client_message_type(message: &agent::agent_client_message::Message) -> &'static str {
    use agent::agent_client_message::Message;
    match message {
        Message::RunRequest(_) => "run_request",
        Message::ExecClientMessage(_) => "exec_client_message",
        Message::ExecClientControlMessage(_) => "exec_client_control_message",
        Message::KvClientMessage(_) => "kv_client_message",
        Message::ConversationAction(_) => "conversation_action",
        Message::InteractionResponse(_) => "interaction_response",
        Message::ClientHeartbeat(_) => "client_heartbeat",
        Message::PrewarmRequest(_) => "prewarm_request",
    }
}

fn history_image_count(history: &agent::ConversationHistory) -> usize {
    use agent::{
        conversation_history_message::Message,
        conversation_history_tool_result_content::Content as ToolContent,
        conversation_history_user_content::Content as UserContent,
    };
    history
        .messages
        .iter()
        .map(|message| match message.message.as_ref() {
            Some(Message::User(user)) => user
                .content
                .iter()
                .filter(|content| matches!(content.content, Some(UserContent::Image(_))))
                .count(),
            Some(Message::Tool(tool)) => tool
                .content
                .iter()
                .filter(|content| matches!(content.content, Some(ToolContent::Image(_))))
                .count(),
            _ => 0,
        })
        .sum()
}

pub fn decode(request: &ai::BidiAppendRequest) -> Result<DecodedAppend> {
    let request_id = request
        .request_id
        .as_ref()
        .map(|id| id.request_id.as_str())
        .filter(|id| !id.is_empty())
        .ok_or_else(|| Error::Protocol("BidiAppend request_id is required".into()))?;
    let payload = message_bytes(request)?;
    let message = if let Ok(message) = agent::AgentClientMessage::decode(payload.as_ref()) {
        DecodedMessage::Agent(Box::new(message))
    } else if let Ok(message) = ai::RunInferenceClientMessage::decode(payload.as_ref()) {
        DecodedMessage::Inference(Box::new(message))
    } else {
        tracing::warn!(
            request_id = %request_id,
            append_seqno = request.append_seqno,
            payload_bytes = payload.len(),
            "BidiAppend contained neither AgentClientMessage nor RunInferenceClientMessage"
        );
        return Err(Error::Protocol(
            "BidiAppend contains no supported Cursor client message".into(),
        ));
    };
    Ok(DecodedAppend {
        request_id: request_id.into(),
        seqno: request.append_seqno,
        message,
    })
}

/// Cursor sends the client message in exactly one field. `data` is the hex
/// encoding used by older clients. `data_binary` is the same protobuf bytes,
/// selected when `bidi_append_binary_encoding_disabled` is off.
fn message_bytes(request: &ai::BidiAppendRequest) -> Result<std::borrow::Cow<'_, [u8]>> {
    let binary = request.data_binary.as_slice();
    if request.data.is_empty() {
        if binary.is_empty() {
            return Err(Error::Protocol(
                "BidiAppend contains no client message".into(),
            ));
        }
        return Ok(std::borrow::Cow::Borrowed(binary));
    }
    let decoded = hex::decode(&request.data)
        .map_err(|error| Error::Protocol(format!("invalid BidiAppend hex: {error}")))?;
    if !binary.is_empty() && binary != decoded.as_slice() {
        return Err(Error::Protocol(
            "BidiAppend data and data_binary contain different messages".into(),
        ));
    }
    Ok(std::borrow::Cow::Owned(decoded))
}

pub async fn append(
    registry: &TransportRegistry,
    state: &RunInferenceState,
    request: DecodedAppend,
    parent: Option<TransportParent>,
) -> Result<ai::BidiAppendResponse> {
    match request.message {
        DecodedMessage::Agent(message) => {
            let message = *message;
            append_agent(registry, request.request_id, request.seqno, message, parent).await
        }
        DecodedMessage::Inference(message) => {
            let message = *message;
            run_inference::append(
                registry,
                state,
                request.request_id,
                request.seqno,
                message,
                parent,
            )
            .await
        }
    }
}

async fn append_agent(
    registry: &TransportRegistry,
    request_id: String,
    seqno: i64,
    message: agent::AgentClientMessage,
    parent: Option<TransportParent>,
) -> Result<ai::BidiAppendResponse> {
    let replace_closing = matches!(
        message.message.as_ref(),
        Some(agent::agent_client_message::Message::RunRequest(request))
            if request.requested_model.is_some()
                || request.model_details.is_some()
    );
    let conversation_id = message.message.as_ref().and_then(|message| match message {
        agent::agent_client_message::Message::RunRequest(request) => {
            request.conversation_id.as_deref()
        }
        _ => None,
    });
    let handle = registry
        .get_or_create_for_append(&request_id, replace_closing)
        .await?;
    let _admission = handle.admit()?;
    if let Some(conversation_id) = conversation_id {
        handle.set_conversation_id(conversation_id)?;
    }
    if let Some(parent) = parent {
        handle.set_parent(parent)?;
    }
    if matches!(
        message.message.as_ref(),
        Some(agent::agent_client_message::Message::ClientHeartbeat(_))
    ) {
        handle.emit(&events::heartbeat())?;
    }
    handle
        .command(TransportCommand::Append {
            seqno,
            message: Box::new(message),
        })
        .await?;
    Ok(ai::BidiAppendResponse {})
}

#[cfg(test)]
mod tests {
    use super::*;

    // BidiAppend `data` payload captured from Cursor 3.27.22. The client no
    // longer sends agent.v1.AgentClientMessage; the first append is a
    // aiserver.v1 RunInferenceClientMessage whose field 1 is a plain UUID.
    const CAPTURED_RUN_INFERENCE_PAYLOAD: &str = "0a94090a2437633335336661342d353061382d343835622d383963312d6161376433353430386366641a450a10326163373230623733343332623236321a0f0a07636f6e7465787412043230306b1a110a09726561736f6e696e671204686967681a0d0a0466617374120566616c73652209080112056e6968616f2a056167656e743a090a0764656661756c743a430a0867726f6b2d342e371a0f0a07636f6e7465787412043235366b1a180a10726561736f6e696e675f6566666f72741204686967681a0c0a04666173741204747275653a280a0867726f6b2d342e361a0e0a066566666f72741204686967681a0c0a04666173741204747275653a1c0a0c636f6d706f7365722d322e351a0c0a04666173741204747275653a430a0f636c617564652d6f7075732d352d351a0f0a07636f6e7465787412043330306b1a100a066566666f727412066d656469756d1a0d0a0466617374120566616c73653a2c0a0e67656d696e692d342d6172676f6e1a1a0a10726561736f6e696e675f6566666f727412066d656469756d3a3c0a0f6d7573652d776f6e6465722d312e301a0f0a07636f6e7465787412043330306b1a180a10726561736f6e696e675f6566666f72741204686967683a420a0b6770742d352e362d736f6c1a0f0a07636f6e7465787412043237326b1a130a09726561736f6e696e6712066d656469756d1a0d0a0466617374120566616c73653a450a10636c617564652d6661626c652d352d311a100a087468696e6b696e671204747275651a0f0a07636f6e7465787412043330306b1a0e0a066566666f72741204686967683a3e0a11636c617564652d736f6e6e65742d352d351a0f0a07636f6e7465787412043330306b1a180a10726561736f6e696e675f6566666f72741204686967683a450a10653562646536303938623261326230351a0f0a07636f6e7465787412043830306b1a110a09726561736f6e696e671204686967681a0d0a0466617374120566616c73653a450a10326163373230623733343332623236321a0f0a07636f6e7465787412043230306b1a110a09726561736f6e696e671204686967681a0d0a0466617374120566616c73653a450a10366562623331663039306533313765321a0f0a07636f6e7465787412043230306b1a110a09726561736f6e696e671204686967681a0d0a0466617374120566616c73653a450a10386162646136393931666136663438301a0f0a07636f6e7465787412043230306b1a110a09726561736f6e696e671204686967681a0d0a0466617374120566616c736550015ac3010a406239353139313165343437363532353038626136346234623230396365623537363237313065313432336465646466373038326136626539336263613763626310031a06737461626c6522286237396630623139336539643532666237336333376134663665363934636566373462343863636128023209647376332d313031383209647376332d31323035320b637572736f722d3032323632066c6174657374320a677074352d636f646578320b636f6465782d636c6f756432056861696b7582010b756e737065636966696564880101900101a80101c80101d80101";

    #[test]
    fn decodes_captured_run_inference_append() {
        let request = ai::BidiAppendRequest {
            data: CAPTURED_RUN_INFERENCE_PAYLOAD.into(),
            request_id: Some(ai::BidiRequestId {
                request_id: "captured-run".into(),
            }),
            append_seqno: 1,
            data_binary: Vec::new(),
        };
        let decoded = decode(&request).expect("captured RunInference payload must decode");
        assert_eq!(decoded.request_id, "captured-run");
        let DecodedMessage::Inference(message) = decoded.message else {
            panic!("expected RunInferenceClientMessage");
        };
        let ai::run_inference_client_message::Message::RunRequest(request) =
            message.message.expect("RunRequest message")
        else {
            panic!("expected RunRequest");
        };
        assert_eq!(
            request.conversation_id,
            "7c353fa4-50a8-485b-89c1-aa7d35408cfd"
        );
        assert_eq!(
            request
                .requested_model
                .as_ref()
                .expect("requested_model")
                .model_id,
            "2ac720b73432b262"
        );
        assert!(request
            .selected_subagent_models
            .iter()
            .any(|model| model.model_id == "grok-4.7"));
    }

    #[test]
    fn decodes_the_same_run_inference_append_from_data_binary() {
        let request = ai::BidiAppendRequest {
            data: String::new(),
            request_id: Some(ai::BidiRequestId {
                request_id: "captured-run".into(),
            }),
            append_seqno: 1,
            data_binary: hex::decode(CAPTURED_RUN_INFERENCE_PAYLOAD).expect("fixture hex"),
        };
        let decoded = decode(&request).expect("binary RunInference payload must decode");
        let DecodedMessage::Inference(message) = decoded.message else {
            panic!("expected RunInferenceClientMessage");
        };
        let ai::run_inference_client_message::Message::RunRequest(request) =
            message.message.expect("RunRequest message")
        else {
            panic!("expected RunRequest");
        };
        assert_eq!(
            request.conversation_id,
            "7c353fa4-50a8-485b-89c1-aa7d35408cfd"
        );
        assert_eq!(
            request
                .requested_model
                .as_ref()
                .expect("requested_model")
                .model_id,
            "2ac720b73432b262"
        );
    }

    #[test]
    fn rejects_data_binary_that_disagrees_with_hex_data() {
        let mut binary = hex::decode(CAPTURED_RUN_INFERENCE_PAYLOAD).expect("fixture hex");
        binary[0] ^= 0xff;
        let request = ai::BidiAppendRequest {
            data: CAPTURED_RUN_INFERENCE_PAYLOAD.into(),
            request_id: Some(ai::BidiRequestId {
                request_id: "captured-run".into(),
            }),
            append_seqno: 1,
            data_binary: binary,
        };
        let error = decode(&request).expect_err("disagreeing payloads must be rejected");
        assert!(error.to_string().contains("different messages"), "{error}");
    }
}
