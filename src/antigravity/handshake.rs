//! Binary-protobuf stdio handshake with the `localharness` binary.
//!
//! The harness reads a single length-prefixed [`InputConfig`] message from
//! stdin at startup and replies with a single length-prefixed
//! [`OutputConfig`] on stdout. Both frames are `4-byte little-endian length +
//! binary protobuf`. These are the only two binary-protobuf messages in the
//! entire protocol (everything after the handshake is proto-JSON over a
//! WebSocket), so the encoding is hand-rolled here rather than pulling in a
//! protobuf dependency.
//!
//! Field numbers are taken from the descriptors shipped inside the
//! `google-antigravity` wheel (`localharness.proto`, package
//! `antigravity.localharness`):
//!
//! | Message      | Field               | Number | Type    |
//! |--------------|---------------------|--------|---------|
//! | `InputConfig`| `storage_directory` | 1      | string  |
//! | `InputConfig`| `port`              | 2      | uint32  |
//! | `InputConfig`| `bind_address`      | 3      | string  |
//! | `InputConfig`| `client_info`       | 4      | message |
//! | `ClientInfo` | `language`          | 1      | string  |
//! | `ClientInfo` | `version`           | 2      | string  |
//! | `ClientInfo` | `language_version`  | 3      | string  |
//! | `OutputConfig`| `port`             | 1      | int32   |
//! | `OutputConfig`| `api_key`          | 2      | string  |

/// Protobuf wire type for varint-encoded scalars.
const WIRE_VARINT: u32 = 0;
/// Protobuf wire type for 64-bit fixed-width scalars.
const WIRE_FIXED64: u32 = 1;
/// Protobuf wire type for length-delimited payloads (strings, messages).
const WIRE_LEN: u32 = 2;
/// Protobuf wire type for 32-bit fixed-width scalars.
const WIRE_FIXED32: u32 = 5;

/// Handshake message written to the harness's stdin.
///
/// Mirrors `antigravity.localharness.InputConfig`. Fields at their proto3
/// defaults (empty string, `0`, `None`) are omitted from the encoding, which
/// matches the reference (protobuf) encoder byte-for-byte.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InputConfig {
    /// Directory where the harness persists trajectories (`save_dir`).
    pub storage_directory: String,
    /// Requested WebSocket port. `0` lets the harness pick a free port.
    pub port: u32,
    /// Bind address for the WebSocket server (harness default: `localhost`).
    pub bind_address: String,
    /// Identifies this client to the harness.
    pub client_info: Option<ClientInfo>,
}

/// Client identification sent in the handshake.
///
/// Mirrors `antigravity.localharness.ClientInfo`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ClientInfo {
    /// Client implementation language (`"rust"`).
    pub language: String,
    /// Client version (this crate's version).
    pub version: String,
    /// Language toolchain version (minimum supported Rust version).
    pub language_version: String,
}

/// Handshake reply read from the harness's stdout.
///
/// Mirrors `antigravity.localharness.OutputConfig`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OutputConfig {
    /// TCP port of the harness's WebSocket server.
    pub port: i32,
    /// Per-process auth token, sent as the `x-goog-api-key` header when
    /// connecting to the WebSocket. This is *not* a Gemini API key.
    pub api_key: String,
}

/// Errors produced while decoding a handshake frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeError {
    /// The buffer ended in the middle of a value.
    Truncated,
    /// A varint ran past 10 bytes (the maximum for 64-bit values).
    VarintOverflow,
    /// A length-delimited field claimed more bytes than remain.
    LengthOutOfBounds,
    /// A field used a wire type this decoder cannot skip (groups).
    UnsupportedWireType(u32),
    /// A string field contained invalid UTF-8.
    InvalidUtf8,
}

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Truncated => write!(f, "buffer truncated mid-value"),
            Self::VarintOverflow => write!(f, "varint exceeds 10 bytes"),
            Self::LengthOutOfBounds => write!(f, "length-delimited field out of bounds"),
            Self::UnsupportedWireType(t) => write!(f, "unsupported wire type {t}"),
            Self::InvalidUtf8 => write!(f, "string field is not valid UTF-8"),
        }
    }
}

impl std::error::Error for DecodeError {}

// =============================================================================
// Primitive encoding
// =============================================================================

