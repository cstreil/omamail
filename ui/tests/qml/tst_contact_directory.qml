import QtQuick
import QtTest
import "../../contacts" as Contacts

Item {
  width: 200
  height: 100

  QtObject {
    id: fakeBackend
    property var calls: []
    property var callbacks: []

    function call(method, params, callback) {
      calls = calls.concat([{ method: String(method), params: params }])
      callbacks = callbacks.concat([callback])
    }
    function reply(index, result, error) { callbacks[index](result, error) }
    function clear() { calls = []; callbacks = [] }
  }

  Contacts.ContactDirectory {
    id: directory
    backend: fakeBackend
    available: true
    activeAccountId: "jmap:me@example.test"
  }

  TestCase {
    name: "ContactDirectory"

    function book(id, extra) {
      var value = { id: id, name: id, default: false, subscribed: false, readOnly: true }
      var fields = extra || ({})
      for (var key in fields) value[key] = fields[key]
      return value
    }

    function init() {
      directory.available = true
      directory.activeAccountId = "jmap:me@example.test"
      directory.reset()
      fakeBackend.clear()
    }

    function test_structured_errors_keep_known_guidance_only() {
      compare(directory.errorText({ code: -32000, message: "contacts_too_many" }),
        "Too many contacts to list. Narrow the search.")
      compare(directory.errorText({ code: -32000, message: "server wrote this" }),
        "The address book could not be read.")
      compare(directory.errorText("contacts_not_found"), "This contact no longer exists.")
      compare(directory.errorText(null), "")
    }

    function test_closing_detail_does_not_invalidate_list_reply() {
      directory.accountId = directory.activeAccountId
      directory.sources = [book("book")]
      directory.sourceId = "book"
      directory.rows = [{ id: "old" }]

      directory.refreshList(directory.accountId, "Bob")
      directory.openDetail("old")
      compare(fakeBackend.calls.length, 2)
      compare(directory.busy, true)
      directory.closeDetail()
      compare(directory.openContact, null)
      compare(directory.busy, true, "the independent list is still in flight")

      fakeBackend.reply(0, { contacts: [{ id: "new", name: "Bob" }], total: 1 }, null)
      compare(directory.rows[0].id, "new")
      compare(directory.total, 1)
      compare(directory.busy, false)

      fakeBackend.reply(1, { contact: { id: "old" } }, null)
      compare(directory.openContact, null, "the closed detail reply stays stale")
    }

    function test_refresh_preserves_query_and_source_when_available() {
      var owner = directory.activeAccountId
      directory.accountId = owner
      directory.sources = [book("a", { default: true }), book("b")]
      directory.sourceId = "b"
      directory.query = "Bob"

      directory.refreshSources(owner, "Bob")
      compare(fakeBackend.calls[0].method, "contacts.sources")
      fakeBackend.reply(0, { sources: [book("a", { default: true }), book("b")] }, null)
      compare(fakeBackend.calls[1].method, "contacts.list")
      compare(fakeBackend.calls[1].params.source, "b")
      compare(fakeBackend.calls[1].params.query, "Bob")
      compare(directory.sourceId, "b")
      compare(directory.query, "Bob")
      fakeBackend.reply(1, { contacts: [], total: 0 }, null)

      directory.refreshSources(owner, "Bob")
      fakeBackend.reply(2, { sources: [book("a", { default: true })] }, null)
      compare(fakeBackend.calls[3].params.source, "a",
        "a missing source falls back to the readable default")
      compare(fakeBackend.calls[3].params.query, "Bob",
        "falling back does not silently remove the visible filter")
    }

    function test_source_chosen_during_refresh_is_not_reverted() {
      var owner = directory.activeAccountId
      directory.accountId = owner
      directory.sources = [book("a", { default: true }), book("b")]
      directory.sourceId = "a"
      directory.query = "Bob"

      directory.refreshSources(owner, "Bob")
      directory.selectSource("b", "Bob")
      compare(fakeBackend.calls[1].method, "contacts.list")
      compare(fakeBackend.calls[1].params.source, "b")

      fakeBackend.reply(0, { sources: [book("a", { default: true }), book("b")] }, null)
      compare(directory.sourceId, "b", "the newer user choice wins over the refresh snapshot")
      compare(fakeBackend.calls[2].params.source, "b")
      fakeBackend.reply(1, { contacts: [{ id: "stale" }], total: 1 }, null)
      compare(directory.rows.length, 0, "the superseded list reply stays stale")
      fakeBackend.reply(2, { contacts: [{ id: "live" }], total: 1 }, null)
      compare(directory.rows[0].id, "live")
    }

    function test_list_and_detail_errors_do_not_clear_each_other() {
      directory.accountId = directory.activeAccountId
      directory.sources = [book("book")]
      directory.sourceId = "book"

      directory.refreshList(directory.accountId, "")
      directory.openDetail("missing")
      fakeBackend.reply(1, null, { code: -32000, message: "contacts_not_found" })
      compare(directory.detailError, "This contact no longer exists.")
      compare(directory.error, "", "a detail-only failure does not replace the usable list")
      fakeBackend.reply(0, { contacts: [], total: 0 }, null)
      compare(directory.detailError, "This contact no longer exists.",
        "a successful list must not erase an independent detail failure")
      directory.closeDetail()
      compare(directory.detailError, "")

      directory.refreshList(directory.accountId, "")
      directory.openDetail("live")
      fakeBackend.reply(2, null, { code: -32000, message: "contacts_source_unknown" })
      compare(directory.error, "That address book is no longer available.")
      fakeBackend.reply(3, { contact: { id: "live" } }, null)
      compare(directory.error, "That address book is no longer available.",
        "a successful detail must not erase an independent list failure")
    }

    function test_late_account_reply_is_ignored() {
      directory.refreshSources("jmap:first@example.test", "Ada")
      compare(directory.busy, true)
      directory.refreshSources("jmap:second@example.test", "Bob")
      compare(fakeBackend.calls.length, 2)
      fakeBackend.reply(0, { sources: [book("stale")] }, null)
      compare(directory.sources.length, 0)
      compare(directory.busy, true, "the live account request is still pending")
      fakeBackend.reply(1, { sources: [book("live", { default: true })] }, null)
      compare(directory.sourceId, "live")
      compare(fakeBackend.calls[2].params.query, "Bob")
    }
  }
}
