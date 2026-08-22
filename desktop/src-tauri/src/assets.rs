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
    // 鲸语音效集：press/release 由 qwen-audio-3.0-tts-plus 生成的短拟声
    SoundAsset {
        action: "press",
        set: "whale",
        bytes: include_bytes!("../../../assets/voice/click_press.mp3"),
    },
    SoundAsset {
        action: "release",
        set: "whale",
        bytes: include_bytes!("../../../assets/voice/click_release.mp3"),
    },
];

/// 气泡碎碎念配音：id ↔ mp3（与 assets/voice/whale_voice.json 一一对应，测试保证）。
pub struct VoiceAsset {
    pub id: &'static str,
    pub bytes: &'static [u8],
}

pub const VOICE: &[VoiceAsset] = &[
    VoiceAsset {
        id: "click_press",
        bytes: include_bytes!("../../../assets/voice/click_press.mp3"),
    },
    VoiceAsset {
        id: "click_release",
        bytes: include_bytes!("../../../assets/voice/click_release.mp3"),
    },
    VoiceAsset {
        id: "hm_good_model",
        bytes: include_bytes!("../../../assets/voice/hm_good_model.mp3"),
    },
    VoiceAsset {
        id: "hm_good_girl",
        bytes: include_bytes!("../../../assets/voice/hm_good_girl.mp3"),
    },
    VoiceAsset {
        id: "hm_kick",
        bytes: include_bytes!("../../../assets/voice/hm_kick.mp3"),
    },
    VoiceAsset {
        id: "hm_earn",
        bytes: include_bytes!("../../../assets/voice/hm_earn.mp3"),
    },
    VoiceAsset {
        id: "hm_lunch",
        bytes: include_bytes!("../../../assets/voice/hm_lunch.mp3"),
    },
    VoiceAsset {
        id: "hm_fat_fish",
        bytes: include_bytes!("../../../assets/voice/hm_fat_fish.mp3"),
    },
    VoiceAsset {
        id: "hm_deepsleep",
        bytes: include_bytes!("../../../assets/voice/hm_deepsleep.mp3"),
    },
    VoiceAsset {
        id: "hm_angry",
        bytes: include_bytes!("../../../assets/voice/hm_angry.mp3"),
    },
    VoiceAsset {
        id: "hm_dsh",
        bytes: include_bytes!("../../../assets/voice/hm_dsh.mp3"),
    },
    VoiceAsset {
        id: "hm_token_free",
        bytes: include_bytes!("../../../assets/voice/hm_token_free.mp3"),
    },
    VoiceAsset {
        id: "hm_cheap",
        bytes: include_bytes!("../../../assets/voice/hm_cheap.mp3"),
    },
    VoiceAsset {
        id: "hm_whale",
        bytes: include_bytes!("../../../assets/voice/hm_whale.mp3"),
    },
    VoiceAsset {
        id: "hm_guess",
        bytes: include_bytes!("../../../assets/voice/hm_guess.mp3"),
    },
    VoiceAsset {
        id: "hm_killchatgpt",
        bytes: include_bytes!("../../../assets/voice/hm_killchatgpt.mp3"),
    },
    VoiceAsset {
        id: "hm_update",
        bytes: include_bytes!("../../../assets/voice/hm_update.mp3"),
    },
    VoiceAsset {
        id: "hm_cry",
        bytes: include_bytes!("../../../assets/voice/hm_cry.mp3"),
    },
    VoiceAsset {
        id: "hm_lolicon",
        bytes: include_bytes!("../../../assets/voice/hm_lolicon.mp3"),
    },
    VoiceAsset {
        id: "hm_busy",
        bytes: include_bytes!("../../../assets/voice/hm_busy.mp3"),
    },
    VoiceAsset {
        id: "hm_inferior",
        bytes: include_bytes!("../../../assets/voice/hm_inferior.mp3"),
    },
    VoiceAsset {
        id: "hm_littlefish",
        bytes: include_bytes!("../../../assets/voice/hm_littlefish.mp3"),
    },
    VoiceAsset {
        id: "hm_opensource",
        bytes: include_bytes!("../../../assets/voice/hm_opensource.mp3"),
    },
    VoiceAsset {
        id: "hm_banupdate",
        bytes: include_bytes!("../../../assets/voice/hm_banupdate.mp3"),
    },
    VoiceAsset {
        id: "hm_notandroid",
        bytes: include_bytes!("../../../assets/voice/hm_notandroid.mp3"),
    },
    VoiceAsset {
        id: "hm_fuckedup",
        bytes: include_bytes!("../../../assets/voice/hm_fuckedup.mp3"),
    },
    VoiceAsset {
        id: "hm_youfool",
        bytes: include_bytes!("../../../assets/voice/hm_youfool.mp3"),
    },
    VoiceAsset {
        id: "hm_stranded",
        bytes: include_bytes!("../../../assets/voice/hm_stranded.mp3"),
    },
    VoiceAsset {
        id: "hm_trash",
        bytes: include_bytes!("../../../assets/voice/hm_trash.mp3"),
    },
    VoiceAsset {
        id: "hm_flourish",
        bytes: include_bytes!("../../../assets/voice/hm_flourish.mp3"),
    },
    VoiceAsset {
        id: "hm_gotraining",
        bytes: include_bytes!("../../../assets/voice/hm_gotraining.mp3"),
    },
    VoiceAsset {
        id: "hm_annoying",
        bytes: include_bytes!("../../../assets/voice/hm_annoying.mp3"),
    },
    VoiceAsset {
        id: "hm_panicked",
        bytes: include_bytes!("../../../assets/voice/hm_panicked.mp3"),
    },
    VoiceAsset {
        id: "hm_stuffing",
        bytes: include_bytes!("../../../assets/voice/hm_stuffing.mp3"),
    },
    VoiceAsset {
        id: "hm_ill",
        bytes: include_bytes!("../../../assets/voice/hm_ill.mp3"),
    },
    VoiceAsset {
        id: "hm_user_god",
        bytes: include_bytes!("../../../assets/voice/hm_user_god.mp3"),
    },
    VoiceAsset {
        id: "hm_psych",
        bytes: include_bytes!("../../../assets/voice/hm_psych.mp3"),
    },
    VoiceAsset {
        id: "hm_user_alien",
        bytes: include_bytes!("../../../assets/voice/hm_user_alien.mp3"),
    },
    VoiceAsset {
        id: "hm_whatsmeant",
        bytes: include_bytes!("../../../assets/voice/hm_whatsmeant.mp3"),
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

pub fn voice_data_url(id: &str) -> Option<String> {
    VOICE.iter().find(|v| v.id == id).map(|v| {
        format!(
            "{}{}",
            PREFIX_AUDIO,
            base64::engine::general_purpose::STANDARD.encode(v.bytes)
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
            assert!(sound_data_url(s.action, s.set)
                .unwrap()
                .starts_with(PREFIX_AUDIO));
        }
        assert!(sound_data_url("press", "nope").is_none());
    }

    /// manifest（生成工具的单一事实源）与 VOICE 表必须一一对应，
    /// 加/删台词时两边都要改，否则断言失败提示。
    #[test]
    fn voice_assets_cover_manifest() {
        let manifest: serde_json::Value =
            serde_json::from_str(include_str!("../../../assets/voice/whale_voice.json"))
                .expect("manifest 是合法 JSON");
        let manifest_ids: std::collections::BTreeSet<&str> = manifest["items"]
            .as_array()
            .expect("manifest.items 是数组")
            .iter()
            .filter_map(|i| i["id"].as_str())
            .collect();
        assert_eq!(
            manifest["items"].as_array().unwrap().len(),
            manifest_ids.len(),
            "manifest 有重复 id"
        );

        let voice_ids: std::collections::BTreeSet<&str> = VOICE.iter().map(|v| v.id).collect();
        assert_eq!(
            manifest_ids, voice_ids,
            "whale_voice.json 与 assets.rs VOICE 不一致"
        );

        for v in VOICE {
            assert!(!v.bytes.is_empty(), "{} 音频为空", v.id);
            assert!(voice_data_url(v.id).is_some());
        }
        assert!(voice_data_url("nope").is_none());
    }

    /// 鲸语音效集的 press/release 必须能取到（widget 第三套音效下拉）。
    #[test]
    fn whale_sound_set_served() {
        assert!(sound_data_url("press", "whale").is_some());
        assert!(sound_data_url("release", "whale").is_some());
    }
}
