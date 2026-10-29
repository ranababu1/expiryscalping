//! JSON text frames on the Kite WebSocket: `{"type": "order" | "error" |
//! "message", "data": ...}`. Order postbacks feed the order manager in
//! Phase 2. Phase 1 classifies them so the shape is fixed and tested.

use serde::Deserialize;

/// The fields of an order postback the order manager needs.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct OrderUpdate {
    pub order_id: String,
    pub status: String,
    #[serde(default)]
    pub tag: Option<String>,
    #[serde(default)]
    pub instrument_token: u32,
    #[serde(default)]
    pub filled_quantity: u32,
    #[serde(default)]
    pub pending_quantity: u32,
    #[serde(default)]
    pub average_price: f64,
    #[serde(default)]
    pub status_message: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TextMessage {
    Order(OrderUpdate),
    Error(String),
    Message(String),
    Unknown,
}

#[derive(Deserialize)]
struct Envelope {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    data: serde_json::Value,
}

pub fn parse_text(s: &str) -> Result<TextMessage, serde_json::Error> {
    let env: Envelope = serde_json::from_str(s)?;
    Ok(match env.kind.as_str() {
        "order" => TextMessage::Order(serde_json::from_value(env.data)?),
        "error" => TextMessage::Error(env.data.as_str().unwrap_or_default().to_string()),
        "message" => TextMessage::Message(env.data.as_str().unwrap_or_default().to_string()),
        _ => TextMessage::Unknown,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn order_postback() {
        let s = r#"{"type":"order","data":{"order_id":"2611030001","status":"COMPLETE","tag":"ESMB1","instrument_token":12345602,"filled_quantity":65,"pending_quantity":0,"average_price":101.5}}"#;
        match parse_text(s).unwrap() {
            TextMessage::Order(o) => {
                assert_eq!(o.status, "COMPLETE");
                assert_eq!(o.tag.as_deref(), Some("ESMB1"));
                assert_eq!(o.filled_quantity, 65);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn error_and_unknown() {
        assert_eq!(
            parse_text(r#"{"type":"error","data":"bad"}"#).unwrap(),
            TextMessage::Error("bad".into())
        );
        assert_eq!(parse_text(r#"{"type":"x"}"#).unwrap(), TextMessage::Unknown);
        assert!(parse_text("nope").is_err());
    }
}
