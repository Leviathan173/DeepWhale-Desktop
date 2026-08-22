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

  document.getElementById('save').addEventListener('click', function () {
    TAPI.invoke('save_credentials', {
      apiKey: (apiEl.value || '').trim() || null,
      platformToken: (tokEl.value || '').trim() || null
    }).then(function () {
      msg('已保存')
      // 让主窗口立即刷新余额（L8: 换 key 后不用等 60s）
      try {
        var e = window.__TAURI__ && window.__TAURI__.event
        if (e && typeof e.emit === 'function') e.emit('refresh-balance')
      } catch (err) {}
    }).catch(function () { msg('保存失败') })
  })

  TAPI.invoke('load_credentials')
    .then(function (c) {
      if (c && c.apiKey) apiEl.value = c.apiKey
      if (c && c.platformToken) tokEl.value = c.platformToken
    })
    .catch(function () {})

  // ---- 用量计价表编辑 ----
  var pricingBox = document.getElementById('pricing')
  var pricestatus = document.getElementById('pricestatus')

  function pricingMsg(t) {
    pricestatus.textContent = t
    setTimeout(function () { pricestatus.textContent = '' }, 2500)
  }

  function modelRow(m) {
    var tr = document.createElement('tr')

    var tdPat = document.createElement('td')
    var pat = document.createElement('input')
    pat.type = 'text'
    pat.placeholder = '模型名片段，如 deepseek'
    if (m && m.pattern) pat.value = m.pattern
    tdPat.appendChild(pat)

    var tdPpm = document.createElement('td')
    var ppm = document.createElement('input')
    ppm.type = 'number'
    ppm.min = '0'
    ppm.step = '0.01'
    ppm.placeholder = '元/百万 (留空=内置)'
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

  function addProv(data) {
    var box = document.createElement('div')
    box.className = 'prov'

    var head = document.createElement('div')
    head.className = 'prov-head'
    var name = document.createElement('input')
    name.type = 'text'
    name.placeholder = '供应商名（deepseek / bailian / LongCat…）'
    if (data && data.name) name.value = data.name
    var metric = document.createElement('input')
    metric.type = 'checkbox'
    if (!data || data.metric !== false) metric.checked = true
    var mlbl = document.createElement('label')
    mlbl.appendChild(metric)
    mlbl.append('按量计费')
    var del = document.createElement('button')
    del.type = 'button'
    del.className = 'del'
    del.textContent = '✕'
    del.addEventListener('click', function () { box.remove() })
    head.append(name, mlbl, del)

    var table = document.createElement('table')
    table.className = 'mini'
    var tbody = document.createElement('tbody')
    table.appendChild(tbody)
    box.append(head, table)

    var addRowBtn = document.createElement('button')
    addRowBtn.type = 'button'
    addRowBtn.className = 'tiny'
    addRowBtn.textContent = '+ 模型'
    addRowBtn.addEventListener('click', function () { tbody.appendChild(modelRow(null)) })
    box.appendChild(addRowBtn)

    if (data && data.models) data.models.forEach(function (m) { tbody.appendChild(modelRow(m)) })
    pricingBox.appendChild(box)
  }

  function collectProviders() {
    var out = []
    pricingBox.querySelectorAll('.prov').forEach(function (prov) {
      var name = prov.querySelector(':scope > .prov-head input[type=text]').value.trim()
      if (!name) return
      var metric = prov.querySelector(':scope > .prov-head input[type=checkbox]').checked
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