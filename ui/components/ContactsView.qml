import QtQuick
import QtQuick.Controls as QQC
import qs.Commons
import qs.Ui

// The address book as a place of its own: one source at a time, a bounded list
// and one open contact. It reads through Service, which owns the account and
// request generation, so nothing here has to know which provider answered.
Item {
  id: root
  objectName: "contacts-view"

  required property var service
  required property color textColor
  required property color backgroundColor
  required property color accentColor
  required property color dimColor
  required property color popupBackgroundColor
  required property color popupBorderColor
  required property string panelFontFamily

  readonly property var sources: root.service && Array.isArray(root.service.contactSources)
    ? root.service.contactSources : []
  readonly property var rows: root.service && Array.isArray(root.service.contactRows)
    ? root.service.contactRows : []
  readonly property var contact: root.service ? root.service.openContact : null
  readonly property string errorText: root.service ? String(root.service.contactsDirectoryError || "") : ""
  readonly property string detailError: root.service
    ? String(root.service.contactsDirectoryDetailError || "") : ""
  readonly property bool busy: root.service ? root.service.contactsDirectoryBusy === true : false
  readonly property bool sourceSelected: root.service
    ? String(root.service.contactSource || "") !== "" : false
  readonly property bool narrow: width < Style.space(620)
  readonly property bool listVisible: !narrow || !root.contact
  readonly property bool searchFocused: searchField.activeFocus

  property string query: ""
  property string selectedId: ""
  // The view is a place: becoming active is what loads it, so the window does
  // not have to remember to ask in every path that leads here.
  property bool active: false

  onActiveChanged: if (active) refresh(root.service ? root.service.activeAccountId : "")

  Rectangle {
    anchors.fill: parent
    color: root.backgroundColor
    z: -1
  }

  function refresh(accountId) {
    if (root.service) root.service.refreshContactSources(accountId, root.query)
  }

  function focusSearch() { searchField.forceActiveFocus() }

  function activateSelection() {
    if (root.selectedId !== "") root.activate(root.selectedId)
  }

  function runShortcut(id) {
    if (id === "contactNext") { root.moveSelection(1); return true }
    if (id === "contactPrevious") { root.moveSelection(-1); return true }
    if (id === "openContact") { root.activateSelection(); return true }
    if (id === "searchContacts") { root.focusSearch(); return true }
    return false
  }

  function goBack() {
    if (root.contact !== null) { root.closeDetail(); return true }
    if (searchField.text !== "") {
      searchField.text = ""
      searchDebounce.stop()
      root.query = ""
      root.submitQuery()
      return true
    }
    return false
  }

  function submitQuery() {
    if (!root.service) return
    root.selectedId = ""
    root.service.refreshContactList(root.service.contactsDirectoryAccountId, root.query)
  }

  function selectSource(sourceId) {
    if (!root.service) return
    root.selectedId = ""
    root.service.selectContactSource(sourceId, root.query)
  }

  function activate(rowId) {
    if (!root.service) return
    root.selectedId = String(rowId || "")
    root.service.openContactDetail(root.selectedId)
  }

  function closeDetail() {
    if (root.service) root.service.closeContactDetail()
  }

  function moveSelection(step) {
    if (root.rows.length === 0) return
    var index = -1
    for (var i = 0; i < root.rows.length; i++) {
      if (String(root.rows[i].id) === root.selectedId) { index = i; break }
    }
    // Movement from nothing starts at the edge the movement comes from.
    if (index < 0) index = step > 0 ? 0 : root.rows.length - 1
    else index = Math.max(0, Math.min(root.rows.length - 1, index + step))
    root.selectedId = String(root.rows[index].id)
    contactList.positionViewAtIndex(index, ListView.Contain)
  }

  // A keystroke must not become one query per character.
  Timer {
    id: searchDebounce
    interval: 200
    onTriggered: root.submitQuery()
  }

  Row {
    anchors.fill: parent
    anchors.margins: Style.space(10)
    spacing: Style.space(10)

    // Sources, search and the bounded list.
    Column {
      id: listColumn
      visible: root.listVisible
      width: root.narrow ? parent.width : Math.min(Style.space(340), parent.width / 2)
      height: parent.height
      spacing: Style.space(8)

      Row {
        width: parent.width
        spacing: Style.space(6)

        Text {
          text: "Contacts"
          color: root.textColor
          font.family: root.panelFontFamily
          font.pixelSize: Style.font.bodySmall
          font.bold: true
          anchors.verticalCenter: parent.verticalCenter
        }

        Text {
          text: root.service && root.service.contactsDirectoryAccountId !== ""
            ? "(" + root.rows.length + (root.service.contactTotal > root.rows.length
              ? " of " + root.service.contactTotal : "") + ")" : ""
          color: root.dimColor
          font.family: root.panelFontFamily
          font.pixelSize: Style.font.caption
          anchors.verticalCenter: parent.verticalCenter
        }
      }

      // One button per readable address book. Service preserves an existing
      // choice, then falls back to the server's readable default.
      Flow {
        id: sourceRow
        objectName: "contact-sources"
        width: parent.width
        spacing: Style.space(4)
        visible: root.sources.length > 1

        Repeater {
          model: root.sources
          delegate: Button {
            required property var modelData
            objectName: "contact-source-" + String(modelData.id)
            text: String(modelData.name || modelData.id || "")
            bordered: true
            fontSize: Style.font.caption
            foreground: String(modelData.id) === String(root.service
              ? root.service.contactSource : "") ? root.accentColor : root.dimColor
            onClicked: root.selectSource(modelData.id)
          }
        }
      }

      TextField {
        id: searchField
        objectName: "contact-search"
        width: parent.width
        foreground: root.textColor
        accent: root.accentColor
        font.family: root.panelFontFamily
        font.pixelSize: Style.font.bodySmall
        placeholderText: "Search contacts..."
        onTextChanged: {
          root.query = text
          searchDebounce.restart()
        }
        onAccepted: {
          searchDebounce.stop()
          root.submitQuery()
        }
      }

      Text {
        objectName: "contact-message"
        visible: root.errorText !== "" || root.detailError !== ""
          || (root.selectedId === "" && root.rows.length === 0)
        width: parent.width
        wrapMode: Text.WordWrap
        textFormat: Text.PlainText
        text: root.errorText !== "" ? root.errorText
          : (root.detailError !== "" ? root.detailError
          : (root.sourceSelected ? "No contacts found." : "No address book on this account."))
        color: root.dimColor
        font.family: root.panelFontFamily
        font.pixelSize: Style.font.caption
      }

      ListView {
        id: contactList
        objectName: "contact-list"
        width: parent.width
        height: parent.height - y
        visible: root.errorText === ""
        clip: true
        spacing: Style.space(1)
        model: root.rows
        QQC.ScrollBar.vertical: QQC.ScrollBar { policy: QQC.ScrollBar.AsNeeded }

        delegate: Rectangle {
          id: contactRow
          required property var modelData
          required property int index

          width: contactList.width
          implicitHeight: Style.space(46)
          radius: Style.cornerRadius
          color: String(contactRow.modelData.id) === root.selectedId
            ? Style.selectedFillFor(root.textColor, root.accentColor)
            : (rowHover.hovered ? Style.hoverFillFor(root.textColor, root.accentColor) : "transparent")

          Rectangle {
            id: avatar
            anchors.left: parent.left
            anchors.leftMargin: Style.space(6)
            anchors.verticalCenter: parent.verticalCenter
            width: Style.space(26)
            height: width
            radius: width / 2
            color: Style.selectedFillFor(root.textColor, root.accentColor)

            Text {
              anchors.centerIn: parent
              textFormat: Text.PlainText
              text: {
                var name = String(contactRow.modelData.name || "")
                if (name === "") name = String((contactRow.modelData.emails || [])[0] || "")
                return name.length > 0 ? name.charAt(0).toUpperCase() : "?"
              }
              color: root.textColor
              font.family: root.panelFontFamily
              font.pixelSize: Style.font.caption
              font.bold: true
            }
          }

          Column {
            anchors.left: avatar.right
            anchors.leftMargin: Style.space(8)
            anchors.right: parent.right
            anchors.rightMargin: Style.space(8)
            anchors.verticalCenter: parent.verticalCenter
            spacing: Style.space(1)

            Text {
              width: parent.width
              textFormat: Text.PlainText
              text: String(contactRow.modelData.name || "")
              visible: text !== ""
              color: root.textColor
              font.family: root.panelFontFamily
              font.pixelSize: Style.font.bodySmall
              font.bold: true
              elide: Text.ElideRight
            }

            Text {
              width: parent.width
              textFormat: Text.PlainText
              text: String((contactRow.modelData.emails || [])[0] || "")
              color: root.dimColor
              font.family: root.panelFontFamily
              font.pixelSize: Style.font.caption
              elide: Text.ElideMiddle
            }
          }

          HoverHandler { id: rowHover }
          TapHandler {
            gesturePolicy: TapHandler.ReleaseWithinBounds
            onTapped: root.activate(contactRow.modelData.id)
          }
        }
      }
    }

    // The open contact, or a plain statement that nothing is open.
    Rectangle {
      id: detailPane
      objectName: "contact-detail"
      visible: !root.listVisible || root.contact !== null
      width: root.narrow ? parent.width : parent.width - listColumn.width - parent.spacing
      height: parent.height
      radius: Style.cornerRadius
      color: "transparent"
      border.width: 1
      border.color: root.popupBorderColor

      Flickable {
        id: detailFlick
        anchors.fill: parent
        anchors.margins: Style.space(12)
        contentWidth: width
        contentHeight: detailColumn.implicitHeight
        clip: true
        visible: root.contact !== null

        Column {
          id: detailColumn
          width: detailFlick.width
          spacing: Style.space(8)

          Row {
            width: parent.width
            spacing: Style.space(8)

            Text {
              width: parent.width
              wrapMode: Text.WordWrap
              textFormat: Text.PlainText
              text: root.contact ? String(root.contact.name || "") : ""
              color: root.textColor
              font.family: root.panelFontFamily
              font.pixelSize: Style.font.title
              font.bold: true
            }
          }

          Repeater {
            model: root.contact ? [
              { title: "Email", rows: root.contact.emails || [] },
              { title: "Phone", rows: root.contact.phones || [] },
              { title: "Address", rows: root.contact.addresses || [] }
            ] : []

            delegate: Column {
              required property var modelData
              width: detailColumn.width
              spacing: Style.space(2)
              visible: modelData.rows.length > 0

              Text {
                text: String(modelData.title)
                color: root.dimColor
                font.family: root.panelFontFamily
                font.pixelSize: Style.font.caption
                font.bold: true
              }

              Repeater {
                model: modelData.rows

                delegate: Row {
                  required property var modelData
                  width: detailColumn.width
                  spacing: Style.space(6)

                  Text {
                    textFormat: Text.PlainText
                    text: String(modelData.value || "")
                    color: root.textColor
                    font.family: root.panelFontFamily
                    font.pixelSize: Style.font.bodySmall
                  }

                  Text {
                    visible: String(modelData.label || "") !== ""
                    textFormat: Text.PlainText
                    text: String(modelData.label || "")
                    color: root.dimColor
                    font.family: root.panelFontFamily
                    font.pixelSize: Style.font.caption
                  }
                }
              }
            }
          }
        }
      }

      Text {
        anchors.centerIn: parent
        visible: root.contact === null && !root.listVisible
        text: "No contact open."
        color: root.dimColor
        font.family: root.panelFontFamily
        font.pixelSize: Style.font.caption
      }

      Button {
        objectName: "contact-back"
        anchors.right: parent.right
        anchors.top: parent.top
        anchors.margins: Style.space(8)
        visible: root.contact !== null
        text: "Back"
        bordered: false
        fontSize: Style.font.caption
        foreground: root.dimColor
        onClicked: root.closeDetail()
      }
    }
  }
}
