import QtQuick
import QtQuick.Controls as QQC
import qs.Commons
import qs.Ui

// A desktop address book: lists on the left, people in the middle and one
// contact card on the right. Service owns provider and request state; this view
// only gives that state the calm, sectioned shape of a native contacts app.
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
  readonly property string errorText: root.service
    ? String(root.service.contactsDirectoryError || "") : ""
  readonly property string detailError: root.service
    ? String(root.service.contactsDirectoryDetailError || "") : ""
  readonly property bool busy: root.service ? root.service.contactsDirectoryBusy === true : false
  readonly property bool sourceSelected: root.service
    ? String(root.service.contactSource || "") !== "" : false
  readonly property bool narrow: width < Style.space(720)
  readonly property bool listVisible: !narrow || root.contact === null
  readonly property bool searchFocused: searchField.activeFocus
  readonly property var sectionRows: root.rowsForSections(root.rows)
  readonly property var detailGroups: root.contact ? [
    { key: "email", title: "Email", fallback: "email", rows: root.contact.emails || [] },
    { key: "phone", title: "Phone", fallback: "phone", rows: root.contact.phones || [] },
    { key: "address", title: "Address", fallback: "address", rows: root.contact.addresses || [] }
  ] : []

  property string query: ""
  property string selectedId: ""
  property bool active: false

  onActiveChanged: if (active) refresh(root.service ? root.service.activeAccountId : "")

  Rectangle {
    anchors.fill: parent
    color: root.backgroundColor
    z: -1
  }

  function rowName(row) {
    var name = String(row && row.name || "").trim()
    if (name !== "") return name
    var emails = row && Array.isArray(row.emails) ? row.emails : []
    return emails.length > 0 ? String(emails[0]) : "Unnamed Contact"
  }

  function sectionName(row) {
    var name = rowName(row)
    if (name === "") return "#"
    var initial = name.charAt(0).toUpperCase()
    return initial >= "A" && initial <= "Z" ? initial : "#"
  }

  function rowsForSections(values) {
    var result = []
    for (var i = 0; i < values.length; i++) {
      var row = values[i] || ({})
      result.push({
        id: String(row.id || ""),
        name: String(row.name || ""),
        displayName: rowName(row),
        emails: Array.isArray(row.emails) ? row.emails : [],
        source: String(row.source || ""),
        sectionName: sectionName(row)
      })
    }
    return result
  }

  function selectedSourceName() {
    var selected = root.service ? String(root.service.contactSource || "") : ""
    for (var i = 0; i < root.sources.length; i++) {
      if (String(root.sources[i].id || "") === selected)
        return String(root.sources[i].name || root.sources[i].id || "")
    }
    return "Contacts"
  }

  function initials(value) {
    var words = String(value || "").trim().split(/\s+/)
    if (words.length === 0 || words[0] === "") return "?"
    var first = words[0].charAt(0)
    var last = words.length > 1 ? words[words.length - 1].charAt(0) : ""
    return (first + last).toUpperCase()
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
    if (index < 0) index = step > 0 ? 0 : root.rows.length - 1
    else index = Math.max(0, Math.min(root.rows.length - 1, index + step))
    root.selectedId = String(root.rows[index].id)
    contactList.positionViewAtIndex(index, ListView.Contain)
  }

  Timer {
    id: searchDebounce
    interval: 200
    onTriggered: root.submitQuery()
  }

  Row {
    id: panes
    anchors.fill: parent
    spacing: 0

    Rectangle {
      id: sourcePane
      objectName: "contact-source-sidebar"
      visible: !root.narrow
      width: visible ? Math.min(Style.space(190), Math.max(Style.space(156), panes.width * 0.2)) : 0
      height: parent.height
      color: root.popupBackgroundColor

      Column {
        anchors.fill: parent
        anchors.margins: Style.space(12)
        spacing: Style.space(8)

        Text {
          text: "Lists"
          textFormat: Text.PlainText
          color: root.textColor
          font.family: root.panelFontFamily
          font.pixelSize: Style.font.title
          font.bold: true
        }

        Text {
          text: "ADDRESS BOOKS"
          textFormat: Text.PlainText
          color: root.dimColor
          font.family: root.panelFontFamily
          font.pixelSize: Style.font.caption
          font.bold: true
        }

        Column {
          width: parent.width
          spacing: Style.space(2)

          Repeater {
            model: root.sources

            delegate: Rectangle {
              id: sourceEntry
              required property var modelData
              readonly property bool selected: String(modelData.id || "") === String(
                root.service ? root.service.contactSource : "")
              objectName: "contact-source-" + String(modelData.id || "")
              width: parent.width
              implicitHeight: Style.space(32)
              radius: Style.cornerRadius
              color: selected ? Style.selectedFillFor(root.textColor, root.accentColor)
                : (sourceHover.hovered ? Style.hoverFillFor(root.textColor, root.accentColor) : "transparent")

              Rectangle {
                anchors.left: parent.left
                anchors.leftMargin: Style.space(8)
                anchors.verticalCenter: parent.verticalCenter
                width: Style.space(7)
                height: width
                radius: width / 2
                color: sourceEntry.selected ? root.accentColor : root.dimColor
                opacity: sourceEntry.selected ? 1 : 0.65
              }

              Text {
                anchors.left: parent.left
                anchors.leftMargin: Style.space(22)
                anchors.right: sourceCount.left
                anchors.rightMargin: Style.space(4)
                anchors.verticalCenter: parent.verticalCenter
                textFormat: Text.PlainText
                text: String(sourceEntry.modelData.name || sourceEntry.modelData.id || "")
                color: sourceEntry.selected ? root.textColor : root.dimColor
                font.family: root.panelFontFamily
                font.pixelSize: Style.font.bodySmall
                font.bold: sourceEntry.selected
                elide: Text.ElideRight
              }

              Text {
                id: sourceCount
                anchors.right: parent.right
                anchors.rightMargin: Style.space(8)
                anchors.verticalCenter: parent.verticalCenter
                visible: sourceEntry.selected && root.service
                text: visible ? String(root.service.contactTotal || root.rows.length) : ""
                color: root.dimColor
                font.family: root.panelFontFamily
                font.pixelSize: Style.font.caption
              }

              HoverHandler { id: sourceHover }
              TapHandler { onTapped: root.selectSource(sourceEntry.modelData.id) }
            }
          }
        }
      }
    }

    Rectangle {
      width: sourcePane.visible ? 1 : 0
      height: parent.height
      visible: sourcePane.visible
      color: root.popupBorderColor
      opacity: 0.65
    }

    Rectangle {
      id: listPane
      objectName: "contact-list-pane"
      visible: root.listVisible
      width: visible ? (root.narrow ? panes.width
        : Math.min(Style.space(330), Math.max(Style.space(270), panes.width * 0.34))) : 0
      height: parent.height
      color: root.backgroundColor

      Column {
        id: listContent
        anchors.fill: parent
        anchors.margins: Style.space(12)
        spacing: Style.space(8)

        Row {
          width: parent.width
          spacing: Style.space(6)

          Text {
            width: parent.width - listCount.width - parent.spacing
            textFormat: Text.PlainText
            text: root.selectedSourceName()
            color: root.textColor
            font.family: root.panelFontFamily
            font.pixelSize: Style.font.title
            font.bold: true
            elide: Text.ElideRight
          }

          Text {
            id: listCount
            anchors.verticalCenter: parent.verticalCenter
            textFormat: Text.PlainText
            text: root.service && root.service.contactsDirectoryAccountId !== ""
              ? String(root.rows.length) + (root.service.contactTotal > root.rows.length
              ? " of " + root.service.contactTotal : "") : ""
            color: root.dimColor
            font.family: root.panelFontFamily
            font.pixelSize: Style.font.caption
          }
        }

        Flow {
          id: compactSources
          objectName: "contact-compact-sources"
          visible: root.narrow && root.sources.length > 1
          width: parent.width
          height: visible ? childrenRect.height : 0
          spacing: Style.space(4)

          Repeater {
            model: root.sources
            delegate: Button {
              required property var modelData
              objectName: "contact-compact-source-" + String(modelData.id || "")
              text: String(modelData.name || modelData.id || "")
              bordered: true
              fontSize: Style.font.caption
              foreground: String(modelData.id || "") === String(root.service
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
          placeholderText: "Search"
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
          color: root.detailError !== "" ? root.accentColor : root.dimColor
          font.family: root.panelFontFamily
          font.pixelSize: Style.font.caption
        }

        ListView {
          id: contactList
          objectName: "contact-list"
          width: parent.width
          height: Math.max(0, parent.height - y)
          visible: root.errorText === ""
          clip: true
          spacing: Style.space(1)
          model: root.sectionRows
          section.property: "sectionName"
          section.criteria: ViewSection.FullString
          QQC.ScrollBar.vertical: QQC.ScrollBar { policy: QQC.ScrollBar.AsNeeded }

          section.delegate: Rectangle {
            required property string section
            objectName: "contact-section-" + section
            width: contactList.width
            implicitHeight: Style.space(24)
            color: root.backgroundColor

            Text {
              anchors.left: parent.left
              anchors.leftMargin: Style.space(8)
              anchors.bottom: parent.bottom
              anchors.bottomMargin: Style.space(3)
              textFormat: Text.PlainText
              text: section
              color: root.accentColor
              font.family: root.panelFontFamily
              font.pixelSize: Style.font.caption
              font.bold: true
            }
          }

          delegate: Rectangle {
            id: contactRow
            required property var modelData
            required property int index
            width: contactList.width
            implicitHeight: Style.space(48)
            radius: Style.cornerRadius
            color: String(contactRow.modelData.id) === root.selectedId
              ? Style.selectedFillFor(root.textColor, root.accentColor)
              : (rowHover.hovered ? Style.hoverFillFor(root.textColor, root.accentColor) : "transparent")

            Rectangle {
              id: avatar
              anchors.left: parent.left
              anchors.leftMargin: Style.space(7)
              anchors.verticalCenter: parent.verticalCenter
              width: Style.space(28)
              height: width
              radius: width / 2
              color: Style.selectedFillFor(root.textColor, root.accentColor)

              Text {
                anchors.centerIn: parent
                textFormat: Text.PlainText
                text: root.initials(contactRow.modelData.displayName)
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
                text: String(contactRow.modelData.displayName || "")
                color: root.textColor
                font.family: root.panelFontFamily
                font.pixelSize: Style.font.bodySmall
                font.bold: String(contactRow.modelData.id) === root.selectedId
                elide: Text.ElideRight
              }

              Text {
                width: parent.width
                visible: (contactRow.modelData.emails || []).length > 0
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
    }

    Rectangle {
      width: listPane.visible && !root.narrow ? 1 : 0
      height: parent.height
      visible: width > 0
      color: root.popupBorderColor
      opacity: 0.65
    }

    Rectangle {
      id: detailPane
      objectName: "contact-detail"
      visible: !root.narrow || root.contact !== null
      width: visible ? Math.max(0, panes.width - sourcePane.width - listPane.width
        - (sourcePane.visible ? 1 : 0) - (listPane.visible && !root.narrow ? 1 : 0)) : 0
      height: parent.height
      color: root.backgroundColor

      Flickable {
        id: detailFlick
        anchors.fill: parent
        anchors.margins: Style.space(22)
        contentWidth: width
        contentHeight: detailColumn.implicitHeight
        clip: true
        visible: root.contact !== null

        Column {
          id: detailColumn
          width: detailFlick.width
          spacing: Style.space(14)

          Rectangle {
            anchors.horizontalCenter: parent.horizontalCenter
            width: Style.space(72)
            height: width
            radius: width / 2
            color: Style.selectedFillFor(root.textColor, root.accentColor)

            Text {
              anchors.centerIn: parent
              textFormat: Text.PlainText
              text: root.initials(root.contact ? root.contact.name : "")
              color: root.textColor
              font.family: root.panelFontFamily
              font.pixelSize: Style.font.title
              font.bold: true
            }
          }

          Text {
            width: parent.width
            horizontalAlignment: Text.AlignHCenter
            wrapMode: Text.WordWrap
            textFormat: Text.PlainText
            text: root.contact ? root.rowName(root.contact) : ""
            color: root.textColor
            font.family: root.panelFontFamily
            font.pixelSize: Style.font.title
            font.bold: true
          }

          Text {
            width: parent.width
            horizontalAlignment: Text.AlignHCenter
            textFormat: Text.PlainText
            text: root.selectedSourceName()
            color: root.dimColor
            font.family: root.panelFontFamily
            font.pixelSize: Style.font.caption
          }

          Repeater {
            model: root.detailGroups

            delegate: Column {
              id: detailSection
              required property var modelData
              objectName: "contact-detail-section-" + String(modelData.key || "")
              width: detailColumn.width
              spacing: Style.space(7)
              visible: modelData.rows.length > 0

              Rectangle {
                width: parent.width
                height: 1
                color: root.popupBorderColor
                opacity: 0.65
              }

              Text {
                textFormat: Text.PlainText
                text: String(detailSection.modelData.title || "")
                color: root.dimColor
                font.family: root.panelFontFamily
                font.pixelSize: Style.font.caption
                font.bold: true
              }

              Repeater {
                model: detailSection.modelData.rows

                delegate: Row {
                  id: detailRow
                  required property var modelData
                  width: detailColumn.width
                  spacing: Style.space(10)

                  Text {
                    width: Math.min(Style.space(92), detailRow.width * 0.28)
                    horizontalAlignment: Text.AlignRight
                    textFormat: Text.PlainText
                    text: String(detailRow.modelData.label || detailSection.modelData.fallback || "")
                    color: root.dimColor
                    font.family: root.panelFontFamily
                    font.pixelSize: Style.font.bodySmall
                  }

                  Text {
                    width: detailRow.width - x
                    wrapMode: Text.Wrap
                    textFormat: Text.PlainText
                    text: String(detailRow.modelData.value || "")
                    color: root.textColor
                    font.family: root.panelFontFamily
                    font.pixelSize: Style.font.bodySmall
                  }
                }
              }
            }
          }

          Column {
            width: parent.width
            spacing: Style.space(7)
            visible: root.contact !== null

            Rectangle {
              width: parent.width
              height: 1
              color: root.popupBorderColor
              opacity: 0.65
            }

            Text {
              text: "Address Book"
              textFormat: Text.PlainText
              color: root.dimColor
              font.family: root.panelFontFamily
              font.pixelSize: Style.font.caption
              font.bold: true
            }

            Row {
              width: parent.width
              spacing: Style.space(10)

              Text {
                width: Math.min(Style.space(92), parent.width * 0.28)
                horizontalAlignment: Text.AlignRight
                text: "list"
                textFormat: Text.PlainText
                color: root.dimColor
                font.family: root.panelFontFamily
                font.pixelSize: Style.font.bodySmall
              }

              Text {
                width: parent.width - x
                text: root.selectedSourceName()
                textFormat: Text.PlainText
                color: root.textColor
                font.family: root.panelFontFamily
                font.pixelSize: Style.font.bodySmall
                elide: Text.ElideRight
              }
            }
          }
        }
      }

      Column {
        anchors.centerIn: parent
        width: Math.min(parent.width - Style.space(40), Style.space(300))
        spacing: Style.space(8)
        visible: root.contact === null

        Rectangle {
          anchors.horizontalCenter: parent.horizontalCenter
          width: Style.space(64)
          height: width
          radius: width / 2
          color: Style.selectedFillFor(root.textColor, root.accentColor)

          Text {
            anchors.centerIn: parent
            text: "?"
            textFormat: Text.PlainText
            color: root.dimColor
            font.family: root.panelFontFamily
            font.pixelSize: Style.font.title
          }
        }

        Text {
          width: parent.width
          horizontalAlignment: Text.AlignHCenter
          text: "Select a contact"
          textFormat: Text.PlainText
          color: root.textColor
          font.family: root.panelFontFamily
          font.pixelSize: Style.font.bodySmall
          font.bold: true
        }

        Text {
          width: parent.width
          horizontalAlignment: Text.AlignHCenter
          wrapMode: Text.WordWrap
          text: "Contact details appear here."
          textFormat: Text.PlainText
          color: root.dimColor
          font.family: root.panelFontFamily
          font.pixelSize: Style.font.caption
        }
      }

      Button {
        objectName: "contact-back"
        anchors.left: parent.left
        anchors.top: parent.top
        anchors.margins: Style.space(8)
        visible: root.narrow && root.contact !== null
        text: "Back"
        bordered: false
        fontSize: Style.font.caption
        foreground: root.dimColor
        onClicked: root.closeDetail()
      }
    }
  }
}