fn encode_varint(mut value: u64, buf: &mut Vec<u8>) {
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        if value == 0 {
            buf.push(byte);
            return;
        }
        buf.push(byte | 0x80);
    }
}

fn encode_tag(field_number: u32, wire_type: u32, buf: &mut Vec<u8>) {
    encode_varint(u64::from((field_number << 3) | wire_type), buf);
}

/// Encodes a string field, omitting it entirely when empty (proto3 default).
fn encode_string_field(field_number: u32, value: &str, buf: &mut Vec<u8>) {
    if value.is_empty() {
        return;
    }
    encode_tag(field_number, WIRE_LEN, buf);
    encode_varint(value.len() as u64, buf);
    buf.extend_from_slice(value.as_bytes());
}

/// Encodes a uint32 varint field, omitting it when zero (proto3 default).
fn encode_uint32_field(field_number: u32, value: u32, buf: &mut Vec<u8>) {
    if value == 0 {
        return;
    }
    encode_tag(field_number, WIRE_VARINT, buf);
    encode_varint(u64::from(value), buf);
}

/// Encodes an embedded message field, omitting it when the encoding is empty.
///
/// Note: unlike scalars, a *present* message with all-default contents would
/// still need to be encoded as a zero-length field to round-trip presence.
/// The harness only checks field contents, so we mirror the reference client
/// and skip fully-default submessages.
fn encode_message_field(field_number: u32, encoded: &[u8], buf: &mut Vec<u8>) {
    if encoded.is_empty() {
        return;
    }
    encode_tag(field_number, WIRE_LEN, buf);
    encode_varint(encoded.len() as u64, buf);
    buf.extend_from_slice(encoded);
}

// =============================================================================
// Primitive decoding
// =============================================================================

fn decode_varint(buf: &[u8], pos: &mut usize) -> Result<u64, DecodeError> {
    let mut value: u64 = 0;
    for shift in 0..10 {
        let byte = *buf.get(*pos).ok_or(DecodeError::Truncated)?;
        *pos += 1;
        // The 10th byte of a 64-bit varint may only contribute 1 bit.
        if shift == 9 && byte > 1 {
            return Err(DecodeError::VarintOverflow);
        }
        value |= u64::from(byte & 0x7f) << (shift * 7);
        if byte & 0x80 == 0 {
            return Ok(value);
        }
    }
    Err(DecodeError::VarintOverflow)
}

fn decode_len_delimited<'a>(buf: &'a [u8], pos: &mut usize) -> Result<&'a [u8], DecodeError> {
    let len = decode_varint(buf, pos)?;
    let len = usize::try_from(len).map_err(|_| DecodeError::LengthOutOfBounds)?;
    let end = pos.checked_add(len).ok_or(DecodeError::LengthOutOfBounds)?;
    if end > buf.len() {
        return Err(DecodeError::LengthOutOfBounds);
    }
    let slice = &buf[*pos..end];
    *pos = end;
    Ok(slice)
}

/// Skips over a field value of the given wire type (Evergreen: unknown fields
/// in the harness's reply are ignored, not rejected).
fn skip_field(wire_type: u32, buf: &[u8], pos: &mut usize) -> Result<(), DecodeError> {
    match wire_type {
        WIRE_VARINT => {
            decode_varint(buf, pos)?;
            Ok(())
        }
        WIRE_FIXED64 => {
            let end = pos.checked_add(8).ok_or(DecodeError::Truncated)?;
            if end > buf.len() {
                return Err(DecodeError::Truncated);
            }
            *pos = end;
            Ok(())
        }
        WIRE_LEN => {
            decode_len_delimited(buf, pos)?;
            Ok(())
        }
        WIRE_FIXED32 => {
            let end = pos.checked_add(4).ok_or(DecodeError::Truncated)?;
            if end > buf.len() {
                return Err(DecodeError::Truncated);
            }
            *pos = end;
            Ok(())
        }
        other => Err(DecodeError::UnsupportedWireType(other)),
    }
}

// =============================================================================
// Message encoding / decoding
// =============================================================================

impl ClientInfo {
    fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        encode_string_field(1, &self.language, &mut buf);
        encode_string_field(2, &self.version, &mut buf);
        encode_string_field(3, &self.language_version, &mut buf);
        buf
    }
}

