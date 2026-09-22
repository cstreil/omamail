import QtQuick
import QtTest
import qs.Commons
import "../../components" as Omamail

Item {
  width: 900
  height: 700

  QtObject {
    id: calendarController
    property var service: null
    property bool sourcesLoaded: true
    property var events: []
    property bool loading: false
    property double pendingRangeStart: 0
    property bool clockRunning: false
    property var availableSources: ({version:1, sources:[]})
    property string lastError: ""
    function colorKeyFor(_sourceId) { return "accent" }
    function refresh(_start, _end) {
      events = []
      loading = true
    }
  }

  Omamail.CalendarView {
    id: view
    anchors.fill: parent
    controller: calendarController
    textColor: Color.foreground
    backgroundColor: Color.background
    accentColor: Color.accent
    urgentColor: Color.accent
    dimColor: Color.foreground
    calendarBorderColor: Color.foreground
    calendarTodayBackgroundColor: Color.background
    calendarBorderWidth: 1
    panelFontFamily: "monospace"
  }

  TestCase {
    name: "CalendarViewSelection"
    when: windowShown

    function init() {
      calendarController.events = []
      calendarController.loading = false
      calendarController.pendingRangeStart = 0
      view.selectedEventId = ""
      view.pendingEventId = ""
      view.detailEvent = null
      wait(1)
    }

    function test_bar_target_survives_cold_cache_until_network_result() {
      view.showEvent("id:opaque-event", new Date(2026, 8, 23, 10).getTime())
      compare(calendarController.loading, true)
      compare(view.selectedEventId, "id:opaque-event")
      compare(view.pendingEventId, "id:opaque-event")

      calendarController.events = [{
        id:"opaque-event", sourceId:"account-source", uid:"same-uid",
        summary:"Target", start:{ms:new Date(2026, 8, 23, 10).getTime()},
        end:{ms:new Date(2026, 8, 23, 11).getTime()}
      }]
      calendarController.loading = false
      tryCompare(view, "pendingEventId", "")
      compare(view.selectedEventId, "id:opaque-event")
    }

    function test_missing_bar_target_clears_after_completed_refresh() {
      view.showEvent("id:missing", new Date(2026, 8, 23, 10).getTime())
      calendarController.events = []
      calendarController.loading = false
      tryCompare(view, "pendingEventId", "")
      compare(view.selectedEventId, "")
    }
  }
}
