import QtQuick
import qs.Commons
import qs.Ui

// One result (a `Row` from crates/core): icon, title, subtitle, the snippet
// with its marked words, and the row's hits: page chips, or for video and
// audio the moments in time order, as thumbnails when there are any. Every
// colour, font, size and radius comes from Omarchy's theme (Color, Style).
Column {
  id: root

  property var item: ({})
  property string iconSource: ""
  property bool selected: false
  property int hit: 0            // the hit ←/→ picked: an index into item.hits
  property bool expanded: false  // Tab: every hit on a line of its own
  property bool notice: false    // nothing local matched: say so above this (web) row
  signal clicked()

  readonly property var hits: item.hits || []
  readonly property var order: item._order || []
  readonly property bool thumbs: hits.some(function(h) { return !!h.thumb })
  readonly property bool emoji: item.kind === "emoji"
  readonly property int iconSize: item.kind === "image" ? Style.space(56) : emoji ? Style.space(36) : Style.space(28)
  readonly property color ink: selected ? Color.menu.selectedText : Color.menu.text
  // Secondary text is the theme's text colour dimmed, as in Omarchy's menu.
  readonly property real dim: 0.52

  spacing: Style.spacing.md

  // EPUB parts carry their chapter as the page.
  readonly property bool epub: /\.epub$/i.test(item.key || "")

  // A hit's place: m:ss (h:mm:ss from an hour on), page or chapter, slide, sheet row, line.
  function where(l) {
    if (l.t !== undefined) {
      var s = Math.floor(l.t), two = function(n) { return ("0" + n).slice(-2) }
      return (s < 3600 ? Math.floor(s / 60) : Math.floor(s / 3600) + ":" + two(Math.floor(s / 60) % 60)) + ":" + two(s % 60)
    }
    if (l.page) return (epub ? "ch. " : "p. ") + l.page
    if (l.slide) return "slide " + l.slide
    if (l.sheet) return l.sheet + (l.row ? " · row " + l.row : "")
    if (l.line) return "line " + l.line
    return ""
  }

  // Marks are character ranges; Array.from counts characters, not UTF-16 units.
  function marked(s) {
    var chars = Array.from(String(s.text || "").replace(/\s/g, " ")), out = "", at = 0
    var esc = function(a, b) { return chars.slice(a, b).join("").replace(/&/g, "&amp;").replace(/</g, "&lt;") }
    ;(s.marks || []).forEach(function(m) {
      if (m[0] < at) return
      out += esc(at, m[0]) + "<b><font color=\"" + Color.accent + "\">" + esc(m[0], m[1]) + "</font></b>"
      at = m[1]
    })
    return out + esc(at)
  }

  readonly property string kindLabel: {
    var dot = String(item.title || "").lastIndexOf(".")
    if (item.kind === "file" && dot > 0) return item.title.slice(dot + 1).toUpperCase()
    return ({ app: "App", setting: "Setting", file: "File", image: "Photo", video: "Video", audio: "Audio",
              clip: "Clipboard", emoji: "Emoji", history: "History", web: "Web" })[item.kind] || ""
  }

  Text {
    visible: root.notice
    leftPadding: Style.spacing.rowPaddingX
    topPadding: Style.spacing.sm
    text: "Nothing on this computer matches"
    color: Color.menu.text
    opacity: 0.7
    font.family: Style.font.menuFamily
    font.pixelSize: Style.font.title
  }

  BorderSurface {
    width: root.width
    height: Math.max(body.implicitHeight, glyph.height) + Style.spacing.lg * 2
    radius: Style.cornerRadius
    color: root.selected ? Color.menu.selectedBackground : "transparent"
    borderSpec: root.selected ? Border.surfaceSpec("menu", "selected-border", Color.menu.selectedBorder, 0) : Border.none()

    Item {
      id: glyph
      x: Style.spacing.rowPaddingX
      y: Style.spacing.lg
      width: Math.max(root.iconSize, Style.space(36))
      height: root.iconSize

      Image {
        visible: !root.emoji
        anchors.centerIn: parent
        width: root.iconSize
        height: root.iconSize
        source: root.emoji ? "" : root.iconSource
        sourceSize: Qt.size(width * Screen.devicePixelRatio, height * Screen.devicePixelRatio)
        fillMode: root.item.kind === "image" ? Image.PreserveAspectCrop : Image.PreserveAspectFit
        asynchronous: true
      }
      Text {
        visible: root.emoji
        anchors.centerIn: parent
        text: String(root.item.key || "").slice(6) // "emoji:<emoji>"
        font.pixelSize: Style.font.displayLarge
      }
    }

    Column {
      id: body
      anchors.left: glyph.right
      anchors.leftMargin: Style.spacing.xxl
      anchors.right: kind.left
      anchors.rightMargin: Style.spacing.lg
      y: Style.spacing.lg
      spacing: Style.spacing.xxs

      Text {
        width: parent.width
        text: root.item.title || ""
        textFormat: Text.PlainText
        elide: Text.ElideRight
        color: root.ink
        font.family: Style.font.menuFamily
        font.pixelSize: Style.font.heading
        font.weight: Font.Medium
      }
      Text {
        width: parent.width
        visible: text.length > 0
        text: root.item.subtitle || ""
        textFormat: Text.PlainText
        elide: Text.ElideMiddle
        color: Color.menu.text
        opacity: root.dim
        font.family: Style.font.menuFamily
        font.pixelSize: Style.font.bodySmall
      }
      Text {
        // The marked snippet, or the text of the hit picked with ←/→.
        readonly property var picked: root.hit > 0 ? root.hits[root.hit] : null
        width: parent.width
        visible: text.length > 0 && !root.expanded
        text: picked ? root.marked({ text: picked.text }) : root.item.snippet ? root.marked(root.item.snippet) : ""
        textFormat: Text.StyledText
        wrapMode: Text.Wrap
        maximumLineCount: 2
        elide: Text.ElideRight
        color: Color.menu.text
        opacity: 0.8
        font.family: Style.font.menuFamily
        font.pixelSize: Style.font.body
      }

      // Moments in time order on a timeline; the picked one is outlined.
      Item {
        visible: root.thumbs
        width: strip.width
        height: strip.height

        Rectangle {
          y: strip.topPadding + Style.space(54) + Style.spacing.xxs
          width: parent.width
          height: Style.spacing.hairline
          color: Color.menu.text
          opacity: 0.2
        }

        Row {
          id: strip
          topPadding: Style.spacing.sm
          spacing: Style.spacing.md
          Repeater {
            model: root.thumbs ? root.order : []
            Column {
              id: moment
              required property int modelData
              readonly property var h: root.hits[modelData]
              readonly property bool picked: root.selected && modelData === root.hit
              spacing: Style.spacing.xxs * 2
              BorderSurface {
                id: frame
                width: Style.space(96)
                height: Style.space(54)
                radius: Math.min(Style.cornerRadius, Style.space(6))
                color: Style.normalFillFor(Color.menu.text, Color.accent)
                borderSpec: moment.picked ? Border.flat(Color.accent, Style.space(2)) : Border.none()
                Image {
                  anchors.fill: parent
                  anchors.margins: frame.borderLeft
                  source: moment.h.thumb ? Util.fileUrl(moment.h.thumb) : ""
                  sourceSize: Qt.size(width * Screen.devicePixelRatio, height * Screen.devicePixelRatio)
                  fillMode: Image.PreserveAspectCrop
                  asynchronous: true
                }
              }
              Text {
                text: root.where(moment.h.locator || {})
                color: moment.picked ? Color.accent : Color.menu.text
                opacity: moment.picked ? 1 : root.dim
                font.family: Style.font.menuFamily
                font.pixelSize: Style.font.caption
              }
            }
          }
        }
      }

      // Pages, slides, lines, or moments without thumbnails, as chips.
      Flow {
        width: parent.width
        visible: root.hits.length > 1 && !root.thumbs && !root.expanded
        spacing: Style.spacing.lg
        Repeater {
          model: root.thumbs || root.expanded ? [] : root.order
          Text {
            required property int modelData
            readonly property bool picked: root.selected && modelData === root.hit
            text: root.where(root.hits[modelData].locator || {})
            color: picked ? Color.accent : Color.menu.text
            opacity: picked ? 1 : root.dim
            font.family: Style.font.menuFamily
            font.pixelSize: Style.font.caption
            font.weight: picked ? Font.DemiBold : Font.Normal
          }
        }
      }

      // Tab: every hit with its text.
      Repeater {
        model: root.expanded ? root.order : []
        Text {
          required property int modelData
          readonly property bool picked: modelData === root.hit
          width: body.width
          text: root.where(root.hits[modelData].locator || {}) + "   " + String(root.hits[modelData].text || "").replace(/\s+/g, " ")
          textFormat: Text.PlainText
          elide: Text.ElideRight
          color: picked ? Color.accent : Color.menu.text
          opacity: picked ? 1 : 0.8
          font.family: Style.font.menuFamily
          font.pixelSize: Style.font.body
        }
      }
    }

    Text {
      id: kind
      anchors.right: parent.right
      anchors.rightMargin: Style.spacing.rowPaddingX
      y: Style.spacing.lg
      text: root.kindLabel
      color: root.ink
      opacity: root.dim
      font.family: Style.font.menuFamily
      font.pixelSize: Style.font.caption
    }

    MouseArea {
      anchors.fill: parent
      cursorShape: Qt.PointingHandCursor
      onClicked: root.clicked()
    }
  }
}
