import QtQuick
import qs.Commons
import qs.Ui

// One refresh control for whichever view is on screen. The mail list, the
// calendar and the address book each answer a refresh differently and report
// their own progress, so the state that decides the icon, the tooltip and the
// click belongs here rather than in three branches at the call site.
IconButton {
  id: root

  required property var service
  required property bool calendarVisible
  required property bool contactsVisible
  required property bool ready
  required property color foreground
  required property color highlightColor
  required property string panelFontFamily

  signal refreshRequested()

  readonly property bool calendarBusy: !!root.service && !!root.service.calendarController
    && root.service.calendarController.loading === true
  readonly property bool contactsBusy: !!root.service
    && root.service.contactsDirectoryBusy === true
  readonly property bool mailBusy: !!root.service && root.service.listLoading === true
  readonly property bool viewBusy: root.calendarVisible ? root.calendarBusy
    : (root.contactsVisible ? root.contactsBusy : root.mailBusy)

  objectName: "refresh-button"
  iconName: "refresh"
  tooltipText: {
    if (root.calendarVisible) return root.calendarBusy ? "Loading calendars" : "Refresh calendars · F5 / Ctrl+R"
    if (root.contactsVisible) return root.contactsBusy ? "Loading contacts" : "Refresh contacts · F5 / Ctrl+R"
    return root.mailBusy ? "Checking for mail" : "Check mail · F5 / Ctrl+R"
  }
  foreground: root.foreground
  hoverColor: root.highlightColor
  fontFamily: root.panelFontFamily
  busy: root.viewBusy
  enabled: root.ready && !root.viewBusy
  onClicked: root.refreshRequested()
}
