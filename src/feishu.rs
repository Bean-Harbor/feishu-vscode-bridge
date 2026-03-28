use prost::Message;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tungstenite::connect;
use tungstenite::Message as WsMessage;

const TOKEN_URL: &str =
    "https://open.feishu.cn/open-apis/auth/v3/tenant_access_token/internal";
const WS_ENDPOINT_URL: &str = "https://open.feishu.cn/callback/ws/endpoint";
const MSG_URL: &str = "https://open.feishu.cn/open-apis/im/v1/messages";

// ──────────────────────────── protobuf types ────────────────────────────
// 飞书 WS 协议使用 pbbp2.proto 定义的 Frame / Header

#[derive(Clone, PartialEq, Message)]
pub struct PbHeader {
    #[prost(string, required, tag = 1)]
    pub key: String,
    #[prost(string, required, tag = 2)]
    pub value: String,
}

#[derive(Clone, PartialEq, Message)]
pub struct PbFrame {
    #[prost(uint64, required, tag = 1)]
    pub seq_id: u64,
    #[prost(uint64, required, tag = 2)]
    pub log_id: u64,
    #[prost(int32, required, tag = 3)]
    pub service: i32,
    #[prost(int32, required, tag = 4)]
    pub method: i32,
    #[prost(message, repeated, tag = 5)]
    pub headers: Vec<PbHeader>,
    #[prost(string, optional, tag = 6)]
    pub payload_encoding: Option<String>,
    #[prost(string, optional, tag = 7)]
    pub payload_type: Option<String>,
    #[prost(bytes = "vec", optional, tag = 8)]
    pub payload: Option<Vec<u8>>,
    #[prost(string, optional, tag = 9)]
    pub log_id_new: Option<String>,
}

impl PbFrame {
    fn get_header(&self, key: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|h| h.key == key)
            .map(|h| h.value.as_str())
    }
}

// ──────────────────────────── public types ────────────────────────────

pub struct FeishuClient {
    app_id: String,
    app_secret: String,
    token: Option<String>,
}

/// 从飞书 WebSocket 收到的一条用户消息
#[derive(Debug, Clone)]
pub struct InboundMessage {
    pub reply_target: ReplyTarget,
    pub chat_id: String,
    pub chat_type: String,
    pub sender_id: String,
    pub text: String,
    pub message_id: String,
}

#[derive(Debug, Clone)]
pub struct ReplyTarget {
    pub receive_id: String,
    pub receive_id_type: String,
}

#[derive(Debug, Clone)]
pub struct CardActionEvent {
    pub reply_target: ReplyTarget,
    pub sender_id: String,
    pub action_command: String,
    pub event_id: String,
}

#[derive(Debug, Clone)]
pub enum FeishuEvent {
    Message(InboundMessage),
    CardAction(CardActionEvent),
}

impl FeishuEvent {
    pub fn dedup_id(&self) -> &str {
        match self {
            FeishuEvent::Message(message) => &message.message_id,
            FeishuEvent::CardAction(action) => &action.event_id,
        }
    }
}

// ──────────────────────────── API payloads ────────────────────────────

#[derive(Serialize)]
struct TokenReq<'a> {
    app_id: &'a str,
    app_secret: &'a str,
}

#[derive(Deserialize)]
struct TokenResp {
    code: i64,
    msg: String,
    tenant_access_token: Option<String>,
}

#[derive(Deserialize)]
struct WsEndpointResp {
    code: Option<i64>,
    data: Option<WsEndpointData>,
}
#[derive(Deserialize)]
struct WsEndpointData {
    #[serde(rename = "URL")]
    url: Option<String>,
}

#[derive(Serialize)]
struct MsgBody<'a> {
    receive_id: &'a str,
    msg_type: &'a str,
    content: &'a str,
}

#[derive(Deserialize)]
struct MsgResp {
    code: i64,
    msg: String,
}

// ──────────────────────────── implementation ────────────────────────────

