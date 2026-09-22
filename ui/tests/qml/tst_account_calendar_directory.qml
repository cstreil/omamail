import QtQuick
import QtTest
import "../../calendar" as Calendar

Item {
  width: 320
  height: 200

  QtObject {
    id: fakeBackend
    property var requests: []
    property var callbacks: []
    property bool ready: true
    function call(method, params, callback) {
      requests.push({method: method, params: params})
      callbacks.push(callback)
    }
  }

  Calendar.AccountCalendarDirectory {
    id: directory
    backend: fakeBackend
    available: true
    accountSummaries: []
  }

  TestCase {
    name: "AccountCalendarDirectory"

    function init() {
      directory.available = false
      directory.accountSummaries = []
      fakeBackend.requests = []
      fakeBackend.callbacks = []
      directory.available = true
      wait(0)
    }

    function jmap(id, email) {
      return {id: id, email: email, provider: "jmap", signedIn: true}
    }

    function answer(index, sources, error) {
      fakeBackend.callbacks[index]({sources: sources || []}, error || null)
      wait(0)
    }

    function test_requests_only_signed_in_jmap_accounts_and_keeps_account_order() {
      directory.accountSummaries = [
        jmap("jmap:first@example.com", "first@example.com"),
        {id: "imap:other@example.com", provider: "imap", signedIn: true},
        jmap("jmap:second@example.com", "second@example.com"),
        {id: "jmap:later@example.com", provider: "jmap", signedIn: false}
      ]
      wait(0)
      compare(fakeBackend.requests.length, 2)
      compare(fakeBackend.requests[0].method, "calendar.sources")
      compare(JSON.stringify(fakeBackend.requests[0].params),
        JSON.stringify({accountId: "jmap:first@example.com"}))
      compare(fakeBackend.requests[1].params.accountId, "jmap:second@example.com")
      verify(directory.busy)

      answer(1, [{id:"second", kind:"account", accountId:"jmap:second@example.com",
        name:"Second", enabled:true, readOnly:true}])
      compare(directory.sources.length, 1)
      verify(directory.busy)
      answer(0, [{id:"first", kind:"account", accountId:"jmap:first@example.com",
        name:"First", enabled:true, readOnly:true}])
      compare(JSON.stringify(directory.sources.map(function(source) { return source.id })),
        JSON.stringify(["first", "second"]))
      compare(directory.busy, false)
    }

    function test_removed_account_invalidates_a_late_reply() {
      directory.accountSummaries = [jmap("jmap:first@example.com", "first@example.com")]
      wait(0)
      compare(fakeBackend.requests.length, 1)
      var stale = fakeBackend.callbacks[0]
      directory.accountSummaries = [jmap("jmap:second@example.com", "second@example.com")]
      wait(0)
      compare(fakeBackend.requests.length, 2)
      stale({sources:[{id:"stale", kind:"account", accountId:"jmap:first@example.com"}]}, null)
      wait(0)
      compare(directory.sources.length, 0)
      fakeBackend.callbacks[1]({sources:[{id:"current", kind:"account",
        accountId:"jmap:second@example.com", name:"Current", enabled:true, readOnly:true}]}, null)
      wait(0)
      compare(directory.sources.length, 1)
      compare(directory.sources[0].id, "current")
    }

    function test_malformed_or_cross_account_rows_fail_closed_without_raw_diagnostics() {
      directory.accountSummaries = [jmap("jmap:first@example.com", "first@example.com")]
      wait(0)
      answer(0, [{id:"foreign", kind:"account", accountId:"jmap:other@example.com"}])
      compare(directory.sources.length, 0)
      compare(directory.error, "Account calendars could not be read")

      fakeBackend.requests = []
      fakeBackend.callbacks = []
      directory.refresh()
      answer(0, [], {code:-32000, message:"private server diagnostic"})
      compare(directory.sources.length, 0)
      compare(directory.error, "Account calendars could not be read")
    }

    function test_disabling_capability_clears_and_invalidates() {
      directory.accountSummaries = [jmap("jmap:first@example.com", "first@example.com")]
      wait(0)
      var stale = fakeBackend.callbacks[0]
      directory.available = false
      compare(directory.sources.length, 0)
      compare(directory.busy, false)
      stale({sources:[{id:"stale",kind:"account",accountId:"jmap:first@example.com"}]}, null)
      compare(directory.sources.length, 0)
    }
  }
}
