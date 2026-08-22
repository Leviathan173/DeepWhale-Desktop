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
// Tauri IPC bridge: 取代原浏览器版的路由请求（/dsh-whale/*）
var WHALE = window.__TAURI__ && window.__TAURI__.core ? window.__TAURI__.core : null
function apiBalance() { return WHALE.invoke('get_balance') }
function apiGetConfig() { return WHALE.invoke('get_config') }
function apiSetConfig(cfg) { return WHALE.invoke('set_config', cfg) }
function apiImageUrl() { return WHALE.invoke('image_data_url') }
function apiSoundUrl(action, set) { return WHALE.invoke('sound_data_url', { action: action, set: set }) }
function apiGetWindowBounds() { return WHALE.invoke('get_window_bounds') }
function apiMoveWindow(x, y) { return WHALE.invoke('move_window', { x: x, y: y }) }
function apiSetWindowBounds(x, y, width, height) {
  return WHALE.invoke('set_window_bounds', { x: x, y: y, width: width, height: height })
}
var IMG_DATA_URL = null
var IMG_URL = null
var SOUND_URLS = { duck: { press: null, release: null }, fx1: { press: null, release: null } }

var root = document.createElement('div')
root.className = 'dshwv-root'

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
soundSelect.addEventListener('change', function () { setSoundSet(soundSelect.value) })
var usageSelect = document.createElement('select')
usageSelect.className = 'dshwv-sound'
usageSelect.appendChild(soundOpt('ledger', '小鲸鱼记账 (推荐)'))
usageSelect.appendChild(soundOpt('token', '实时·令牌 (设置里自动获取)'))
usageSelect.appendChild(soundOpt('opencode', '本地·opencode+Claude (读本地记账)'))
usageSelect.addEventListener('change', function () { setUsageMode(usageSelect.value) })
var providerSelect = document.createElement('select')
providerSelect.className = 'dshwv-sound'
providerSelect.addEventListener('change', function () { setProvider(providerSelect.value) })
function syncProviderSelect() {
  var hasBail = state.hasBailian
  providerSelect.innerHTML = ''
  var opD = document.createElement('option')
  opD.value = 'deepseek'
  opD.textContent = 'DeepSeek'
  providerSelect.appendChild(opD)
  if (hasBail) {
    var opB = document.createElement('option')
    opB.value = 'bailian'
    opB.textContent = '百炼 TokenPlan'
    providerSelect.appendChild(opB)
  }
  if (!hasBail && state.provider === 'bailian') state.provider = 'deepseek'
  providerSelect.value = state.provider
}
var row1 = menuRow()
row1.appendChild(menuLabel('大小'))
row1.appendChild(scaleInput)
row1.appendChild(scaleNumber)
var row2 = menuRow()
row2.appendChild(menuLabel('音效'))
row2.appendChild(soundSelect)
var rowProv = menuRow()
rowProv.appendChild(menuLabel('供应商'))
rowProv.appendChild(providerSelect)
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
menuBox.appendChild(rowProv)
menuBox.appendChild(row3)
menuBox.appendChild(row4)
menuBox.appendChild(rowSet)

var textBox = document.createElement('div')
textBox.className = 'dshwv-text'
var labelEl = document.createElement('div')
labelEl.className = 'dshwv-label'
labelEl.textContent = '余额'
var amountEl = document.createElement('div')
amountEl.className = 'dshwv-amount'
var hintEl = document.createElement('div')
hintEl.className = 'dshwv-hint'
textBox.appendChild(labelEl)
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
  if (bubbleRandomActive) {
    // 再次点击：关闭
    hideBubble()
  } else {
    // 首次点击：切到随机台词段（不延长总显示时长）
    bubbleRandomActive = true
    bubbleRandomLines = pickRandomLines()
    swapBubbleContent(function () { applyBubbleLines(bubbleRandomLines) })
  }
})

