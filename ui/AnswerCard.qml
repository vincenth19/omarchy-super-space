import QtQuick
import qs.Commons
import qs.Ui

// An instant answer (an `Answer` from crates/core), two-sided like Raycast:
// what you typed on the left, the result on the right, one detail line under.
BorderSurface {
  id: root

  property var answer: ({})
  property bool selected: false
  signal clicked()

  readonly property color ink: selected ? Color.menu.selectedText : Color.menu.text
  readonly property int side: (width - Style.space(40)) / 2

  height: body.implicitHeight + Style.spacing.huge * 2
  radius: Style.cornerRadius
  color: selected ? Color.menu.selectedBackground : Style.normalFillFor(Color.menu.text, Color.accent)
  borderSpec: selected ? Border.surfaceSpec("menu", "selected-border", Color.menu.selectedBorder, 0) : Border.none()

  component Line: Text {
    width: root.side
    horizontalAlignment: Text.AlignHCenter
    textFormat: Text.PlainText
    elide: Text.ElideRight
    font.family: Style.font.menuFamily
  }

  Column {
    id: body
    y: Style.spacing.huge
    width: root.width
    spacing: Style.spacing.xs

    Row {
      Line { text: root.answer.input || ""; color: root.ink; font.pixelSize: Style.font.display; font.weight: Font.Medium }
      Line { width: Style.space(40); text: "→"; color: Color.menu.text; opacity: 0.52; font.pixelSize: Style.font.display }
      Line { text: root.answer.output || ""; color: root.ink; font.pixelSize: Style.font.display; font.weight: Font.Medium }
    }
    Row {
      Line { text: root.answer.input_label || ""; color: Color.menu.text; opacity: 0.52; font.pixelSize: Style.font.caption }
      Item { width: Style.space(40); height: 1 }
      Line { text: root.answer.output_label || ""; color: Color.menu.text; opacity: 0.52; font.pixelSize: Style.font.caption }
    }
    Line {
      width: root.width
      visible: text.length > 0
      topPadding: Style.spacing.sm
      text: root.answer.detail || ""
      color: Color.menu.text
      opacity: 0.52
      font.pixelSize: Style.font.caption
    }
  }

  MouseArea {
    anchors.fill: parent
    cursorShape: Qt.PointingHandCursor
    onClicked: root.clicked()
  }
}