impl InputConfig {
    /// Encodes this message as binary protobuf (without the length prefix).
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        encode_string_field(1, &self.storage_directory, &mut buf);
        encode_uint32_field(2, self.port, &mut buf);
        encode_string_field(3, &self.bind_address, &mut buf);
        if let Some(client_info) = &self.client_info {
            encode_message_field(4, &client_info.encode(), &mut buf);
        }
        buf
    }

    /// Encodes this message as a stdio frame: 4-byte little-endian length
    /// followed by the binary protobuf payload.
    #[must_use]
    pub fn encode_frame(&self) -> Vec<u8> {
        let payload = self.encode();
        let mut frame = Vec::with_capacity(4 + payload.len());
        frame.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        frame.extend_from_slice(&payload);
        frame
    }
}

impl OutputConfig {
    /// Decodes a binary-protobuf `OutputConfig` payload (without the length
    /// prefix). Unknown fields are skipped.
    pub fn decode(buf: &[u8]) -> Result<Self, DecodeError> {
        let mut config = Self::default();
        let mut pos = 0;
        while pos < buf.len() {
            let tag = decode_varint(buf, &mut pos)?;
            let field_number = u32::try_from(tag >> 3).map_err(|_| DecodeError::VarintOverflow)?;
            let wire_type = (tag & 0x7) as u32;
            match (field_number, wire_type) {
                (1, WIRE_VARINT) => {
                    // int32 is varint-encoded as a 64-bit two's-complement value.
                    let raw = decode_varint(buf, &mut pos)?;
                    config.port = raw as i32;
                }
                (2, WIRE_LEN) => {
                    let bytes = decode_len_delimited(buf, &mut pos)?;
                    config.api_key = std::str::from_utf8(bytes)
                        .map_err(|_| DecodeError::InvalidUtf8)?
                        .to_string();
                }
                (_, wire_type) => skip_field(wire_type, buf, &mut pos)?,
            }
        }
        Ok(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Golden bytes generated with the reference implementation
    // (google-antigravity 0.1.5, localharness_pb2 + protobuf SerializeToString).

    #[test]
    fn test_encode_full_input_config_golden() {
        let config = InputConfig {
            storage_directory: "/tmp/storage".to_string(),
            port: 8080,
            bind_address: "127.0.0.1".to_string(),
            client_info: Some(ClientInfo {
                language: "rust".to_string(),
                version: "0.7.2".to_string(),
                language_version: "1.88".to_string(),
            }),
        };
        let golden: &[u8] = &[
            10, 12, 47, 116, 109, 112, 47, 115, 116, 111, 114, 97, 103, 101, 16, 144, 63, 26, 9,
            49, 50, 55, 46, 48, 46, 48, 46, 49, 34, 19, 10, 4, 114, 117, 115, 116, 18, 5, 48, 46,
            55, 46, 50, 26, 4, 49, 46, 56, 56,
        ];
        assert_eq!(config.encode(), golden);
    }

    #[test]
    fn test_encode_minimal_input_config_golden() {
        let config = InputConfig {
            client_info: Some(ClientInfo {
                language: "rust".to_string(),
                version: "0.1.0".to_string(),
                language_version: "1.88.0".to_string(),
            }),
            ..Default::default()
        };
        let golden: &[u8] = &[
            34, 21, 10, 4, 114, 117, 115, 116, 18, 5, 48, 46, 49, 46, 48, 26, 6, 49, 46, 56, 56,
            46, 48,
        ];
        assert_eq!(config.encode(), golden);
    }

    #[test]
    fn test_encode_empty_input_config_is_empty() {
        assert_eq!(InputConfig::default().encode(), Vec::<u8>::new());
    }

    #[test]
    fn test_encode_unicode_storage_directory_golden() {
        let config = InputConfig {
            storage_directory: "/tmp/\u{65e5}\u{672c}\u{8a9e}".to_string(),
            ..Default::default()
        };
        let golden: &[u8] = &[
            10, 14, 47, 116, 109, 112, 47, 230, 151, 165, 230, 156, 172, 232, 170, 158,
        ];
        assert_eq!(config.encode(), golden);
    }

    #[test]
    fn test_encode_frame_prepends_le_length() {
        let config = InputConfig {
            storage_directory: "/x".to_string(),
            ..Default::default()
        };
        let payload = config.encode();
        let frame = config.encode_frame();
        assert_eq!(&frame[..4], (payload.len() as u32).to_le_bytes());
        assert_eq!(&frame[4..], payload.as_slice());
    }

    #[test]
    fn test_decode_output_config_golden() {
        let golden: &[u8] = &[
            8, 177, 168, 3, 18, 16, 115, 101, 99, 114, 101, 116, 45, 116, 111, 107, 101, 110, 45,
            49, 50, 51,
        ];
        let config = OutputConfig::decode(golden).unwrap();
        assert_eq!(config.port, 54321);
        assert_eq!(config.api_key, "secret-token-123");
    }

    #[test]
    fn test_decode_output_config_port_only_golden() {
        let config = OutputConfig::decode(&[8, 1]).unwrap();
        assert_eq!(config.port, 1);
        assert_eq!(config.api_key, "");
    }

    #[test]
    fn test_decode_output_config_multibyte_varint_golden() {
        // port=65535, api_key="" (explicitly-empty string field present).
        let config = OutputConfig::decode(&[8, 255, 255, 3, 18, 0]).unwrap();
        assert_eq!(config.port, 65535);
        assert_eq!(config.api_key, "");
    }

    #[test]
    fn test_decode_empty_output_config() {
        let config = OutputConfig::decode(&[]).unwrap();
        assert_eq!(config, OutputConfig::default());
    }

    #[test]
    fn test_decode_skips_unknown_fields() {
        // Field 3 (varint), field 4 (len-delimited), field 5 (fixed32),
        // field 6 (fixed64) are unknown to OutputConfig and must be skipped.
        let mut buf = Vec::new();
        buf.extend_from_slice(&[24, 42]); // field 3, varint 42
        buf.extend_from_slice(&[34, 2, 104, 105]); // field 4, "hi"
        buf.extend_from_slice(&[45, 1, 2, 3, 4]); // field 5, fixed32
        buf.extend_from_slice(&[49, 1, 2, 3, 4, 5, 6, 7, 8]); // field 6, fixed64
        buf.extend_from_slice(&[8, 7]); // field 1, port=7
        let config = OutputConfig::decode(&buf).unwrap();
        assert_eq!(config.port, 7);
    }

    #[test]
    fn test_decode_truncated_varint_errors() {
        assert_eq!(
            OutputConfig::decode(&[8, 0x80]),
            Err(DecodeError::Truncated)
        );
    }

    #[test]
    fn test_decode_truncated_string_errors() {
        // Field 2 claims 10 bytes but only 2 follow.
        assert_eq!(
            OutputConfig::decode(&[18, 10, 104, 105]),
            Err(DecodeError::LengthOutOfBounds)
        );
    }

    #[test]
    fn test_decode_varint_overflow_errors() {
        // 11 continuation bytes: longer than any valid 64-bit varint.
        let buf = [
            8, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x7f,
        ];
        assert_eq!(OutputConfig::decode(&buf), Err(DecodeError::VarintOverflow));
    }

    #[test]
    fn test_decode_group_wire_type_errors() {
        // Wire type 3 (start group) is unsupported.
        assert_eq!(
            OutputConfig::decode(&[27]),
            Err(DecodeError::UnsupportedWireType(3))
        );
    }

    #[test]
    fn test_decode_invalid_utf8_api_key_errors() {
        assert_eq!(
            OutputConfig::decode(&[18, 2, 0xff, 0xfe]),
            Err(DecodeError::InvalidUtf8)
        );
    }

    #[test]
    fn test_varint_boundaries() {
        for value in [
            0u64,
            1,
            127,
            128,
            16383,
            16384,
            u64::from(u32::MAX),
            u64::MAX,
        ] {
            let mut buf = Vec::new();
            encode_varint(value, &mut buf);
            let mut pos = 0;
            assert_eq!(decode_varint(&buf, &mut pos).unwrap(), value);
            assert_eq!(pos, buf.len());
        }
    }

    #[test]
    fn test_negative_port_roundtrip() {
        // int32 -1 is encoded as a 10-byte varint (two's complement, 64-bit).
        let mut buf = vec![8];
        encode_varint((-1i64) as u64, &mut buf);
        let config = OutputConfig::decode(&buf).unwrap();
        assert_eq!(config.port, -1);
    }
}
