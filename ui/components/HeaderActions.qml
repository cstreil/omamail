import QtQuick
import qs.Commons
import qs.Ui

// What you do to the mailbox as a whole, in one place: refresh whatever is on
// screen, create an event while the calendar is up, write a message while it
// is not. App.qml composes the window and keeps the layout; the buttons and
// the state they read live here.
Row {
  id: root

  required property var service
  required property bool calendarVisible
  required property bool contactsVisible
  required property bool showPage
  required property bool composing
  required property bool ready
  required property color foreground
  required property color dimColor
  required property color accentColor
  required property string panelFontFamily

  // The AI button beside this row sizes itself from the compose button, so the
  // header keeps one control height without reaching into a sibling.
  readonly property real buttonHeight: composeButton.implicitHeight

  signal refreshRequested()
  signal createEventRequested()
  signal composeRequested()

  spacing: Style.space(8)

  RefreshButton {
    anchors.verticalCenter: parent.verticalCenter
    visible: !root.showPage && !root.composing
    service: root.service
    calendarVisible: root.calendarVisible
    contactsVisible: root.contactsVisible
    ready: root.ready
    foreground: root.dimColor
    highlightColor: root.foreground
    panelFontFamily: root.panelFontFamily
    onRefreshRequested: root.refreshRequested()
  }

  Button {
    objectName: "create-event-button"
    anchors.verticalCenter: parent.verticalCenter
    visible: !root.showPage && !root.composing && root.calendarVisible
    text: "Create event"
    tooltipText: "Create event"
    foreground: root.dimColor
    bordered: true
    accent: root.accentColor
    fontFamily: root.panelFontFamily
    fontSize: Style.font.caption
    enabled: root.ready && !!root.service && !!root.service.calendarController
      && root.service.calendarController.writableSourceGroups.length > 0
    onClicked: root.createEventRequested()
  }

  Button {
    id: composeButton
    objectName: "compose-button"
    anchors.verticalCenter: parent.verticalCenter
    visible: !root.showPage && !root.composing && !root.calendarVisible
      && !root.contactsVisible
    text: "Compose"
    tooltipText: "Compose · c"
    foreground: root.dimColor
    bordered: true
    accent: root.accentColor
    fontFamily: root.panelFontFamily
    fontSize: Style.font.caption
    enabled: root.ready
    onClicked: root.composeRequested()
  }
}
