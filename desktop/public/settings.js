(function () {
  var TAPI = (window.__TAURI__ && window.__TAURI__.core) || { invoke: function () { return Promise.reject(new Error('no tauri')) } }
  var apiEl = document.getElementById('api-key')
  var tokEl = document.getElementById('platform-token')
  var status = document.getElementById('status')

  function msg(t) {
    status.textContent = t
    setTimeout(function () { status.textContent = '' }, 2500)
  }

  document.getElementById('save').addEventListener('click', function () {
    TAPI.invoke('save_credentials', {
      apiKey: (apiEl.value || '').trim() || null,
      platformToken: (tokEl.value || '').trim() || null
    }).then(function () { msg('已保存') }).catch(function () { msg('保存失败') })
  })

  TAPI.invoke('load_credentials')
    .then(function (c) {
      if (c && c.apiKey) apiEl.value = c.apiKey
      if (c && c.platformToken) tokEl.value = c.platformToken
    })
    .catch(function () {})
})()