impl FeishuClient {
    pub fn from_env() -> Result<Self, String> {
        dotenvy::dotenv().ok();
        let app_id = std::env::var("FEISHU_APP_ID")
            .map_err(|_| "环境变量 FEISHU_APP_ID 未设置，请先运行 setup-gui 进行配置".to_string())?;
        let app_secret = std::env::var("FEISHU_APP_SECRET")
            .map_err(|_| "环境变量 FEISHU_APP_SECRET 未设置".to_string())?;
        Ok(Self {
            app_id,
            app_secret,
            token: None,
        })
    }

    /// 获取 tenant_access_token
    pub fn authenticate(&mut self) -> Result<(), String> {
        let body = TokenReq {
            app_id: &self.app_id,
            app_secret: &self.app_secret,
        };
        let resp: TokenResp = ureq::post(TOKEN_URL)
            .send_json(&body)
            .map_err(|e| format!("请求 token 失败: {e}"))?
            .into_json()
            .map_err(|e| format!("解析 token 响应失败: {e}"))?;

        if resp.code != 0 {
            return Err(format!("获取 token 失败 (code={}): {}", resp.code, resp.msg));
        }
        self.token = resp.tenant_access_token;
        Ok(())
    }

    fn token(&self) -> Result<&str, String> {
        self.token
            .as_deref()
            .ok_or_else(|| "尚未认证，请先调用 authenticate()".to_string())
    }

    /// 向指定目标发送文本消息
    pub fn send_text_to(
        &self,
        receive_id: &str,
        receive_id_type: &str,
        text: &str,
    ) -> Result<(), String> {
        let token = self.token()?;
        let content = serde_json::json!({ "text": text }).to_string();
        let body = MsgBody {
            receive_id,
            msg_type: "text",
            content: &content,
        };
        let url = format!("{MSG_URL}?receive_id_type={receive_id_type}");
        let resp: MsgResp = ureq::post(&url)
            .set("Authorization", &format!("Bearer {token}"))
            .send_json(&body)
            .map_err(|e| format!("发送消息失败: {e}"))?
            .into_json()
            .map_err(|e| format!("解析发送响应失败: {e}"))?;

        if resp.code != 0 {
            return Err(format!("发送消息失败 (code={}): {}", resp.code, resp.msg));
        }
        Ok(())
    }

    pub fn send_card_to(
        &self,
        receive_id: &str,
        receive_id_type: &str,
        card: &Value,
    ) -> Result<(), String> {
        let token = self.token()?;
        let content = serde_json::to_string(card)
            .map_err(|e| format!("序列化卡片失败: {e}"))?;
        let body = MsgBody {
            receive_id,
            msg_type: "interactive",
            content: &content,
        };
        let url = format!("{MSG_URL}?receive_id_type={receive_id_type}");
        let resp: MsgResp = ureq::post(&url)
            .set("Authorization", &format!("Bearer {token}"))
            .send_json(&body)
            .map_err(|e| format!("发送卡片失败: {e}"))?
            .into_json()
            .map_err(|e| format!("解析卡片响应失败: {e}"))?;

        if resp.code != 0 {
            return Err(format!("发送卡片失败 (code={}): {}", resp.code, resp.msg));
        }
        Ok(())
    }

    /// 回复一条入站消息（自动使用消息中的 chat_id）
    pub fn reply(&self, inbound: &InboundMessage, text: &str) -> Result<(), String> {
        self.send_text_to(
            &inbound.reply_target.receive_id,
            &inbound.reply_target.receive_id_type,
            text,
        )
    }

    pub fn reply_card(&self, target: &ReplyTarget, card: &Value) -> Result<(), String> {
        self.send_card_to(&target.receive_id, &target.receive_id_type, card)
    }

    // ── WebSocket 长连接 ──

