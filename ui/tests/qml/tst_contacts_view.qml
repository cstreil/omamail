import QtQuick
import QtTest
import "../../components" as Omamail

Item {
  width: 900
  height: 600

  QtObject {
    id: directory
    property string activeAccountId: "me@example.com"
    property string contactsDirectoryAccountId: "me@example.com"
    property var contactSources: [
      ({ id: "book", name: "Address Book", default: true, subscribed: true, readOnly: false }),
      ({ id: "trusted", name: "Trusted Senders", default: false, subscribed: false, readOnly: false })
    ]
    property string contactSource: "book"
    property var contactRows: [
      ({ id: "one", name: "Alice", emails: ["alice@example.test"], source: "book" }),
      ({ id: "two", name: "Bob", emails: ["bob@example.test"], source: "book" })
    ]
    property int contactTotal: 2
    property bool contactsDirectoryBusy: false
    property var openContact: null
    property string contactsDirectoryError: ""
    property string contactsDirectoryDetailError: ""

    property var listCalls: []
    property var sourceCalls: []
    property var detailCalls: []
    property int closeCalls: 0

    function refreshContactSources(accountId, query) {
      sourceCalls.push({ accountId: String(accountId), query: String(query || "") })
    }
    function refreshContactList(accountId, query) {
      listCalls.push({ accountId: String(accountId), query: String(query) })
    }
    function selectContactSource(sourceId, query) {
      sourceCalls.push("select:" + String(sourceId))
      contactSource = String(sourceId)
      contactRows = []
      contactTotal = 0
    }
    function openContactDetail(contactId) {
      detailCalls.push(String(contactId))
      openContact = {
        id: String(contactId), source: "book", name: "Alice",
        emails: [{ value: "alice@example.test", label: "work" }],
        phones: [{ value: "+49-1", label: "mobile" }],
        addresses: [{ value: "Example Street 1", label: "" }]
      }
    }
    function closeContactDetail() {
      closeCalls += 1
      openContact = null
    }
  }

  Omamail.ContactsView {
    id: view
    anchors.fill: parent
    service: directory
    textColor: Qt.rgba(1, 1, 1, 1)
    backgroundColor: Qt.rgba(0.06, 0.06, 0.06, 1)
    accentColor: Qt.rgba(1, 0.5, 0, 1)
    dimColor: Qt.rgba(0.67, 0.67, 0.67, 1)
    popupBackgroundColor: Qt.rgba(0.13, 0.13, 0.13, 1)
    popupBorderColor: Qt.rgba(0.53, 0.53, 0.53, 1)
    panelFontFamily: "monospace"
  }

  TestCase {
    name: "ContactsView"
    when: windowShown

    function named(item, objectName) {
      if (!item) return null
      if (item.objectName === objectName) return item
      var values = item.children || []
      for (var i = 0; i < values.length; i++) {
        var found = named(values[i], objectName)
        if (found) return found
      }
      return null
    }

    function init() {
      directory.contactSource = "book"
      directory.contactRows = [
        ({ id: "one", name: "Alice", emails: ["alice@example.test"], source: "book" }),
        ({ id: "two", name: "Bob", emails: ["bob@example.test"], source: "book" })
      ]
      directory.contactTotal = 2
      directory.openContact = null
      directory.contactsDirectoryError = ""
      directory.contactsDirectoryDetailError = ""
      directory.listCalls = []
      directory.sourceCalls = []
      directory.detailCalls = []
      directory.closeCalls = 0
      view.query = ""
      view.selectedId = ""
    }

    function test_rows_sources_and_search_reach_the_directory() {
      compare(named(view, "contact-list") !== null, true)
      compare(named(view, "contact-source-book") !== null, true,
        "one button per readable address book")
      compare(named(view, "contact-source-trusted") !== null, true)

      view.selectSource("trusted")
      compare(directory.contactSource, "trusted")
      compare(directory.sourceCalls.indexOf("select:trusted") >= 0, true)

      var field = named(view, "contact-search")
      verify(field !== null)
      field.text = "bob"
      tryCompare(directory.listCalls, "length", 1)
      compare(directory.listCalls[0].query, "bob")
      view.refresh(directory.activeAccountId)
      compare(directory.sourceCalls[directory.sourceCalls.length - 1].query, "bob",
        "refresh keeps the query visible in the field and active in the request")
      compare(field.text, "bob")
    }

    function test_escape_clears_search_with_one_request() {
      var field = named(view, "contact-search")
      field.text = "bob"
      tryCompare(directory.listCalls, "length", 1)
      directory.listCalls = []
      compare(view.goBack(), true)
      compare(field.text, "")
      compare(directory.listCalls.length, 1)
      compare(directory.listCalls[0].query, "")
      wait(250)
      compare(directory.listCalls.length, 1,
        "the text change must not leave a duplicate debounce request armed")
    }

    function test_keyboard_moves_selection_and_opens_a_contact() {
      view.moveSelection(-1)
      compare(view.selectedId, "two", "up from nothing selects the last row")
      view.selectedId = ""
      view.moveSelection(1)
      compare(view.selectedId, "one", "the first row is selected from nothing")
      view.moveSelection(1)
      compare(view.selectedId, "two")
      view.moveSelection(1)
      compare(view.selectedId, "two", "selection stops at the last row")
      view.moveSelection(-1)
      compare(view.selectedId, "one")

      view.activate(view.selectedId)
      compare(directory.detailCalls[directory.detailCalls.length - 1], "one")
      verify(view.contact !== null)
      compare(view.contact.name, "Alice")
      compare(named(view, "contact-detail").visible, true)

      view.closeDetail()
      compare(directory.closeCalls, 1)
      compare(view.contact, null)
    }

    function test_unavailable_and_empty_states_are_explained() {
      directory.contactRows = []
      directory.contactTotal = 0
      directory.contactSource = ""
      compare(named(view, "contact-message").text, "No address book on this account.")

      directory.contactSource = "book"
      compare(named(view, "contact-message").text, "No contacts found.")

      directory.contactsDirectoryError = "This account has no address book."
      compare(named(view, "contact-message").text, "This account has no address book.")
      compare(named(view, "contact-list").visible, false,
        "a source/list error replaces the list instead of pretending it is empty")

      directory.contactsDirectoryError = ""
      directory.contactsDirectoryDetailError = "This contact no longer exists."
      directory.contactRows = [{ id: "live", name: "Alice", emails: ["alice@example.test"] }]
      compare(named(view, "contact-message").text, "This contact no longer exists.")
      compare(named(view, "contact-list").visible, true,
        "a detail-only failure keeps the recoverable list usable")
    }
  }
}
