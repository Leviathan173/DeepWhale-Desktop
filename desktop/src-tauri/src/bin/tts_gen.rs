//! 一次性生成工具：用百炼 DashScope 的 qwen-audio-3.0-tts-plus 把
//! assets/voice/whale_voice.json 里每条台词合成为 assets/voice/<id>.mp3。
//! 运行：cargo run --bin tts_gen [--key sk-xxx] [--force]
//! Key 来源：--key > 环境变量 DASHSCOPE_API_KEY > opencode 的 auth.json/opencode.json 里百炼 key。
//! 幂等：mp3 已存在则跳过，--force 强制重新生成。
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use base64::Engine;
use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::Message;

const API_URL: &str = "https://dashscope.aliyuncs.com/api/v1/services/aigc/text2audio/generation";
const WS_URL_DEFAULT: &str = "wss://dashscope.aliyuncs.com/api-ws/v1/inference";
const QWEN3_HTTP_URL: &str =
    "https://dashscope.aliyuncs.com/api/v1/services/aigc/multimodal-generation/generation";

/// 从当前目录向上找仓库根（包含 assets/voice/whale_voice.json 的目录）。
fn repo_root() -> Option<PathBuf> {
    let mut dir = std::env::current_dir().ok()?;
    loop {
        if dir
            .join("assets")
            .join("voice")
            .join("whale_voice.json")
            .exists()
        {
            return Some(dir);
        }
        if !dir.pop() {
            return None;
        }
    }
}

fn find_key(explicit: Option<&str>) -> Option<String> {
    if let Some(k) = explicit {
        return Some(k.trim().to_string());
    }
    if let Ok(k) = std::env::var("DASHSCOPE_API_KEY") {
        if !k.trim().is_empty() {
            return Some(k.trim().to_string());
        }
    }
    if let Ok(k) = std::env::var("Dashscope_ApiKey") {
        if !k.trim().is_empty() {
            return Some(k.trim().to_string());
        }
    }
    // opencode 凭据：auth.json（{provider: {key}}）+ opencode.json provider.options.apiKey
    let mut data_dir = std::env::var_os("OPENCODE_DATA_DIR").map(PathBuf::from);
    if data_dir.is_none() {
        let base = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME"))?;
        let mut b = PathBuf::from(base);
        b.push(".local/share/opencode");
        data_dir = Some(b);
    }
    let dir = data_dir?;
    let mut providers: BTreeMap<String, String> = BTreeMap::new();
    if let Ok(s) = std::fs::read_to_string(dir.join("auth.json")) {
        if let Ok(v) = serde_json::from_str::<Value>(&s) {
            if let Some(m) = v.as_object() {
                for (prov, val) in m {
                    let key = val.get("key").and_then(|x| x.as_str()).unwrap_or("").trim();
                    if !key.is_empty() {
                        providers
                            .entry(prov.to_ascii_lowercase())
                            .or_insert_with(|| key.to_string());
                    }
                }
            }
        }
    }
    let mut cfg_dir = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let base = std::env::var_os("USERPROFILE")
                .or_else(|| std::env::var_os("HOME"))
                .unwrap_or_default();
            PathBuf::from(base).join(".config")
        });
    cfg_dir.push("opencode/opencode.json");
    if let Ok(s) = std::fs::read_to_string(&cfg_dir) {
        if let Ok(v) = serde_json::from_str::<Value>(&s) {
            if let Some(provs) = v.get("provider").and_then(|x| x.as_object()) {
                for (prov, conf) in provs {
                    if let Some(key) = conf
                        .get("options")
                        .and_then(|o| o.get("apiKey"))
                        .and_then(|x| x.as_str())
                    {
                        let key = key.trim();
                        if !key.is_empty() {
                            providers
                                .entry(prov.to_ascii_lowercase())
                                .or_insert_with(|| key.to_string());
                        }
                    }
                }
            }
        }
    }
    for (prov, key) in &providers {
        if prov.contains("bailian")
            || prov.contains("dashscope")
            || prov.contains("qwen")
            || prov.contains("ali")
        {
            return Some(key.clone());
        }
    }
    providers.into_iter().next().map(|(_, k)| k)
}