    /// 获取飞书 WebSocket 连接地址
    fn get_ws_url(&self) -> Result<(String, i32), String> {
        let body = serde_json::json!({
            "AppID": self.app_id,
            "AppSecret": self.app_secret,
        });
        let resp_str = ureq::post(WS_ENDPOINT_URL)
            .set("locale", "zh")
            .send_json(&body)
            .map_err(|e| format!("请求 WS endpoint 失败: {e}"))?
            .into_string()
            .map_err(|e| format!("读取 WS endpoint 响应失败: {e}"))?;

        let resp: WsEndpointResp = serde_json::from_str(&resp_str)
            .map_err(|e| format!("解析 WS endpoint 响应失败: {e}"))?;

        let code = resp.code.unwrap_or(-1);
        if code != 0 {
            return Err(format!("获取 WS endpoint 失败 (code={code}): {resp_str}"));
        }
        let url = resp
            .data
            .and_then(|d| d.url)
            .ok_or_else(|| "WS endpoint 响应中没有 URL".to_string())?;

        // 从 URL 中提取 service_id 参数
        let service_id = url
            .split("service_id=")
            .nth(1)
            .and_then(|s| s.split('&').next())
            .and_then(|s| s.parse::<i32>().ok())
            .unwrap_or(0);

        Ok((url, service_id))
    }

