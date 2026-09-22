import QtQuick

// Shared, read-only account-calendar source registry. It is intentionally above
// the two range controllers: the window and bar preview consume the same source
// snapshot, while keeping independent event ranges and caches.
Item {
  id: root
  visible: false
  width: 0
  height: 0

  required property var backend
  property bool available: false
  property var accountSummaries: []
  property var sources: []
  property bool busy: false
  property string error: ""
  property int generation: 0
  property int pending: 0
  property var requestAccounts: []
  property var rowsByAccount: ({})

  readonly property string accountKey: eligibleAccounts().map(function(account) {
    return String(account.id || account.email || "")
  }).join("\n")

  onAvailableChanged: refresh()
  onAccountKeyChanged: refresh()
  Component.onCompleted: refresh()

  Connections {
    target: root.backend
    ignoreUnknownSignals: true
    function onReadyChanged() { root.refresh() }
  }

  function eligibleAccounts() {
    var values = Array.isArray(accountSummaries) ? accountSummaries : []
    var out = []
    for (var i = 0; i < values.length; i++) {
      var account = values[i] || {}
      var id = String(account.id || account.email || "")
      if (id !== "" && account.provider === "jmap" && account.signedIn === true)
        out.push(account)
    }
    return out
  }

  function rebuild() {
    var out = []
    for (var i = 0; i < requestAccounts.length; i++) {
      var id = String(requestAccounts[i].id || requestAccounts[i].email || "")
      var rows = rowsByAccount[id]
      if (!Array.isArray(rows)) continue
      for (var r = 0; r < rows.length; r++) out.push(rows[r])
    }
    sources = out
  }

  function validRows(owner, result) {
    if (!result || !Array.isArray(result.sources)) return null
    var out = []
    for (var i = 0; i < result.sources.length; i++) {
      var source = result.sources[i]
      if (!source || String(source.id || "") === ""
          || String(source.accountId || "") !== owner
          || String(source.kind || "") !== "account"
          || String(source.name || "") === "") return null
      out.push(source)
    }
    return out
  }

  function refresh() {
    var mine = ++generation
    pending = 0
    busy = false
    error = ""
    sources = []
    rowsByAccount = ({})
    requestAccounts = eligibleAccounts()
    if (!available || !backend || backend.ready !== true || requestAccounts.length === 0) return
    pending = requestAccounts.length
    busy = true
    for (var i = 0; i < requestAccounts.length; i++) {
      var owner = String(requestAccounts[i].id || requestAccounts[i].email || "")
      request(owner, mine)
    }
  }

  function request(owner, mine) {
    backend.call("calendar.sources", { accountId: owner }, function(result, failure) {
      if (mine !== root.generation || !root.available) return
      var rows = failure ? null : root.validRows(owner, result)
      if (rows === null) {
        root.error = "Account calendars could not be read"
        rows = []
      }
      var next = ({})
      for (var key in root.rowsByAccount) next[key] = root.rowsByAccount[key]
      next[owner] = rows
      root.rowsByAccount = next
      root.pending = Math.max(0, root.pending - 1)
      root.busy = root.pending > 0
      root.rebuild()
    })
  }
}
