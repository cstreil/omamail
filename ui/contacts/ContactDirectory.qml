import QtQuick

// Account-scoped, read-only address-book state. Sources, rows and detail reads
// are independent requests: closing a detail must not invalidate a list reply,
// while an account or source change deliberately invalidates narrower work.
QtObject {
  id: root

  required property var backend
  property bool available: false
  property string activeAccountId: ""

  property var sources: []
  property string sourceId: ""
  property var rows: []
  property int total: 0
  property var openContact: null
  property string sourcesError: ""
  property string listError: ""
  property string detailError: ""
  readonly property string error: sourcesError !== "" ? sourcesError : listError
  property string accountId: ""
  property string query: ""

  property int sourcesGeneration: 0
  property int listGeneration: 0
  property int detailGeneration: 0
  property bool sourcesBusy: false
  property bool listBusy: false
  property bool detailBusy: false
  readonly property bool busy: sourcesBusy || listBusy || detailBusy

  onAvailableChanged: if (!available) reset()

  function errorText(value) {
    var code = value && typeof value === "object"
      ? String(value.message || value.code || "") : String(value || "")
    if (code === "contacts_not_supported") return "This account has no address book."
    if (code === "contacts_account_unknown") return "This account is no longer configured."
    if (code === "contacts_no_address_book") return "No readable address book on this account."
    if (code === "contacts_source_unknown") return "That address book is no longer available."
    if (code === "contacts_too_many") return "Too many contacts to list. Narrow the search."
    if (code === "contacts_not_found") return "This contact no longer exists."
    return code === "" ? "" : "The address book could not be read."
  }

  function invalidateAll() {
    sourcesGeneration += 1
    listGeneration += 1
    detailGeneration += 1
    sourcesBusy = false
    listBusy = false
    detailBusy = false
  }

  function reset() {
    invalidateAll()
    sources = []
    sourceId = ""
    rows = []
    total = 0
    openContact = null
    sourcesError = ""
    listError = ""
    detailError = ""
    accountId = ""
    query = ""
  }

  function sourceById(id) {
    var wanted = String(id || "")
    for (var i = 0; i < sources.length; i++) {
      if (sources[i] && String(sources[i].id || "") === wanted) return sources[i]
    }
    return null
  }

  function preferredSource(previous) {
    var kept = sourceById(previous)
    if (kept) return kept
    for (var i = 0; i < sources.length; i++) {
      if (sources[i] && sources[i].default === true) return sources[i]
    }
    for (var j = 0; j < sources.length; j++) {
      if (sources[j] && sources[j].subscribed === true) return sources[j]
    }
    return sources.length > 0 ? sources[0] : null
  }

  function refreshSources(requestAccountId, requestQuery) {
    var owner = requestAccountId === undefined || requestAccountId === null
      ? String(activeAccountId || "") : String(requestAccountId)
    var nextQuery = requestQuery === undefined || requestQuery === null
      ? query : String(requestQuery)
    if (!available || owner === "") {
      reset()
      return
    }
    if (accountId !== owner) {
      sources = []
      sourceId = ""
      rows = []
      total = 0
      openContact = null
    }
    accountId = owner
    query = nextQuery
    sourcesError = ""
    listError = ""
    detailError = ""
    sourcesGeneration += 1
    listGeneration += 1
    detailGeneration += 1
    listBusy = false
    detailBusy = false
    openContact = null
    var generation = sourcesGeneration
    sourcesBusy = true
    backend.call("contacts.sources", { accountId: owner }, function(result, failure) {
      if (generation !== root.sourcesGeneration || owner !== root.accountId) return
      root.sourcesBusy = false
      if (failure) {
        root.sourcesError = root.errorText(failure)
        root.sources = []
        root.sourceId = ""
        root.rows = []
        root.total = 0
        return
      }
      root.sources = result && Array.isArray(result.sources) ? result.sources : []
      root.sourcesError = root.sources.length === 0 ? "This account has no address book." : ""
      var chosen = root.preferredSource(root.sourceId)
      root.sourceId = chosen ? String(chosen.id || "") : ""
      if (root.sourceId !== "") root.refreshList(owner, root.query)
      else { root.rows = []; root.total = 0 }
    })
  }

  function refreshList(requestAccountId, requestQuery) {
    var owner = requestAccountId === undefined || requestAccountId === null
      ? accountId : String(requestAccountId)
    var nextQuery = requestQuery === undefined || requestQuery === null
      ? query : String(requestQuery)
    if (!available || owner === "" || owner !== accountId || sourceId === "") return
    query = nextQuery
    listError = ""
    var requestedSource = sourceId
    var generation = ++listGeneration
    listBusy = true
    var params = { accountId: owner, source: requestedSource, limit: 200 }
    if (query !== "") params.query = query
    backend.call("contacts.list", params, function(result, failure) {
      if (generation !== root.listGeneration || owner !== root.accountId
          || requestedSource !== root.sourceId) return
      root.listBusy = false
      if (failure) {
        root.listError = root.errorText(failure)
        root.rows = []
        root.total = 0
        return
      }
      root.listError = ""
      root.rows = result && Array.isArray(result.contacts) ? result.contacts : []
      root.total = result && typeof result.total === "number" ? result.total : root.rows.length
    })
  }

  function selectSource(nextSourceId, nextQuery) {
    var next = String(nextSourceId === undefined || nextSourceId === null ? "" : nextSourceId)
    if (!sourceById(next)) return
    var wantedQuery = nextQuery === undefined || nextQuery === null ? query : String(nextQuery)
    if (next === sourceId) {
      if (wantedQuery !== query) refreshList(accountId, wantedQuery)
      return
    }
    sourceId = next
    query = wantedQuery
    listGeneration += 1
    detailGeneration += 1
    listBusy = false
    detailBusy = false
    listError = ""
    detailError = ""
    openContact = null
    rows = []
    total = 0
    refreshList(accountId, query)
  }

  function openDetail(contactId) {
    var owner = accountId
    var id = String(contactId === undefined || contactId === null ? "" : contactId)
    if (!available || owner === "" || id === "") return
    var generation = ++detailGeneration
    detailError = ""
    detailBusy = true
    backend.call("contacts.get", { accountId: owner, id: id }, function(result, failure) {
      if (generation !== root.detailGeneration || owner !== root.accountId) return
      root.detailBusy = false
      if (failure) {
        root.detailError = root.errorText(failure)
        return
      }
      root.detailError = ""
      root.openContact = result && result.contact ? result.contact : null
    })
  }

  function closeDetail() {
    detailGeneration += 1
    detailBusy = false
    detailError = ""
    openContact = null
  }
}
