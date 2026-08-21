//! 内嵌鲸鱼图与音效资源，以 data URL 提供给前端（与旧插件路由等价）。
use base64::Engine;

const PREFIX_IMG: &str = "data:image/png;base64,";
const PREFIX_AUDIO: &str = "data:audio/mpeg;base64,";

// include_bytes! 相对本文件（desktop/src-tauri/src/）指向仓库根 assets/
pub const WHALE_PNG: &[u8] = include_bytes!("../../../assets/DSniang1.png");

pub struct SoundAsset {
    pub action: &'static str,
    pub set: &'static str,
    pub bytes: &'static [u8],
}

pub const SOUNDS: &[SoundAsset] = &[
    SoundAsset {
        action: "press",
        set: "duck",
        bytes: include_bytes!("../../../assets/Ya1.mp3"),
    },
    SoundAsset {
        action: "release",
        set: "duck",
        bytes: include_bytes!("../../../assets/Ya2.mp3"),
    },
    SoundAsset {
        action: "press",
        set: "fx1",
        bytes: include_bytes!("../../../assets/D1.mp3"),
    },
    SoundAsset {
        action: "release",
        set: "fx1",
        bytes: include_bytes!("../../../assets/D2.mp3"),
    },
];

pub fn image_data_url() -> String {
    format!(
        "{}{}",
        PREFIX_IMG,
        base64::engine::general_purpose::STANDARD.encode(WHALE_PNG)
    )
}

pub fn sound_data_url(action: &str, set: &str) -> Option<String> {
    SOUNDS
        .iter()
        .find(|s| s.action == action && s.set == set)
        .map(|s| {
            format!(
                "{}{}",
                PREFIX_AUDIO,
                base64::engine::general_purpose::STANDARD.encode(s.bytes)
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assets_embedded() {
        assert!(WHALE_PNG.starts_with(&[0x89, b'P', b'N', b'G']));
        for s in SOUNDS {
            assert!(!s.bytes.is_empty());
            assert!(image_data_url().starts_with(PREFIX_IMG));
            assert!(sound_data_url(s.action, s.set)
                .unwrap()
                .starts_with(PREFIX_AUDIO));
        }
        assert!(sound_data_url("press", "nope").is_none());
    }
}