var body = document.createElement('div')
body.className = 'dshwv-body'
body.appendChild(img)
body.appendChild(bubbleBox)
root.appendChild(body)
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
  provider: 'deepseek',
  hasBailian: false,
  bailian: null
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
var BUBBLE_STYLE_CLASS = { A: 'dshwv-label', B: 'dshwv-amount', P: 'dshwv-period', C: 'dshwv-hint' }
function pickOne(arr) { return arr[Math.floor(Math.random() * arr.length)] }
function singleCenter(style, text, color, wrap) { return [null, { t: text, s: style, c: color || '', w: !!wrap }, null] }
function providerLabel() {
  return state.provider === 'bailian' ? '百炼 TokenPlan' : 'DeepSeek 余额'
}
function buildGroup1() {
  if (state.provider === 'bailian') {
    var b = state.bailian
    if (!b || !b.ok) {
      return [
        { t: '百炼 · 未配置', s: 'A', c: '' },
        { t: '请在设置获取凭据', s: 'C', c: '' }
      ]
    }
    return [
      { t: '百炼周额度剩余:', s: 'A', c: '' },
      { t: String(b.remaining != null ? b.remaining : '--'), s: 'B', c: '' },
      { t: '重置 ' + fmtReset(b.resetAt), s: 'C', c: '' }
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
  { w: 7, lines: function () { return singleCenter('B', pickOne(['好模型... ↓', '好女孩...↓'])) } },
  { w: 7, lines: function () { return singleCenter('A', pickOne(['不知道用户有什么用，先赶走吧~', '我...我...我也要挣钱吗？', '我去吃饭啦，测完叫我', '压力一只蓝色大肥鱼？！', 'DeepSleep...', '坏了...用户彻底怒了！']), '', true) } },
  { w: 3, lines: function () { return singleCenter('A', pickOne(['你目录里的dsh是什么...大烧货吗...?', '恭喜你实现token自由！token全跑了！', '真当我是便宜货啊...']), '', true) } },
  { w: 1, lines: function () { return [{ t: '这个', s: 'A', c: '' }, { t: '凶', s: 'B', c: '' }, { t: '是什么意思呀...', s: 'A', c: '' }] } },
  { w: 1, lines: function () { return singleCenter('B', '哦鲸鲸... ') } },
]
function pickRandomLines() {
  var total = 0
  for (var i = 0; i < RANDOM_GROUPS.length; i++) total += RANDOM_GROUPS[i].w
  var r = Math.random() * total
  for (var i = 0; i < RANDOM_GROUPS.length; i++) {
    r -= RANDOM_GROUPS[i].w
    if (r < 0) return RANDOM_GROUPS[i].lines()
  }
  return RANDOM_GROUPS[RANDOM_GROUPS.length - 1].lines()
}
function applyBubbleLines(lines) {
  var els = [labelEl, amountEl, hintEl]
  for (var i = 0; i < 3; i++) {
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
function showBubble() {
  if (bubbleTimer) { clearTimeout(bubbleTimer); bubbleTimer = null }
  bubbleShown = true
  bubbleRandomActive = false
  restoreBubbleLines()
  bubbleBox.classList.add('dshwv-bubble-open')
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
  var amount, hint
  labelEl.textContent = providerLabel()
  if (state.provider === 'bailian') {
    var b = state.bailian
    if (state.status === 'error') {
      amount = '--'
      hint = state.message ? state.message.slice(0, 14) : '获取失败 · 点击重试'
    } else if (!b || !b.ok) {
      amount = '--'
      hint = (b && b.error) ? (b.configured === false ? '未配置百炼 · 请在设置获取' : b.error.slice(0, 14)) : '未配置百炼'
    } else {
      amount = b.remaining != null ? String(b.remaining) : '--'
      hint = '周剩余额度 · 重置 ' + fmtReset(b.resetAt)
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
  if (bubbleRandomActive && bubbleRandomLines) {
    applyBubbleLines(bubbleRandomLines)
  } else {
    setHint(hint)
  }
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
  root.classList.toggle('dshwv-left', state.h === 'left')
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
        var nb = Number(data.totalBalance)
        var nc = String(data.currency || 'CNY')
        var changed = state.balance !== null && (nb !== state.balance || nc !== state.currency)
        var currencyChanged = state.currency !== null && nc !== state.currency
        state.balance = nb
        state.currency = nc
        state.message = ''
        state.todayUsage = data.todayUsage !== undefined ? data.todayUsage : null
        state.isPeak = !!data.isPeak
        state.bailian = (data && data.bailian) || null
        // 凭据存在与否以接口返回为准：configured===false 才是「没配置」；
        // 账户运行中新增百炼凭据（设置页抓取后 emit refresh）也能在这里生效。
        if (data && data.bailian) {
          state.hasBailian = data.bailian.configured === false ? false : true
        }
        syncProviderSelect()
        if (state.provider === 'bailian') {
          // 百炼不走余额滚动动画（金额语义不同），直接渲染订阅数据
          state.status = 'ok'
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
var usageMode = 'ledger'
function saveConfig() {
  try {
    apiSetConfig({ scale: state.scale, sound: soundOn, vol: soundVol, soundSet: soundSet, usageMode: usageMode, provider: state.provider })
  } catch (err) {}
}
function setUsageMode(v) {
  usageMode = ['ledger', 'token', 'opencode'].indexOf(v) >= 0 ? v : 'ledger'
  usageSelect.value = usageMode
  saveConfig()
  refresh(false)
}
function setProvider(v) {
  if (!(v === 'deepseek' || v === 'bailian')) v = 'deepseek'
  if (v === 'bailian' && !state.hasBailian) v = 'deepseek'
  state.provider = v
  providerSelect.value = v
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
  soundSet = v === 'fx1' ? 'fx1' : 'duck'
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
  if (releasePlayed || !releaseAudio || !soundOn) return
  releasePlayed = true
  try {
    releaseAudio.currentTime = 0
    var p = releaseAudio.play()
    if (p && typeof p.catch === 'function') p.catch(function () {})
  } catch (err) {}
}
function pressDown() {
  body.style.transform = SQUISH
  pressing = true
  playPress()
}
function pressUp() {
  body.style.transform = 'scaleY(1) scaleX(1)'
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
}
function snapCheck() {
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
function isWhaleHit(e) {
  if (!hitCanvas || !hitReady) return false
  try {
    var r = img.getBoundingClientRect()
    if (!r || r.width <= 0 || r.height <= 0) return false
    var lx = (e.clientX - r.left) / r.width * 610
    var ly = (e.clientY - r.top) / r.height * 610
    if (lx < 0 || ly < 0 || lx >= 610 || ly >= 610) return false
    if (state.h === 'left') lx = 610 - lx
    var data = hitCanvas.getContext('2d').getImageData(Math.floor(lx), Math.floor(ly), 1, 1).data
    return data[3] > 10
  } catch (err) {
    return false
  }
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
  if (dx * dx + dy * dy >= CLICK_SQ) drag.moved = true
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

var widgetCursor = ''
function setWidgetCursor(v) {
  if (v !== widgetCursor) {
    widgetCursor = v
    try { document.body.style.cursor = v } catch (err) {}
  }
}
function onDocPointerMoveCursor(e) {
  if (drag && drag.active) { setWidgetCursor('grabbing'); return }
  var el = null
  try { el = document.elementFromPoint(e.clientX, e.clientY) } catch (err) {}
  if (el && el.closest && (el.closest('.dshwv-bubble') || el.closest('.dshwv-menu') || el.closest('.dshwv-menu-btn'))) {
    setWidgetCursor('')
    menuBtn.classList.add('dshwv-menu-btn-visible')
    return
  }
  var over = isWhaleHit(e)
  setWidgetCursor(over ? 'grab' : '')
  menuBtn.classList.toggle('dshwv-menu-btn-visible', over || menuOpen)
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
  if (clickAllowed && !drag.moved) { showBubble(); refresh(true); return }
  var dx = e.screenX - drag.startScreenX
  var dy = e.screenY - drag.startScreenY
  var left = clamp(drag.origLeft + dx, 0, Math.max(0, SCREEN.w - drag.w))
  var top = clamp(drag.origTop + dy, 0, Math.max(0, SCREEN.h - drag.h))
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
    soundSet = d.soundSet === 'fx1' ? 'fx1' : 'duck'
    soundSelect.value = soundSet
    applySoundSet()
  }
  if (d && typeof d.usageMode === 'string') {
    usageMode = ['ledger', 'token', 'opencode'].indexOf(d.usageMode) >= 0 ? d.usageMode : 'ledger'
    usageSelect.value = usageMode
  }
  if (d && typeof d.hasBailian === 'boolean') {
    state.hasBailian = d.hasBailian
  }
  if (d && typeof d.provider === 'string') {
    state.provider = d.provider === 'bailian' && state.hasBailian ? 'bailian' : 'deepseek'
  }
  syncProviderSelect()
  render()
  refresh(false)
}
function bootWidget() {
  setupHitTest()
  // 托盘「刷新余额」/ 设置窗保存凭据后触发响应刷新（H2/L8）
  try {
    var evt = window.__TAURI__ && window.__TAURI__.event
    if (evt && typeof evt.listen === 'function') {
      evt.listen('refresh-balance', function () { refresh(true) })
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
}
loadAssets()
setInterval(function () { refresh(false) }, REFRESH_MS)
})()
