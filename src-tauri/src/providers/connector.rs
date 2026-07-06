use serde_json::Value;
use tokio_tungstenite::tungstenite::{http::Request, Message};

/// 实时识别协议无关的抽象事件流。会话循环（`commands/asr.rs`）只消费这一层，
/// 不感知具体供应商/协议的消息格式。
#[derive(Debug)]
pub enum AsrEvent {
    Started,
    Partial(String),
    Final(String),
    TaskFinished,
    TaskFailed { code: String, message: String },
    Other(Value),
}

/// 实时识别连接器：把某个协议族的连接请求、开始消息、音频消息、结束消息与事件解析
/// 封装成统一接口。新增协议族只需新增一个实现 + 工厂分支，会话循环无需改动。
/// 用 trait（而非在会话循环里 match 协议族）是因为消息构造/解析所需的状态
/// （如 duplex 的 task_id）天然属于连接器自身，trait 对象刚好把状态和行为绑在一起。
pub trait RealtimeAsrConnector: Send + Sync {
    fn connect_request(&self) -> Result<Request<()>, String>;
    fn start_messages(&self) -> Vec<Message>;
    fn audio_message(&self, bytes: Vec<u8>) -> Message;
    fn finish_message(&self) -> Message;
    fn parse_message(&self, text: &str) -> AsrEvent;
}
