(function () {
  var TAPI = (window.__TAURI__ && window.__TAURI__.core) || { invoke: function () { return Promise.reject(new Error('no tauri')) } }
  var apiEl = document.getElementById('api-key')
  var tokEl = document.getElementById('platform-token')
  var status = document.getElementById('status')
  var tokBtn = document.getElementById('autotoken')
  var tokStatus = document.getElementById('tokstatus')

  function msg(t) {
    status.textContent = t
    setTimeout(function () { status.textContent = '' }, 2500)
  }

  function tokMsg(t, err) {
    tokStatus.style.color = err ? '#e0433f' : '#2fa24c'
    tokStatus.textContent = t
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

  /// 一个模型 = 上下两行（空闲/高峰）。非峰谷供应商的高峰行隐藏（display:none）。
  /// 返回 [off 行, peak 行]。
  function modelRows(m, peak) {
    var grp = { inputs: {} }
    var off = document.createElement('tr')
    off.className = 'moff'
    var pk = document.createElement('tr')
    pk.className = 'mpeak' + (peak ? '' : ' hidden')

    var tdPat = document.createElement('td')
    var pat = document.createElement('input')
    pat.type = 'text'
    pat.placeholder = '模型全名，如 deepseek-v4-flash'
    if (m && m.pattern) pat.value = m.pattern
    tdPat.appendChild(pat)
    tdPat.rowSpan = 2

    var tdDel = document.createElement('td')
    var del = document.createElement('button')
    del.type = 'button'
    del.className = 'del'
    del.textContent = '✕'
    del.addEventListener('click', function () { off.remove(); pk.remove(); scheduleProvSave() })
    tdDel.appendChild(del)
    tdDel.rowSpan = 2

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

    pk.appendChild(tierCell('高峰'))
    PRICE_FIELDS.forEach(function (f) {
      var inp = priceInput(f, 'peak', m)
      grp.inputs[f].peak = inp
      var td = document.createElement('td')
      td.appendChild(inp)
      pk.appendChild(td)
    })

    grp.patInput = pat
    off._grp = pk._grp = grp
    // 改名后重填内置价目
    pat.addEventListener('change', function () { refill(grp, peak) })
    if (m && m.pattern) refill(grp, peak)
    return [off, pk]
  }

  /// 峰谷开关切换：显示/隐藏高峰行并回填。
  function applyPeakMode(box, peak) {
    box.querySelectorAll('tr.mpeak').forEach(function (tr) { tr.classList.toggle('hidden', !peak) })
    box.querySelectorAll('tr.moff').forEach(function (tr) { if (tr._grp) refill(tr._grp, peak) })
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
      modelRows(null, peak).forEach(function (tr) { tbody.appendChild(tr) })
      scheduleProvSave()
    })
    var note = document.createElement('div')
    note.className = 'prov-note'

    box.append(title, models, addRowBtn, note)
    applyBillMode(box, metric)
    if (data && data.models) data.models.forEach(function (m) {
      modelRows(m, peak).forEach(function (tr) { tbody.appendChild(tr) })
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
    link('用量计价', 'sec-pricing')
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
    document.getElementById('savepricing').addEventListener('click', function () {
      doSavePricing(false)
    })
    TAPI.invoke('get_usage_providers')
      .then(function (providers) {
        if (Array.isArray(providers)) providers.forEach(addProv)
        buildNav()
      })
      .catch(function () {})
  }
})()