fn looks_like_audio(b: &[u8]) -> bool {
    (b.len() > 2 && b[0] == 0xFF && (b[1] & 0xE0) == 0xE0)
        || b.starts_with(b"ID3")
        || b.starts_with(b"RIFF")
}

async fn audio_from_url_or_b64(client: &reqwest::Client, au: &str) -> Result<Vec<u8>, String> {
    if let Some(stripped) = au.strip_prefix("data:") {
        let comma = stripped.find(',').ok_or("audio data URL 格式异常")?;
        let b64 = stripped[comma + 1..].trim();
        return base64::engine::general_purpose::STANDARD
            .decode(b64)
            .map_err(|e| e.to_string());
    }
    let resp = client
        .get(au)
        .timeout(Duration::from_secs(60))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("audio 下载 http {}", resp.status().as_u16()));
    }
    resp.bytes()
        .await
        .map(|b| b.to_vec())
        .map_err(|e| e.to_string())
}

/// WAV(Riff) → mp3（生成器内转码，依赖本机 ffmpeg）。非 wav 原样返回。
fn to_mp3_if_wav(bytes: Vec<u8>) -> Result<Vec<u8>, String> {
    if !bytes.starts_with(b"RIFF") {
        return Ok(bytes);
    }
    use std::io::Write as _;
    use std::process::{Command, Stdio};
    let mut child = Command::new("ffmpeg")
        .args([
            "-y", "-i", "-", "-f", "mp3", "-ar", "24000", "-ac", "1", "-",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("启动 ffmpeg 失败（需安装并加入 PATH）: {e}"))?;
    child
        .stdin
        .take()
        .ok_or("ffmpeg stdin")?
        .write_all(&bytes)
        .map_err(|e| e.to_string())?;
    let out = child
        .wait_with_output()
        .map_err(|e| format!("ffmpeg 执行失败: {e}"))?;
    if !out.status.success() || out.stdout.is_empty() {
        return Err("ffmpeg 转 mp3 失败".into());
    }
    Ok(out.stdout)
}

/// Qwen3-TTS（qwen3-tts-flash 等）：multimodal-generation HTTP 通道，返回音频 URL/base64。
async fn synth_qwen3(
    client: &reqwest::Client,
    key: &str,
    model: &str,
    voice: &str,
    instruction: &str,
    text: &str,
) -> Result<Vec<u8>, String> {
    let mut input = json!({ "text": text });
    // 注意：instruct 系列不接受字符串 response_format，返回的永远是 wav，由 to_mp3_if_wav 转码
    let mut parameters = json!({});
    if model.contains("vc") {
        // Qwen3-TTS-VC：复刻音色，voice 放 input（与官方文档一致）；
        // 附加 voice_prompt 描述音色/语气（可被 vc 模型应用，忽略也不影响）
        input["voice"] = json!(voice);
        if !instruction.is_empty() {
            parameters["voice_prompt"] = json!(instruction);
        }
    } else if model.contains("instruct") {
        // qwen3-tts-instruct-*：系统音色（voice）打底 + 指令（instructions）调语气风格
        if !voice.is_empty() {
            parameters["voice"] = json!(voice);
        }
        if !instruction.is_empty() {
            parameters["instructions"] = json!(instruction);
        }
    } else {
        parameters["voice"] = json!(voice);
        if !instruction.is_empty() {
            parameters["instructions"] = json!(instruction);
        }
    }
    let body = json!({
        "model": model,
        "input": input,
        "parameters": parameters,
    });
    // 限流重试：rate limit / 429 时退避重试
    let mut attempt = 0;
    let v: Value = loop {
        attempt += 1;
        let resp = match client
            .post(QWEN3_HTTP_URL)
            .header("Authorization", format!("Bearer {key}"))
            .json(&body)
            .timeout(Duration::from_secs(120))
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                if attempt < 4 {
                    tokio::time::sleep(Duration::from_secs(2 * attempt)).await;
                    continue;
                }
                return Err(format!("请求失败: {e}"));
            }
        };
        let status = resp.status();
        let v_result: Result<Value, String> = resp.json().await.map_err(|e| e.to_string());
        match v_result {
            Ok(v) => {
                let msg = v["message"]
                    .as_str()
                    .or_else(|| v["code"].as_str())
                    .unwrap_or("");
                if status.is_success() {
                    break v;
                }
                if msg.to_ascii_lowercase().contains("rate limit") && attempt < 4 {
                    tokio::time::sleep(Duration::from_secs(2 * attempt)).await;
                    continue;
                }
                let detail = if msg.is_empty() {
                    format!("HTTP {}", status.as_u16())
                } else {
                    msg.to_string()
                };
                return Err(detail);
            }
            Err(_) if attempt < 4 => {
                tokio::time::sleep(Duration::from_secs(2 * attempt)).await;
                continue;
            }
            Err(e) => return Err(e),
        }
    };
    let au = v["output"]["audio"]["url"]
        .as_str()
        .or_else(|| v["output"]["audio"]["data"].as_str())
        .ok_or(format!(
            "无音频响应: {}",
            serde_json::to_string(&v).unwrap_or_default()
        ))?;
    if let Some(b64) = v["output"]["audio"]["data"].as_str() {
        if !b64.is_empty() {
            return to_mp3_if_wav(
                base64::engine::general_purpose::STANDARD
                    .decode(b64)
                    .map_err(|e| e.to_string())?,
            );
        }
    }
    let bytes = audio_from_url_or_b64(client, au).await?;
    to_mp3_if_wav(bytes)
}

