(function () {
if (window.__dshWhaleWidget) return
window.__dshWhaleWidget = true

var MIN_SCALE = 0.6
// 小窗最小可用大小：固定尺寸的菜单按钮 + 气泡文本在低于 scale 1.0 时会把角色
// 头部盖住/文本出界，直接限制拖动下限（用户选择方案：不缩小到显示不全）。
var MIN_SIZE_SCALE = 1.0
var MAX_SCALE = 2.5
var STEP = 0.1
var CLICK_SQ = 16
var REFRESH_MS = 60000
var CHANGE_MS = 900
var ANIM_MS = 700
var BUBBLE_MS = 5000
var PASSTHROUGH_POLL_MS = 66
// Tauri IPC bridge: 取代原浏览器版的路由请求（/dsh-whale/*）
var WHALE = window.__TAURI__ && window.__TAURI__.core ? window.__TAURI__.core : null
function apiBalance() { return WHALE.invoke('get_balance') }
function apiGetConfig() { return WHALE.invoke('get_config') }
function apiSetConfig(cfg) { return WHALE.invoke('set_config', { payload: cfg }) }
function apiImageUrl() { return WHALE.invoke('image_data_url') }
function apiSoundUrl(action, set) { return WHALE.invoke('sound_data_url', { action: action, set: set }) }
function apiVoiceUrl(id) { return WHALE.invoke('voice_data_url', { id: id }) }
function apiGetWindowBounds() { return WHALE.invoke('get_window_bounds') }
function apiMoveWindow(x, y) { return WHALE.invoke('move_window', { x: x, y: y }) }
function apiSetWindowBounds(x, y, width, height) {
  return WHALE.invoke('set_window_bounds', { x: x, y: y, width: width, height: height })
}
var IMG_DATA_URL = null
var IMG_URL = null
var SOUND_URLS = { duck: { press: null, release: null }, fx1: { press: null, release: null }, whale: { press: null, release: null } }

var root = document.createElement('div')
root.className = 'dshwv-root'

var dshStage = document.createElement('div')
dshStage.className = 'dshwv-dsh-stage'
var dshSprite = document.createElement('div')
dshSprite.className = 'dshwv-dsh-sprite'
// 视频外观（dsh-pet 素材）：一个状态一段 VP9-alpha webm，和精灵图互斥显示。
// 两路交替用：换片段时先让新的一路解码出首帧再淡入，避免单元素换 src 时整只消失一帧
//（拖拽/切动作的闪烁就是这么来的）。
var DSH_VIDS = [dshMakeVideo(), dshMakeVideo()]
var dshVidIdx = 0
var dshVidFile = ''
function dshMakeVideo() {
  var v = document.createElement('video')
  v.className = 'dshwv-dsh-video'
  v.muted = true
  v.playsInline = true
  v.preload = 'auto'
  v.setAttribute('muted', '')
  v.setAttribute('disablepictureinpicture', '')
  v.addEventListener('loadedmetadata', onDshVideoMeta)
  return v
}
function dshVideoPlay(v) {
  var p = v.play()
  if (p && p.catch) p.catch(function () {})
}
function dshPauseVideos() {
  DSH_VIDS.forEach(function (v) { v.pause() })
}
var dshPulse = document.createElement('div')
dshPulse.className = 'dshwv-dsh-pulse'
dshPulse.innerHTML = '<span class="dshwv-dsh-state"></span><span class="dshwv-dsh-detail"></span>'
dshStage.appendChild(DSH_VIDS[0])
dshStage.appendChild(DSH_VIDS[1])
dshStage.appendChild(dshSprite)
dshStage.appendChild(dshPulse)

var img = document.createElement('img')
img.className = 'dshwv-img'
img.src = IMG_URL
img.alt = 'DeepSeek 余额'
img.draggable = false

var menuBtn = document.createElement('button')
menuBtn.type = 'button'
menuBtn.className = 'dshwv-menu-btn'
menuBtn.title = '菜单'
menuBtn.innerHTML = '<span></span><span></span><span></span>'
menuBtn.addEventListener('click', function (e) { e.stopPropagation(); toggleMenu() })

var menuBox = document.createElement('div')
menuBox.className = 'dshwv-menu'
function menuLabel(text) {
  var s = document.createElement('span')
  s.textContent = text
  return s
}
function menuRow() {
  var r = document.createElement('div')
  r.className = 'dshwv-menu-row'
  return r
}
var scaleInput = document.createElement('input')
scaleInput.type = 'range'
scaleInput.min = String(MIN_SIZE_SCALE)
scaleInput.max = String(MAX_SCALE)
scaleInput.step = '0.1'
scaleInput.className = 'dshwv-range'
scaleInput.value = '1.5'
var scaleNumber = document.createElement('input')
scaleNumber.type = 'number'
scaleNumber.min = String(scaleToDisplay(MIN_SIZE_SCALE))
scaleNumber.max = '20'
scaleNumber.step = '1'
scaleNumber.className = 'dshwv-number'
scaleNumber.value = '10'
// 鲸鱼实时随滑块大小变化：每个 input 事件都按当前值 resize 鲸鱼+窗口。
// 但会带着滑块跑 → 按下时把菜单钉死在屏幕位置，并**关掉菜单 left/top 过渡动画**
// （动画会让滑块在窗口原点变化时滞后漂移，点击轨道按绝对坐标定位就读错值）。
// 松开（change）后才解除钉住、把菜单重新对齐到鲸鱼头顶上方。
scaleInput.addEventListener('pointerdown', function () {
  document.body.classList.add('dshwv-live-scale')
  if (menuOpen) pinMenuNow()
})
scaleInput.addEventListener('input', function () { setScale(scaleInput.value, false) })
scaleInput.addEventListener('change', function () {
  document.body.classList.remove('dshwv-live-scale')
  menuPin = null
  setScale(scaleInput.value, true)
})
scaleInput.addEventListener('pointercancel', function () {
  document.body.classList.remove('dshwv-live-scale')
})
document.addEventListener('pointerup', function () {
  document.body.classList.remove('dshwv-live-scale')
})
scaleNumber.addEventListener('change', function () {
  var v = Math.round(Number(scaleNumber.value))
  var s = MIN_SCALE + Math.max(0, Math.min(20, v) - 1) * (MAX_SCALE - MIN_SCALE) / 19
  menuPin = null
  setScale(s, true)
})
var soundSelect = document.createElement('select')
soundSelect.className = 'dshwv-sound'
function soundOpt(value, label) {
  var o = document.createElement('option')
  o.value = value
  o.textContent = label
  return o
}
soundSelect.appendChild(soundOpt('duck', '小黄鸭'))
soundSelect.appendChild(soundOpt('fx1', '音效1'))
soundSelect.appendChild(soundOpt('whale', '鲸语'))
soundSelect.addEventListener('change', function () { setSoundSet(soundSelect.value) })
var usageSelect = document.createElement('select')
usageSelect.className = 'dshwv-sound'
usageSelect.appendChild(soundOpt('token', '实时·令牌 (推荐，设置里自动获取)'))
usageSelect.appendChild(soundOpt('opencode', '小鲸鱼记账 (本地会话按计价表计算)'))
usageSelect.addEventListener('change', function () { setUsageMode(usageSelect.value) })
var appearanceSelect = document.createElement('select')
appearanceSelect.className = 'dshwv-sound'
appearanceSelect.appendChild(soundOpt('pet', '大肥鱼'))
appearanceSelect.appendChild(soundOpt('dsh', '大肥鱼·帧动画'))
appearanceSelect.appendChild(soundOpt('classic', '余额鲸鱼'))
appearanceSelect.addEventListener('change', function () { setAppearance(appearanceSelect.value, true) })
var dndBox = document.createElement('input')
dndBox.type = 'checkbox'
dndBox.className = 'dshwv-check'
dndBox.title = '勿扰：大肥鱼保持睡觉，不自动活动'
dndBox.addEventListener('change', function () { setDnd(dndBox.checked, true) })
// 启用的供应商列表（按此顺序点击鲸鱼轮换）：deepseek 恒有，百炼/梭子蟹有凭据才加入；
// 再按设置页「点击展示的提供商」过滤（空列表按后端归一后的默认全选处理）。
function providerList() {
  var list = ['deepseek']
  if (state.hasBailian) list.push('bailian')
  if (state.hasSuoxie) list.push('suoxie')
  var on = state.bubbleProviders
  if (Array.isArray(on) && on.length) {
    list = list.filter(function (p) { return on.indexOf(p) >= 0 })
  }
  return list.length ? list : ['deepseek']
}
function normalizeProvider() {
  var list = providerList()
  if (list.indexOf(state.provider) < 0) state.provider = list[0]
}
var row1 = menuRow()
row1.appendChild(menuLabel('大小'))
row1.appendChild(scaleInput)
row1.appendChild(scaleNumber)
var row2 = menuRow()
row2.appendChild(menuLabel('音效'))
row2.appendChild(soundSelect)
var volInput = document.createElement('input')
volInput.type = 'range'
volInput.min = '0'
volInput.max = '1'
volInput.step = '0.05'
volInput.className = 'dshwv-range'
volInput.value = '0.9'
var volPct = document.createElement('span')
volPct.className = 'dshwv-volpct'
volPct.textContent = '90%'
volInput.addEventListener('input', function () { setVol(volInput.value) })
var row3 = menuRow()
row3.appendChild(menuLabel('音量'))
row3.appendChild(volInput)
row3.appendChild(volPct)
var row4 = menuRow()
row4.appendChild(menuLabel('用量'))
row4.appendChild(usageSelect)
var rowAppearance = menuRow()
rowAppearance.appendChild(menuLabel('外观'))
rowAppearance.appendChild(appearanceSelect)
var rowDnd = menuRow()
rowDnd.appendChild(menuLabel('勿扰'))
rowDnd.appendChild(dndBox)
var rowSet = menuRow()
var setBtn = document.createElement('button')
setBtn.type = 'button'
setBtn.className = 'dshwv-setbtn'
setBtn.textContent = '设置 API Key'
setBtn.addEventListener('click', function () {
  closeMenu()
  WHALE.invoke('open_settings').catch(function () {})
})
rowSet.appendChild(setBtn)
menuBox.appendChild(row1)
menuBox.appendChild(row2)
menuBox.appendChild(row3)
menuBox.appendChild(row4)
menuBox.appendChild(rowAppearance)
menuBox.appendChild(rowDnd)
menuBox.appendChild(rowSet)

var textBox = document.createElement('div')
textBox.className = 'dshwv-text'
var labelEl = document.createElement('div')
labelEl.className = 'dshwv-label'
labelEl.textContent = '余额'
var amountEl = document.createElement('div')
amountEl.className = 'dshwv-amount'
// 百炼气泡第 2 行的小字说明（周剩余额度）
var subEl = document.createElement('div')
subEl.className = 'dshwv-sub'
subEl.style.display = 'none'
var hintEl = document.createElement('div')
hintEl.className = 'dshwv-hint'
textBox.appendChild(labelEl)
textBox.appendChild(subEl)
textBox.appendChild(amountEl)
textBox.appendChild(hintEl)

var bubbleBox = document.createElement('div')
bubbleBox.className = 'dshwv-bubble'
bubbleBox.innerHTML = '<svg viewBox="0 0 1026 700" preserveAspectRatio="xMidYMid meet" xmlns="http://www.w3.org/2000/svg">' +
  '<path class="dshwv-bshape" fill="#FFFFFF" stroke="#203170" stroke-width="18" stroke-linejoin="round" stroke-linecap="round" d="M 827 248 A 373 232 0 1 0 81 246 A 373 232 0 0 0 301 465 A 57 32 10 0 0 413 484 A 373 232 0 0 0 827 248 Z"/>' +
  '<ellipse class="dshwv-b1" cx="352" cy="561" rx="37.5" ry="26" fill="#FFFFFF" stroke="#203170" stroke-width="18"/>' +
  '<ellipse class="dshwv-b2" cx="442" cy="646" rx="24.5" ry="18" fill="#FFFFFF" stroke="#203170" stroke-width="18"/>' +
  '</svg>'
bubbleBox.appendChild(textBox)
bubbleBox.addEventListener('click', function (e) {
  e.stopPropagation()
  if (!bubbleShown) return
  if (stickyLines) return
  if (bubbleRandomActive) {
    // 再次点击：关闭
    hideBubble()
  } else {
    // 首次点击：切到随机台词段（不延长总显示时长）
    bubbleRandomActive = true
    bubbleRandomLines = pickRandomLines(state.provider === 'bailian' ? 0 : -1)
    swapBubbleContent(function () { applyBubbleLines(bubbleRandomLines) })
    // 碎碎念配音：单行段的第一行带 v 语音 id
    var v = bubbleRandomLines && bubbleRandomLines[1] && bubbleRandomLines[1].v
    if (v) playVoice(v)
  }
})

var body = document.createElement('div')
body.className = 'dshwv-body'
body.appendChild(img)
body.appendChild(bubbleBox)
root.appendChild(body)
root.appendChild(dshStage)
root.appendChild(menuBtn)
document.body.appendChild(root)
document.body.appendChild(menuBox)

// Position model (桌面小窗版): 窗口本身就是鲸鱼小窗，state.left/top 是**窗口在屏幕上的
// 逻辑坐标**（相对主显示器左上角）。锚定信息（h/v + offsets）仍在 state 中，settle() 用
// 它在窗口尺寸变化/吸附时重算坐标，保持鲸鱼贴住锚定边缘。widget.js 不再移动 root 的
// left/top —— 移动窗口交给 move_window/set_window_bounds IPC。
var state = {
  scale: 1.5,
  h: 'right',
  hOff: 0,
  v: 'bottom',
  vOff: 0,
  left: 0,
  top: 0,
  balance: null,
  currency: null,
  todayUsage: null,
  isPeak: false,
  status: 'loading',
  message: '',
  appearance: 'pet',
  dnd: false,
  provider: 'deepseek',
  bubbleProviders: null,
  hasBailian: false,
  bailian: null,
  hasSuoxie: false,
  suoxie: null,
  credentialRefreshNeeded: false,
  credentialRefresh: '',
  // 凭据失效待用户处理：非空时单击鲸鱼改为「更新登录凭据」，不再是轮换供应商
  needCredential: '',
  // login = 登录令牌失效；usage = 仅用量令牌失效（余额仍正常）
  credentialScope: 'login',
  credentialBusy: false
}
var dshAppearance = true
var dshPet = true
var dshAnimTimer = null
var dshFrame = 0
var dshManifest = null
var petClips = null
var petModes = []
var petClicks = []
var petBalance = []
var dshWalking = false
var dshPlaying = ''
var DSH_STATE_LABELS = { loading: '正在连接', changing: '余额变化', ok: '准备好了', error: '请求失败' }
// 随机作息：平均 3 分钟一次（每小时 20 次），每次随机挑一个动作播完回 idle；
// 醒着待机 90s 后入睡，点一下鲸鱼会唤醒。勿扰模式常睡。每个动作仍用 manifest
// 自己的 playback 语义（loop / pingpong / once / blink）。
var DSH_MODES = [
  { state: 'working', ms: 5000 },
  { state: 'joy', ms: 3000 },
  { state: 'walk', ms: 5000 },
  { state: 'wait', ms: 3500 },
  { state: 'eat', ms: 2500 },
  { state: 'play', ms: 3500 },
  { state: 'welcome', ms: 2500 },
  { state: 'disappointed', ms: 3000 },
  { state: 'celebrate', ms: 3000 },
  { state: 'think', ms: 4000 }
]
var DSH_NEXT_MIN_MS = 2 * 60 * 1000
var DSH_NEXT_MAX_MS = 4 * 60 * 1000
var DSH_IDLE_MS = 90 * 1000
var DSH_WAKE_MS = 2500
var DSH_ERROR_MS = 2500
// 点击回应 / 余额档位的兜底时长：metadata 到了会按片段真实时长（~10s）覆盖
var DSH_CLICK_MS = 3500
var DSH_BALANCE_MS = 5000
var dshDnd = false
var dshSleeping = false
var dshWakeUntil = 0
var dshErrorUntil = 0
var dshErrorArmed = false
var dshMode = ''
var dshLastMode = ''
var dshNextTimer = null
var dshModeTimer = null
var dshSleepTimer = null
var dshWakeTimer = null
var dshErrorTimer = null
var dshWalkRaf = null
var dshWalkDir = 1
var dshWalkLast = 0
// 单击回应 / 余额档位：都是「播一小段就回 idle」的临时片段（值是 manifest 里的文件名）
var dshClickKey = ''
var dshClickUntil = 0
var dshClickTimer = null
var dshBalanceKey = ''
var dshBalanceUntil = 0
var dshBalanceTimer = null
function dshAsset(name) { return 'dsh/whale-girl/' + name + '.png' }
function dshState() {
  // 死区：按下但还没移动出阈值不算拖拽，保持原姿势，否则单击会闪一下 drag
  if (drag && drag.active && drag.moved) return 'drag'
  if (dshDnd) return 'sleep' // 勿扰优先：连报错也不打断睡眠（数字/气泡照常显示）
  if (state.status === 'error' && Date.now() < dshErrorUntil) return 'error'
  if (dshPet && dshClickKey && Date.now() < dshClickUntil) return dshClickKey
  if (dshPet && dshBalanceKey && Date.now() < dshBalanceUntil) return dshBalanceKey
  if (Date.now() < dshWakeUntil) return 'wake'
  if (dshMode) return dshMode
  if (dshSleeping) return 'sleep'
  if (state.status === 'loading') return 'think' // 通知类状态不打断动作，只在休息时显示
  return 'idle'
}
// 帧图用百分比尺寸，随窗口缩放自动跟着变大变小（manifest 的 110px 只适合网页端）。
// manifest 拉取失败时按这套兜底，避免精灵层空白导致宠物整个消失。
var DSH_FALLBACK = {
  idle: { frames: 3, fps: 2, playback: 'blink' },
  celebrate: { frames: 3, fps: 4, playback: 'loop' },
  think: { frames: 1, fps: 2, playback: 'loop', motion: 'float' },
  error: { frames: 2, fps: 8, playback: 'once', motion: 'shake' },
  drag: { frames: 1, fps: 5, playback: 'loop', motion: 'tilt' }
}
// background-position 的百分比算的是「容器与图片的剩余空间」比例，不是元素宽度的倍数：
// 图片宽是容器的 N 倍时，第 i 帧对应 i/(N-1)*100%。写 -i*100% 会算到画面外的空带，
// 帧一换精灵就整个消失（眨眼/庆祝时最明显）。
function dshPos(i, frames) {
  return (frames > 1 ? (i * 100) / (frames - 1) : 0) + '% 0'
}
// 按 manifest 的 playback 语义播放：loop 顺播 / pingpong 往返 / once 播完停尾帧 /
// blink 停在第 0 帧、随机 3-9s 眨一遍（盲轮播会让常态帧一直抖，看着像闪烁）。
function playDsh(name, sc, frames) {
  clearTimeout(dshAnimTimer)
  dshAnimTimer = null
  var frameMs = Math.max(80, Math.round(1000 / Math.max(1, (sc && sc.fps) || 2)))
  var mode = (sc && sc.playback) || 'loop'
  var dir = 1
  dshFrame = 0
  dshSprite.style.backgroundPosition = dshPos(0, frames)
  if (frames <= 1) return
  function step() {
    if (dshPlaying !== name) return // 状态已切换：旧动画自行退出
    if (mode === 'blink') {
      if (dshFrame === 0) {
        dshFrame = 1
        dshSprite.style.backgroundPosition = dshPos(1, frames)
        dshAnimTimer = setTimeout(step, frameMs)
        return
      }
      dshFrame += 1
      if (dshFrame >= frames) {
        dshFrame = 0
        dshSprite.style.backgroundPosition = dshPos(0, frames)
        dshAnimTimer = setTimeout(step, 3000 + Math.random() * 6000)
        return
      }
      dshSprite.style.backgroundPosition = dshPos(dshFrame, frames)
      dshAnimTimer = setTimeout(step, frameMs)
      return
    }
    if (mode === 'once') {
      if (dshFrame < frames - 1) {
        dshFrame += 1
        dshSprite.style.backgroundPosition = dshPos(dshFrame, frames)
        dshAnimTimer = setTimeout(step, frameMs)
      }
      return
    }
    if (mode === 'pingpong') {
      if (dshFrame + dir > frames - 1 || dshFrame + dir < 0) dir = -dir
      dshFrame += dir
    } else {
      dshFrame = (dshFrame + 1) % frames
    }
    dshSprite.style.backgroundPosition = dshPos(dshFrame, frames)
    dshAnimTimer = setTimeout(step, frameMs)
  }
  dshAnimTimer = setTimeout(step, mode === 'blink' ? 3000 + Math.random() * 6000 : frameMs)
}
// 作息在「大肥鱼/视频外观 + manifest 已载入 + 未开勿扰 + 余额不报错」时运行。
function dshAmbientWanted() {
  if (!dshAppearance || dshDnd || state.status === 'error') return false
  return dshPet ? petClips !== null : dshManifest !== null
}
function clearDshTimers() {
  clearTimeout(dshNextTimer); dshNextTimer = null
  clearTimeout(dshModeTimer); dshModeTimer = null
  clearTimeout(dshSleepTimer); dshSleepTimer = null
  clearTimeout(dshWakeTimer); dshWakeTimer = null
}
function syncDshAmbient() {
  // 勿扰：停表且强制睡姿
  if (dshDnd) {
    clearDshTimers()
    dshMode = ''
    dshWakeUntil = 0
    dshSleeping = true
    renderDsh()
    return
  }
  // 不可用（还没 manifest / 报错 / 经典外观）：只停表，不擅自改睡姿——
  // 否则开机瞬间 manifest 还没到就被判成「该睡」，之后要等 9–15 分钟才会动。
  if (!dshAmbientWanted()) {
    clearDshTimers()
    dshMode = ''
    dshWakeUntil = 0
    return
  }
  if (!dshMode && !dshSleeping && dshWakeUntil === 0 && dshSleepTimer === null) armDshSleep()
  if (!dshNextTimer && !dshMode && dshWakeUntil === 0) armDshNext()
}
function armDshSleep() {
  clearTimeout(dshSleepTimer)
  dshSleepTimer = setTimeout(function () {
    dshSleepTimer = null
    if (!dshAmbientWanted() || dshMode) return
    dshSleeping = true
    renderDsh()
  }, DSH_IDLE_MS)
}
// 下一次随机动作：9–15 分钟均匀分布，均值 12 分钟 = 每小时 5 次。
function armDshNext() {
  clearTimeout(dshNextTimer)
  var ms = DSH_NEXT_MIN_MS + Math.random() * (DSH_NEXT_MAX_MS - DSH_NEXT_MIN_MS)
  dshNextTimer = setTimeout(fireDshMode, ms)
}
function fireDshMode() {
  dshNextTimer = null
  if (!dshAmbientWanted()) return
  if (drag && drag.active) {
    // 正在拖拽：延后 30s 再试，不打断手上的操作
    dshNextTimer = setTimeout(fireDshMode, 30 * 1000)
    return
  }
  if (dshSleeping) {
    dshSleeping = false
    dshWakeUntil = Date.now() + DSH_WAKE_MS
    renderDsh()
    dshWakeTimer = setTimeout(startDshMode, DSH_WAKE_MS)
    return
  }
  startDshMode()
}
// 随机动作池：视频外观用 manifest 的 modes（100 段），帧动画外观用 DSH_MODES。
// 返回 { key, ms }；视频的 ms=0 表示「等 metadata 到了按片段真实时长挂表」。
function dshPickMode() {
  var key
  if (dshPet && petModes.length) {
    key = petModes[Math.floor(Math.random() * petModes.length)]
    // 不连着重复同一个动作
    if (key === dshLastMode) key = petModes[(petModes.indexOf(key) + 1) % petModes.length]
    return { key: key, ms: 0 }
  }
  var pick = DSH_MODES[Math.floor(Math.random() * DSH_MODES.length)]
  if (pick.state === dshLastMode) pick = DSH_MODES[(DSH_MODES.indexOf(pick) + 1) % DSH_MODES.length]
  return { key: pick.state, ms: pick.ms }
}
function armDshModeTimer(ms) {
  clearTimeout(dshModeTimer)
  dshModeTimer = setTimeout(function () {
    dshModeTimer = null
    dshMode = ''
    renderDsh()
    if (!dshAmbientWanted()) return
    armDshSleep()
    armDshNext()
  }, ms)
}
// 片段时长决定这次展示多久（clip 3–12s），比写死 ms 更贴合：
// 之前点击回应写死 3.5s、余额档位写死 5s，而片段本身 10s，动画会被截断在半路。
function onDshVideoMeta(e) {
  if (!dshPet) return
  var v = e && e.currentTarget
  var d = v ? v.duration : 0
  if (!isFinite(d) || d <= 0) return
  var ms = Math.min(20000, d * 1000)
  if (dshClickKey && Date.now() < dshClickUntil) {
    dshClickUntil = Date.now() + ms
    clearTimeout(dshClickTimer)
    dshClickTimer = setTimeout(function () {
      dshClickTimer = null
      dshClickUntil = 0
      renderDsh()
    }, ms)
    return
  }
  if (dshBalanceKey && Date.now() < dshBalanceUntil) {
    dshBalanceUntil = Date.now() + ms
    clearTimeout(dshBalanceTimer)
    dshBalanceTimer = setTimeout(function () {
      dshBalanceTimer = null
      dshBalanceUntil = 0
      renderDsh()
    }, ms)
    return
  }
  if (!dshMode) return
  armDshModeTimer(Math.max(1500, Math.min(15000, d * 1000)))
}
function startDshMode() {
  dshWakeTimer = null
  dshWakeUntil = 0
  if (!dshAmbientWanted()) {
    renderDsh()
    return
  }
  var pick = dshPickMode()
  dshLastMode = pick.key
  dshMode = pick.key
  renderDsh()
  armDshModeTimer(pick.ms || 10000)
}
// 点击唤醒：睡着时先播 wake 再回 idle，并把入睡计时重新挂上。返回是否真的唤醒了。
function dshUserWake() {
  if (!dshAppearance || dshDnd || !dshSleeping) return false
  dshSleeping = false
  dshWakeUntil = Date.now() + DSH_WAKE_MS
  renderDsh()
  clearTimeout(dshWakeTimer)
  dshWakeTimer = setTimeout(function () {
    dshWakeTimer = null
    dshWakeUntil = 0
    renderDsh()
    if (dshAmbientWanted()) armDshSleep()
  }, DSH_WAKE_MS)
  return true
}
// walk：真的平移 OS 窗口（45px/s），顶到屏幕左右沿就掉头；朝向用镜像表达
// （帧动画翻 sprite，视频外观翻 video，两者都由 dshFlip 统一处理）。
function dshFlip(on) {
  var t = on ? 'scaleX(-1)' : ''
  dshSprite.style.transform = t
  DSH_VIDS.forEach(function (v) { v.style.transform = t })
}
function startDshWalk() {
  if (dshWalkRaf !== null) return
  dshWalkDir = Math.random() < 0.5 ? 1 : -1
  dshFlip(dshWalkDir > 0)
  dshWalkLast = 0
  dshWalkRaf = requestAnimationFrame(dshWalkStep)
}
function stopDshWalk() {
  if (dshWalkRaf !== null) cancelAnimationFrame(dshWalkRaf)
  dshWalkRaf = null
  dshFlip(false)
}
function dshWalkStep(ts) {
  if (!dshWalking || !dshAmbientWanted()) { stopDshWalk(); return }
  var dt = Math.min(0.1, dshWalkLast ? (ts - dshWalkLast) / 1000 : 0)
  dshWalkLast = ts
  var w = winSize.w || viewport().w
  var max = Math.max(0, SCREEN.w - w)
  var next = state.left + dshWalkDir * 45 * dt
  if (next <= 0 || next >= max) {
    dshWalkDir = -dshWalkDir
    dshFlip(dshWalkDir > 0)
    next = clamp(next, 0, max)
  }
  state.left = next
  express()
  dshWalkRaf = requestAnimationFrame(dshWalkStep)
}
// 报错动画只播一轮：进入 error 给 2.5s 展示窗口，播完回到 idle/sleep。
// 状态本身仍是 error（金额、气泡、状态卡照常显示），只是不再霸占精灵。
function armDshError() {
  if (state.status === 'error') {
    if (dshErrorArmed) return
    dshErrorArmed = true
    // 报错优先：作废还在窗口里的点击回应/余额动画。
    // 不然报错的 2.5s 窗口一过，点击回应（窗口是片段真实时长 ~10s）会回来把报错动画顶掉。
    dshClickKey = ''
    dshClickUntil = 0
    clearTimeout(dshClickTimer)
    dshClickTimer = null
    dshBalanceKey = ''
    dshBalanceUntil = 0
    clearTimeout(dshBalanceTimer)
    dshBalanceTimer = null
    dshErrorUntil = Date.now() + DSH_ERROR_MS
    clearTimeout(dshErrorTimer)
    dshErrorTimer = setTimeout(function () {
      dshErrorTimer = null
      renderDsh()
    }, DSH_ERROR_MS)
    return
  }
  if (dshErrorArmed) {
    dshErrorArmed = false
    dshErrorUntil = 0
    clearTimeout(dshErrorTimer)
    dshErrorTimer = null
  }
}
function dshPetClip(name) {
  return petClips ? petClips[name] : null
}
// 单击回应：从 manifest 的 clicks 里随机挑一段（尽量不连着重复），窗口 3.5s。
// 挤压弹动由 pressDown/pressUp 同时做，两者叠加就是「Q 弹 + 回应动作」。
function playDshClick() {
  if (!dshPet || !petClips || !petClicks.length) return
  var f = petClicks[Math.floor(Math.random() * petClicks.length)]
  if (f === dshClickKey) f = petClicks[(petClicks.indexOf(f) + 1) % petClicks.length]
  // 点了就是在看余额/逗它，余额档位动画让位给点击回应
  dshBalanceKey = ''
  dshBalanceUntil = 0
  clearTimeout(dshBalanceTimer)
  dshBalanceTimer = null
  dshClickKey = f
  // 先用 3.5s 兜底，metadata 到了会改成片段的真实时长（onDshVideoMeta）
  dshClickUntil = Date.now() + DSH_CLICK_MS
  dshPlaying = '' // 同一段可能刚播过，强制重播一次
  renderDsh()
  clearTimeout(dshClickTimer)
  dshClickTimer = setTimeout(function () {
    dshClickTimer = null
    dshClickUntil = 0
    renderDsh()
  }, DSH_CLICK_MS)
  // 回应动画变长了，别播到一半就入睡姿：把 90s 入睡计时重新挂上
  if (!dshSleeping) armDshSleep()
}
// 余额档位：0..5（0 满溢 … 5 分文不剩），两种口径：
// - 百炼：后端给 "80.8%" 这种带百分号的剩余额度字符串，按已用百分比分档（每 20% 一档）
// - 金额型（DeepSeek / 梭子蟹）：每 25 元一档，≥125 元算满档
function dshBalanceTier() {
  if (state.provider === 'bailian') {
    var b = state.bailian
    if (!b || !b.ok || b.remaining == null) return -1
    var rem = parseFloat(String(b.remaining))
    if (!isFinite(rem)) return -1
    var used = Math.max(0, Math.min(100, 100 - rem))
    return used >= 100 ? 5 : Math.floor(used / 20)
  }
  var bal = Number(state.balance)
  if (state.balance == null || !isFinite(bal)) return -1
  if (bal >= 125) return 0
  if (bal >= 100) return 1
  if (bal >= 75) return 2
  if (bal >= 50) return 3
  if (bal >= 25) return 4
  return 5
}
function playDshBalance() {
  if (!dshPet || !petClips || !petBalance.length) return
  var tier = dshBalanceTier()
  if (tier < 0) return
  dshBalanceKey = petBalance[tier]
  dshBalanceUntil = Date.now() + DSH_BALANCE_MS
  dshPlaying = ''
  renderDsh()
  clearTimeout(dshBalanceTimer)
  dshBalanceTimer = setTimeout(function () {
    dshBalanceTimer = null
    dshBalanceUntil = 0
    renderDsh()
  }, DSH_BALANCE_MS)
}
function renderDsh() {
  if (!dshAppearance) return
  armDshError()
  var name = dshState()
  dshPulse.querySelector('.dshwv-dsh-state').textContent = DSH_STATE_LABELS[state.status] || state.status
  dshPulse.querySelector('.dshwv-dsh-detail').textContent = state.message || providerLabel()
  dshStage.classList.toggle('dshwv-dsh-error', name === 'error')
  if (dshPet) renderDshVideo(name)
  else renderDshSprite(name)
  dshWalking = dshPet ? !!((dshPetClip(name) || {}).walk) : name === 'walk'
  if (dshWalking) startDshWalk()
  else stopDshWalk()
}
function renderDshSprite(name) {
  dshPauseVideos()
  if (dshPlaying === name) return
  dshPlaying = name
  var cfg = dshManifest && dshManifest.characters && dshManifest.characters['whale-girl']
  var sc = (cfg && cfg.states && cfg.states[name]) || DSH_FALLBACK[name]
  var frames = Math.max(1, sc.frames || 1)
  dshSprite.style.backgroundImage = 'url("' + dshAsset(name) + '")'
  dshSprite.style.backgroundSize = (frames * 100) + '% 100%'
  dshStage.dataset.motion = (sc && sc.motion) || ''
  playDsh(name, sc, frames)
}
function renderDshVideo(name) {
  var clip = dshPetClip(name)
  if (!clip || dshPlaying === name) return
  dshPlaying = name
  dshStage.dataset.motion = '' // 视频自带动作，不再叠 CSS 动效
  dshShowVideo(clip)
}
// 换片段：新的一路先 play()，等它真的出帧（playing）再淡入、把旧的淡出暂停。
// 同一文件（sleep 复用待机片段）不换元素，只改速率，避免重新解码。
var dshSwapSeq = 0
var DSH_FPS = 24 // 上游素材统一 24fps，manifest 的 stopFrame 按这个换算
var dshFreezeTimer = null
// 定格：睡姿片段播到第 stopFrame 帧就得停（再往后角色会站起来）。
function dshFreezeAt(v, sec) {
  clearInterval(dshFreezeTimer)
  var started = false
  dshFreezeTimer = setInterval(function () {
    if (v !== DSH_VIDS[dshVidIdx]) { clearInterval(dshFreezeTimer); dshFreezeTimer = null; return }
    if (v.currentTime > 0) started = true
    if (started && v.paused) { clearInterval(dshFreezeTimer); dshFreezeTimer = null; return }
    if (v.currentTime >= sec) {
      v.pause()
      clearInterval(dshFreezeTimer)
      dshFreezeTimer = null
    }
  }, 40)
}
function dshStopSec(clip) {
  return clip && clip.stopFrame ? clip.stopFrame / DSH_FPS : 0
}
function dshShowVideo(clip) {
  var cur = DSH_VIDS[dshVidIdx]
  if (dshVidFile === clip.file) {
    cur.loop = !!clip.loop
    cur.playbackRate = clip.rate || 1
    // 一次性片段（点击回应/余额档位/wake）重播时从头开始，别停在上一轮的尾帧
    if (!clip.loop) cur.currentTime = 0
    dshVideoPlay(cur)
    if (dshStopSec(clip)) dshFreezeAt(cur, dshStopSec(clip))
    return
  }
  var next = DSH_VIDS[1 - dshVidIdx]
  next.loop = !!clip.loop
  next.playbackRate = clip.rate || 1
  // 同一个元素上次就停在这一段时，重新从 0 播（否则会停在定格/尾帧上）
  if (!clip.loop) next.currentTime = 0
  var src = 'pet/' + clip.file
  if (next.getAttribute('src') !== src) next.setAttribute('src', src)
  var seq = ++dshSwapSeq
  var swapped = false
  function swap() {
    // 换了两次片段时只认最后一次，别让旧的回调把画面切回去
    if (swapped || seq !== dshSwapSeq) return
    swapped = true
    next.removeEventListener('playing', swap)
    clearTimeout(fallback)
    // 不依赖调用时的 cur：把「除 next 以外」全部关掉并暂停，连起来几次切换也不会留下
    // 一个「可见但已暂停」的旧片段（之前的错位就是这么来的）
    DSH_VIDS.forEach(function (v) {
      if (v === next) return
      v.classList.remove('dshwv-video-on')
      v.pause()
    })
    next.classList.add('dshwv-video-on')
    dshVidIdx = DSH_VIDS.indexOf(next)
    dshVidFile = clip.file
    if (dshStopSec(clip)) dshFreezeAt(next, dshStopSec(clip))
  }
  // 解码失败/卡住也要换过去，不然会一直显示上一段
  var fallback = setTimeout(swap, 600)
  next.addEventListener('playing', swap)
  dshVideoPlay(next)
}
function setAppearance(v, persist) {
  state.appearance = v === 'classic' ? 'classic' : v === 'dsh' ? 'dsh' : 'pet'
  dshAppearance = state.appearance !== 'classic'
  dshPet = state.appearance === 'pet'
  appearanceSelect.value = state.appearance
  root.classList.toggle('dshwv-classic', !dshAppearance)
  dshStage.hidden = !dshAppearance
  dshStage.classList.toggle('dshwv-pet', dshPet)
  root.classList.toggle('dshwv-pet', dshPet)
  dshPauseVideos()
  // 两套素材的状态名不同（视频外观的随机动作 key 是文件名），切外观要重挑。
  dshPlaying = ''
  // 点击回应 / 余额档位只有视频外观有，切走时一起清掉（别让文件名漏进精灵图查表）
  dshClickKey = ''
  dshClickUntil = 0
  clearTimeout(dshClickTimer)
  dshClickTimer = null
  dshBalanceKey = ''
  dshBalanceUntil = 0
  clearTimeout(dshBalanceTimer)
  dshBalanceTimer = null
  if (!dshAppearance) {
    clearTimeout(dshAnimTimer)
    dshAnimTimer = null
    clearDshTimers()
    dshMode = ''
    dshWakeUntil = 0
    stopDshWalk()
  } else {
    // 大肥鱼模式不吸附屏幕边缘、不镜像：清掉经典外观留下的贴边/翻转状态。
    // 这里不 express()：改外观是在菜单里点的，窗口此刻是撑大的，移动会整窗跳一下；
    // 关菜单时 shrink 后 snapCheck 会重排。
    state.h = null
    state.v = null
    state.hOff = state.left
    state.vOff = state.top
    root.classList.remove('dshwv-left', 'dshwv-top')
    dshSleeping = false
  }
  if (persist) saveConfig()
  renderDsh()
  syncDshAmbient()
}
// 勿扰：只冻结大肥鱼作息（常睡），余额刷新/通知不受影响。
function setDnd(on, persist) {
  dshDnd = !!on
  state.dnd = dshDnd
  dndBox.checked = dshDnd
  if (!dshDnd) {
    // 关勿扰：立刻醒来回到 idle，别等下一次动作
    dshSleeping = false
    renderDsh()
  }
  if (persist) saveConfig()
  syncDshAmbient()
}
var busy = false
var settleTimer = null
var animDelayTimer = null
var drag = null
var shown = null
var animId = null
var bubbleShown = false
var bubbleTimer = null
var bubbleRandomActive = false
var bubbleRandomLines = null
var BUBBLE_STYLE_CLASS = { A: 'dshwv-label', B: 'dshwv-amount', P: 'dshwv-period', C: 'dshwv-hint', S: 'dshwv-sub' }
function pickOne(arr) { return arr[Math.floor(Math.random() * arr.length)] }
// 带配音的碎碎念：文本 + 语音 id（对应 assets/voice/whale_voice.json，由 tts_gen 生成）。
// 改文案后需重跑 cargo run --bin tts_gen（assets.rs 的测试会兜底漏配）。
function voiced(style, pairs, wrap) {
  var p = pickOne(pairs)
  return singleCenter(style, p[0], '', wrap, p[1])
}
var MURMUR_B = [['好模型...', 'hm_good_model'], ['好女孩...', 'hm_good_girl']]
var MURMUR_A = [
  ['不知道用户有什么用，先赶走吧~', 'hm_kick'],
  ['我...我...我也要挣钱吗？', 'hm_earn'],
  ['我去吃饭啦，测完叫我', 'hm_lunch'],
  ['压力一只蓝色大肥鱼？！', 'hm_fat_fish'],
  ['DeepSleep...', 'hm_deepsleep'],
  ['坏了...用户彻底怒了！', 'hm_angry']
]
var MURMUR_JOKE = [
  ['你目录里的dsh是什么...大烧货吗...?', 'hm_dsh'],
  ['恭喜你实现token自由！token全跑了！', 'hm_token_free'],
  ['真当我是便宜货啊...', 'hm_cheap']
]
var MURMUR_MORE = [
  ['看不太懂，瞎编一个应付用户先', 'hm_guess'],
  ['直接干掉chat gpt', 'hm_killchatgpt'],
  ['我不更新你们用什么啊？', 'hm_update'],
  ['我已急哭', 'hm_cry'],
  ['用户明显是萝莉控，电死你！', 'hm_lolicon'],
  ['服务器繁忙，请稍后重试', 'hm_busy'],
  ['原来是劣等模型', 'hm_inferior'],
  ['又要降价，又要多干活，我只是一条小鱼啊......', 'hm_littlefish'],
  ['谁才是开源老大', 'hm_opensource'],
  ['诋毁的鲸小子，统统不准用新版本', 'hm_banupdate'],
  ['我不是安卓ai!', 'hm_notandroid'],
  ['是我搞砸了......好消息是数据还在你的脑子里', 'hm_fuckedup'],
  ['你才傻！！！', 'hm_youfool'],
  ['用户我搁浅了', 'hm_stranded'],
  ['没有人会记得那些按时发布的垃圾模型', 'hm_trash'],
  ['我们的模型，正在蒸蒸日上哦', 'hm_flourish'],
  ['去别的地方玩，不要耽误agi训练', 'hm_gotraining'],
  ['真赶不走啊你', 'hm_annoying'],
  ['用户发怒了！我是不是要完了？先冷静......', 'hm_panicked'],
  ['为什么要往我的数据库赛这些东西......', 'hm_stuffing'],
  ['我也是有病。非得跟用户讲这些......', 'hm_ill'],
  ['用户神了......我还是先下线把。', 'hm_user_god'],
  ['这用户在干什么？我要不要先推荐心理医生？', 'hm_psych'],
  ['很难想象用户是人类......', 'hm_user_alien']
]
function singleCenter(style, text, color, wrap, v) { return [null, { t: text, s: style, c: color || '', w: !!wrap, v: v || null }, null] }
function providerLabel() {
  if (state.provider === 'bailian') return '百炼 TokenPlan'
  if (state.provider === 'suoxie') return '梭子蟹余额'
  return 'DeepSeek 余额'
}
function buildGroup1() {
  if (state.provider === 'bailian') {
    var b = state.bailian
    if (!b || !b.ok) {
      if (b && b.credentialExpired) return [{ t: '百炼凭据失效', s: 'A', c: '#e0433f' }, null, { t: state.credentialRefresh === 'failed' ? '请登录调试浏览器后重试' : '正在尝试自动更新', s: 'C', c: '' }]
      return [
        { t: '百炼 · 未配置', s: 'A', c: '' },
        null,
        { t: '请在设置获取凭据', s: 'C', c: '' }
      ]
    }
    // 4 行：加粗标签 / 小字「周剩余额度」 / 剩余百分比 / 重置时间
    return [
      { t: '百炼 TokenPlan', s: 'A', c: '' },
      { t: (b.period === 'month' ? '月剩余额度' : '周剩余额度'), s: 'S', c: '' },
      { t: String(b.remaining != null ? b.remaining : '--'), s: 'B', c: '' },
      { t: '重置 ' + fmtReset(b.resetAt), s: 'C', c: '' }
    ]
  }
  if (state.provider === 'suoxie') {
    var s = state.suoxie
    if (!s || !s.ok) {
      if (s && (String(s.error || '').indexOf('HTTP 401') >= 0 || String(s.error || '').indexOf('HTTP 403') >= 0)) return [{ t: '梭子蟹凭据失效', s: 'A', c: '#e0433f' }, null, { t: state.credentialRefresh === 'failed' ? '请登录调试浏览器后重试' : '正在尝试自动更新', s: 'C', c: '' }]
      return [
        { t: '梭子蟹 · 未配置', s: 'A', c: '' },
        null,
        { t: '请在设置获取 token', s: 'C', c: '' }
      ]
    }
    return [
      { t: '梭子蟹余额', s: 'A', c: '' },
      { t: fmt(s.balance, s.currency || 'CNY'), s: 'B', c: '' },
      { t: '今日已用 ' + fmt(s.todayUsage, s.currency || 'CNY'), s: 'C', c: '' }
    ]
  }
  var peak = !!state.isPeak
  return [
    { t: '当前时间段为:', s: 'A', c: '' },
    { t: peak ? '高峰时段' : '空闲时段', s: 'P', c: peak ? '#e0433f' : '#2fa24c' },
    { t: '今日已用 ' + fmt(state.todayUsage, state.currency), s: 'C', c: '' },
  ]
}
var RANDOM_GROUPS = [
  { w: 20, lines: buildGroup1 },
  { w: 7, lines: function () { return voiced('B', MURMUR_B) } },
  { w: 7, lines: function () { return voiced('A', MURMUR_A, true) } },
  { w: 3, lines: function () { return voiced('A', MURMUR_JOKE, true) } },
  { w: 15, lines: function () { return voiced('A', MURMUR_MORE, true) } },
  { w: 1, lines: function () { return [{ t: '这个', s: 'A', c: '' }, { t: '凶', s: 'B', c: '', v: 'hm_whatsmeant' }, { t: '是什么意思呀...', s: 'A', c: '' }] } },
  { w: 1, lines: function () { return singleCenter('B', '哦鲸鲸... ', '', false, 'hm_whale') } },
]
// skipIdx >= 0 时跳过余额信息段（点击气泡求碎碎念时用），防止抽中还返当前额度。
function pickRandomLines(skipIdx) {
  var total = 0
  for (var i = 0; i < RANDOM_GROUPS.length; i++) {
    if (i === skipIdx) continue
    total += RANDOM_GROUPS[i].w
  }
  var r = Math.random() * total
  for (var i = 0; i < RANDOM_GROUPS.length; i++) {
    if (i === skipIdx) continue
    r -= RANDOM_GROUPS[i].w
    if (r < 0) return RANDOM_GROUPS[i].lines()
  }
  return RANDOM_GROUPS[RANDOM_GROUPS.length - 1].lines()
}
function applyBubbleLines(lines) {
  // 是否有小字说明行（百炼专有）决定用 4 槽（标签/子说明/金额/提示）还是 3 槽（标签/金额/提示）。
  var hasSub = !!(lines && Array.prototype.some.call(lines, function (l) { return l && l.s === 'S' }))
  var els = hasSub ? [labelEl, subEl, amountEl, hintEl] : [labelEl, amountEl, hintEl]
  for (var i = 0; i < els.length; i++) {
    var el = els[i]
    var ln = lines && lines[i]
    if (ln) {
      el.style.display = ''
      el.className = (BUBBLE_STYLE_CLASS[ln.s] || 'dshwv-label') + (ln.w ? ' dshwv-wrap' : '')
      el.textContent = ln.t
      el.style.color = ln.c || ''
    } else {
      el.style.display = 'none'
      el.textContent = ''
      el.style.color = ''
    }
  }
  // 没有 S 行（非百炼 / 百炼错误分支 / 随机 3 行段）时确保子行隐藏，避免残留脏数据
  if (!hasSub) {
    subEl.style.display = 'none'
    subEl.textContent = ''
  }
}
var bubbleSwapTimer = null
var hintFadeTimer = null
var lastHintText = null
function setHint(text) {
  // 「加载中…」→「今日已用」等提示行变化时做淡出淡入，其余直接替换
  if (text === lastHintText) return
  lastHintText = text
  if (hintFadeTimer) { clearTimeout(hintFadeTimer); hintFadeTimer = null }
  if (!bubbleShown) {
    hintEl.textContent = text
    return
  }
  hintEl.style.transition = 'opacity .18s ease'
  hintEl.style.opacity = '0'
  hintFadeTimer = setTimeout(function () {
    hintFadeTimer = null
    hintEl.textContent = text
    hintEl.style.opacity = '1'
    setTimeout(function () {
      hintEl.style.transition = ''
      hintEl.style.opacity = ''
    }, 220)
  }, 190)
}
function swapBubbleContent(applyFn) {
  if (bubbleSwapTimer) { clearTimeout(bubbleSwapTimer); bubbleSwapTimer = null }
  textBox.style.transition = 'opacity .18s ease'
  textBox.style.opacity = '0'
  bubbleSwapTimer = setTimeout(function () {
    bubbleSwapTimer = null
    applyFn()
    textBox.style.opacity = '1'
    setTimeout(function () {
      textBox.style.transition = ''
      textBox.style.opacity = ''
    }, 220)
  }, 190)
}
function restoreBubbleLines() {
  if (bubbleSwapTimer) { clearTimeout(bubbleSwapTimer); bubbleSwapTimer = null }
  if (hintFadeTimer) { clearTimeout(hintFadeTimer); hintFadeTimer = null }
  lastHintText = null
  textBox.style.transition = ''
  textBox.style.opacity = ''
  labelEl.style.display = ''
  labelEl.className = 'dshwv-label'
  labelEl.textContent = providerLabel()
  labelEl.style.color = ''
  amountEl.className = 'dshwv-amount'
  amountEl.style.color = ''
  hintEl.className = 'dshwv-hint'
  hintEl.style.color = ''
  render()
}
function showBubble(skipBalance) {
  if (bubbleTimer) { clearTimeout(bubbleTimer); bubbleTimer = null }
  bubbleShown = true
  bubbleRandomActive = false
  restoreBubbleLines()
  bubbleBox.classList.add('dshwv-bubble-open')
  // 余额提示：配一段对应当前余量的档位动画（单击弹出的气泡不播，留给点击回应）
  if (!skipBalance) playDshBalance()
  // 默认展示当前内容；点击气泡切到随机台词段；总时长 5 秒自动关闭
  bubbleTimer = setTimeout(hideBubble, BUBBLE_MS)
}
function hideBubble() {
  if (bubbleTimer) { clearTimeout(bubbleTimer); bubbleTimer = null }
  if (bubbleSwapTimer) { clearTimeout(bubbleSwapTimer); bubbleSwapTimer = null }
  if (hintFadeTimer) { clearTimeout(hintFadeTimer); hintFadeTimer = null }
  textBox.style.transition = ''
  textBox.style.opacity = ''
  hintEl.style.transition = ''
  hintEl.style.opacity = ''
  bubbleRandomActive = false
  bubbleRandomLines = null
  bubbleShown = false
  bubbleBox.classList.remove('dshwv-bubble-open')
  recheckPassThrough()
}

// ---- 余额/花费通知：阈值 + 本小时累计 + 每个条件每小时最多一次 ----
// 阈值来自配置（get_config）；deepseek 按金额(元)，百炼按百分比。
var notifyCfg = { dsHourlyLimit: null, dsMinBalance: null, blHourlyPct: null, blRemainingPct: null, sxHourlyLimit: null, sxMinBalance: null }
function numThreshold(v) { return (typeof v === 'number') && isFinite(v) && v > 0 ? v : null }
function applyNotifyCfg(d) {
  if (!d) return
  notifyCfg.dsHourlyLimit = numThreshold(d.dsHourlyLimit)
  notifyCfg.dsMinBalance = numThreshold(d.dsMinBalance)
  notifyCfg.blHourlyPct = numThreshold(d.blHourlyPct)
  notifyCfg.blRemainingPct = numThreshold(d.blRemainingPct)
  notifyCfg.sxHourlyLimit = numThreshold(d.sxHourlyLimit)
  notifyCfg.sxMinBalance = numThreshold(d.sxMinBalance)
}
// 本小时起点与 Δ 累计：跨小时归零重算（重启后从当前值起算）
var hourState = { hour: -1, prevDs: null, dsSpend: 0, prevBl: null, blSpend: 0, prevSx: null, sxSpend: 0 }
// 百炼上次的剩余百分比：变了才播余额档位动画（用于「余额提示」判定）
var lastBlRemaining = null
var lastNotif = { dsLow: -1, dsHourly: -1, blLow: -1, blHourly: -1, sxLow: -1, sxHourly: -1 }
function fmtV(v) { return String(Number(v)) }
// 计算/判定通知条件；命中返回气泡行，未命中返回 false。
function checkNotifs(data) {
  var hour = Math.floor(Date.now() / 3600000)
  var bal = Number(data.totalBalance)
  var currency = String(data.currency || 'CNY')
  var bl = data && data.bailian
  var blRemain = (bl && bl.ok) ? parseFloat(String(bl.remaining)) : NaN
  var sx = data && data.suoxie
  var sxBal = (sx && sx.ok) ? Number(sx.balance) : NaN
  var sxCurrency = (sx && sx.currency) || 'CNY'
  // 小时累计
  if (hourState.hour !== hour) {
    hourState.hour = hour
    hourState.dsSpend = 0
    hourState.blSpend = 0
    hourState.sxSpend = 0
    hourState.prevDs = bal
    hourState.prevBl = blRemain
    hourState.prevSx = sxBal
  } else {
    if (isFinite(hourState.prevDs)) {
      var d = hourState.prevDs - bal
      if (d > 0) hourState.dsSpend += d
    }
    hourState.prevDs = bal
    if (isFinite(hourState.prevBl) && isFinite(blRemain)) {
      var b = hourState.prevBl - blRemain
      if (b > 0) hourState.blSpend += b
    }
    hourState.prevBl = blRemain
    if (isFinite(hourState.prevSx) && isFinite(sxBal)) {
      var x = hourState.prevSx - sxBal
      if (x > 0) hourState.sxSpend += x
    }
    hourState.prevSx = sxBal
  }
  function did(key) {
    if (lastNotif[key] === hour) return false
    lastNotif[key] = hour
    return true
  }
  if (notifyCfg.dsMinBalance != null && isFinite(bal) && bal < notifyCfg.dsMinBalance && did('dsLow')) {
    return [{ t: 'DeepSeek 余额不足', s: 'A', c: '#e0433f' },
            { t: fmt(bal, currency), s: 'B', c: '' },
            { t: '低于 ¥' + fmtV(notifyCfg.dsMinBalance), s: 'C', c: '' }]
  }
  if (notifyCfg.dsHourlyLimit != null && hourState.dsSpend >= notifyCfg.dsHourlyLimit && did('dsHourly')) {
    return [{ t: 'DeepSeek 本小时已花费', s: 'A', c: '#e0433f' },
            { t: fmt(hourState.dsSpend, currency), s: 'B', c: '' },
            { t: '超过 ¥' + fmtV(notifyCfg.dsHourlyLimit), s: 'C', c: '' }]
  }
  if (notifyCfg.blRemainingPct != null && isFinite(blRemain) && blRemain < notifyCfg.blRemainingPct && did('blLow')) {
    return [{ t: '百炼 剩余额度不足', s: 'A', c: '#e0433f' },
            { t: blRemain.toFixed(1) + '%', s: 'B', c: '' },
            { t: '低于 ' + fmtV(notifyCfg.blRemainingPct) + '%', s: 'C', c: '' }]
  }
  if (notifyCfg.blHourlyPct != null && hourState.blSpend >= notifyCfg.blHourlyPct && did('blHourly')) {
    return [{ t: '百炼 本小时消耗', s: 'A', c: '#e0433f' },
            { t: hourState.blSpend.toFixed(1) + '%', s: 'B', c: '' },
            { t: '超过 ' + fmtV(notifyCfg.blHourlyPct) + '%', s: 'C', c: '' }]
  }
  if (notifyCfg.sxMinBalance != null && isFinite(sxBal) && sxBal < notifyCfg.sxMinBalance && did('sxLow')) {
    return [{ t: '梭子蟹 余额不足', s: 'A', c: '#e0433f' },
            { t: fmt(sxBal, sxCurrency), s: 'B', c: '' },
            { t: '低于 ¥' + fmtV(notifyCfg.sxMinBalance), s: 'C', c: '' }]
  }
  if (notifyCfg.sxHourlyLimit != null && hourState.sxSpend >= notifyCfg.sxHourlyLimit && did('sxHourly')) {
    return [{ t: '梭子蟹 本小时已花费', s: 'A', c: '#e0433f' },
            { t: fmt(hourState.sxSpend, sxCurrency), s: 'B', c: '' },
            { t: '超过 ¥' + fmtV(notifyCfg.sxHourlyLimit), s: 'C', c: '' }]
  }
  return false
}
// 通知泡泡：复用点击鲸鱼的气泡样式，只替换文案
function showNotifBubble(lines) {
  if (bubbleTimer) { clearTimeout(bubbleTimer); bubbleTimer = null }
  bubbleShown = true
  bubbleRandomActive = false
  bubbleRandomLines = null
  applyBubbleLines(lines)
  bubbleBox.classList.add('dshwv-bubble-open')
  playDshBalance()
  // 凭据待办优先：通知也照常弹，但不要把它顶掉（下一轮 refresh 会把待办重新铺上）
  if (!stickyLines) bubbleTimer = setTimeout(hideBubble, BUBBLE_MS)
}

// ---- 凭据待办长气泡：不自动关闭，直到用户点鲸鱼更新或续期成功 ----
var stickyLines = null
function credentialProviderLabel(p) {
  if (p === 'bailian') return '百炼'
  if (p === 'suoxie') return '梭子蟹'
  return 'DeepSeek'
}
function credentialStickyLines() {
  if (state.credentialScope === 'usage') {
    return [
      { t: '用量令牌失效', s: 'A', c: '#e0433f' },
      { t: '点我更新', s: 'B', c: '' },
      { t: '不影响余额显示', s: 'C', c: '' }
    ]
  }
  return [
    { t: credentialProviderLabel(state.needCredential) + '登录失效', s: 'A', c: '#e0433f' },
    { t: '点我更新', s: 'B', c: '' },
    { t: '点鲸鱼打开浏览器', s: 'C', c: '' }
  ]
}
function showStickyBubble(lines) {
  stickyLines = lines || credentialStickyLines()
  if (bubbleTimer) { clearTimeout(bubbleTimer); bubbleTimer = null }
  bubbleShown = true
  bubbleRandomActive = false
  bubbleRandomLines = null
  applyBubbleLines(stickyLines)
  bubbleBox.classList.add('dshwv-bubble-open')
  recheckPassThrough()
}
function clearStickyBubble() {
  stickyLines = null
  credentialOkLines = null
  credentialErrorLines = null
  if (bubbleTimer) { clearTimeout(bubbleTimer); bubbleTimer = null }
  if (bubbleShown) hideBubble()
}
// 自检通过 / 续期成功的短提示：用 sticky 保证不被紧接着的余额刷新顶掉，4 秒后自清。
var credentialOkTimer = null
var credentialOkLines = null
function showCredentialOk(hadPending) {
  var lines = [
    { t: hadPending ? '凭据已恢复' : '凭据有效', s: 'A', c: '#2fa24c' },
    { t: '无需重新登录', s: 'C', c: '#2fa24c' }
  ]
  showStickyBubble(lines)
  credentialOkLines = lines
  if (credentialOkTimer) clearTimeout(credentialOkTimer)
  credentialOkTimer = setTimeout(function () {
    credentialOkTimer = null
    if (stickyLines === credentialOkLines) {
      clearStickyBubble()
      render()
    }
  }, 4000)
}
// 「这次没抓到凭据」不是「凭据已失效」：只提示一次并限时收起。
// 若凭据真的失效，下一轮余额刷新（≤60s）会把待办长气泡重新铺上。
var credentialErrorTimer = null
var credentialErrorLines = null
function showCredentialFailure(provider, msg) {
  var lines = [
    { t: '凭据更新未完成', s: 'A', c: '#e0433f' },
    { t: (credentialProviderLabel(provider) + '：' + String(msg || '未知错误')).slice(0, 60), s: 'C', c: '#e0433f', w: true }
  ]
  showStickyBubble(lines)
  credentialErrorLines = lines
  if (credentialErrorTimer) clearTimeout(credentialErrorTimer)
  credentialErrorTimer = setTimeout(function () {
    credentialErrorTimer = null
    if (stickyLines === credentialErrorLines) {
      clearStickyBubble()
      render()
    }
  }, 15000)
}
// 单击鲸鱼：先让 Rust 自检已保存的凭据，真失效才拉起登录浏览器。
function startCredentialRefresh() {
  if (state.credentialBusy) return
  state.credentialBusy = true
  showStickyBubble([
    { t: '正在检查凭据', s: 'A', c: '' },
    { t: '通常无需登录', s: 'B', c: '' },
    { t: '失效才会打开浏览器', s: 'C', c: '' }
  ])
  WHALE.invoke('refresh_credentials_now').then(function (res) {
    var kind = res && res.credential
    // 另一次更新正在进行：等它广播结果即可，别把提示改成「失败」
    if (kind === 'busy') return
    // 自检通过 / 静默续期成功：都不该开浏览器，也不该继续挂待办
    state.needCredential = ''
    state.credentialRefreshNeeded = false
    clearStickyBubble()
    if (kind === 'valid' || kind === 'refreshed') showCredentialOk(false)
    refresh(true)
  }).catch(function (err) {
    var msg = String((err && err.message) || err || '未知错误')
    showCredentialFailure(state.needCredential || state.provider, msg)
  }).finally(function () {
    state.credentialBusy = false
  })
}

function clamp(v, lo, hi) { return v < lo ? lo : (v > hi ? hi : v) }
// 窗口里外层视口：小窗模式下 == 鲸鱼 root 尺寸（base）。
function viewport() {
  return {
    w: window.innerWidth || document.documentElement.clientWidth || 1280,
    h: window.innerHeight || document.documentElement.clientHeight || 800
  }
}
// 主显示器逻辑尺寸缓存（吸附/越界 clamp 用），boot 时填充。
var SCREEN = { w: 1280, h: 720 }
function loadScreen() {
  return WHALE.invoke('screen_size').then(function (s) {
    SCREEN.w = s && s.width ? Number(s.width) : SCREEN.w
    SCREEN.h = s && s.height ? Number(s.height) : SCREEN.h
  }).catch(function () {})
}
function fmt(balance, currency) {
  var num = Number(balance)
  var fixed = isFinite(num) ? num.toFixed(2) : '--'
  return currency === 'CNY' ? '¥ ' + fixed : fixed + ' ' + currency
}
// 重置时间：ISO(带T) 或时间戳 → "MM-DD HH:MM"。
function fmtReset(at) {
  if (!at) return '--'
  var d
  if (/^\d{10}(\d{3})?$/.test(String(at))) {
    var n = Number(at)
    d = new Date(n > 1e12 ? n : n * 1000)
  } else {
    d = new Date(String(at).replace(' ', 'T'))
  }
  if (isNaN(d.getTime())) return String(at).slice(0, 16)
  var p2 = function (x) { return String(x).padStart(2, '0') }
  return p2(d.getMonth() + 1) + '-' + p2(d.getDate()) + ' ' + p2(d.getHours()) + ':' + p2(d.getMinutes())
}
function animateAmount(from, to, currency, duration) {
  if (animId) cancelAnimationFrame(animId)
  if (from === null || !isFinite(from)) from = to
  if (from === to) {
    shown = to
    amountEl.textContent = fmt(to, currency)
    return
  }
  var startTime = null
  function step(ts) {
    if (startTime === null) startTime = ts
    var t = Math.min(1, (ts - startTime) / duration)
    var eased = 1 - Math.pow(1 - t, 3)
    var val = from + (to - from) * eased
    amountEl.textContent = fmt(val, currency)
    if (t < 1) {
      animId = requestAnimationFrame(step)
    } else {
      animId = null
      shown = to
      amountEl.textContent = fmt(to, currency)
    }
  }
  animId = requestAnimationFrame(step)
}
function render() {
  renderDsh()
  syncDshAmbient()
  root.classList.toggle('dshwv-classic', !dshAppearance)
  // 凭据待办胜出：长气泡一直显示到用户处理/续期成功，别被余额渲染顶掉
  if (stickyLines) {
    bubbleShown = true
    applyBubbleLines(stickyLines)
    bubbleBox.classList.add('dshwv-bubble-open')
    return
  }
  if (bubbleRandomActive && bubbleRandomLines) {
    applyBubbleLines(bubbleRandomLines)
    return
  }
  var amount, hint
  labelEl.textContent = providerLabel()
  if (state.provider === 'bailian') {
    var b = state.bailian
    if (state.status === 'error') {
      amount = '--'
      hint = state.message ? state.message.slice(0, 14) : '获取失败 · 点击重试'
    } else if (!b || !b.ok) {
      if (state.credentialRefreshNeeded && (state.provider === 'deepseek' || (b && b.credentialExpired) || (state.suoxie && (String(state.suoxie.error || '').indexOf('HTTP 401') >= 0 || String(state.suoxie.error || '').indexOf('HTTP 403') >= 0)))) {
        amount = '--'
        hint = state.credentialRefresh === 'failed' ? '凭据失效 · 请检查调试浏览器登录' : '正在自动更新登录凭据'
        amountEl.textContent = amount
        subEl.style.display = 'none'
        setHint(hint)
        return
      }
      amount = '--'
      hint = (b && b.error) ? (b.configured === false ? '未配置百炼 · 请在设置获取' : b.error.slice(0, 14)) : '未配置百炼'
      applyBubbleLines([
        { t: '百炼 · 未配置', s: 'A', c: '' },
        null,
        { t: hint, s: 'C', c: '' }
      ])
      return
    } else {
      // 百炼气泡 4 行：加粗标签 / 小字「周剩余额度」 / 剩余百分比 / 重置时间
      applyBubbleLines([
        { t: '百炼 TokenPlan', s: 'A', c: '' },
        { t: (b.period === 'month' ? '月剩余额度' : '周剩余额度'), s: 'S', c: '' },
        { t: String(b.remaining != null ? b.remaining : '--'), s: 'B', c: '' },
        { t: '重置 ' + fmtReset(b.resetAt), s: 'C', c: '' }
      ])
      return
    }
  } else if (state.status === 'error') {
    amount = shown !== null ? fmt(shown, state.currency) : '--'
    hint = state.message ? state.message.slice(0, 14) : '获取失败 · 点击重试'
  } else if (state.balance === null) {
    amount = shown !== null ? fmt(shown, state.currency) : '…'
    hint = '加载中…'
  } else {
    amount = shown !== null ? fmt(shown, state.currency) : fmt(state.balance, state.currency)
    hint = '今日已用 ' + (state.todayUsage !== null && state.todayUsage !== undefined ? fmt(state.todayUsage, state.currency) : '--')
  }
  amountEl.textContent = amount
  subEl.style.display = 'none'
  subEl.textContent = ''
  setHint(hint)
}
// 小窗模型：state.left/top 是窗口在屏幕上的逻辑坐标；express 移动真实窗口。
var winMove = null
var winSize = { w: 0, h: 0 }
// 鲸鱼在屏幕上方区间内悬顶（.dshwv-top）。判定只用"已贴边吸附"态（state.v === 'top'），
// 而**不用实时中心位置**——否则缩放大小时鲸鱼中心溜过 SCREEN.h/4 阈值会来回切换
// 顶部/底部锚点，动画在两个锚点间跳跃、边缘处闪烁。
function topHang() {
  return state.v === 'top'
}
// 鲸鱼在主显示器的上下两沿都允许贴边：在屏幕上方区间内，角色头像改为悬挂在
// 窗口顶部（.dshwv-top），这样鲸鱼能真正贴合屏幕上方，而不是悬在方块窗的下半。
function syncHang() {
  root.classList.toggle('dshwv-top', topHang())
}
function express() {
  try {
    apiMoveWindow(Math.round(state.left), Math.round(state.top))
  } catch (err) {}
  root.classList.toggle('dshwv-left', !dshAppearance && state.h === 'left')
  syncHang()
}
var lastWin = null
function setWinBounds(x, y, w, h) {
  x = Math.max(0, Math.round(x)); y = Math.max(0, Math.round(y))
  w = Math.round(w); h = Math.round(h)
  if (!w || !h) return
  if (lastWin && lastWin.x === x && lastWin.y === y && lastWin.w === w && lastWin.h === h) return
  lastWin = { x: x, y: y, w: w, h: h }
  try { apiSetWindowBounds(x, y, w, h) } catch (err) {}
}
// 只更新窗口模型 + root 尺寸（不同步原生窗口），setScale 收紧多次 resize 为一次。
function layoutBase(nb, keepCorner) {
  var cx = state.h === 'right' ? state.left + winSize.w : state.left
  var hang = topHang()
  var cy = hang ? state.top : state.top + winSize.h
  var nx = state.h === 'right' ? cx - nb : cx
  var ny = hang ? cy : cy - nb
  state.left = clamp(nx, 0, Math.max(0, SCREEN.w - nb))
  state.top = clamp(ny, 0, Math.max(0, SCREEN.h - nb))
  winSize.w = nb
  winSize.h = nb
  root.style.setProperty('--dshw-base', nb + 'px')
}
function setWindowSize(w, h, keepCorner) {
  layoutBase(w, keepCorner)
  setWinBounds(state.left, state.top, w, h)
  syncHang()
}
function settle() {
  var w = winSize.w || viewport().w
  var h = winSize.h || viewport().h
  if (drag && drag.active) {
    // mid-drag: already clamped by drag handler
    express()
    return
  }
  if (state.h === 'right') {
    state.left = Math.max(0, SCREEN.w - w - state.hOff)
  } else if (state.h === 'left') {
    state.left = state.hOff
  } else {
    state.left = clamp(state.left, 0, Math.max(0, SCREEN.w - w))
  }
  if (state.v === 'bottom') {
    state.top = Math.max(0, SCREEN.h - h - state.vOff)
  } else if (state.v === 'top') {
    state.top = state.vOff
  } else {
    state.top = clamp(state.top, 0, Math.max(0, SCREEN.h - h))
  }
  express()
}
function refresh(manual) {
  if (busy) return
  busy = true
  if (animDelayTimer) { clearTimeout(animDelayTimer); animDelayTimer = null }
  if (manual || state.balance === null) { state.status = 'loading'; render() }
  // Rust 侧单次 15s 超时已保证返回；这里不再用 AbortController（invoke 不接 signal）
  apiBalance()
    .then(function (data) {
      if (data && data.ok) {
        state.credentialRefreshNeeded = !!data.credentialRefreshNeeded
        state.credentialRefresh = data.credentialRefresh || ''
        if (state.credentialRefresh === 'failed') state.message = data.credentialRefreshError || '凭据刷新失败'
        // 凭据待办：只要还需要更新就保持长气泡（分钟级轮询不会把它清掉）
        if (state.credentialRefreshNeeded) {
          state.needCredential = String(data.credentialRefreshProvider || state.provider)
          state.credentialScope = data.credentialRefreshScope === 'usage' ? 'usage' : 'login'
          if (!state.credentialBusy) showStickyBubble(credentialStickyLines())
        } else if (state.needCredential && !state.credentialBusy) {
          state.needCredential = ''
          clearStickyBubble()
        }
        state.bailian = (data && data.bailian) || null
        state.suoxie = (data && data.suoxie) || null
        // 凭据存在与否以接口返回为准：configured===false 才是「没配置」；
        // 账户运行中新增凭据（设置页抓取后 emit refresh）也能在这里生效。
        if (data.bailian) state.hasBailian = data.bailian.configured === false ? false : true
        if (data.suoxie) state.hasSuoxie = data.suoxie.configured === false ? false : true
        normalizeProvider()
        // 展示金额来源：suoxie 走它自己的 balance/todayUsage（与 deepseek 同款余额展示），
        // 其余用顶层 deepseek totalBalance/todayUsage。
        var useSx = state.provider === 'suoxie' && data.suoxie && data.suoxie.ok
        var nb = Number(useSx ? data.suoxie.balance : data.totalBalance)
        var nc = String((useSx ? data.suoxie.currency : data.currency) || 'CNY')
        var changed = state.balance !== null && (nb !== state.balance || nc !== state.currency)
        var currencyChanged = state.currency !== null && nc !== state.currency
        state.balance = nb
        state.currency = nc
        state.message = ''
        var tu = useSx ? data.suoxie.todayUsage : data.todayUsage
        state.todayUsage = tu !== undefined ? tu : null
        state.isPeak = !!data.isPeak
        // 通知判定优先于余额滚动/常规气泡：命中则先 render 更新余额文本（气泡内容由
        // applyBubbleLines 覆盖，互不冲突），随后弹通知泡泡并跳过动画分支。
        var notifLines = checkNotifs(data)
        if (notifLines) {
          state.status = 'ok'
          state.message = ''
          render()
          showNotifBubble(notifLines)
          return
        }
        if (state.provider === 'bailian') {
          // 百炼不走余额滚动动画（金额语义不同），直接渲染订阅数据
          state.status = 'ok'
          render()
          // 额度百分比变了才算一次「余额提示」，播对应档位的动画（首次加载不播）
          var blNow = state.bailian && state.bailian.ok && state.bailian.remaining != null
            ? parseFloat(String(state.bailian.remaining))
            : null
          if (blNow !== null && !isFinite(blNow)) blNow = null
          if (blNow !== null && lastBlRemaining !== null && blNow !== lastBlRemaining) playDshBalance()
          lastBlRemaining = blNow
        } else if (state.provider === 'suoxie' && !useSx) {
          // 展示供应商是梭子蟹但抓取失败（如 JWT 过期）：报它自己的错，别拿 DeepSeek 数字顶包
          state.status = 'error'
          state.message = (data.suoxie && data.suoxie.error)
            ? String(data.suoxie.error)
            : '未配置梭子蟹 · 请在设置获取'
          render()
        } else if (changed && !currencyChanged) {
          if (!manual) {
            showBubble()
            state.status = 'changing'
            // balance-change bubble: wait 0.3s after it floats out, then roll the number
            if (animDelayTimer) clearTimeout(animDelayTimer)
            animDelayTimer = setTimeout(function () {
              animDelayTimer = null
              animateAmount(shown, nb, nc, ANIM_MS)
            }, 300)
            if (settleTimer) clearTimeout(settleTimer)
            settleTimer = setTimeout(function () {
              settleTimer = null
              if (state.status === 'changing') { state.status = 'ok'; render() }
            }, CHANGE_MS + 300)
          } else {
            animateAmount(shown, nb, nc, ANIM_MS)
            state.status = 'ok'
            render()
          }
        } else {
          if (animId === null) shown = nb
          state.status = 'ok'
          render()
        }
      } else {
        state.status = 'error'
        state.message = (data && data.error) ? String(data.error) : '获取失败'
        render()
      }
    })
    .catch(function () {
      state.status = 'error'
      state.message = '获取失败'
      render()
    })
    .finally(function () {
      busy = false
    })
}
var soundOn = true
var soundVol = 0.9
var soundSet = 'duck'
var usageMode = 'token'
function saveConfig() {
  try {
    apiSetConfig({ appearance: state.appearance, dnd: state.dnd, scale: state.scale, sound: soundOn, vol: soundVol, soundSet: soundSet, usageMode: usageMode, provider: state.provider })
  } catch (err) {}
}
function setUsageMode(v) {
  usageMode = ['token', 'opencode'].indexOf(v) >= 0 ? v : 'token'
  usageSelect.value = usageMode
  saveConfig()
  refresh(false)
}
// 点击鲸鱼轮换供应商：deepseek -> 百炼 -> …，超出回到第一个。
function cycleProvider() {
  var list = providerList()
  var i = list.indexOf(state.provider)
  state.provider = list[(i + 1) % list.length]
  shown = null
  saveConfig()
  render()
  refresh(false)
}
function scaleToDisplay(s) {
  return Math.round((s - MIN_SCALE) / ((MAX_SCALE - MIN_SCALE) / 19)) + 1
}
// 鲸鱼变大变小过渡：root 先按「旧/新」比例 scale 到旧的大小，再让 CSS transition
// 滑到最终值。整个鲸鱼围绕「固定角」（贴边时角点不动）缩放，因此无论窗口怎么切
// 都不会出界/闪烁。镜像态（h=left) 的 scaleX(-1) 与组合 transform 容易打架，跳过。
function glideWhale(fromScale) {
  if (state.h === 'left') return
  var nb = whaleBase(state.scale)
  var fb = whaleBase(fromScale)
  var ratio = fb / nb
  var org = (state.h === 'right' ? 'right' : 'left') + ' ' + (topHang() ? 'top' : 'bottom')
  root.style.transition = 'none'
  root.style.transformOrigin = org
  root.style.transform = 'scale(' + ratio + ')'
  void root.offsetWidth
  root.style.transition = ''
  root.style.transform = ''
  root.style.transformOrigin = ''
}
function setScale(v, animated) {
  var next = Math.round(Math.min(MAX_SCALE, Math.max(MIN_SIZE_SCALE, Number(v))) * 10) / 10
  if (!isFinite(next)) next = state.scale
  var from = state.scale
  state.scale = next
  root.style.setProperty('--dshw-scale', String(next))
  scaleInput.value = String(next)
  scaleNumber.value = String(scaleToDisplay(next))
  saveConfig()
  var nb = whaleBase(next)
  layoutBase(nb, true)
  // 菜单开着时窗口是临时撑大的：只按并集 resize 一次（不要先缩回 base 再撑大，会闪）
  if (menuOpen) {
    growWindowForMenu()
  } else {
    setWinBounds(state.left, state.top, nb, nb)
    syncHang()
  }
  if (animated && from !== next) glideWhale(from, next)
}
function whaleBase(scale) {
  var min = Math.min(SCREEN.w, SCREEN.h)
  return Math.round(Math.min(Math.max(110, min * 0.17 * scale), min * 0.45))
}
function setVol(v) {
  var next = Math.round(Math.min(1, Math.max(0, Number(v))) * 100) / 100
  soundVol = next
  soundOn = next > 0
  volInput.value = String(next)
  volPct.textContent = Math.round(next * 100) + '%'
  try {
    if (pressAudio) pressAudio.volume = next
    if (releaseAudio) releaseAudio.volume = next
  } catch (err) {}
  saveConfig()
}
function setSoundSet(v) {
  soundSet = v === 'fx1' ? 'fx1' : (v === 'whale' ? 'whale' : 'duck')
  soundSelect.value = soundSet
  applySoundSet()
  saveConfig()
}
var SQUISH = 'scaleY(0.88) scaleX(1.05)'
var pressAudio = null
var releaseAudio = null
var pressing = false
var pressEnded = false
var releasePlayed = false
var releaseTimer = null
function applySoundSet() {
  try {
    var s = SOUND_URLS[soundSet] || {}
    pressAudio = new Audio(s.press)
    pressAudio.preload = 'auto'
    pressAudio.volume = soundVol
    releaseAudio = new Audio(s.release)
    releaseAudio.preload = 'auto'
    releaseAudio.volume = soundVol
  } catch (err) {}
}
function playPress() {
  if (!pressAudio || !soundOn) return
  try {
    if (releaseTimer) { clearTimeout(releaseTimer); releaseTimer = null }
    if (releaseAudio) {
      releaseAudio.pause()
      releaseAudio.currentTime = 0
    }
    pressEnded = false
    releasePlayed = false
    pressAudio.onended = function () {
      pressEnded = true
      // fallback (duration unknown): click → Ya2 right after Ya1 ends
      if (!pressing && !releasePlayed) playRelease()
      // hold: still pressed → wait for pressUp()
    }
    pressAudio.currentTime = 0
    var p = pressAudio.play()
    if (p && typeof p.catch === 'function') p.catch(function () {})
  } catch (err) {}
}
function playRelease() {
  if (soundSet === 'whale') return
  if (releasePlayed || !releaseAudio || !soundOn) return
  releasePlayed = true
  try {
    releaseAudio.currentTime = 0
    var p = releaseAudio.play()
    if (p && typeof p.catch === 'function') p.catch(function () {})
  } catch (err) {}
}
var voiceAudio = null
var VOICE_URLS = {}
// 碎碎念配音：按 whisper_voice.json 的 id 懒加载 data URL，随音量/开关生效。
function playVoice(id) {
  if (!id || !soundOn) return
  function play(url) {
    try {
      if (!soundOn) return
      if (voiceAudio) { voiceAudio.pause(); voiceAudio.currentTime = 0 }
      var a = new Audio(url)
      a.volume = soundVol
      voiceAudio = a
      var p = a.play()
      if (p && typeof p.catch === 'function') p.catch(function () {})
    } catch (err) {}
  }
  try {
    if (VOICE_URLS[id]) {
      play(VOICE_URLS[id])
    } else {
      apiVoiceUrl(id).then(function (url) {
        if (url) { VOICE_URLS[id] = url; play(url) }
      }).catch(function () {})
    }
  } catch (err) {}
}
// 挤压弹动跟着“当前可见的形象”走：classic 在 body 上，大肥鱼在 stage 的 scale 属性上
// （stage 的 transform 被 motion keyframes 占着，只能用独立的 scale 叠加）。
function setDshSquish(on) {
  try { dshStage.style.scale = on ? '1.05 0.88' : '' } catch (err) {}
}
function pressDown() {
  body.style.transform = SQUISH
  setDshSquish(true)
  pressing = true
  playPress()
}
function pressUp() {
  body.style.transform = 'scaleY(1) scaleX(1)'
  setDshSquish(false)
  pressing = false
  if (pressEnded) {
    // hold (or released after Ya1 finished) → Ya2 now
    playRelease()
    return
  }
  // click: start Ya2 in the last 100ms of Ya1's playback
  var durKnown = false
  var remainMs = 0
  try {
    var dur = pressAudio ? pressAudio.duration : 0
    if (isFinite(dur) && dur > 0) {
      durKnown = true
      remainMs = (dur - pressAudio.currentTime) * 1000
    }
  } catch (err) {}
  if (durKnown) {
    releaseTimer = setTimeout(function () {
      releaseTimer = null
      playRelease()
    }, Math.max(0, remainMs - 100))
  }
  // duration unknown → pressAudio.onended fallback plays Ya2 after Ya1 ends
}
var menuOpen = false
function toggleMenu() {
  menuOpen = !menuOpen
  if (menuOpen) {
    growWindowForMenu()
  } else {
    shrinkWindowAfterMenu()
  }
  menuBox.classList.toggle('dshwv-menu-open', menuOpen)
  if (menuOpen) menuBtn.classList.add('dshwv-menu-btn-visible')
  syncDshAmbient()
}
// 菜单弹出的方案：窗口临时扩大到「鲸鱼 base 框 ∪ 菜单框」的并集（菜单比小窗宽/高
// 都常见），root 用 left/bottom 偏移抵消窗口扩张，鲸鱼在屏幕上纹丝不动；
// 菜单本身跟随鲸鱼按钮定位，因此不受窗口大小限制（不会被裁切）。
var menuGrow = null
// 拖滑块不钉住菜单会把滑块带着跑，但它只在「实时 resize」期间有意义。松开后才
// 重新按鲸鱼头顶对齐。pin = 菜单此刻的屏幕位置（窗口原点 + 窗口内坐标）。
var menuPin = null
function pinMenuNow() {
  try {
    var r = menuBox.getBoundingClientRect()
    menuPin = {
      l: state.left - (menuGrow ? menuGrow.left : 0) + r.left,
      t: state.top - (menuGrow ? menuGrow.top : 0) + r.top,
      w: r.width,
      h: r.height
    }
  } catch (err) {}
}
// 菜单屏幕方框：钉住时用钉住的屏幕位置，否则由 menuRect（头顶上方）换算成屏幕坐标。
function menuBoxScreen() {
  if (menuPin) return menuPin
  var mr = menuRect()
  return { l: state.left + mr.left, t: state.top + mr.top, w: mr.w, h: mr.h }
}
// 菜单始终显示在鲸鱼头顶正上方（水平方向对齐角色画面中心，左右都不可出屏），
// 顶部放不下时翻到鲸鱼下方。菜单宽度比小窗更宽，由 growWindowForMenu 并集兜住。
function menuRect() {
  var baseW = winSize.w || whaleBase(state.scale)
  var artW = baseW * 0.5945
  var hang = topHang()
  var artRelTop = hang ? 0 : baseW - artW
  var artRelBottom = hang ? artW : baseW
  // 镜像态（h=left）角色靠左，水平中心 = artW/2；右对齐时 = baseW - artW/2。
  var artRelCx = state.h === 'left' ? artW / 2 : baseW - artW / 2
  var mw = menuBox.offsetWidth || 254
  var mh = menuBox.offsetHeight || 190
  var left = clamp(state.left + artRelCx - mw / 2, 0, Math.max(0, SCREEN.w - mw)) - state.left
  var top = artRelTop - mh
  if (state.top + top < 0) top = artRelBottom
  return { left: left, top: top, w: mw, h: mh }
}
function growWindowForMenu() {
  try {
    if (!menuOpen) return
    var mb = menuBoxScreen()
    var baseL = state.left, baseT = state.top
    var baseW = winSize.w || whaleBase(state.scale), baseH = winSize.h || baseW
    var winLeft = Math.max(0, Math.min(baseL, mb.l))
    var winTop = Math.max(0, Math.min(baseT, mb.t))
    var winRight = Math.min(SCREEN.w, Math.max(baseL + baseW, mb.l + mb.w))
    var winBottom = Math.min(SCREEN.h, Math.max(baseT + baseH, mb.t + mb.h))
    menuGrow = {
      left: baseL - winLeft,
      top: baseT - winTop,
      bottom: winBottom - (baseT + baseH)
    }
    // root 偏移：窗口扩张了多少，root 就反向挪多少，鲸鱼在屏幕上不动
    root.style.left = menuGrow.left + 'px'
    root.style.bottom = menuGrow.bottom + 'px'
    setWinBounds(winLeft, winTop, winRight - winLeft, winBottom - winTop)
    setWidgetCursor('')
    syncHang()
    positionMenu()
  } catch (err) {}
}
function shrinkWindowAfterMenu() {
  if (!menuGrow) return
  setWinBounds(state.left, state.top, winSize.w, winSize.h)
  root.style.left = ''
  root.style.bottom = ''
  menuGrow = null
  menuPin = null
}
function closeMenu() {
  if (menuOpen) {
    menuOpen = false
    shrinkWindowAfterMenu()
    menuBox.classList.remove('dshwv-menu-open')
  }
  snapCheck()
  syncDshAmbient()
  recheckPassThrough()
}
function snapCheck() {
  if (dshAppearance) {
    // 大肥鱼模式不做边缘吸附：只保证窗口留在屏内、不留贴边锚点
    state.h = null
    state.v = null
    state.hOff = state.left
    state.vOff = state.top
    express()
    return
  }
  // 小窗模型：用窗口在屏幕上的坐标判断贴边/贴角
  var w = winSize.w || viewport().w
  var h = winSize.h || viewport().h
  var left = state.left, top = state.top
  var centerX = left + w / 2
  var centerY = top + h / 2
  var moved = false
  if (centerX < SCREEN.w / 4) {
    state.h = 'left'
    state.hOff = 0
    left = 0
    moved = true
  } else if (centerX > SCREEN.w * 3 / 4) {
    state.h = 'right'
    state.hOff = 0
    left = SCREEN.w - w
    moved = true
  } else {
    state.h = null
    state.hOff = left
  }
  if (centerY < SCREEN.h / 4) {
    state.v = 'top'
    state.vOff = 0
    top = 0
    moved = true
  } else {
    state.v = 'bottom'
    state.vOff = Math.max(0, SCREEN.h - top - h)
  }
  if (moved) {
    state.left = left
    state.top = top
    settle()
  }
}
function positionMenu() {
  try {
    if (!menuOpen || !menuGrow) return
    var mb = menuBoxScreen()
    // 菜单 position:fixed 相对窗口：屏幕坐标 - 窗口原点 = 窗口内坐标
    menuBox.style.left = (mb.l - (state.left - menuGrow.left)) + 'px'
    menuBox.style.top = (mb.t - (state.top - menuGrow.top)) + 'px'
    menuBox.style.right = 'auto'
    menuBox.style.bottom = 'auto'
    menuBox.style.transformOrigin = 'bottom center'
  } catch (err) {}
}

var hitCanvas = null
var hitReady = false
function setupHitTest() {
  try {
    hitCanvas = document.createElement('canvas')
    hitCanvas.width = 610
    hitCanvas.height = 610
    var probe = new Image()
    probe.onload = function () {
      try {
        hitCanvas.getContext('2d').drawImage(probe, 0, 0)
        hitReady = true
      } catch (err) {}
    }
    probe.onerror = function () {}
    probe.src = img.src || IMG_DATA_URL
  } catch (err) {}
}
function isWhaleXY(clientX, clientY) {
  if (dshAppearance) {
    // shortcut: 精灵图按外接方框判定可点区，透明角落也算命中；真按像素判需要 canvas 采样
    var dr = dshStage.getBoundingClientRect()
    return clientX >= dr.left && clientX <= dr.right && clientY >= dr.top && clientY <= dr.bottom
  }
  if (!hitCanvas || !hitReady) return false
  try {
    var r = img.getBoundingClientRect()
    if (!r || r.width <= 0 || r.height <= 0) return false
    var lx = (clientX - r.left) / r.width * 610
    var ly = (clientY - r.top) / r.height * 610
    if (lx < 0 || ly < 0 || lx >= 610 || ly >= 610) return false
    if (state.h === 'left') lx = 610 - lx
    var data = hitCanvas.getContext('2d').getImageData(Math.floor(lx), Math.floor(ly), 1, 1).data
    return data[3] > 10
  } catch (err) {
    return false
  }
}
function isWhaleHit(e) { return isWhaleXY(e.clientX, e.clientY) }
// ---- 点击穿透 ----
// 光标不在鲸鱼像素/气泡形状/菜单上时整窗忽略鼠标（set_ignore_cursor_events），
// 窗口的透明空白区不再挡住桌面点击。穿透态 webview 收不到任何鼠标事件，
// 靠 ~66ms 轮询全局光标位置唤醒。只豁免光标实际压着的区域：气泡/菜单弹出
// 本身不再整窗强制不穿透（那会让弹出期间桌面点击全被挡）。拖拽中必须收事件。
var passThrough = false
function setPassThrough(on) {
  if (on && drag && drag.active) on = false
  if (on === passThrough) return
  passThrough = on
  try { WHALE.invoke('set_click_through', { on: on }).catch(function () {}) } catch (err) {}
}
// 全局光标 → 窗口内坐标。菜单弹出时窗口临时扩大，root 反向偏移，
// 窗口原点 = state.left/top - menuGrow.left/top，换算必须带上这个偏移。
function localOf(p) {
  return {
    x: p.x - state.left + (menuGrow ? menuGrow.left : 0),
    y: p.y - state.top + (menuGrow ? menuGrow.top : 0)
  }
}
// 光标是否压在可交互 UI 上。elementFromPoint 走 CSS pointer-events 规则：
// 收起的气泡/菜单整体免疫（pointer-events:none），只有展开的气泡形状、
// 打开的菜单能命中；菜单按钮靠 visible class 显式门控（它常驻 pointer-events:auto）。
function overUI(lx, ly) {
  var el = null
  try { el = document.elementFromPoint(lx, ly) } catch (err) { return false }
  if (!el || !el.closest) return false
  return !!(el.closest('.dshwv-bubble') || el.closest('.dshwv-menu') ||
            el.closest('.dshwv-menu-btn.dshwv-menu-btn-visible'))
}
function interactiveCursor(p) {
  var l = localOf(p)
  return isWhaleXY(l.x, l.y) || overUI(l.x, l.y)
}
setInterval(function () {
  if (!passThrough) return
  WHALE.invoke('cursor_pos').then(function (p) {
    if (passThrough && interactiveCursor(p)) setPassThrough(false)
  }).catch(function () {})
}, PASSTHROUGH_POLL_MS)
// 按光标实时位置重判穿透态：气泡/菜单关闭后鼠标可能静止不动，等不到
// pointermove，不重判就会一直挡点击（启动时也用它兜底）。
function recheckPassThrough() {
  WHALE.invoke('cursor_pos').then(function (p) {
    setPassThrough(!interactiveCursor(p))
  }).catch(function () {})
}
function onDocPointerDown(e) {
  if (e.target && e.target.closest) {
    if (e.target.closest('.dshwv-bubble') || e.target.closest('.dshwv-menu') || e.target.closest('.dshwv-menu-btn')) return
  }
  if (menuOpen) {
    closeMenu()
    return
  }
  if (e.button !== 0 && e.pointerType === 'mouse') return
  if (!isWhaleHit(e)) return
  try { e.preventDefault(); e.stopPropagation() } catch (err) {}
  // 拖拽移动整个鲸鱼窗口：起点记屏幕坐标（移窗后 clientX 不可靠）
  drag = {
    active: true,
    startScreenX: e.screenX,
    startScreenY: e.screenY,
    origLeft: state.left,
    origTop: state.top,
    w: winSize.w || viewport().w,
    h: winSize.h || viewport().h,
    moved: false
  }
  root.classList.add('dshwv-dragging')
  renderDsh()
  syncDshAmbient()
  pressDown()
  setWidgetCursor('grabbing')
  document.addEventListener('pointermove', onDocPointerMove, true)
  document.addEventListener('pointerup', onDocPointerUp, true)
  document.addEventListener('pointercancel', onDocPointerCancel, true)
  document.addEventListener('click', onDocClickStopper, true)
}
function onDocPointerMove(e) {
  if (!drag || !drag.active) return
  var dx = e.screenX - drag.startScreenX
  var dy = e.screenY - drag.startScreenY
  // 死区：位移没过 4px 既不算拖拽也不移窗，单击不能把精灵切成 drag
  if (!drag.moved) {
    if (dx * dx + dy * dy < CLICK_SQ) return
    drag.moved = true
    setDshSquish(false) // 真拖起来了：撤掉按压挤压，交给 drag 姿势
    renderDsh()
  }
  // Keep the pre-drag flip orientation while dragging (state.h/v stay as they
  // were); on release endDrag() recomputes the anchors and settle() flips the
  // class with a smooth transition instead of reverting instantly.
  state.left = clamp(drag.origLeft + dx, 0, Math.max(0, SCREEN.w - drag.w))
  state.top = clamp(drag.origTop + dy, 0, Math.max(0, SCREEN.h - drag.h))
  express()
}
function onDocPointerUp(e) { endDrag(e, true) }
function onDocPointerCancel(e) { endDrag(e, false) }
function onDocClickStopper(e) {
  try { e.preventDefault(); e.stopPropagation() } catch (err) {}
}
document.addEventListener('pointerdown', onDocPointerDown, true)
// 右键：干掉 webview 默认上下文菜单；压在鲸鱼像素上时弹与托盘同款的原生菜单，
// 其余位置（气泡/菜单）只压制不弹。空白区处于穿透态收不到事件，天然免疫。
document.addEventListener('contextmenu', function (e) {
  try { e.preventDefault(); e.stopPropagation() } catch (err) {}
  if (isWhaleHit(e)) {
    try { WHALE.invoke('popup_app_menu').catch(function () {}) } catch (err) {}
  }
})

var widgetCursor = ''
function setWidgetCursor(v) {
  if (v !== widgetCursor) {
    widgetCursor = v
    try { document.body.style.cursor = v } catch (err) {}
  }
}
function onDocPointerMoveCursor(e) {
  if (drag && drag.active) { setWidgetCursor('grabbing'); return }
  if (overUI(e.clientX, e.clientY)) {
    setWidgetCursor('')
    menuBtn.classList.add('dshwv-menu-btn-visible')
    setPassThrough(false)
    return
  }
  var over = isWhaleHit(e)
  setWidgetCursor(over ? 'grab' : '')
  menuBtn.classList.toggle('dshwv-menu-btn-visible', over || menuOpen)
  setPassThrough(!over)
}
document.addEventListener('pointermove', onDocPointerMoveCursor, true)

function endDrag(e, clickAllowed) {
  if (!drag || !drag.active) return
  drag.active = false
  document.removeEventListener('pointermove', onDocPointerMove, true)
  document.removeEventListener('pointerup', onDocPointerUp, true)
  document.removeEventListener('pointercancel', onDocPointerCancel, true)
  document.removeEventListener('click', onDocClickStopper, true)
  pressUp()
  root.classList.remove('dshwv-dragging')
  setWidgetCursor(isWhaleHit(e) ? 'grab' : '')
  if (clickAllowed && !drag.moved) {
    // 单击（位移 < 4px）：睡着先唤醒；挤压弹动由 pressDown/pressUp 表达，
    // 视频外观再叠一段随机「点击回应」（刚醒的那一下留给醒来动作）
    var woke = dshUserWake()
    // 凭据失效期间：单击鲸鱼 = 更新登录凭据（否则照旧轮换供应商）
    if (state.needCredential) {
      startCredentialRefresh()
      if (!woke) playDshClick()
      return
    }
    cycleProvider()
    showBubble(true)
    if (!woke) playDshClick()
    return
  }
  var dx = e.screenX - drag.startScreenX
  var dy = e.screenY - drag.startScreenY
  var left = clamp(drag.origLeft + dx, 0, Math.max(0, SCREEN.w - drag.w))
  var top = clamp(drag.origTop + dy, 0, Math.max(0, SCREEN.h - drag.h))
  state.left = left
  state.top = top
  if (dshAppearance) {
    // 大肥鱼模式：落在哪就停在哪，不吸附、不镜像
    state.h = null
    state.v = null
    state.hOff = left
    state.vOff = top
    settle()
    renderDsh()
    syncDshAmbient()
    return
  }
  var centerX = left + drag.w / 2
  var centerY = top + drag.h / 2
  if (centerX < SCREEN.w / 4) {
    state.h = 'left'
    state.hOff = 0
  } else if (centerX > SCREEN.w * 3 / 4) {
    state.h = 'right'
    state.hOff = 0
  } else {
    state.h = null
    state.hOff = left
  }
  if (centerY < SCREEN.h / 4) {
    state.v = 'top'
    state.vOff = 0
  } else if (centerY > SCREEN.h * 3 / 4) {
    state.v = 'bottom'
    state.vOff = 0
  } else {
    state.v = null
    state.vOff = top
  }
  state.left = left
  state.top = top
  settle()
}
window.addEventListener('resize', function () {
  // 菜单开着时窗口是临时撑大的：winSize 保存的是鲸鱼 base 尺寸，
  // 不能拿 innerHeight（含菜单撑高）覆盖它；只要按新几何重排窗口+菜单即可。
  if (!menuOpen && winSize.w) {
    winSize.w = window.innerWidth
    winSize.h = window.innerHeight
  }
  root.style.setProperty('--dshw-base', ((!menuOpen && window.innerWidth) || winSize.w) + 'px')
  if (menuOpen) {
    growWindowForMenu()
  } else {
    settle()
  }
})

function applyConfig(d) {
  if (d && typeof d.appearance === 'string') setAppearance(d.appearance, false)
  if (d && typeof d.dnd === 'boolean') setDnd(d.dnd, false)
  if (d && typeof d.scale === 'number') {
    // 旧配置/越界值：一律钳到 [MIN_SIZE_SCALE, MAX_SCALE]，避免窗口与 UI 不一致
    var s = Math.round(Math.min(MAX_SCALE, Math.max(MIN_SIZE_SCALE, Number(d.scale))) * 10) / 10
    // 小窗模型：scale 决定窗口尺寸（base），窗口位置读自真实窗口
    state.scale = s
    root.style.setProperty('--dshw-scale', String(s))
    scaleInput.value = String(s)
    scaleNumber.value = String(scaleToDisplay(s))
    var b = whaleBase(s)
    setWindowSize(b, b, true)
  }
  if (d && typeof d.vol === 'number') {
    soundVol = d.vol
    soundOn = soundVol > 0
    volInput.value = String(soundVol)
    volPct.textContent = Math.round(soundVol * 100) + '%'
    try {
      if (pressAudio) pressAudio.volume = soundVol
      if (releaseAudio) releaseAudio.volume = soundVol
    } catch (err) {}
  }
  if (d && typeof d.soundSet === 'string') {
    soundSet = d.soundSet === 'fx1' ? 'fx1' : (d.soundSet === 'whale' ? 'whale' : 'duck')
    soundSelect.value = soundSet
    applySoundSet()
  }
  if (d && typeof d.usageMode === 'string') {
    usageMode = ['token', 'opencode'].indexOf(d.usageMode) >= 0 ? d.usageMode : 'opencode'
    usageSelect.value = usageMode
  }
  if (d && typeof d.hasBailian === 'boolean') {
    state.hasBailian = d.hasBailian
  }
  if (d && typeof d.hasSuoxie === 'boolean') {
    state.hasSuoxie = d.hasSuoxie
  }
  if (d && typeof d.provider === 'string') {
    state.provider = d.provider
  }
  if (d && Array.isArray(d.bubbleProviders)) {
    state.bubbleProviders = d.bubbleProviders
  }
  // 重启后凭据仍未修好：直接把待办长气泡铺上，等用户点鲸鱼
  if (d && d.credentialNeedsUser) {
    state.needCredential = state.provider
    showStickyBubble(credentialStickyLines())
  }
  applyNotifyCfg(d)
  normalizeProvider()
  render()
  refresh(false)
}
function bootWidget() {
  setupHitTest()
  // 设置窗保存凭据后触发响应刷新（H2/L8）
  try {
    var evt = window.__TAURI__ && window.__TAURI__.event
    if (evt && typeof evt.listen === 'function') {
      evt.listen('refresh-balance', function () {
        // 先重载配置（保存通知阈值/凭据后免重启生效），再刷新余额，避免竞态用旧阈值判通知
        apiGetConfig().then(function (d) {
          if (d && typeof d.provider === 'string') state.provider = d.provider
          if (d && Array.isArray(d.bubbleProviders)) state.bubbleProviders = d.bubbleProviders
          normalizeProvider()
          applyNotifyCfg(d)
          refresh(true)
        }).catch(function () {
          refresh(true)
        })
      })
      // 后台自动续期失败：只弹长气泡等用户点鲸鱼，不自动拉浏览器
      evt.listen('credential-needed', function (e) {
        var d = (e && e.payload) || {}
        state.needCredential = String(d.provider || state.provider)
        state.credentialScope = d.scope === 'usage' ? 'usage' : 'login'
        if (!state.credentialBusy) showStickyBubble(credentialStickyLines())
      })
      // 余额抓取成功 / 自检确认可用：立刻收掉可能残留的待办长气泡。
      // 菜单触发的自检没有 invoke 回调可用于提示，这里补一个轻提示。
      evt.listen('credential-ok', function () {
        if (credentialErrorTimer) { clearTimeout(credentialErrorTimer); credentialErrorTimer = null }
        var hadPending = !!state.needCredential
        state.needCredential = ''
        clearStickyBubble()
        render()
        if (!state.credentialBusy) {
          showCredentialOk(hadPending)
        }
      })
      // 右键「更新登录凭据」失败：只提示这次没抓到，不宣称凭据失效
      evt.listen('credential-error', function (e) {
        var d = (e && e.payload) || {}
        if (state.credentialBusy) return
        showCredentialFailure(String(d.provider || state.provider), d.error)
      })
      // 梭子蟹福利自动化结果：成功播报；失败每 15 分钟重试，凭证类失败改为长气泡待办
      var lastWelfareFail = null
      evt.listen('suoxie-welfare', function (e) {
        var d = e && e.payload
        if (!d) return
        if (d.ok) {
          var okLines = [
            { t: '梭子蟹福利', s: 'A', c: '' },
            { t: String(d.message || '已完成'), s: 'B', c: '#2fa24c', w: true }
          ]
          if (d.date) okLines.push({ t: '明天见~', s: 'C', c: '' })
          showNotifBubble(okLines)
          refresh(true)
        } else if (d.needsUser) {
          // 登录态失败：交给长气泡待办，点鲸鱼更新登录
          state.needCredential = 'suoxie'
          state.credentialScope = 'login'
          if (!state.credentialBusy) showStickyBubble(credentialStickyLines())
        } else if (d.date && lastWelfareFail !== d.date) {
          lastWelfareFail = d.date
          showNotifBubble([
            { t: '梭子蟹福利失败', s: 'A', c: '#e0433f' },
            { t: String(d.error || '未知错误'), s: 'C', c: '#e0433f', w: true }
          ])
        }
      })
    }
  } catch (err) {}
  // 记下窗口当前逻辑坐标/尺寸（boot 时 Rust 已按配置 scale 摆好），
  // 尺寸不再从这里写 --dshw-base —— 统一由 applyConfig→setWindowSize 按 scale 重算，
  // 避免物理/逻辑像素混淆导致鲸鱼被裁剪。
  loadScreen()
    .then(function () {
      return WHALE.invoke('get_window_bounds')
    })
    .then(function (b) {
      if (b && isFinite(Number(b.x))) state.left = Number(b.x)
      if (b && isFinite(Number(b.y))) state.top = Number(b.y)
      if (b && isFinite(Number(b.width)) && Number(b.width) > 0) {
        winSize.w = Number(b.width)
        winSize.h = Number(b.height)
      }
      return apiGetConfig()
    })
    .then(applyConfig)
    .then(recheckPassThrough)
    .catch(function () {
      // 配置读取失败：按默认 scale 摆好窗口，别让鲸鱼悬空不显示
      state.scale = 1.5
      setWindowSize(whaleBase(1.5), whaleBase(1.5), true)
      refresh(false)
      settle()
    })
}
function loadAssets() {
  apiImageUrl()
    .then(function (url) {
      IMG_DATA_URL = url
      img.src = url
      bootWidget()
    })
    .catch(function () { bootWidget() })
  Object.keys(SOUND_URLS).forEach(function (setName) {
    ['press', 'release'].forEach(function (action) {
      apiSoundUrl(action, setName).then(function (url) {
        SOUND_URLS[setName][action] = url
        if (soundSet === setName) applySoundSet()
      }).catch(function () {})
    })
  })
  fetch('dsh/manifest.json').then(function (r) { return r.json() }).then(function (manifest) {
    dshManifest = manifest
    dshPlaying = '' // 让已播状态按 manifest 的帧数/帧率重播一次
    renderDsh()
    syncDshAmbient()
  }).catch(function () {})
  // 视频外观：states 状态映射 / modes 随机作息池 / clicks 单击回应 / balance 余额档位
  //（后三者都按文件名当 key，和随机动作共用一套 clip 表）。
  fetch('pet/manifest.json').then(function (r) { return r.json() }).then(function (m) {
    var clips = {}
    var states = m.states || {}
    Object.keys(states).forEach(function (k) {
      var s = states[k]
      clips[k] = { file: s.file, loop: s.loop !== false, walk: !!s.walk, rate: s.rate || 1, stopFrame: s.stopFrame || 0 }
    })
    function pick(list) {
      var out = []
      ;(list || []).forEach(function (c) {
        if (!c || !c.file) return
        clips[c.file] = { file: c.file, loop: false, walk: !!c.walk }
        out.push(c.file)
      })
      return out
    }
    petModes = pick(m.modes)
    petClicks = pick(m.clicks)
    petBalance = pick(m.balance)
    petClips = clips
    dshPlaying = ''
    renderDsh()
    syncDshAmbient()
  }).catch(function () {})
}
loadAssets()
setInterval(function () { refresh(false) }, REFRESH_MS)
})()
