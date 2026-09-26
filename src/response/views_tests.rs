//! Unit tests for the `ImageInfo` and `AudioInfo` view types.

use super::*;

#[test]
fn test_image_info_extension() {
    let check = |mime: Option<&str>, expected: &str| {
        let info = ImageInfo {
            data: "",
            mime_type: mime,
        };
        assert_eq!(info.extension(), expected);
    };

    check(Some("image/jpeg"), "jpg");
    check(Some("image/jpg"), "jpg");
    check(Some("image/png"), "png");
    check(Some("image/webp"), "webp");
    check(Some("image/gif"), "gif");
    check(Some("image/unknown"), "png"); // default
    check(None, "png"); // default
}

// =========================================================================
// AudioInfo Tests
// =========================================================================

#[test]
fn test_audio_info_extension() {
    let check = |mime: Option<&str>, expected: &str| {
        let info = AudioInfo {
            data: "",
            mime_type: mime,
            sample_rate: None,
            channels: None,
        };
        assert_eq!(info.extension(), expected);
    };

    check(Some("audio/wav"), "wav");
    check(Some("audio/x-wav"), "wav");
    check(Some("audio/mp3"), "mp3");
    check(Some("audio/mpeg"), "mp3");
    check(Some("audio/ogg"), "ogg");
    check(Some("audio/flac"), "flac");
    check(Some("audio/aac"), "aac");
    check(Some("audio/webm"), "webm");
    // PCM/L16 format from TTS API
    check(Some("audio/L16;codec=pcm;rate=24000"), "pcm");
    check(Some("audio/l16"), "pcm");
    check(Some("audio/unknown"), "wav"); // default
    check(None, "wav"); // default
}