/// 从 opencode.json 的 bailian provider baseURL 推导实时 WS 地址（TokenPlan 网关同构）。
fn find_ws_base(explicit: Option<String>) -> String {
    if let Some(u) = explicit {
        return u.trim_end_matches('/').to_string();
    }
    let mut cfg_dir = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let base = std::env::var_os("USERPROFILE")
                .or_else(|| std::env::var_os("HOME"))
                .unwrap_or_default();
            PathBuf::from(base).join(".config")
        });
    cfg_dir.push("opencode/opencode.json");
    if let Ok(s) = std::fs::read_to_string(&cfg_dir) {
        if let Ok(v) = serde_json::from_str::<Value>(&s) {
            let base = v
                .pointer("/provider/bailian/options/baseURL")
                .and_then(|x| x.as_str());
            if let Some(base) = base {
                let host = base
                    .split("://")
                    .nth(1)
                    .and_then(|r| r.split('/').next())
                    .unwrap_or(base);
                return format!("wss://{host}/api-ws/v1/inference");
            }
        }
    }
    WS_URL_DEFAULT.to_string()
}

/// qwen-audio-tts 实时语音合成（与 CosyVoice 同协议）：
/// run-task 握手 → task-started 后 continue-task 给文本 → 服务端分片回二进制音频 →
/// task-finished 结束。音频直接是响应分片拼出来的完整文件（mp3）。
async fn ws_send<S>(ws: &mut S, payload: &serde_json::Value) -> Result<(), String>
where
    S: futures_util::Sink<Message, Error = tokio_tungstenite::tungstenite::Error> + Unpin,
{
    use futures_util::SinkExt;
    ws.send(Message::Text(payload.to_string().into()))
        .await
        .map_err(|e| e.to_string())
}

