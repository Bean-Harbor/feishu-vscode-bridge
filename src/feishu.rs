use serde::{Deserialize, Serialize};

const TOKEN_URL: &str =
    "https://open.feishu.cn/open-apis/auth/v3/tenant_access_token/internal";
const MSG_URL: &str = "https://open.feishu.cn/open-apis/im/v1/messages";
const CHATS_URL: &str = "https://open.feishu.cn/open-apis/im/v1/chats";

// ──────────────────────────── public types ────────────────────────────

pub struct FeishuClient {
    app_id: String,
    app_secret: String,
    token: Option<String>,
    /// 显式指定的目标（chat_id 或 open_id），优先于自动发现
    target_id: Option<String>,
    target_id_type: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ChatInfo {
    pub chat_id: String,
    pub name: String,
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
struct ChatsResp {
    code: i64,
    msg: String,
    data: Option<ChatsData>,
}
#[derive(Deserialize)]
struct ChatsData {
    items: Option<Vec<ChatItem>>,
}
#[derive(Deserialize)]
struct ChatItem {
    chat_id: Option<String>,
    name: Option<String>,
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

        // 支持显式指定目标：FEISHU_CHAT_ID 或 FEISHU_OPEN_ID
        let (target_id, target_id_type) =
            if let Ok(chat_id) = std::env::var("FEISHU_CHAT_ID") {
                (Some(chat_id), Some("chat_id".to_string()))
            } else if let Ok(open_id) = std::env::var("FEISHU_OPEN_ID") {
                (Some(open_id), Some("open_id".to_string()))
            } else {
                (None, None)
            };

        Ok(Self {
            app_id,
            app_secret,
            token: None,
            target_id,
            target_id_type,
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

    /// 列出机器人所在的聊天（包括 P2P 单聊和群聊）
    pub fn list_chats(&self) -> Result<Vec<ChatInfo>, String> {
        let token = self.token()?;
        let resp: ChatsResp = ureq::get(CHATS_URL)
            .set("Authorization", &format!("Bearer {token}"))
            .call()
            .map_err(|e| format!("列出聊天失败: {e}"))?
            .into_json()
            .map_err(|e| format!("解析聊天列表失败: {e}"))?;

        if resp.code != 0 {
            return Err(format!("列出聊天失败 (code={}): {}", resp.code, resp.msg));
        }
        let items = resp.data.and_then(|d| d.items).unwrap_or_default();
        Ok(items
            .into_iter()
            .filter_map(|c| {
                Some(ChatInfo {
                    chat_id: c.chat_id?,
                    name: c.name.unwrap_or_default(),
                })
            })
            .collect())
    }

    /// 解析发送目标：优先用 .env 中的 FEISHU_CHAT_ID / FEISHU_OPEN_ID
    pub fn resolve_target(&self) -> Result<(String, String), String> {
        if let (Some(id), Some(id_type)) = (&self.target_id, &self.target_id_type) {
            return Ok((id.clone(), id_type.clone()));
        }
        Err("未设置发送目标。请在 .env 中配置 FEISHU_CHAT_ID（推荐）或 FEISHU_OPEN_ID".to_string())
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
}
