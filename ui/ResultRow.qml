import QtQuick
import qs.Commons
import qs.Commons as Commons  // Qt 6.12's QtQuick has a Color of its own: the theme's is Commons.Color
import qs.Ui

// One result (a `Row` from crates/core): icon, title, subtitle, the snippet
// with its marked words, and the row's hits: page chips, or for video and
// audio a timeline of the whole length with each section on it, and under it
// the picked section's text and, for video, every section's thumbnail (a
// photo is its own icon). An emoji row is a strip: its hits are emoji rows, drawn
// large, the picked one named under them. Indexed text is always drawn as
// plain text, never as markup. Every colour, font, size and radius comes from
// Omarchy's theme.
Column {
  id: root

  property var item: ({})
  property string iconSource: ""
  property bool selected: false
  property int hit: 0            // the hit ←/→ picked: an index into item.hits
  property bool expanded: false  // Tab: every hit on a line of its own
  property bool notice: false    // nothing local matched: say so above this (web) row
  signal clicked(int pick)  // pick: the emoji clicked in a strip, else -1

  readonly property bool emoji: item.kind === "emoji"
  readonly property var hits: emoji ? [] : item.hits || []
  readonly property var order: emoji ? [] : item._order || []
  readonly property bool thumbs: item.kind !== "image" && hits.some(function(h) { return !!h.thumb })
  // Video and audio with a known length and sections get a timeline; songs
  // matched by their whole lyrics have no sections and keep their snippet.
  readonly property bool timed: item.duration > 0 && hits.length > 0
  readonly property int iconSize: item.kind === "image" ? Style.space(56) : Style.space(28)
  readonly property color ink: selected ? Commons.Color.menu.selectedText : Commons.Color.menu.text
  // Secondary text is the theme's text colour dimmed, as in Omarchy's menu.
  readonly property real dim: 0.52

  spacing: Style.spacing.md

  // EPUB parts carry their chapter as the page.
  readonly property bool epub: /\.epub$/i.test(item.key || "")

  // A hit's place: m:ss–m:ss (h:mm:ss from an hour on), page or chapter, slide, sheet row, line.
  function where(l) {
    var clock = function(t) {
      var s = Math.floor(t), two = function(n) { return ("0" + n).slice(-2) }
      return (s < 3600 ? Math.floor(s / 60) : Math.floor(s / 3600) + ":" + two(Math.floor(s / 60) % 60)) + ":" + two(s % 60)
    }
    if (l.t !== undefined) return clock(l.t) + (l.t_end >= l.t + 1 ? "–" + clock(l.t_end) : "")
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
      out += esc(at, m[0]) + "<b><font color=\"" + Commons.Color.accent + "\">" + esc(m[0], m[1]) + "</font></b>"
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
    color: Commons.Color.menu.text
    opacity: 0.7
    font.family: Style.font.menuFamily
    font.pixelSize: Style.font.title
  }

  BorderSurface {
    width: root.width
    height: Math.max(body.implicitHeight, glyph.height) + Style.spacing.lg * 2
    radius: Style.cornerRadius
    color: root.selected ? Commons.Color.menu.selectedBackground : "transparent"
    borderSpec: root.selected ? Border.surfaceSpec("menu", "selected-border", Commons.Color.menu.selectedBorder, 0) : Border.none()

    MouseArea {  // first, so an emoji's own area is above it
      anchors.fill: parent
      cursorShape: Qt.PointingHandCursor
      onClicked: root.clicked(-1)
    }

    Item {
      id: glyph
      x: Style.spacing.rowPaddingX
      y: Style.spacing.lg
      width: root.emoji ? 0 : Math.max(root.iconSize, Style.space(36))
      height: root.iconSize

      Image {
        anchors.centerIn: parent
        width: root.iconSize
        height: root.iconSize
        source: root.emoji ? "" : root.iconSource
        sourceSize: Qt.size(width * Screen.devicePixelRatio, height * Screen.devicePixelRatio)
        fillMode: root.item.kind === "image" ? Image.PreserveAspectCrop : Image.PreserveAspectFit
        asynchronous: true
      }
    }

    Column {
      id: body
      anchors.left: glyph.right
      anchors.leftMargin: root.emoji ? 0 : Style.spacing.xxl
      anchors.right: kind.left
      anchors.rightMargin: Style.spacing.lg
      y: Style.spacing.lg
      spacing: Style.spacing.xxs

      Row {
        visible: root.emoji
        spacing: Style.spacing.xs
        Repeater {
          model: root.emoji ? root.item.hits : []
          BorderSurface {
            required property var modelData
            required property int index
            readonly property bool picked: root.selected && index === root.hit
            width: Style.space(44)
            height: width
            radius: Math.min(Style.cornerRadius, Style.space(6))
            color: picked ? Style.normalFillFor(Commons.Color.menu.text, Commons.Color.accent) : "transparent"
            borderSpec: picked ? Border.flat(Commons.Color.accent, Style.space(2)) : Border.none()
            Text {
              anchors.centerIn: parent
              text: String(modelData.key).slice(6) // "emoji:<emoji>"
              textFormat: Text.PlainText
              font.pixelSize: Style.font.displayLarge
            }
            MouseArea { anchors.fill: parent; onClicked: root.clicked(index) }
          }
        }
      }
      Text {
        width: parent.width
        visible: !root.emoji
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
        text: root.emoji ? (root.item.hits[root.hit] || root.item.hits[0]).title : root.item.subtitle || ""
        textFormat: Text.PlainText
        elide: Text.ElideMiddle
        color: Commons.Color.menu.text
        opacity: root.dim
        font.family: Style.font.menuFamily
        font.pixelSize: Style.font.bodySmall
      }
      // Media: the whole length, each section a segment, the picked one emphasised.
      Item {
        visible: root.timed
        width: parent.width
        height: Math.max(Style.space(10), count.implicitHeight)

        Rectangle {
          id: track
          anchors.verticalCenter: parent.verticalCenter
          width: parent.width - (count.visible ? count.width + Style.spacing.lg : 0)
          height: Style.space(4)
          radius: Math.min(Style.cornerRadius, height / 2)
          color: Commons.Color.menu.text
          opacity: 0.2
        }
        Repeater {
          model: root.timed ? root.hits : []
          Rectangle {
            required property var modelData
            required property int index
            readonly property var l: modelData.locator || {}
            readonly property bool picked: root.selected && index === root.hit
            visible: l.t !== undefined
            anchors.verticalCenter: parent.verticalCenter
            width: Math.max(Style.space(4), track.width * ((l.t_end || l.t) - l.t) / root.item.duration)
            x: Math.min(track.width - width, track.width * l.t / root.item.duration)
            height: picked ? Style.space(10) : track.height
            radius: Math.min(Style.cornerRadius, height / 2)
            color: Commons.Color.accent
            opacity: picked || !root.selected ? 1 : 0.5
          }
        }
        Text {
          id: count
          anchors.right: parent.right
          anchors.verticalCenter: parent.verticalCenter
          visible: root.hits.length > 1
          text: root.hits.length + " sections"
          textFormat: Text.PlainText
          color: Commons.Color.menu.text
          opacity: root.dim
          font.family: Style.font.menuFamily
          font.pixelSize: Style.font.caption
        }
      }

      Text {
        // The marked snippet, or the text of the hit picked with ←/→; on audio after the picked section's time.
        readonly property var picked: root.hit > 0 ? root.hits[root.hit] : null
        readonly property string said: picked ? root.marked({ text: picked.text }) : root.item.snippet ? root.marked(root.item.snippet) : ""
        width: parent.width
        visible: text.length > 0 && !root.expanded
        text: said && root.timed && !root.thumbs ? root.where(root.hits[root.hit].locator || {}) + "   " + said : said
        textFormat: Text.StyledText
        wrapMode: Text.Wrap
        maximumLineCount: 2
        elide: Text.ElideRight
        color: Commons.Color.menu.text
        opacity: 0.8
        font.family: Style.font.menuFamily
        font.pixelSize: Style.font.body
      }

      // Sections in time order, each a thumbnail over its time and text; the picked one is outlined.
      Row {
        id: strip
        visible: root.thumbs
        topPadding: Style.spacing.sm
        spacing: Style.spacing.md
        // As wide as fits: up to 96, so 8 sections still fit the row.
        readonly property real cell: Math.min(Style.space(96), (body.width - spacing * (root.order.length - 1)) / root.order.length)
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
              width: strip.cell
              height: Math.round(width * 9 / 16)
              radius: Math.min(Style.cornerRadius, Style.space(6))
              color: Style.normalFillFor(Commons.Color.menu.text, Commons.Color.accent)
              borderSpec: moment.picked ? Border.flat(Commons.Color.accent, Style.space(2)) : Border.none()
              Image {
                anchors.fill: parent
                anchors.margins: frame.borderLeft
                source: moment.h.thumb ? Util.fileUrl(moment.h.thumb) : ""
                sourceSize: Qt.size(width * Screen.devicePixelRatio, height * Screen.devicePixelRatio)
                fillMode: Image.PreserveAspectCrop
                asynchronous: true
              }
            }
            Repeater {  // its time, then its text (a label or what is said) when it has any
              model: [root.where(moment.h.locator || {}), String(moment.h.text || "").replace(/\s+/g, " ").trim()].filter(String)
              Text {
                required property string modelData
                width: strip.cell
                text: modelData
                textFormat: Text.PlainText
                elide: Text.ElideRight
                color: moment.picked ? Commons.Color.accent : Commons.Color.menu.text
                opacity: moment.picked ? 1 : root.dim
                font.family: Style.font.menuFamily
                font.pixelSize: Style.font.caption
              }
            }
          }
        }
      }

      // Pages, slides or lines as chips.
      Flow {
        width: parent.width
        visible: root.hits.length > 1 && !root.thumbs && !root.timed && !root.expanded
        spacing: Style.spacing.lg
        Repeater {
          model: root.thumbs || root.timed || root.expanded ? [] : root.order
          Text {
            required property int modelData
            readonly property bool picked: root.selected && modelData === root.hit
            text: root.where(root.hits[modelData].locator || {})
            textFormat: Text.PlainText
            color: picked ? Commons.Color.accent : Commons.Color.menu.text
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
          color: picked ? Commons.Color.accent : Commons.Color.menu.text
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
      textFormat: Text.PlainText
      color: root.ink
      opacity: root.dim
      font.family: Style.font.menuFamily
      font.pixelSize: Style.font.caption
    }
  }
}