async fn synth_ws(
    ws_base: &str,
    key: &str,
    model: &str,
    voice: &str,
    instruction: &str,
    text: &str,
) -> Result<Vec<u8>, String> {
    use futures_util::{SinkExt, StreamExt};
    let task_id = {
        let dur = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        format!("deepwhale-{dur}")
    };
    let send_text = ws_send::<_>;
    let mk_header = |action: &str| {
        json!({
            "action": action,
            "task_id": task_id,
            "streaming": "duplex",
        })
    };
    let run = |params: Option<&serde_json::Value>| {
        let mut parameters = json!({
            "text_type": "PlainText",
            "voice": voice,
            "format": "mp3",
            "sample_rate": 22050,
            "volume": 50,
            "rate": 1,
            "pitch": 1,
            "enable_ssml": false,
        });
        if let Some(p) = params {
            if let Some(ins) = p["instruction"].as_str() {
                parameters
                    .as_object_mut()
                    .unwrap()
                    .insert("instruction".into(), json!(ins));
            }
        }
        json!({
            "header": mk_header("run-task"),
            "payload": {
                "task_group": "audio",
                "task": "tts",
                "function": "SpeechSynthesizer",
                "model": model,
                "parameters": parameters,
                "input": {},
            }
        })
    };
    let continue_task = text.to_string();
    let finish = json!({
        "header": mk_header("finish-task"),
        "payload": { "input": {} }
    });

    let mut attempt = 0;
    let mut last_err: Option<String> = None;
    while attempt < 2 {
        attempt += 1;
        let uri: http::Uri = ws_base
            .parse()
            .map_err(|e| format!("WS URI 解析失败: {e}"))?;
        let req = tokio_tungstenite::tungstenite::ClientRequestBuilder::new(uri)
            .with_header("Authorization", format!("Bearer {key}"))
            .with_header("user-agent", "deepwhale-tts-gen/0.1");
        let (mut ws, _) = tokio_tungstenite::connect_async(req)
            .await
            .map_err(|e| format!("WS 连接失败: {e}"))?;

        // 第一次带 instruction；若服务端不认该参数则按错误响应退回不带的版本
        let params = (attempt == 1).then(|| json!({ "instruction": instruction }));
        send_text(&mut ws, &run(params.as_ref()))
            .await
            .map_err(|e| format!("发送 run-task 失败: {e}"))?;

        let mut audio: Vec<u8> = Vec::new();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
        'outer: loop {
            if tokio::time::Instant::now() >= deadline {
                if audio.is_empty() {
                    return Err("WS 接收超时且无音频".into());
                }
                break;
            }
            let msg = match tokio::time::timeout(Duration::from_secs(10), ws.next()).await {
                Ok(Some(m)) => m.map_err(|e| format!("WS 消息错误: {e}"))?,
                Ok(None) => break,
                Err(_) => continue,
            };
            match msg {
                Message::Binary(b) => {
                    if !b.is_empty() {
                        audio.extend_from_slice(&b);
                    }
                }
                Message::Text(s) => {
                    let v: Value = serde_json::from_str(s.as_str()).unwrap_or_default();
                    let ev = v["header"]["event"].as_str().unwrap_or("");
                    match ev {
                        "task-started" => {
                            send_text(
                                &mut ws,
                                &json!({
                                    "header": mk_header("continue-task"),
                                    "payload": { "input": { "text": continue_task } }
                                }),
                            )
                            .await
                            .map_err(|e| format!("发送 continue-task 失败: {e}"))?;
                            tokio::time::sleep(Duration::from_millis(500)).await;
                            let _ = send_text(&mut ws, &finish).await;
                        }
                        "task-finished" => {
                            break 'outer;
                        }
                        "task-failed" => {
                            if std::env::var("TTS_DEBUG").is_ok() {
                                eprintln!("[ws][failure] {s}");
                            }
                            last_err = Some(
                                v["payload"]["message"]
                                    .as_str()
                                    .or_else(|| v["payload"]["text"].as_str())
                                    .or_else(|| v["message"].as_str())
                                    .unwrap_or("task-failed")
                                    .to_string(),
                            );
                            break 'outer;
                        }
                        "error" => {
                            last_err = Some(v["message"].as_str().unwrap_or("error").to_string());
                            break 'outer;
                        }
                        _ => {}
                    }
                }
                Message::Ping(p) => {
                    ws.send(Message::Pong(p)).await.map_err(|e| e.to_string())?;
                }
                Message::Close(_) => break,
                _ => {}
            }
        }
        let _ = ws.send(Message::Text(finish.to_string().into())).await;
        let _ = ws.close(None).await;
        if audio.is_empty() {
            // 带 instruction 失败且还没退过 → 退掉再试一次
            if attempt == 1
                && last_err.as_deref().map(|e| {
                    e.to_ascii_lowercase().contains("instruction")
                        || e.to_ascii_lowercase().contains("param")
                }) == Some(true)
            {
                continue;
            }
            if let Some(e) = last_err {
                return Err(format!("{e}（无音频）"));
            }
            return Err("WS 无音频返回".into());
        }
        return Ok(audio);
    }
    Err("WS 尝试次数已尽".into())
}

