# 小鲸鱼余额挂件（桌面版）

> QQ 宠物式透明桌面小鲸鱼：实时展示 DeepSeek / 阿里百炼（TokenPlan 订阅制）等多供应商 API 余额与今日已用。

基于 Tauri v2 的独立桌面程序，全屏透明置顶、可拖拽吸附、支持本地会话记账。不依赖浏览器插件宿主，开箱即用。

## 特性

- 🐋 **透明置顶小鲸鱼**：全屏透明无边框置顶，常驻桌面，拖拽 + 四边四分之一吸附（角落可组合）
- 💰 **多供应商余额**：
  - DeepSeek 按量余额：60 秒自动刷新 + 点击鲸鱼手动刷新；余额变化数字滚动动画；瞬时网络抖动沿用最近余额不报错
  - 阿里百炼 TokenPlan（订阅制）：显示周剩余额度百分比与周重置时间
  - 点击鲸鱼在已启用供应商间轮换（deepseek → 百炼 → …）
  - 凭据缺失的供应商自动跳过
- 📊 **今日已用（本地记账）**，直接按会话历史重算当日消耗：
  - opencode 记账：读取 `opencode.sqlite` 会话历史
  - Claude Code 记账：读取 `~/.claude/projects/**` 的 jsonl 用量记录
  - 计价表：供应商/模型单价四段拆分（输入/输出/缓存读/缓存写）+ 峰谷两档（周末全天低谷价，2026-08-23 起）；未配置模型自动套内置官方价
- 🎚️ **汉堡菜单**（悬停鲸鱼出现）：大小缩放、音效切换等
- 🔊 **音效**：按压/松手音效，三套可选（小黄鸭 / 音效1 / 鲸语，缺失时静默降级）
- 💬 **随机台词**：点击气泡切换台词段，5 秒自动收起；**碎碎念同步语音配音**（百炼 qwen-audio-3.0-tts-plus 离线生成、内嵌播放）
- 🧸 **按压 Q 弹**玩偶效果；左吸附水平镜像翻转
- 🔄 **在线更新**：托盘或设置页「检查更新」，新版本一键下载安装（GitHub Releases 源 + minisign 签名校验），无需重新下载覆盖安装包

## 环境要求

| 依赖 | 版本 |
| ---- | ---- |
| Rust | stable（含 cargo、rustfmt、clippy） |
| Node.js | >= 20（含 npm） |
| OS | Windows 10/11（当前目标平台） |
| WebView2 | 系统内置（Windows 10+ 自带，无需安装） |

后端依赖音效、打包等由 [Tauri v2](https://tauri.app) 构建链自动拉取。

## 快速开始

```powershell
# 1. 安装前端 CLI 依赖
cd desktop
npm install

# 2. 开发运行（热重载）
npm run tauri dev
```

首次运行后，在**托盘菜单 → 设置**填写凭证（见下文「凭证配置」）。窗口缺失时点击托盘图标重新显示。

## 编译打包

```powershell
cd desktop
# 在线更新的产物需签名（CI 用 GitHub secrets，本地构建手动注入私钥）
$env:TAURI_SIGNING_PRIVATE_KEY = Get-Content "$env:USERPROFILE\.tauri\deepwhale.key" -Raw
$env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = '你的私钥密码'   # 没设密码则留空字符串
npm run tauri build   # 产出 Windows 安装包（.msi/.exe/.nsis.zip + latest.json）
```

产物位于 `desktop/src-tauri/target/release/bundle/`。

桌面版与旧版浏览器插件互不影响，可同时使用。

## 配置

配置文件：Windows `%APPDATA%\com.leviathan.deepwhale-desktop\config.json`。

### 凭证

| 供应商 | 说明 |
| ------ | ---- |
| DeepSeek | 填入 `DEEPSEEK_API_KEY`（按量余额）。平台会话令牌可点「自动抓取」，用独立浏览器 + CDP 嗅探登录态获取 |
| 百炼 TokenPlan | 控制台内部接口无公开 API-key 入口。设置页点「自动抓取百炼登录态」，拉起独立浏览器吸取已登录浏览器的 Cookie + 请求体；凭据过期后重新抓取即可 |

### 今日已用（本地记账）

- 数据源：`opencode.sqlite` 与 `~/.claude/projects/**` 当日会话记录
- 计价：按供应商/模型匹配计价表；未配置模型兜底内置官方价（DeepSeek / Claude / Qwen 系）；其他模型不计

## 项目结构

```text
desktop/
├── public/                  # 前端
│   ├── index.html           # 主程序入口
│   ├── widget.js            # 小鲸鱼交互（拖拽/吸附/气泡/供应商轮换）
│   ├── settings.html        # 设置页
│   └── settings.js          # 设置页逻辑
└── src-tauri/
    ├── src/                 # Rust 后端
    │   ├── main.rs          # 窗口/托盘/命令注册
    │   ├── balance.rs       # 余额抓取（DeepSeek / 百炼订阅）
    │   ├── login.rs         # CDP 凭据自动抓取
    │   ├── config.rs        # 配置持久化
    │   ├── pricing.rs       # 计价表/峰谷定价
    │   ├── opencode.rs      # opencode.sqlite 记账
    │   └── claude.rs        # Claude Code jsonl 记账
    ├── Cargo.toml
    └── tauri.conf.json
```

## 生成鲸语音效

「鲸语」音效集与气泡碎碎念配音由百炼 `qwen-audio-3.0-tts-plus` 离线生成，产物 mp3 提交进 `assets/voice/`，运行时零 API 依赖。

```powershell
cd desktop/src-tauri
cargo run --bin tts_gen            # 幂等：已存在的 mp3 跳过；改文案后删对应文件或 --force 重跑
```

- 文本/音色/指令清单：`assets/voice/whale_voice.json`（单一事实源）
- Key 来源：`--key` > 环境变量 `DASHSCOPE_API_KEY` > opencode `auth.json`/`opencode.json` 里的百炼 key（TokenPlan 网关自动推导 WS 地址；用普通 DashScope Key 复刻/生成时加 `--ws-url wss://dashscope.aliyuncs.com/api-ws/v1/inference`）
- 当前 `voice` 是「声音复刻」出的专属音色（音色 id 与账号绑定，见阿里云声音复刻文档）。换账号/重做复刻后，把新 `voice_id` 填回 manifest 再重跑生成
- 改台词文案后，同时更新 `public/widget.js` 里 `MURMUR_B/A/JOKE` 对应文本与语音 id，再重跑生成即可；`cargo test` 会兜底校验 manifest 与资源一一对应

## 验证

```powershell
cd desktop/src-tauri
cargo fmt --check                                # 格式
cargo clippy --all-targets -- -D warnings        # 静态检查
cargo test                                       # 16 项单元测试
```

## 常见问题

- **百炼显示 `--`**：凭证过期/未抓取，去托盘「设置 → 百炼」重新自动抓取。
- **今日已用为 0**：数据源无当日会话，或模型未匹配计价表；仅 DeepSeek/Claude/Qwen 系有内置兜底价。
- **没有声音**：音效 mp3 缺失时静默降级，忽略即可。
- **窗口不见**：点击托盘图标菜单显示。