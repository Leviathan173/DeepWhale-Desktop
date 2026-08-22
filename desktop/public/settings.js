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

  function modelRow(m) {
    var tr = document.createElement('tr')
    var tdPat = document.createElement('td')
    var pat = document.createElement('input')
    pat.type = 'text'
    pat.placeholder = 'deepseek / longcat…'
    if (m && m.pattern) pat.value = m.pattern
    tdPat.appendChild(pat)
    var tdPpm = document.createElement('td')
    var ppm = document.createElement('input')
    ppm.type = 'number'
    ppm.min = '0'
    ppm.step = '0.01'
    ppm.placeholder = '留空=内置价目'
    if (m && m.ppm != null) ppm.value = m.ppm
    tdPpm.appendChild(ppm)
    var tdDel = document.createElement('td')
    var del = document.createElement('button')
    del.type = 'button'
    del.className = 'del'
    del.textContent = '✕'
    del.addEventListener('click', function () { tr.remove() })
    tdDel.appendChild(del)
    tr.append(tdPat, tdPpm, tdDel)
    return tr
  }

  /// 按计费模式切换整个卡片可用态：套餐下模型区置灰并提示。
  function applyBillMode(box, metric) {
    box.classList.toggle('disabled', !metric)
    var note = box.querySelector('.prov-note')
    if (!note) return
    note.textContent = metric ? '' : '套餐（订阅制）：不计入今日金额，模型单价不生效。'
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
    del.addEventListener('click', function () { box.remove() })
    title.append(lbl, name, radioButton('按量计费', true), radioButton('套餐·订阅制', false), del)

    // 模型区
    var models = document.createElement('div')
    models.className = 'prov-models'
    var table = document.createElement('table')
    table.className = 'mini'
    var thead = document.createElement('thead')
    var hr0 = document.createElement('tr')
    hr0.innerHTML = '<th>模型（名字匹配）</th><th>单价 元/百万 token</th><th></th>'
    thead.appendChild(hr0)
    var tbody = document.createElement('tbody')
    table.append(thead, tbody)
    models.appendChild(table)
    var addRowBtn = document.createElement('button')
    addRowBtn.type = 'button'
    addRowBtn.className = 'tiny models-add'
    addRowBtn.textContent = '+ 添加模型'
    addRowBtn.addEventListener('click', function () { tbody.appendChild(modelRow(null)) })
    var note = document.createElement('div')
    note.className = 'prov-note'

    box.append(title, models, addRowBtn, note)
    applyBillMode(box, metric)
    if (data && data.models) data.models.forEach(function (m) { tbody.appendChild(modelRow(m)) })
    pricingBox.appendChild(box)
  }

  function collectProviders() {
    var out = []
    pricingBox.querySelectorAll('.prov').forEach(function (prov) {
      var name = prov.querySelector(':scope > .prov-title input[type=text]').value.trim()
      if (!name) return
      var metric = prov.querySelector(':scope > .prov-title input[type=radio]').checked
      var models = []
      prov.querySelectorAll('tbody tr').forEach(function (tr) {
        var pattern = tr.querySelector('input[type=text]').value.trim()
        if (!pattern) return
        var ppmV = tr.querySelector('input[type=number]').value
        var rec = { pattern: pattern }
        if (ppmV !== '') rec.ppm = parseFloat(ppmV)
        models.push(rec)
      })
      out.push({ name: name, metric: metric, models: models })
    })
    return out
  }

  var addProvBtn = document.getElementById('addprov')
  if (addProvBtn) {
    addProvBtn.addEventListener('click', function () { addProv(null) })
    document.getElementById('savepricing').addEventListener('click', function () {
      TAPI.invoke('set_usage_providers', { providers: collectProviders() }).then(function () {
        pricingMsg('计价表已保存')
        try {
          var e = window.__TAURI__ && window.__TAURI__.event
          if (e && typeof e.emit === 'function') e.emit('refresh-balance')
        } catch (err) {}
      }).catch(function () { pricingMsg('保存失败') })
    })
    TAPI.invoke('get_usage_providers')
      .then(function (providers) {
        if (Array.isArray(providers)) providers.forEach(addProv)
      })
      .catch(function () {})
  }
})()