/// 非实时 text2audio：优先异步任务（X-DashScope-Async），同时兼容同步直接返回音频/
/// 响应里带 base64 audio 的三种形态。
async fn synth(
    client: &reqwest::Client,
    key: &str,
    model: &str,
    voice: &str,
    instruction: &str,
    text: &str,
) -> Result<Vec<u8>, String> {
    let body = json!({
        "model": model,
        "input": { "text": text, "instruction": instruction },
        "instruction": instruction,
        "voice": voice,
        "response_format": "mp3",
        "format": "mp3",
        "sample_rate": 48000,
    });
    let resp = client
        .post(API_URL)
        .header("Authorization", format!("Bearer {key}"))
        .header("X-DashScope-Async", "enable")
        .json(&body)
        .timeout(Duration::from_secs(60))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let status = resp.status();
    let bytes = resp.bytes().await.map_err(|e| e.to_string())?;
    if status.is_success() && looks_like_audio(&bytes) {
        return Ok(bytes.to_vec());
    }
    let v: Value = match serde_json::from_slice(&bytes) {
        Ok(v) => v,
        Err(_) => json!({ "raw": String::from_utf8_lossy(&bytes).to_string() }),
    };
    let msg_field = |v: &Value| -> String {
        v["message"]
            .as_str()
            .or_else(|| v["code"].as_str())
            .or_else(|| v["errMsg"].as_str())
            .or_else(|| v["output"]["message"].as_str())
            .map(str::to_string)
            .unwrap_or_default()
    };
    if !status.is_success() {
        let msg = msg_field(&v);
        if msg.is_empty() {
            return Err(format!(
                "HTTP {}: {}",
                status.as_u16(),
                serde_json::to_string(&v).unwrap_or_default()
            ));
        }
        return Err(msg);
    }
    if let Some(au) = v["output"]["audio"].as_str() {
        return audio_from_url_or_b64(client, au).await;
    }
    let Some(task_id) = v["output"]["task_id"].as_str() else {
        return Err(format!(
            "未识别的响应: {}",
            serde_json::to_string(&v).unwrap_or_default()
        ));
    };
    for _ in 0..180 {
        tokio::time::sleep(Duration::from_secs(1)).await;
        let t = client
            .get(format!(
                "https://dashscope.aliyuncs.com/api/v1/tasks/{task_id}"
            ))
            .header("Authorization", format!("Bearer {key}"))
            .timeout(Duration::from_secs(30))
            .send()
            .await
            .map_err(|e| e.to_string())?;
        let tv: Value = t.json().await.map_err(|e| e.to_string())?;
        let st = tv["output"]["task_status"].as_str().unwrap_or("");
        match st {
            "SUCCEEDED" => {
                if let Some(au) = tv["output"]["audio_url"].as_str() {
                    return audio_from_url_or_b64(client, au).await;
                }
                if let Some(au) = tv["output"]["audio"]["url"].as_str() {
                    return audio_from_url_or_b64(client, au).await;
                }
                return Err("任务成功但响应里没有音频地址".into());
            }
            "FAILED" | "CANCELED" => {
                let msg = tv["output"]["message"].as_str().unwrap_or(st);
                return Err(msg.to_string());
            }
            _ => continue,
        }
    }
    Err("任务轮询超时".into())
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let mut key_arg: Option<String> = None;
    let mut ws_url_arg: Option<String> = None;
    let mut manifest_arg: Option<String> = None;
    let mut model_arg: Option<String> = None;
    let mut force = false;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--key" => key_arg = args.next(),
            "--force" => force = true,
            "--model" => model_arg = args.next(),
            "--ws-url" => ws_url_arg = args.next(),
            "--manifest" => manifest_arg = args.next(),
            _ => {
                eprintln!("未知参数: {a}\n用法: tts_gen [--key sk-xxx] [--ws-url wss://.../api-ws/v1/inference] [--manifest <json>] [--model model] [--force]");
                return ExitCode::from(2);
            }
        }
    }
    let Some(root) = repo_root() else {
        eprintln!("找不到仓库根：当前目录往上找不到 assets/voice/whale_voice.json");
        return ExitCode::from(2);
    };
    let manifest_path = manifest_arg
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("assets/voice/whale_voice.json"));
    let manifest: Value = match std::fs::read_to_string(&manifest_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
    {
        Some(v) => v,
        None => {
            eprintln!("读取/解析 {} 失败", manifest_path.display());
            return ExitCode::from(2);
        }
    };
    let Some(key) = find_key(key_arg.as_deref()) else {
        eprintln!("没有可用 API Key：请用 --key 显式传入，或设置 DASHSCOPE_API_KEY，或在 opencode 里配置过百炼");
        return ExitCode::from(2);
    };
    let model = model_arg.unwrap_or_else(|| {
        manifest["model"]
            .as_str()
            .unwrap_or("qwen-audio-3.0-tts-plus")
            .to_string()
    });
    let voice = manifest["voice"]
        .as_str()
        .unwrap_or("longanhuan_v3.6")
        .to_string();
    let instruction = manifest["instruction"].as_str().unwrap_or("").to_string();
    let Some(items) = manifest["items"].as_array() else {
        eprintln!("manifest 缺少 items");
        return ExitCode::from(2);
    };
    let out_dir = manifest_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| root.join("assets/voice"));
    let ws_base = find_ws_base(ws_url_arg);
    let client = reqwest::Client::new();
    let mut fail = 0usize;
    let mut done = 0usize;
    for item in items {
        let (Some(id), Some(text)) = (item["id"].as_str(), item["text"].as_str()) else {
            eprintln!("manifest 条目缺少 id/text: {}", item);
            fail += 1;
            continue;
        };
        // 条目级 voicePrompt 覆盖全局 instruction（VC 复刻时按每条台词单独调语调）
        let item_instruction = item["voicePrompt"].as_str().unwrap_or("").to_string();
        let instruction = if item_instruction.is_empty() {
            instruction.clone()
        } else {
            item_instruction
        };
        let out = out_dir.join(format!("{id}.mp3"));
        if out.exists() && !force {
            println!("跳过 {id}（已存在，--force 重新生成）");
            continue;
        }
        let result = if model.starts_with("qwen3-tts") {
            synth_qwen3(&client, &key, &model, &voice, &instruction, text).await
        } else {
            match synth_ws(&ws_base, &key, &model, &voice, &instruction, text).await {
                Ok(bytes) => Ok(bytes),
                Err(ws_err) => {
                    // 标准 DashScope key 走 HTTP 非实时通道兜底
                    match synth(&client, &key, &model, &voice, &instruction, text).await {
                        Ok(bytes) => Ok(bytes),
                        Err(h_err) => Err(format!("WS 失败({ws_err})；HTTP 失败({h_err})")),
                    }
                }
            }
        };
        match result {
            Ok(bytes) => {
                if bytes.is_empty() {
                    eprintln!("失败 {id}: 返回空音频");
                    fail += 1;
                    continue;
                }
                if let Err(e) = std::fs::write(&out, &bytes) {
                    eprintln!("失败 {id}: 写文件 {e}");
                    fail += 1;
                    continue;
                }
                done += 1;
                println!("生成 {id}.mp3 ({} 字节)", bytes.len());
            }
            Err(e) => {
                eprintln!("失败 {id}「{text}」: {e}");
                fail += 1;
            }
        }
        // 挫开请求间隔，缓解部分模型的请求速率限制
        tokio::time::sleep(Duration::from_millis(700)).await;
    }
    println!(
        "完成：生成 {done} 条，失败 {fail} 条，目录 {}",
        out_dir.display()
    );
    if fail > 0 {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}
