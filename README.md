# 小鲸鱼余额挂件（DeepSeek Balance Whale Widget）

桌面端 QQ 宠物式透明小鲸鱼：悬浮显示 DeepSeek 与百炼（阿里云百炼）等多供应商 API 余额与今日已用，独立桌面程序（Tauri v2），不依赖任何浏览器插件宿主。

## 特性

- 🐋 **透明置顶小鲸鱼**：全屏透明无边框置顶窗口，桌面任意角落常驻
- 💰 **多供应商余额**：
  - **DeepSeek** 按量余额（60 秒自动刷新 + 点击鲸鱼手动刷新；余额变化数字滚动动画；瞬时网络抖动自动沿用最近余额不报错）
  - **百炼 TokenPlan（订阅制）**：周剩余额度（百分比）+ 周重置时间
  - **供应商切换**：点击鲸鱼在已启用供应商间轮换（deepseek → 百炼 → …）
- 📊 **今日已用（本地记账）**：直接按会话历史重算当日消耗，无需余额差值
  - **opencode 记账**：读取 `opencode.sqlite` 会话历史，按供应商/模型匹配计价表重算
  - **Claude Code 记账**：读取 `~/.claude/projects/**` 的 jsonl 用量记录计价
  - **计价表**：按供应商配置模型单价（输入/输出/缓存读/缓存写四段拆分）、峰谷两档；未配置模型自动套内置官方价
- 🖱️ **拖拽 + 四边四分之一吸附**（左/右/上/下，角落可组合）
- 🔄 左吸附时整体**水平镜像翻转**（文字同步反向、带动画）
- 🧸 **按压 Q 弹**玩偶效果（按压时底部坐标不变）
- 🎚️ **汉堡菜单**（悬停鲸鱼参考线出现）：大小滑块、音效切换、音量调节等
- 🔊 **音效**：按压/松手音效（可选 mp3，缺失时静默降级）
- 💬 **随机台词**：点击气泡切换随机台词段（加权随机，含峰谷提示/今日已用/卖萌吐槽），再点一次关闭；气泡显示 5 秒自动收起

## 目录结构

```text
desktop/
├── public/
│   ├── widget.js          # 小鲸鱼前端交互（拖拽/吸附/气泡/供应商轮换）
│   ├── settings.js        # 设置页逻辑
│   ├── settings.html      # 设置页（凭证/百炼/计价表）
│   └── index.html         # 主程序入口
└── src-tauri/
    ├── src/
    │   ├── main.rs        # 窗口/托盘/命令注册
    │   ├── balance.rs     # 余额与百炼订阅抓取
    │   ├── login.rs       # CDP 凭据自动抓取
    │   ├── config.rs      # 配置持久化
    │   ├── pricing.rs     # 计价表/峰谷定价
    │   ├── opencode.rs    # opencode.sqlite 记账
    │   └── claude.rs      # Claude Code jsonl 记账
    ├── Cargo.toml
    └── tauri.conf.json
```

## 安装/开发

依赖：Rust 工具链、Node.js（前端构建）。

```powershell
cd desktop
npm install
npm run tauri dev    # 开发运行
npm run tauri build  # 打包 Windows 安装包
```

首次运行在托盘菜单「设置」填写凭证，配置存于系统 app data 目录（Windows：`%APPDATA%\com.leviathan.dshwhale\config.json`）。

## 凭证配置（设置页）

- **DeepSeek**：填 `DEEPSEEK_API_KEY`（按量余额）。平台会话令牌可点「自动抓取」用独立浏览器 + CDP 嗅探登录态获得（用于平台用量接口）。
- **百炼 TokenPlan（订阅制）**：控制台内部接口没有公开 API-key 入口，点「自动抓取百炼登录态」会拉起独立浏览器、在已登录浏览器上吸取登录 Cookie + 请求体并保存。凭据会过期，过期后重新抓取即可。
- 计价表：可编辑模型单价与峰谷档位。
- 无凭据的供应商在余额轮换中自动跳过。

## 验证

```powershell
cd desktop/src-tauri
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test    # 16 项单元测试
```

## 常见问题

- **百炼显示 `--` / 未配置**：凭证过期或尚未抓取，去设置页重新「自动抓取百炼登录态」。
- **今日已用为 0**：本地记账数据源（`opencode.sqlite` / `~/.claude/projects`）没有当日会话，或模型未匹配计价表（未配置模型按内置官方价兜底；非 DeepSeek/Claude/Qwen 系不计）。
- **没有声音**：确认音效 mp3 文件在资源目录；缺失时静默降级。
- **窗口找不到/隐藏**：在托盘图标菜单点击显示。