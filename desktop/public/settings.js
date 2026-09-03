(function () {
  var TAPI = (window.__TAURI__ && window.__TAURI__.core) || { invoke: function () { return Promise.reject(new Error('no tauri')) } }
  var apiEl = document.getElementById('api-key')
  var tokEl = document.getElementById('platform-token')
  var status = document.getElementById('status')
  var tokBtn = document.getElementById('autotoken')
  var tokStatus = document.getElementById('tokstatus')
  var bailCookEl = document.getElementById('bailian-cookie')
  var bailBodyEl = document.getElementById('bailian-post-data')
  var bailBtn = document.getElementById('autobailian')
  var bailSaveBtn = document.getElementById('savebailian')
  var bailStatus = document.getElementById('bailianstatus')
  var hadBailCookie = null
  var hadBailBody = null

  if (bailBtn) {
    bailBtn.addEventListener('click', function () {
      bailBtn.disabled = true
      bailMsg('正在打开浏览器…请在独立窗口里登录百炼控制台')
      TAPI.invoke('capture_bailian_credentials').then(function (res) {
        if (res && res.bailianCookie) {
          bailCookEl.value = res.bailianCookie
          hadBailCookie = res.bailianCookie
        }
        if (res && res.bailianPostData) {
          bailBodyEl.value = res.bailianPostData
          hadBailBody = res.bailianPostData
        }
        bailMsg('已自动获取并保存百炼凭据 ✓')
        refreshBalance()
      }).catch(function (e) {
        bailMsg('失败：' + ((e && e.message) || '请尝试手动粘贴'), true)
      }).finally(function () { bailBtn.disabled = false })
    })
  }

  if (bailSaveBtn) {
    bailSaveBtn.addEventListener('click', function () {
      bailMsg('正在保存…')
      TAPI.invoke('save_bailian_credentials', {
        bCookie: ((bailCookEl && bailCookEl.value) || '').trim() || hadBailCookie,
        bPostData: ((bailBodyEl && bailBodyEl.value) || '').trim() || hadBailBody
      }).then(function () {
        bailMsg('百炼凭据已保存', false)
        refreshBalance()
      }).catch(function () { bailMsg('保存失败', true) })
    })
  }

  function msg(t) {
    status.textContent = t
    setTimeout(function () { status.textContent = '' }, 2500)
  }

  // ---- 余额/花费通知阈值 ----
  var notifyEls = {
    dsHourlyLimit: document.getElementById('ds-hourly-limit'),
    dsMinBalance: document.getElementById('ds-min-balance'),
    blHourlyPct: document.getElementById('bl-hourly-pct'),
    blRemainingPct: document.getElementById('bl-remaining-pct')
  }
  var notifyStatus = document.getElementById('notifystatus')
  var notifyBtn = document.getElementById('savenotify')
  var notifyMsgSeq = 0
  function notifyMsg(t, err) {
    var seq = ++notifyMsgSeq
    if (!notifyStatus) return
    notifyStatus.style.color = err ? '#e0433f' : '#2fa24c'
    notifyStatus.textContent = t
    setTimeout(function () { if (seq === notifyMsgSeq) notifyStatus.textContent = '' }, 2500)
  }
  function notifyPayload() {
    // 空输入 = None（关闭通知）；number 输入自带 NaN 兜底
    var out = {}
    Object.keys(notifyEls).forEach(function (k) {
      var el = notifyEls[k]
      var v = el ? parseFloat(el.value) : NaN
      out[k] = isFinite(v) && v > 0 ? v : null
    })
    return out
  }
  function fillNotify(payload) {
    if (!payload) return
    Object.keys(notifyEls).forEach(function (k) {
      var el = notifyEls[k]
      var v = payload[k]
      if (el && v != null && isFinite(Number(v))) el.value = v
    })
  }
  if (notifyBtn) {
    notifyBtn.addEventListener('click', function () {
      notifyMsg('正在保存…')
      TAPI.invoke('set_notify_prefs', notifyPayload()).then(function () {
        notifyMsg('通知设置已保存')
        refreshBalance()
      }).catch(function () { notifyMsg('保存失败', true) })
    })
  }
  TAPI.invoke('get_config')
    .then(function (cfg) { if (cfg) fillNotify(cfg) })
    .catch(function () {})

  var tokMsgSeq = 0
  function tokMsg(t, err) {
    var seq = ++tokMsgSeq
    tokStatus.style.color = err ? '#e0433f' : '#2fa24c'
    tokStatus.textContent = t
    setTimeout(function () { if (seq === tokMsgSeq) tokStatus.textContent = '' }, 2500)
  }

  function refreshBalance() {
    try {
      var e = window.__TAURI__ && window.__TAURI__.event
      if (e && typeof e.emit === 'function') e.emit('refresh-balance')
    } catch (err) {}
  }

  // 只清掉本 timer 对应的消息，避免快速连发时旧 timer 误清新消息
  var bailMsgSeq = 0
  function bailMsg(t, err) {
    var seq = ++bailMsgSeq
    bailStatus.style.color = err ? '#e0433f' : '#2fa24c'
    bailStatus.textContent = t
    setTimeout(function () { if (seq === bailMsgSeq) bailStatus.textContent = '' }, 2500)
  }

  if (tokBtn) {
    tokBtn.addEventListener('click', function () {
      tokBtn.disabled = true
      tokMsg('正在打开浏览器…请在独立窗口里登录 platform.deepseek.com/usage')
      TAPI.invoke('capture_login_token').then(function (token) {
        if (token) { tokEl.value = token; msg('已自动获取并保存令牌') }
        tokMsg('已获取令牌 ✓')
        try {
          var e = window.__TAURI__ && window.__TAURI__.event
          if (e && typeof e.emit === 'function') e.emit('refresh-balance')
        } catch (err) {}
      }).catch(function (e) {
        tokMsg('失败：' + ((e && e.message) || '请手动复制 Authorization'), true)
      }).finally(function () { tokBtn.disabled = false })
    })
  }

  var hadKey = null
  var hadToken = null

  document.getElementById('save').addEventListener('click', function () {
    TAPI.invoke('save_credentials', {
      // 输入框留空 = 保留已保存的值；只有改填新值才覆盖（防止误清空又得重新获取）。
      apiKey: (apiEl.value || '').trim() || hadKey,
      platformToken: (tokEl.value || '').trim() || hadToken
    }).then(function () {
      msg('已保存')
      // 让主窗口立即刷新余额（L8: 换 key 后不用等 60s）
      try {
        var e = window.__TAURI__ && window.__TAURI__.event
        if (e && typeof e.emit === 'function') e.emit('refresh-balance')
      } catch (err) {}
    }).catch(function () { msg('保存失败') })
  })

  function loadedCreds(c) {
    if (c) {
      if (c.apiKey) { apiEl.value = c.apiKey; hadKey = c.apiKey }
      if (c.platformToken) { tokEl.value = c.platformToken; hadToken = c.platformToken }
      if (c.bailianCookie && bailCookEl) { bailCookEl.value = c.bailianCookie; hadBailCookie = c.bailianCookie }
      if (c.bailianPostData && bailBodyEl) { bailBodyEl.value = c.bailianPostData; hadBailBody = c.bailianPostData }
    }
  }
  TAPI.invoke('load_credentials')
    .then(loadedCreds)
    .catch(function () {})

  // ---- 用量计价表编辑：以提供商为粒度的卡片 ----
  var pricingBox = document.getElementById('pricing')
  var pricestatus = document.getElementById('pricestatus')
  var provSeq = 0

  function pricingMsg(t) {
    pricestatus.textContent = t
    setTimeout(function () { pricestatus.textContent = '' }, 2500)
  }

  /// 模型单价四类（元/百万 token），值留空=该类别用内置价目。
  var PRICE_FIELDS = ['input', 'output', 'cache_read', 'cache_creation']

  function priceInput(field, tier, m) {
    var inp = document.createElement('input')
    inp.type = 'number'
    inp.min = '0'
    inp.step = '0.01'
    inp.placeholder = '无内置'
    var src = m ? (tier === 'peak' ? m['peak_' + field] : m[field]) : null
    if (src != null) inp.value = src
    // 用户手动改动 → 视为自定义（去掉内置标记，才会被保存）
    inp.addEventListener('input', function () {
      if (inp.value !== '') { inp.classList.remove('auto'); inp.classList.remove('auto-fill') }
      else inp.classList.remove('auto-fill')
    })
    return inp
  }

  /// 覆盖/标记某格为「内置价目」填充（灰显、保存时跳过）。
  function markAuto(inp, v) {
    if (inp.classList.contains('auto-fill')) return // 用户已手动改过，不覆盖
    if (inp.value === '' || inp.classList.contains('auto')) {
      inp.classList.add('auto', 'auto-fill')
      inp.value = v == null ? '' : v
    }
  }

  /// 按内置价目回填一行模型（空闲/峰谷两档；非峰谷只填空闲行）。
  function refill(grp, peak) {
    var pat = grp.patInput.value.trim().toLowerCase()
    if (!pat) return
    TAPI.invoke('builtin_prices', { model: pat }).then(function (r) {
      PRICE_FIELDS.forEach(function (f) {
        var b = r == null ? null : r[f]
        markAuto(grp.inputs[f].off, b == null ? null : b[0])
        if (peak) markAuto(grp.inputs[f].peak, b == null ? null : b[1])
      })
    }).catch(function () {})
  }

  /// 高峰行（仅峰谷供应商创建，避免 rowspan 跨隐藏行导致后续模型列错位）。
  function buildPeakRow(grp) {
    var pk = document.createElement('tr')
    pk.className = 'mpeak'
    var tierCell = function () {
      var td = document.createElement('td')
      td.className = 'tier'
      td.textContent = '高峰'
      return td
    }
    pk.appendChild(tierCell())
    PRICE_FIELDS.forEach(function (f) {
      var inp = priceInput(f, 'peak', grp._m)
      grp.inputs[f].peak = inp
      var td = document.createElement('td')
      td.appendChild(inp)
      pk.appendChild(td)
    })
    pk._grp = grp
    return pk
  }

  /// 一个模型对应一行（非峰谷）或两行（峰谷：空闲+高峰）。
  /// 返回 [off 行, peak 行或 null]。
  function modelRows(m, peak) {
    var grp = { inputs: {}, _m: m || null }
    var off = document.createElement('tr')
    off.className = 'moff'

    var tdPat = document.createElement('td')
    var pat = document.createElement('input')
    pat.type = 'text'
    pat.placeholder = '模型全名，如 deepseek-v4-flash'
    if (m && m.pattern) pat.value = m.pattern
    tdPat.appendChild(pat)
    tdPat.rowSpan = peak ? 2 : 1

    var tdDel = document.createElement('td')
    var del = document.createElement('button')
    del.type = 'button'
    del.className = 'del'
    del.textContent = '✕'
    del.addEventListener('click', function () {
      off.remove()
      if (grp.peakRow) grp.peakRow.remove()
      scheduleProvSave()
    })
    tdDel.appendChild(del)
    tdDel.rowSpan = peak ? 2 : 1

    var tierCell = function (label) {
      var td = document.createElement('td')
      td.className = 'tier'
      td.textContent = label
      return td
    }

    off.appendChild(tierCell('空闲'))
    off.appendChild(tdPat)
    PRICE_FIELDS.forEach(function (f) {
      var inp = priceInput(f, 'off', m)
      grp.inputs[f] = { off: inp }
      var td = document.createElement('td')
      td.appendChild(inp)
      off.appendChild(td)
    })
    off.appendChild(tdDel)

    grp.patInput = pat
    grp.peakRow = null
    off._grp = grp
    // 改名后重填内置价目
    pat.addEventListener('change', function () { refill(grp, peak) })
    if (peak) {
      grp.peakRow = buildPeakRow(grp)
    }
    if (m && m.pattern) refill(grp, peak)
    return [off, grp.peakRow]
  }

  /// 峰谷开关切换：增删高峰行并回填（用 rowSpan 而非隐藏行，避免列错位）。
  function applyPeakMode(box, peak) {
    box.querySelectorAll('tr.moff').forEach(function (tr) {
      var g = tr._grp
      var patTd = tr.children[1]
      var delTd = tr.children[6]
      if (patTd) patTd.rowSpan = peak ? 2 : 1
      if (delTd) delTd.rowSpan = peak ? 2 : 1
      if (peak && !g.peakRow) {
        g.peakRow = buildPeakRow(g)
        tr.parentNode.insertBefore(g.peakRow, tr.nextSibling)
      } else if (!peak && g.peakRow) {
        g.peakRow.remove()
        g.peakRow = null
      }
      if (g.peakRow) refill(g, peak)
    })
  }

  /// 按计费模式切换整个卡片可用态：套餐下模型区置灰并提示。
  function applyBillMode(box, metric) {
    box.classList.toggle('disabled', !metric)
    var note = box.querySelector('.prov-note')
    if (note) note.textContent = metric ? '' : '套餐（订阅制）：不计入今日金额，模型单价不生效。'
    var peakLbl = box.querySelector('.peak-lbl')
    if (peakLbl) peakLbl.style.display = metric ? '' : 'none'
  }

  function addProv(data) {
    var box = document.createElement('div')
    box.className = 'prov'

    // 标题行：提供商名 + 计费模式单选 + 删除
    var title = document.createElement('div')
    title.className = 'prov-title'
    var lbl = document.createElement('span')
    lbl.className = 'lbl'
    lbl.textContent = '提供商'
    var name = document.createElement('input')
    name.type = 'text'
    name.placeholder = 'deepseek / bailian / LongCat…'
    if (data && data.name) name.value = data.name
    var group = 'bill-' + (provSeq++)
    box.id = 'prov-' + (provSeq - 1)
    var metric = !(data && data.metric === false)
    var radioButton = function (text, isMetric) {
      var b = document.createElement('input')
      b.type = 'radio'
      b.name = group
      if (isMetric === metric) b.checked = true
      b.addEventListener('change', function () { applyBillMode(box, isMetric) })
      var lb = document.createElement('label')
      lb.className = 'bill'
      lb.appendChild(b)
      lb.appendChild(document.createTextNode(text))
      return lb
    }
    var del = document.createElement('button')
    del.type = 'button'
    del.className = 'del'
    del.textContent = '✕ 删除'
    del.addEventListener('click', function () { box.remove(); buildNav(); scheduleProvSave() })
    name.addEventListener('change', function () { buildNav(); scheduleProvSave() })
    title.append(lbl, name, radioButton('按量计费', true), radioButton('套餐·订阅制', false))
    var peakCb = document.createElement('input')
    peakCb.type = 'checkbox'
    peakCb.checked = !data || data.peak !== false
    peakCb.addEventListener('change', function () { applyPeakMode(box, peakCb.checked) })
    var peakLbl = document.createElement('label')
    peakLbl.className = 'bill peak-lbl'
    peakLbl.appendChild(peakCb)
    peakLbl.append('峰谷计价')
    title.append(peakLbl, del)

    // 模型区
    var models = document.createElement('div')
    models.className = 'prov-models'
    var table = document.createElement('table')
    table.className = 'mini'
    // 固定布局 + colgroup：列宽稳定，不依赖各行业行 nth-child
    var colgroup = document.createElement('colgroup')
    var colW = ['42px', '24%', '', '', '', '', '30px']
    colW.forEach(function (w) {
      var col = document.createElement('col')
      if (w) col.style.width = w
      colgroup.appendChild(col)
    })
    table.appendChild(colgroup)
    var thead = document.createElement('thead')
    var hr0 = document.createElement('tr')
    hr0.innerHTML = '<th>档位</th><th>模型（精确全名）</th><th>输入</th><th>输出</th><th>缓存读取</th><th>缓存创建</th><th></th>'
    thead.appendChild(hr0)
    var tbody = document.createElement('tbody')
    table.append(thead, tbody)
    models.appendChild(table)
    var peak = !data || data.peak !== false
    // 一行模型 = 空闲/高峰两行
    var addRowBtn = document.createElement('button')
    addRowBtn.type = 'button'
    addRowBtn.className = 'tiny models-add'
    addRowBtn.textContent = '+ 添加模型'
    addRowBtn.addEventListener('click', function () {
      modelRows(null, peak).filter(Boolean).forEach(function (tr) { tbody.appendChild(tr) })
      scheduleProvSave()
    })
    var note = document.createElement('div')
    note.className = 'prov-note'

    box.append(title, models, addRowBtn, note)
    applyBillMode(box, metric)
    if (data && data.models) data.models.forEach(function (m) {
      modelRows(m, peak).filter(Boolean).forEach(function (tr) { tbody.appendChild(tr) })
    })
    pricingBox.appendChild(box)
  }

  /// 左侧导航：区块锚点 + 各提供商卡片锚点，点击平滑滚动。
  function buildNav() {
    var nav = document.getElementById('sidenav')
    if (!nav) return
    nav.innerHTML = ''
    function title(t) {
      var d = document.createElement('div')
      d.className = 'nav-title'
      d.textContent = t
      nav.appendChild(d)
    }
    function link(label, target) {
      var a = document.createElement('a')
      a.textContent = label
      a.addEventListener('click', function () {
        var el = typeof target === 'string' ? document.getElementById(target) : target
        if (el) el.scrollIntoView({ behavior: 'smooth', block: 'start' })
        nav.querySelectorAll('a').forEach(function (x) { x.classList.remove('active') })
        a.classList.add('active')
      })
      nav.appendChild(a)
    }
    title('设置')
    link('凭据设置', 'sec-creds')
    link('百炼令牌套餐', 'sec-bailian')
    link('余额/花费通知', 'sec-notify')
    link('用量计价', 'sec-pricing')
    link('关于与更新', 'sec-update')
    var boxCount = pricingBox.querySelectorAll('.prov').length
    if (boxCount > 0) {
      title('提供商')
      pricingBox.querySelectorAll('.prov').forEach(function (box) {
        var nm = box.querySelector('.prov-title input[type=text]') ? box.querySelector('.prov-title input[type=text]').value : ''
        link(nm || '（未命名）', box.id)
      })
    }
  }

  function collectProviders() {
    var out = []
    pricingBox.querySelectorAll('.prov').forEach(function (prov) {
      var name = prov.querySelector(':scope > .prov-title input[type=text]').value.trim()
      if (!name) return
      var metric = prov.querySelector(':scope > .prov-title input[type=radio]').checked
      var models = []
      prov.querySelectorAll('tbody tr.moff').forEach(function (tr) {
        var grp = tr._grp
        var pattern = grp.patInput.value.trim()
        if (!pattern) return
        var rec = { pattern: pattern }
        PRICE_FIELDS.forEach(function (f) {
          var off = grp.inputs[f].off
          var pk = grp.inputs[f].peak
          // 灰显的内置价目仅是展示、不落盘（定价仍走内置表）
          if (!off.classList.contains('auto-fill') && off.value !== '') rec[f] = parseFloat(off.value)
          if (!pk.classList.contains('auto-fill') && pk.value !== '') rec['peak_' + f] = parseFloat(pk.value)
        })
        models.push(rec)
      })
      out.push({ name: name, metric: metric, peak: (prov.querySelector('.peak-lbl input[type=checkbox]') || {}).checked, models: models })
    })
    return out
  }

  /// 编辑结束（去抖）后立即自动保存计价表。
  var saveTimer = null

  function scheduleProvSave() {
    if (saveTimer) clearTimeout(saveTimer)
    saveTimer = setTimeout(function () { doSavePricing(true) }, 600)
  }

  function doSavePricing(auto) {
    saveTimer = null
    TAPI.invoke('set_usage_providers', { providers: collectProviders() }).then(function () {
      buildNav()
      pricingMsg(auto ? '已自动保存' : '计价表已保存')
      try {
        var e = window.__TAURI__ && window.__TAURI__.event
        if (e && typeof e.emit === 'function') e.emit('refresh-balance')
      } catch (err) {}
    }).catch(function () { pricingMsg('保存失败') })
  }

  // 任意编辑（输入/切换）都触发自动保存；程序化赋值不派发事件，初始加载/自动获取不会误存。
  pricingBox.addEventListener('input', scheduleProvSave)
  pricingBox.addEventListener('change', scheduleProvSave)

  var addProvBtn = document.getElementById('addprov')
  if (addProvBtn) {
    addProvBtn.addEventListener('click', function () { addProv(null); buildNav() })
    var autoBtn = document.getElementById('autoprov')
    if (autoBtn) {
      autoBtn.addEventListener('click', function () {
        autoBtn.disabled = true
        TAPI.invoke('auto_discover_pricing').then(function (res) {
          var providers = res && res.providers
          // 清掉重建 DOM 前可能挂着的去抖保存，避免它用旧 table 快照覆盖新结果
          if (saveTimer) { clearTimeout(saveTimer); saveTimer = null }
          pricingBox.innerHTML = ''
          if (Array.isArray(providers)) providers.forEach(addProv)
          buildNav()
          var keys = (res && res.apiKeys) || {}
          var filled = []
          if (keys.deepseek) { apiEl.value = keys.deepseek; hadKey = keys.deepseek; filled.push('DeepSeek') }
          if (keys.bailian) { filled.push('百炼') }
          var others = Object.keys(keys).filter(function (k) {
            return k !== 'deepseek' && k !== 'bailian'
          })
          if (others.length) filled.push(others.join('、'))
          pricingMsg(filled.length
            ? '已自动填入凭据：' + filled.join('、') + '（后续编辑自动保存）'
            : '已生成计价表，检查后编辑即自动保存')
        }).catch(function () { pricingMsg('自动获取失败') })
          .finally(function () { autoBtn.disabled = false })
      })
    }
    var savePricingBtn = document.getElementById('savepricing')
    if (savePricingBtn) {
      savePricingBtn.addEventListener('click', function () { doSavePricing(false) })
    }
    TAPI.invoke('get_usage_providers')
      .then(function (providers) {
        if (Array.isArray(providers)) providers.forEach(addProv)
        buildNav()
      })
      .catch(function () {})
  }

  // ===== 关于与更新（在线升级，tauri-plugin-updater） =====
  var upCheckBtn = document.getElementById('checkupdate')
  var upInstallBtn = document.getElementById('installupdate')
  var upStatus = document.getElementById('updatestatus')
  var pendingUpdate = null
  var upBusy = false
  function upMsg(t) { if (upStatus) upStatus.textContent = t }

  TAPI.invoke('app_version')
    .then(function (v) {
      var el = document.getElementById('curver')
      if (el) el.textContent = 'v' + v
    })
    .catch(function () {})

  // 无前端打包环境，直接调 updater 插件的 IPC 命令（等价 @tauri-apps/plugin-updater 的 check/downloadAndInstall）
  function doCheck() {
    if (!upCheckBtn || upBusy) return
    upBusy = true
    upCheckBtn.disabled = true
    upMsg('正在检查更新…')
    TAPI.invoke('plugin:updater|check')
      .then(function (u) {
        if (u) {
          pendingUpdate = u
          upMsg('发现新版本 v' + u.version + (u.body ? '\n' + u.body : ''))
          if (upInstallBtn) upInstallBtn.disabled = false
        } else {
          pendingUpdate = null
          if (upInstallBtn) upInstallBtn.disabled = true
          upMsg('已是最新版本')
        }
      })
      .catch(function () { upMsg('检查失败（离线或无可用更新源）') })
      .finally(function () { upBusy = false; upCheckBtn.disabled = false })
  }

  if (upCheckBtn) upCheckBtn.addEventListener('click', doCheck)
  if (upInstallBtn) {
    upInstallBtn.addEventListener('click', function () {
      if (!pendingUpdate) return
      upInstallBtn.disabled = true
      upCheckBtn.disabled = true
      var got = 0
      var total = 0
      TAPI.invoke('plugin:updater|download_and_install', {
        rid: pendingUpdate.rid,
        onEvent: function (ev) {
          if (ev.event === 'Started') {
            total = (ev.data && ev.data.contentLength) || 0
            upMsg('下载中 0%')
          } else if (ev.event === 'Progress') {
            got += (ev.data && ev.data.chunkLength) || 0
            if (total) upMsg('下载中 ' + Math.min(99, Math.floor((got * 100) / total)) + '%')
          } else if (ev.event === 'Finished') {
            upMsg('下载完成，正在安装…')
          }
        },
      })
        .then(function () {
          upMsg('安装完成，正在重启…')
          return TAPI.invoke('restart_app')
        })
        .catch(function (e) {
          upMsg('更新失败：' + e)
          upInstallBtn.disabled = false
          upCheckBtn.disabled = false
        })
    })
  }

  // 托盘「检查更新」：事件/启动两条路各自原子取挂起标志，谁取到谁触发（事件丢失由启动 take 兜底，无竞态）
  function takeAndCheck() {
    TAPI.invoke('take_check_update')
      .then(function (on) { if (on) doCheck() })
      .catch(function () {})
  }
  var upEvt = window.__TAURI__ && window.__TAURI__.event
  if (upEvt) upEvt.listen('check-update', takeAndCheck)
  takeAndCheck()
})()