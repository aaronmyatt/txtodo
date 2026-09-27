//! This device's pairing offer, drawn for Settings › Devices (task `tui-revamp/tui-settings`, the
//! pairing-code decide line of 2026-09-25): the code text comes ready-made from the daemon
//! (`PairOfferResponse.code`); the QR holds the JSON payload an existing scanner reads, which stays
//! each client's job. Pure: no daemon, no terminal.

use std::fmt::Write as _;

use txtodo_proto::v1 as pb;

/// The QR's text: the offer's nine fields as one JSON object, in the order and shape `txtodo pair`
/// prints (serde_json of its `PairingCode`) and the daemon's `pairing_wire` parses. Hand-built:
/// every field is a string, and this crate has no JSON dependency.
pub fn qr_payload(offer: &pb::PairOfferResponse) -> String {
    let fields = [
        ("device", &offer.device),
        ("group_id", &offer.group_id),
        ("x25519_pub", &offer.x25519_pub),
        ("endpoint", &offer.endpoint),
        ("nonce", &offer.nonce),
        ("identity_mode", &offer.identity_mode),
        ("relay_node_id", &offer.relay_node_id),
        ("relay_url", &offer.relay_url),
        ("workspace_id", &offer.workspace_id),
    ];
    let body: Vec<String> = fields
        .iter()
        .map(|(key, value)| format!("\"{key}\":{}", json_string(value)))
        .collect();
    format!("{{{}}}", body.join(","))
}

/// `s` as a JSON string, escaped the way serde_json does it: quote, backslash, the short forms for
/// five control characters, `\u00XX` for the rest below U+0020; everything else as is.
/// Ref: <https://www.rfc-editor.org/rfc/rfc8259#section-7>
fn json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if c < ' ' => {
                let _ = write!(out, "\\u{:04x}", u32::from(c));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// `text` as QR rows, two modules per character cell top to bottom, the renderer `txtodo pair`
/// uses. Empty when the text is too long for any QR version.
/// Ref: <https://docs.rs/qrcode/latest/qrcode/render/unicode/type.Dense1x2.html>
pub fn qr_lines(text: &str) -> Vec<String> {
    qrcode::QrCode::new(text.as_bytes())
        .map(|code| {
            code.render::<qrcode::render::unicode::Dense1x2>()
                .build()
                .lines()
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn offer() -> pb::PairOfferResponse {
        pb::PairOfferResponse {
            device: "dev1".to_owned(),
            group_id: "42".to_owned(),
            x25519_pub: "ab".to_owned(),
            endpoint: "192.168.1.2:7700".to_owned(),
            nonce: "cd".to_owned(),
            identity_mode: "sidecar".to_owned(),
            workspace_id: "01J0000000000000000000ABC".to_owned(),
            code: "NOTINTHEQR".to_owned(),
            ..pb::PairOfferResponse::default()
        }
    }

    #[test]
    fn the_payload_is_the_nine_fields_in_the_cli_order() {
        assert_eq!(
            qr_payload(&offer()),
            "{\"device\":\"dev1\",\"group_id\":\"42\",\"x25519_pub\":\"ab\",\
             \"endpoint\":\"192.168.1.2:7700\",\"nonce\":\"cd\",\"identity_mode\":\"sidecar\",\
             \"relay_node_id\":\"\",\"relay_url\":\"\",\"workspace_id\":\"01J0000000000000000000ABC\"}"
        );
        assert!(
            !qr_payload(&offer()).contains("NOTINTHEQR"),
            "the code is not a field"
        );
    }

    #[test]
    fn strings_escape_like_serde_json() {
        assert_eq!(json_string("a\"b\\c"), "\"a\\\"b\\\\c\"");
        assert_eq!(json_string("\n\t\u{1}"), "\"\\n\\t\\u0001\"");
        assert_eq!(json_string("wss://relay/é"), "\"wss://relay/é\"");
    }

    #[test]
    fn the_qr_is_a_block_of_equal_rows() {
        let lines = qr_lines(&qr_payload(&offer()));
        assert!(lines.len() > 10, "{} rows", lines.len());
        let width = lines[0].chars().count();
        assert!(lines.iter().all(|l| l.chars().count() == width));
        // Two modules per row: about half as tall as wide.
        assert!(lines.len() * 2 >= width && lines.len() * 2 <= width + 2);
    }
}
