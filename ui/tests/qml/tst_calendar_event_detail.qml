import QtQuick
import QtTest
import qs.Commons
import "../../components" as Omamail

Item {
  width: 600
  height: 520

  QtObject {
    id: calendarController
    property var service: null
    property var availableSources: ({version:1, sources:[]})
  }

  Omamail.CalendarEventDetail {
    id: detail
    width: parent.width
    height: parent.height
    controller: calendarController
    event: ({sourceId:"caldav:old", href:"event.ics",
      uid:"synthetic", summary:"Synthetic appointment",
      start:{ms:1000}, end:{ms:2000}})
    textColor: Color.foreground
    backgroundColor: Color.background
    accentColor: Color.accent
    urgentColor: Color.accent
    dimColor: Color.foreground
    panelFontFamily: "monospace"
  }

  TestCase {
    name: "CalendarEventDetailSourceRights"
    when: windowShown

    function init() {
      calendarController.availableSources = ({version:1, sources:[{
        id:"caldav:old", kind:"caldav", enabled:true, readOnly:false,
        url:"https://calendar.example/dav/"
      }]})
    }

    function test_disabling_old_transport_removes_edit_and_delete_affordance() {
      compare(detail.canWrite, true)
      calendarController.availableSources = ({version:1, sources:[{
        id:"caldav:old", kind:"caldav", enabled:false, readOnly:false,
        url:"https://calendar.example/dav/"
      }]})
      compare(detail.canWrite, false)
    }
  }
}
