import QtQuick
import qs.Commons
import qs.Commons as Commons  // Qt 6.12's QtQuick has a Color of its own: the theme's is Commons.Color
import qs.Ui

// One result (a `Row` from crates/core). The headline says why it matched: when a
// file, photo, video or recording matched by what is in it (not by its name), the
// matched passage, quoted, its matched words in bold, with the name and folder under
// it as the citation; otherwise the title and subtitle. On the right, where Enter
// lands (p. 3, slide 4, 0:13–0:27, line 15, row 4 of a sheet), larger on the selected row,
// over the picked part's frame when it has one. Pages, slides, lines and sheets are
// numbered chips; video and audio a timeline of the whole length, each section on it
// and labelled with its times. An emoji row is a strip: its hits are emoji rows, drawn
// large, the picked one named under them. Indexed text is always drawn as plain text,
// never as markup. Colours, sizes and radii come from Omarchy's theme; the font is the
// system's UI sans.
Column {
  id: root

  property var item: ({})
  property string iconSource: ""
  property bool selected: false
  // ↑/↓ are in the rows: the selected row is solid. While typing it is soft: what Enter opens.
  property bool focused: true
  readonly property bool solid: selected && focused
  property int hit: 0            // the picked hit: an index into item.hits
  property bool expanded: false  // Tab: every hit on a line of its own
  property bool notice: false    // nothing local matched: say so above this (web) row
  signal clicked(int pick)  // pick: the emoji, page or section clicked, else -1
  signal seek(real t)       // a click on a timeline away from its sections: play from there

  readonly property bool emoji: item.kind === "emoji"
  readonly property var hits: emoji ? [] : item.hits || []
  readonly property var order: emoji ? [] : item._order || []
  readonly property var picked: hits[hit] || ({})
  // Video and audio with a known length and sections get a timeline; songs
  // matched by their whole lyrics have no sections and keep their snippet.
  readonly property bool timed: item.duration > 0 && hits.length > 0
  readonly property int iconSize: item.kind === "image" ? Style.space(56) : Style.space(28)
  readonly property color ink: solid ? Commons.Color.menu.selectedText : Commons.Color.menu.text
  // Secondary text is the theme's text colour dimmed, as in Omarchy's menu.
  readonly property real dim: 0.52
  readonly property string sans: Qt.application.font.family
  readonly property var tabular: ({ "tnum": 1 })

  // What matched inside: the snippet for the best part, the picked part's own text for another, from its best sentence.
  readonly property string said: ["file", "image", "video", "audio"].indexOf(item.kind) >= 0 && item.why !== "name"
    ? lead(hit > 0 && picked.text || (item.snippet || {}).text || picked.text || "") : ""
  // The words the daemon marked in the snippet, longest first.
  readonly property var words: {
    var s = item.snippet || {}, c = Array.from(String(s.text || "")).map(function(x) { return x.toLowerCase() })
    var all = (s.marks || []).map(function(m) { return c.slice(m[0], m[1]).join("") })
    return all.filter(function(w, i) { return w && all.indexOf(w) === i }).sort(function(a, b) { return b.length - a.length })
  }
  // Two headline lines whenever ←/→ can bring a passage that needs them, so stepping never moves the rows below.
  readonly property bool tall: said !== "" && order.length > 1 && [(item.snippet || {}).text].concat(hits.map(function(h) { return h.text }))
    .some(function(t) { return head.advanceWidth("“" + lead(t || "") + "”") * 1.05 > body.width })
  readonly property string place: figure(picked.locator || {})
  property real hoverT: -1  // the time under the pointer on the timeline
  readonly property var framed: hoverT >= 0 ? hits[nearest(hoverT)] : picked
  readonly property string frame: item.kind !== "image" && framed.thumb ? framed.thumb : ""
  readonly property var chips: order.filter(function(i) { return where(hits[i].locator || {}) !== "" })

  spacing: Style.spacing.md

  // EPUB parts carry their chapter as the page.
  readonly property bool epub: /\.epub$/i.test(item.key || "")

  // m:ss, or h:mm:ss from an hour on.
  function clock(t) {
    var s = Math.floor(t), two = function(n) { return ("0" + n).slice(-2) }
    return (s < 3600 ? Math.floor(s / 60) : Math.floor(s / 3600) + ":" + two(Math.floor(s / 60) % 60)) + ":" + two(s % 60)
  }

  // What the place column draws large: a sheet's row (its chip names the sheet), else the place.
  function figure(l) { return l.sheet && l.row ? "row " + l.row : where(l) }

  // A hit's place: m:ss–m:ss, page or chapter, slide, sheet row, line.
  function where(l) {
    if (l.t !== undefined) return clock(l.t) + (l.t_end >= l.t + 1 ? "–" + clock(l.t_end) : "")
    if (l.page) return (epub ? "ch. " : "p. ") + l.page
    if (l.slide) return "slide " + l.slide
    if (l.sheet) return l.sheet + (l.row ? " · row " + l.row : "")
    if (l.line) return "line " + l.line
    return ""
  }

  // Where one of `words` starts a word in the characters c, as [start, end] to the end of that word.
  function spans(c) {
    var lc = c.map(function(x) { return x.toLowerCase() }), out = []
    for (var i = 0; i < c.length; i++) {
      var w = i > 0 && /\w/.test(c[i - 1]) ? null : words.find(function(w) { return lc.slice(i, i + Array.from(w).length).join("") === w })
      if (!w) continue
      var end = i + Array.from(w).length
      while (end < c.length && /\w/.test(c[end])) end++
      out.push([i, end])
      i = end - 1
    }
    return out
  }

  // The quote starts at the sentence with the most marked words, after "…" when that is not the first.
  function lead(text) {
    var t = String(text), starts = [0], re = /[.!?…]\s+|\n\s*/g, best = 0
    while (re.exec(t) && re.lastIndex < t.length) starts.push(re.lastIndex)
    var count = function(i) { return spans(Array.from(t.slice(starts[i], starts[i + 1]))).length }
    for (var i = 1; i < starts.length; i++) if (count(i) > count(best)) best = i
    return best ? "…" + t.slice(starts[best]) : t
  }

  // As StyledText: escaped, each marked word in bold.
  function marked(text) {
    var c = Array.from(String(text).replace(/\s+/g, " ")), out = "", at = 0
    var esc = function(a, b) { return c.slice(a, b).join("").replace(/&/g, "&amp;").replace(/</g, "&lt;") }
    spans(c).forEach(function(s) { out += esc(at, s[0]) + "<b>" + esc(s[0], s[1]) + "</b>"; at = s[1] })
    return out + esc(at)
  }

  // Seconds from time t to section i (0 inside it).
  function gap(i, t) {
    var l = hits[i].locator || {}
    return l.t === undefined ? Infinity : Math.max(l.t - t, t - (l.t_end || l.t), 0)
  }
  function nearest(t) {
    return hits.reduce(function(best, h, i) { return gap(i, t) < gap(best, t) ? i : best }, 0)
  }

  // Under the timeline: each section's times at its start, the picked one first, then the
  // others in drawn order and the whole length at the end, each where it overlaps none placed.
  function labels(w) {
    var out = []
    ;[hit].concat(order.filter(function(i) { return i !== hit }), [-1]).forEach(function(i) {
      var l = i < 0 ? { t: item.duration } : hits[i].locator || {}
      if (l.t === undefined) return
      var s = i < 0 ? clock(l.t) : where(l), lw = small.advanceWidth(s), x = Math.max(0, Math.min(w - lw, w * l.t / item.duration))
      if (out.every(function(o) { return x + lw + Style.spacing.lg <= o.x || x >= o.x + o.w + Style.spacing.lg }))
        out.push({ i: i, x: x, w: lw, s: s })
    })
    return out
  }

  FontMetrics { id: head; font.family: root.sans; font.pixelSize: Style.font.heading }
  FontMetrics { id: small; font.family: root.sans; font.pixelSize: Style.font.caption; font.features: root.tabular }
  FontMetrics { id: large; font.family: root.sans; font.pixelSize: Style.font.display; font.features: root.tabular }

  readonly property string kindLabel: {
    var dot = String(item.title || "").lastIndexOf("."), audio = /^(Song|Recording) ·/.exec(item.subtitle || "")
    if (audio) return audio[1]
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
    font.family: root.sans
    font.pixelSize: Style.font.title
  }

  BorderSurface {
    width: root.width
    height: Math.max(body.implicitHeight, glyph.height, side.implicitHeight) + Style.spacing.lg * 2
    radius: Style.cornerRadius
    color: root.solid ? Commons.Color.menu.selectedBackground : root.selected ? Style.normalFillFor(Commons.Color.menu.text, Commons.Color.accent) : "transparent"
    borderSpec: root.solid ? Border.surfaceSpec("menu", "selected-border", Commons.Color.menu.selectedBorder, 0) : Border.none()

    MouseArea {  // first, so the emoji, chips and timeline have their own above it
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
      anchors.right: parent.right
      anchors.rightMargin: Style.spacing.rowPaddingX + side.width + Style.spacing.lg
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
      Text {  // the headline: what matched, quoted (a video's section label is not), or the title
        width: parent.width
        visible: !root.emoji
        text: !root.said ? root.item.title || "" : root.item.kind === "video" ? root.marked(root.said) : "“" + root.marked(root.said) + "”"
        textFormat: root.said ? Text.StyledText : Text.PlainText
        wrapMode: Text.Wrap
        maximumLineCount: 2
        height: root.tall ? implicitHeight / Math.max(1, lineCount) * 2 : implicitHeight
        color: root.ink
        font.family: root.sans
        font.pixelSize: Style.font.heading
        font.weight: root.said ? Font.Normal : Font.Medium
      }
      Text {  // the citation, the subtitle, or the picked emoji's name
        width: parent.width
        visible: text.length > 0
        text: root.emoji ? (root.item.hits[root.hit] || root.item.hits[0]).title
              : root.said ? [root.item.title, root.item.subtitle].filter(Boolean).join("  ·  ") : root.item.subtitle || ""
        textFormat: Text.PlainText
        elide: Text.ElideMiddle
        color: root.ink
        opacity: root.dim
        font.family: root.sans
        font.pixelSize: Style.font.bodySmall
      }

      // Media: the whole length, each section a segment, the picked one raised, times under it.
      // The pointer over it reads the time there and shows the nearest section's frame; a click
      // on a section plays from its start, anywhere else from that time.
      Item {
        id: timeline
        readonly property int bar: Style.space(12)
        visible: root.timed
        width: parent.width
        height: Style.spacing.sm + bar + Style.spacing.xs + small.height

        Item {
          y: Style.spacing.sm
          width: parent.width
          height: timeline.bar

          Rectangle {
            anchors.verticalCenter: parent.verticalCenter
            width: parent.width
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
              readonly property bool picked: index === root.hit
              visible: l.t !== undefined
              anchors.verticalCenter: parent.verticalCenter
              width: Math.max(Style.space(4), timeline.width * ((l.t_end || l.t) - l.t) / root.item.duration)
              x: Math.min(timeline.width - width, timeline.width * l.t / root.item.duration)
              height: picked ? timeline.bar : Style.space(4)
              radius: Math.min(Style.cornerRadius, height / 2)
              color: Commons.Color.accent
              opacity: picked || !root.selected ? 1 : 0.5
            }
          }
          Rectangle {  // the pointer
            visible: root.hoverT >= 0
            x: timeline.width * root.hoverT / root.item.duration - width / 2
            width: Style.space(2)
            height: parent.height
            color: root.ink
          }
          Repeater {
            model: root.timed ? root.labels(timeline.width) : []
            Text {
              required property var modelData
              readonly property bool picked: modelData.i === root.hit
              x: modelData.x
              y: timeline.bar + Style.spacing.xs
              text: modelData.s
              textFormat: Text.PlainText
              color: root.ink
              opacity: picked ? 1 : root.dim
              font.family: root.sans
              font.pixelSize: Style.font.caption
              font.features: root.tabular
              font.weight: picked ? Font.DemiBold : Font.Normal
            }
          }
        }
        MouseArea {
          anchors.fill: parent
          hoverEnabled: true
          cursorShape: Qt.PointingHandCursor
          function at(x) { return Math.max(0, Math.min(1, x / width)) * root.item.duration }
          onPositionChanged: function(m) { root.hoverT = at(m.x) }
          onExited: root.hoverT = -1
          onClicked: function(m) {
            var t = at(m.x), i = root.nearest(t)
            if (root.gap(i, t) * width / root.item.duration <= Style.space(4)) root.clicked(i)
            else root.seek(t)
          }
        }
      }

      // Pages, slides, lines or sheets as numbered chips; the picked one outlined.
      Flow {
        width: parent.width
        visible: (root.chips.length > 1 || !!(root.picked.locator || {}).sheet) && !root.timed && !root.expanded
        topPadding: Style.spacing.xs
        spacing: Style.spacing.sm
        Repeater {
          model: root.timed || root.expanded ? [] : root.chips
          BorderSurface {
            required property int modelData
            readonly property var l: root.hits[modelData].locator || {}
            readonly property bool picked: modelData === root.hit
            width: Math.max(Style.space(28), number.implicitWidth + Style.spacing.lg * 2)
            height: Style.space(24)
            radius: Math.min(Style.cornerRadius, Style.space(6))
            color: Style.normalFillFor(Commons.Color.menu.text, Commons.Color.accent)
            borderSpec: picked ? Border.flat(Commons.Color.accent, Style.space(2)) : Border.none()
            Text {
              id: number
              anchors.centerIn: parent
              text: String(l.page || l.slide || l.line || root.where(l))
              textFormat: Text.PlainText
              color: root.ink
              opacity: picked ? 1 : 0.8
              font.family: root.sans
              font.pixelSize: Style.font.bodySmall
              font.features: root.tabular
              font.weight: picked ? Font.DemiBold : Font.Normal
            }
            MouseArea { anchors.fill: parent; cursorShape: Qt.PointingHandCursor; onClicked: root.clicked(modelData) }
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
          font.family: root.sans
          font.pixelSize: Style.font.body
          font.weight: picked ? Font.DemiBold : Font.Normal
        }
      }
    }

    // Where Enter lands (or the time under the pointer), else the kind; under it the frame.
    Column {
      id: side
      anchors.right: parent.right
      anchors.rightMargin: Style.spacing.rowPaddingX
      y: Style.spacing.lg
      spacing: Style.spacing.sm
      // As wide (up to a third of the row) as its widest place drawn large, and as tall, so neither selecting nor stepping reflows it.
      width: Math.min(root.width / 3, Math.max.apply(null, [root.place ? 0 : small.advanceWidth(root.kindLabel), root.frame ? Style.space(80) : 0]
        .concat(root.hits.map(function(h) { return large.advanceWidth(root.figure(h.locator || {})) }))))

      Text {
        width: parent.width
        height: (root.place ? large : small).height
        horizontalAlignment: Text.AlignRight
        readonly property bool loud: root.place !== "" && (root.selected || root.hoverT >= 0)
        text: root.hoverT >= 0 ? root.clock(root.hoverT) : root.place || root.kindLabel
        textFormat: Text.PlainText
        elide: Text.ElideRight
        color: root.ink
        opacity: loud ? 1 : root.dim
        font.family: root.sans
        font.pixelSize: loud ? Style.font.display : Style.font.caption
        font.features: root.tabular
      }
      Image {
        visible: !!root.frame
        anchors.right: parent.right
        width: Style.space(80)
        height: Math.round(width * 9 / 16)
        source: root.frame ? Util.fileUrl(root.frame) : ""
        sourceSize: Qt.size(width * Screen.devicePixelRatio, height * Screen.devicePixelRatio)
        fillMode: Image.PreserveAspectCrop
        asynchronous: true
      }
    }
  }
}