    /// 构建响应帧（处理完消息后回 ACK）
    fn build_response_frame(req_frame: &PbFrame) -> Vec<u8> {
        let mut resp_headers = req_frame.headers.clone();
        // 添加 biz_rt header
        resp_headers.push(PbHeader {
            key: "biz_rt".to_string(),
            value: "0".to_string(),
        });
        let frame = PbFrame {
            seq_id: req_frame.seq_id,
            log_id: req_frame.log_id,
            service: req_frame.service,
            method: req_frame.method,
            headers: resp_headers,
            payload_encoding: None,
            payload_type: None,
            payload: Some(br#"{"code":200}"#.to_vec()),
            log_id_new: None,
        };
        frame.encode_to_vec()
    }

    /// 启动 WebSocket 长连接，持续监听飞书消息。
    /// 收到消息时调用 handler(client, event)。
    /// 此函数会阻塞当前线程。
    pub fn listen<F>(&mut self, mut handler: F) -> Result<(), String>
    where
        F: FnMut(&FeishuClient, FeishuEvent),
    {
        println!("🔗 正在获取 WebSocket 连接地址...");
        let (ws_url, service_id) = self.get_ws_url()?;
        println!("🔗 连接到飞书 WebSocket...");

        let (mut socket, _response) =
            connect(&ws_url).map_err(|e| format!("WebSocket 连接失败: {e}"))?;
        println!("✅ WebSocket 已连接，等待飞书消息...");

        loop {
            let msg = socket
                .read()
                .map_err(|e| format!("WebSocket 读取失败: {e}"))?;

            match msg {
                WsMessage::Binary(data) => {
                    match PbFrame::decode(data.as_slice()) {
                        Ok(frame) => {
                            let method = frame.method; // 0=CONTROL, 1=DATA
                            let msg_type = frame.get_header("type").unwrap_or("");

                            if method == 0 {
                                // CONTROL frame (ping/pong)
                                if msg_type == "ping" {
                                    // 飞书发来 ping，不需要额外处理
                                    // 我们主动发 ping 来保活
                                }
                            } else if method == 1 && msg_type == "event" {
                                // DATA frame — 事件消息
                                if let Some(payload) = &frame.payload {
                                    let payload_str =
                                        String::from_utf8_lossy(payload);

                                    if let Some(inbound) =
                                        Self::parse_event_payload(&payload_str)
                                    {
                                        handler(self, inbound);
                                    } else {
                                        eprintln!(
                                            "[DEBUG] 未识别的飞书事件 payload: {}",
                                            payload_str
                                        );
                                    }
                                }
                                // 发送 ACK 响应
                                let resp_bytes = Self::build_response_frame(&frame);
                                let _ = socket.send(WsMessage::Binary(resp_bytes));
                            }
                        }
                        Err(e) => {
                            eprintln!(
                                "[DEBUG] protobuf decode failed: {e}, raw: {:?}",
                                String::from_utf8_lossy(&data)
                            );
                        }
                    }
                }
                WsMessage::Ping(data) => {
                    let _ = socket.send(WsMessage::Pong(data));
                }
                WsMessage::Close(reason) => {
                    println!("⚠️  WebSocket 连接已关闭: {reason:?}");
                    break;
                }
                _ => {}
            }

            let _ = service_id;
        }
        Ok(())
    }

    /// 解析飞书事件 payload（JSON），提取用户消息
    fn parse_event_payload(payload: &str) -> Option<FeishuEvent> {
        let val: serde_json::Value = serde_json::from_str(payload).ok()?;

        let event_type = val
            .pointer("/header/event_type")
            .or_else(|| val.pointer("/schema"))
            .and_then(|v| v.as_str())
            .unwrap_or("");

        if !event_type.is_empty() {
            println!("📨 收到飞书事件类型: {}", event_type);
        }

        if event_type == "card.action.trigger" || val.pointer("/event/action").is_some() {
            return Self::parse_card_action_event(&val).map(FeishuEvent::CardAction);
        }

        Self::parse_message_event(&val).map(FeishuEvent::Message)
    }

    fn parse_message_event(val: &serde_json::Value) -> Option<InboundMessage> {
        let event = val.get("event")?;
        let message = event.get("message")?;

        let chat_id = message.get("chat_id")?.as_str()?.to_string();
        let chat_type = message
            .get("chat_type")
            .and_then(|v| v.as_str())
            .unwrap_or("p2p")
            .to_string();
        let message_id = message
            .get("message_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let sender = event.get("sender")?.get("sender_id")?;
        let sender_id = sender
            .get("open_id")
            .or_else(|| sender.get("user_id"))
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();

        let content_str = message.get("content")?.as_str()?;
        let content: serde_json::Value = serde_json::from_str(content_str).ok()?;
        let text = extract_message_text(&content)?;

        if text.is_empty() {
            return None;
        }

        Some(InboundMessage {
            reply_target: ReplyTarget {
                receive_id: chat_id.clone(),
                receive_id_type: "chat_id".to_string(),
            },
            chat_id,
            chat_type,
            sender_id,
            text,
            message_id,
        })
    }

    fn parse_card_action_event(val: &serde_json::Value) -> Option<CardActionEvent> {
        let event = val.get("event")?;

        let receive_id = event
            .pointer("/context/chat_id")
            .or_else(|| event.get("chat_id"))
            .or_else(|| event.pointer("/context/open_chat_id"))
            .or_else(|| event.get("open_chat_id"))
            .and_then(|v| v.as_str())?
            .to_string();

        // 卡片回调里常见的是 open_chat_id，但消息发送接口需要使用 chat_id。
        // 当前机器人收消息时拿到的 chat_id 与这里的 open_chat_id 值一致，统一归一化成 chat_id。
        let receive_id_type = "chat_id".to_string();

        let sender_id = event
            .pointer("/operator/operator_id/open_id")
            .or_else(|| event.pointer("/operator/operator_id/user_id"))
            .or_else(|| event.pointer("/operator/open_id"))
            .or_else(|| event.pointer("/operator/user_id"))
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();

        let action_command = event
            .pointer("/action/value/command")
            .or_else(|| event.pointer("/action/value/action"))
            .or_else(|| event.pointer("/action/value/text"))
            .and_then(|v| v.as_str())?
            .to_string();

        let event_id = val
            .pointer("/header/event_id")
            .or_else(|| event.pointer("/context/open_message_id"))
            .or_else(|| event.get("open_message_id"))
            .and_then(|v| v.as_str())
            .unwrap_or("card_action")
            .to_string();

        Some(CardActionEvent {
            reply_target: ReplyTarget {
                receive_id,
                receive_id_type,
            },
            sender_id,
            action_command,
            event_id,
        })
    }
}

fn extract_message_text(content: &Value) -> Option<String> {
    if let Some(text) = content.get("text").and_then(|v| v.as_str()) {
        let text = sanitize_message_text(text);
        if !text.is_empty() {
            return Some(text);
        }
    }

    let post = content.get("post")?;
    let locale_content = ["zh_cn", "en_us"]
        .into_iter()
        .find_map(|locale| post.get(locale))
        .or_else(|| post.as_object().and_then(|obj| obj.values().next()))?;

    let paragraphs = locale_content.get("content")?.as_array()?;
    let mut lines = Vec::new();

    for paragraph in paragraphs {
        let Some(items) = paragraph.as_array() else {
            continue;
        };

        let mut line = String::new();
        for item in items {
            let tag = item.get("tag").and_then(|v| v.as_str()).unwrap_or("");
            let text = match tag {
                "text" | "a" | "at" => item.get("text").and_then(|v| v.as_str()),
                _ => None,
            };

            if let Some(text) = text {
                line.push_str(text);
            }
        }

        let line = sanitize_message_text(&line);
        if !line.is_empty() {
            lines.push(line);
        }
    }

    if lines.is_empty() {
        None
    } else {
        Some(lines.join("\n"))
    }
}

fn sanitize_message_text(text: &str) -> String {
    text.lines()
        .map(|line| {
            line.split_whitespace()
                .filter(|word| !word.starts_with('@'))
                .collect::<Vec<_>>()
                .join(" ")
        })
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_message_event_payload() {
        let payload = r#"{
          "schema": "2.0",
          "header": { "event_id": "evt_1", "event_type": "im.message.receive_v1" },
          "event": {
            "sender": { "sender_id": { "open_id": "ou_123" } },
            "message": {
              "chat_id": "oc_123",
              "chat_type": "p2p",
              "message_id": "om_123",
              "content": "{\"text\":\"继续\"}"
            }
          }
        }"#;

        let event = FeishuClient::parse_event_payload(payload).unwrap();
        match event {
            FeishuEvent::Message(message) => {
                assert_eq!(message.text, "继续");
                assert_eq!(message.reply_target.receive_id_type, "chat_id");
            }
            _ => panic!("expected message event"),
        }
    }

        #[test]
        fn parse_multiline_text_message_payload() {
                let payload = r#"{
                    "schema": "2.0",
                    "header": { "event_id": "evt_2", "event_type": "im.message.receive_v1" },
                    "event": {
                        "sender": { "sender_id": { "open_id": "ou_123" } },
                        "message": {
                            "chat_id": "oc_123",
                            "chat_type": "p2p",
                            "message_id": "om_124",
                            "content": "{\"text\":\"执行计划 git status\\n$ pwd\"}"
                        }
                    }
                }"#;

                let event = FeishuClient::parse_event_payload(payload).unwrap();
                match event {
                        FeishuEvent::Message(message) => {
                                assert_eq!(message.text, "执行计划 git status\n$ pwd");
                        }
                        _ => panic!("expected message event"),
                }
        }

        #[test]
        fn parse_post_message_payload() {
                let payload = r#"{
                    "schema": "2.0",
                    "header": { "event_id": "evt_3", "event_type": "im.message.receive_v1" },
                    "event": {
                        "sender": { "sender_id": { "open_id": "ou_123" } },
                        "message": {
                            "chat_id": "oc_123",
                            "chat_type": "p2p",
                            "message_id": "om_125",
                            "content": "{\"post\":{\"zh_cn\":{\"content\":[[{\"tag\":\"text\",\"text\":\"执行计划 git status\"}],[{\"tag\":\"text\",\"text\":\"$ pwd\"}]]}}}"
                        }
                    }
                }"#;

                let event = FeishuClient::parse_event_payload(payload).unwrap();
                match event {
                        FeishuEvent::Message(message) => {
                                assert_eq!(message.text, "执行计划 git status\n$ pwd");
                        }
                        _ => panic!("expected message event"),
                }
        }

    #[test]
    fn parse_card_action_payload() {
        let payload = r#"{
          "schema": "2.0",
          "header": { "event_id": "evt_card_1", "event_type": "card.action.trigger" },
          "event": {
            "operator": { "operator_id": { "open_id": "ou_456" } },
            "action": { "value": { "command": "继续" } },
            "context": {
              "open_chat_id": "oc_card_123",
              "open_message_id": "om_card_123"
            }
          }
        }"#;

        let event = FeishuClient::parse_event_payload(payload).unwrap();
        match event {
            FeishuEvent::CardAction(action) => {
                assert_eq!(action.action_command, "继续");
                assert_eq!(action.reply_target.receive_id, "oc_card_123");
                assert_eq!(action.reply_target.receive_id_type, "chat_id");
            }
            _ => panic!("expected card action event"),
        }
    }
}
