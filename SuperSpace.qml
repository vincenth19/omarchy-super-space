import QtQuick
import Quickshell
import Quickshell.Io
import Quickshell.Wayland
import qs.Commons
import qs.Ui
import "ui"

// Super Space: one search box over everything on this computer, as an Omarchy
// menu plugin. It talks to `superspace serve` over one persistent socket
// (DESIGN.md "Protocol"): each keystroke sends one search with a new id, and
// only responses carrying the latest id are shown, the instant phase as soon
// as it lands and the final phase in its place. Rows stay until new ones
// arrive and the selection stays on the same key, so typing is calm (DESIGN.md
// "UI"). Running a result is the daemon's job; this sends `activate`.
//
// Colours, fonts, sizes, radii and transparency all come from Omarchy's
// theme (Color, Style, Border): the same tokens Omarchy's own menu uses.
Item {
  id: root

  property var shell: null  // injected by omarchy-shell
  readonly property var apps: shell ? shell.appLibrary : null

  property bool opened: false
  property int seq: 0           // id of the latest request
  property string phase: ""
  property var answer: null
  property var rows: []
  property bool empty: false
  property int sel: 0           // 0 is the answer card when there is one
  property string selKey: ""    // the key the selection stays on as responses arrive
  property int hit: 0           // the selected row's picked hit, an index into its hits
  property bool expanded: false
  property var activated: null  // the last activate sent, for the test bench
  property var perf: []         // [phase, ms] per response since opening, for the bench
  readonly property int offset: answer ? 1 : 0
  readonly property var current: sel >= offset ? rows[sel - offset] || null : null
  readonly property string selectedKey: current ? current.key : answer ? "answer" : ""
  // "Nothing on this computer matches" only once the final phase says so, never mid-word.
  readonly property bool nothing: empty && phase === "final"

  function open(payloadJson) {
    perf = []
    opened = true
    Qt.callLater(function() { field.forceActiveFocus() })
  }

  function close() {
    opened = false
    field.text = ""
    if (progress && progress.step === "ready") progress = null
  }

  // ------------------------------------------------------------ daemon

  // One persistent connection. Quickshell's Socket cannot retry once a
  // connect has failed, so every attempt is a fresh Socket: made when the
  // plugin loads, and again each second while open and not connected (the
  // daemon is not up yet, or restarted). Connected, it stays open between opens.
  Loader {
    id: link
    sourceComponent: Socket {
      id: socket
      path: Quickshell.env("XDG_RUNTIME_DIR") + "/superspace.sock"
      connected: true
      parser: SplitParser { onRead: function(line) { root.receive(line) } }
      onConnectionStateChanged: {
        root.sock = connected ? socket : null
        if (connected && field.text.trim()) root.search(field.text)
      }
    }
  }
  property QtObject sock: null  // the connected Socket
  readonly property bool connected: sock !== null

  Timer {
    property int tries: 0
    running: root.opened && !root.connected
    interval: 1000
    repeat: true
    triggeredOnStart: true
    onRunningChanged: tries = 0
    onTriggered: {
      if (++tries === 6) setup.running = true
      link.active = false
      link.active = true
    }
  }

  // Still no daemon five seconds after opening: run the plugin's setup and
  // show its progress (DESIGN.md "Setup"), unless superspace.service is up and
  // merely slow. Setup runs as a systemd unit of its own, so closing the box,
  // a plugin reload or a shell restart never kills it mid-download; this
  // follows the JSON lines it writes to a file (and picks a running one back
  // up): {step, message} or {step: "download", file, done, total}; "ask-bind"
  // means it left Super+Space alone, so the box asks.
  readonly property string pluginDir: decodeURIComponent(String(Qt.resolvedUrl(".")).replace(/^file:\/\//, ""))
  property var progress: null   // setup's last line
  property bool askBind: false

  Process {
    id: setup
    command: ["sh", "-c", `systemctl --user -q is-active superspace.service && exit
      systemctl --user -q is-active superspace-setup.service || { : >"$1"
        systemd-run --user -q --collect -u superspace-setup -p StandardOutput=append:"$1" -p StandardError=journal "$0" --progress; }
      exec tail -F -n +1 "$1" 2>/dev/null`, root.pluginDir + "bin/superspace-setup", Quickshell.env("XDG_RUNTIME_DIR") + "/superspace-setup.jsonl"]
    stdout: SplitParser {
      onRead: function(line) {
        try { root.progress = JSON.parse(line) } catch (e) { return }
        if (root.progress.step === "ask-bind") root.askBind = true
        if (root.progress.step === "ready" || root.progress.step === "error") setup.running = false
      }
    }
  }
  Process { id: bind; command: [root.pluginDir + "bin/superspace-setup", "bind"] }

  function send(request) {
    if (!connected) return false
    sock.write(JSON.stringify(request) + "\n")
    sock.flush()
    return true
  }

  function search(q) {
    seq += 1
    if (!q.trim() || !send({ op: "search", id: seq, q: q }))
      show({ id: seq, phase: "", rows: [], empty: false })
  }

  function receive(line) {
    var msg
    try { msg = JSON.parse(line) } catch (e) { return }
    if (msg.id !== seq || !Array.isArray(msg.rows)) return
    // An instant phase with nothing local and no answer leaves the rows on
    // screen until the final phase: no empty flash mid-word.
    if (msg.phase === "instant" && !msg.answer && msg.rows.every(function(r) { return r.kind === "web" })) return
    show(msg)
  }

  // Hits are listed best first; they are shown and stepped in document or
  // time order. The web row is always last, whatever arrives.
  function order(hits) {
    var at = function(h) { var l = h.locator || {}; return l.t ?? l.page ?? l.slide ?? l.row ?? l.line ?? 0 }
    return hits.map(function(h, i) { return i }).sort(function(a, b) { return at(hits[a]) - at(hits[b]) })
  }

  function show(msg) {
    var started = Date.now(), keep = selKey
    var web = msg.rows.filter(function(r) { return r.kind === "web" })
    var all = msg.rows.filter(function(r) { return r.kind !== "web" }).concat(web)
    all.forEach(function(r) { r._order = order(r.hits || []) })
    answer = msg.answer || null
    rows = all
    empty = msg.empty === true
    phase = msg.phase
    var found = keep === "answer" ? (answer ? 0 : -1) : rows.findIndex(function(r) { return r.key === keep })
    if (found < 0) { sel = 0; hit = 0; expanded = false }
    else sel = keep === "answer" ? 0 : found + offset
    if (current && hit >= (current.hits || []).length) hit = 0
    // Landing on the web row by default is not kept: local rows come first.
    selKey = current ? (found < 0 && current.kind === "web" ? "" : current.key) : answer ? "answer" : ""
    list.forceLayout()
    if (current) list.positionViewAtIndex(sel - offset, ListView.Contain)
    if (msg.phase) perf = perf.concat([[msg.phase, Date.now() - started]])
  }

  // ------------------------------------------------------------ keys

  function move(step) {
    var count = offset + rows.length
    if (!count) return
    sel = Math.max(0, Math.min(count - 1, sel + step))
    selKey = current ? current.key : "answer"
    hit = 0
    expanded = false
    if (current) list.positionViewAtIndex(sel - offset, ListView.Contain)
  }

  // ←/→ walk the selected row's hits; with fewer than two they move the cursor.
  function stepHit(step) {
    var o = current ? current._order : []
    if (o.length < 2) return false
    hit = o[Math.max(0, Math.min(o.length - 1, o.indexOf(hit) + step))]
    return true
  }

  function copy(text) {
    if (text) Quickshell.execDetached(["wl-copy", "--", String(text)])
  }

  function copyText() {
    var a = current ? current.action || {} : {}
    return !current ? (answer ? answer.copy : "") : a.path || a.url || a.text || current.title
  }

  // The daemon runs it; alt (Ctrl+Enter) is the alternative: show in folder, or copy.
  function activate(i, alt) {
    var row = i < offset ? { key: "answer" } : rows[i - offset]
    if (!row) return
    var request = { op: "activate", id: seq + 1, key: row.key, q: field.text, hit: (row.hits || []).length ? (i === sel ? hit : 0) : null, alt: !!alt }
    activated = request
    send(request)
    close()
  }

  // True when the key was ours; the text field handles the rest.
  function press(key, mods) {
    var ctrl = (mods & Qt.ControlModifier) !== 0
    var enter = key === Qt.Key_Return || key === Qt.Key_Enter
    if (askBind && !field.text && (enter || key === Qt.Key_Escape)) { bind.running = enter; askBind = false }
    else if (key === Qt.Key_Escape) close()
    else if (key === Qt.Key_Down || (ctrl && key === Qt.Key_N)) move(1)
    else if (key === Qt.Key_Up || (ctrl && key === Qt.Key_P)) move(-1)
    else if (enter) activate(sel, ctrl)
    else if (key === Qt.Key_Right || key === Qt.Key_Left) return stepHit(key === Qt.Key_Right ? 1 : -1)
    else if (key === Qt.Key_Tab) expanded = !expanded && !!current && (current.hits || []).length > 0
    else if (ctrl && key >= Qt.Key_1 && key <= Qt.Key_9) activate(offset + key - Qt.Key_1)
    else if (ctrl && key === Qt.Key_C && !field.selectedText && copyText()) { copy(copyText()); close() }
    else if (ctrl && key === Qt.Key_U) field.text = ""
    else return false
    return true
  }

  // ------------------------------------------------------------ test bench
  //   omarchy-shell superspace open 'golden retriever'   open if closed, set the query
  //   omarchy-shell superspace key Down                  Up Left Right Enter Escape Tab Backspace ctrl+Enter ctrl+2 …
  //   omarchy-shell superspace state                     JSON, described in ui/README.md

  IpcHandler {
    target: "superspace"

    function open(q: string): string {
      if (!root.opened) root.open("{}")
      field.text = q
      return "ok"
    }

    function key(name: string): string {
      var parts = String(name).split("+"), k = parts.pop(), mods = 0
      parts.forEach(function(p) {
        if (/^ctrl$/i.test(p)) mods |= Qt.ControlModifier
        else if (/^shift$/i.test(p)) mods |= Qt.ShiftModifier
      })
      var code = ({ Down: Qt.Key_Down, Up: Qt.Key_Up, Left: Qt.Key_Left, Right: Qt.Key_Right, Enter: Qt.Key_Return,
                    Return: Qt.Key_Return, Escape: Qt.Key_Escape, Tab: Qt.Key_Tab, Backspace: Qt.Key_Backspace })[k]
      if (code === undefined && k.length === 1) code = k.toUpperCase().charCodeAt(0)  // Qt.Key_A is "A"
      if (code === undefined) return "unknown key: " + k
      if (!root.press(code, mods) && code === Qt.Key_Backspace) field.text = field.text.slice(0, -1)
      return "ok"
    }

    function state(): string {
      var hex = function(c) { return "#" + [c.r, c.g, c.b].map(function(v) { return ("0" + Math.round(v * 255).toString(16)).slice(-2) }).join("") }
      return JSON.stringify({
        open: root.opened, query: field.text, connected: root.connected, phase: root.phase,
        selected: root.sel - root.offset, selectedKey: root.selectedKey, hit: root.current && (root.current.hits || []).length ? root.hit : null,
        expanded: root.expanded, empty: root.empty, answer: root.answer,
        rows: root.rows.map(function(r) { return { key: r.key, kind: r.kind, title: r.title, hits: (r.hits || []).length } }),
        activated: root.activated, perf: root.perf, setup: root.progress, askBind: root.askBind,
        theme: { background: hex(Color.menu.background), text: hex(Color.menu.text) }
      })
    }
  }

  // ------------------------------------------------------------ view

  PanelWindow {
    id: panel
    visible: root.opened
    anchors { top: true; bottom: true; left: true; right: true }
    color: "transparent"
    exclusionMode: ExclusionMode.Ignore
    // Omarchy's menu namespace, so each theme's rules for menus (blur, no
    // open animation) apply here too.
    WlrLayershell.namespace: "omarchy-menu"
    WlrLayershell.layer: WlrLayer.Overlay
    WlrLayershell.keyboardFocus: WlrKeyboardFocus.OnDemand

    Rectangle { anchors.fill: parent; color: Color.menu.scrim }
    MouseArea { anchors.fill: parent; onClicked: root.close() }

    BorderSurface {
      id: card
      width: Math.min(Style.space(680), panel.width - Style.gapsOut * 2)
      height: column.implicitHeight + contentTopInset + contentBottomInset
      x: Math.round((panel.width - width) / 2)
      y: Math.round(panel.height * 0.18)
      radius: Style.cornerRadius
      color: Color.menu.background
      borderSpec: Border.surfaceSpec("menu", "border", Color.menu.border, Math.max(1, Style.space(2)))
      padding: Style.spacing.panelPadding

      MouseArea { anchors.fill: parent }

      Column {
        id: column
        x: card.contentLeftInset
        y: card.contentTopInset
        width: card.width - card.contentLeftInset - card.contentRightInset
        spacing: Style.spacing.lg

        TextInput {
          id: field
          width: parent.width
          leftPadding: Style.spacing.rowPaddingX
          rightPadding: Style.spacing.rowPaddingX
          focus: true
          clip: true
          color: Color.menu.text
          selectionColor: Style.selectionFill
          selectedTextColor: Color.menu.text
          font.family: Style.font.menuFamily
          font.pixelSize: Style.font.display
          onTextChanged: root.search(text)
          // Like Spotlight: another window taking the keyboard closes the box.
          onActiveFocusChanged: if (!activeFocus && root.opened) root.close()
          Keys.onPressed: function(event) { if (root.press(event.key, event.modifiers)) event.accepted = true }

          Text {
            x: field.leftPadding
            visible: !field.text
            text: "Search apps, files, photos, answers…"
            color: Color.menu.text
            opacity: 0.58
            font: field.font
          }
        }

        AnswerCard {
          visible: !!root.answer
          width: parent.width
          answer: root.answer || ({})
          selected: root.sel === 0
          onClicked: root.activate(0)
        }

        Text {
          readonly property var p: root.progress || ({})
          // Setup's progress, until the box is used for searching.
          readonly property bool setupShown: !!root.progress && (!root.connected || !field.text.trim())
          visible: setupShown || (field.text.trim() !== "" && (!root.connected || (root.nothing && !root.rows.length)))
          width: parent.width
          leftPadding: Style.spacing.rowPaddingX
          rightPadding: Style.spacing.rowPaddingX
          text: setupShown ? (p.file ? "Downloading " + p.file + (p.total ? " · " + Math.floor(p.done * 100 / p.total) + "% of " + Math.round(p.total / 1048576) + " MB" : "") : p.message || "")
                : root.connected ? "Nothing on this computer matches" : "Waiting for Super Space (superspace.service)…"
          textFormat: Text.PlainText
          wrapMode: Text.Wrap
          color: p.step === "error" ? Color.urgent : Color.menu.text
          opacity: 0.7
          font.family: Style.font.menuFamily
          font.pixelSize: Style.font.title

          Rectangle {  // download progress
            visible: parent.setupShown && parent.p.total > 0
            anchors.top: parent.bottom
            x: parent.leftPadding
            width: (parent.width - parent.leftPadding * 2) * Math.min(1, parent.p.done / parent.p.total)
            height: Style.space(3)
            radius: height / 2
            color: Color.accent
          }
        }

        Text {
          visible: root.askBind && !field.text
          width: parent.width
          leftPadding: Style.spacing.rowPaddingX
          text: "Make Super+Space open Super Space?    ↵ yes    esc no"
          color: Color.menu.selectedText
          font.family: Style.font.menuFamily
          font.pixelSize: Style.font.title
        }

        ListView {
          id: list
          width: parent.width
          height: Math.min(contentHeight, Math.round(panel.height * 0.6))
          visible: count > 0
          model: root.rows
          spacing: Style.spacing.xs
          clip: true
          interactive: contentHeight > height
          boundsBehavior: Flickable.StopAtBounds

          delegate: ResultRow {
            required property var modelData
            required property int index
            width: ListView.view.width
            item: modelData
            iconSource: root.iconSource(modelData.icon)
            selected: index + root.offset === root.sel
            hit: selected ? root.hit : 0
            expanded: selected && root.expanded
            notice: root.nothing && modelData.kind === "web"
            onClicked: root.activate(index + root.offset)
          }
        }

        Text {
          width: parent.width
          horizontalAlignment: Text.AlignRight
          rightPadding: Style.spacing.rowPaddingX
          visible: list.count > 0 || !!root.answer
          text: "↵ open    ctrl ↵ reveal    ← → pages, moments    tab expand    ctrl 1–9 row    esc close"
          color: Color.menu.text
          opacity: 0.52
          font.family: Style.font.menuFamily
          font.pixelSize: Style.font.caption
        }
      }
    }
  }

  // Row icons are icon names, desktop ids or image paths. A desktop id is
  // looked up for its icon; Omarchy's app library then resolves the name.
  function iconSource(icon) {
    var name = String(icon || "")
    var entry = name && name.charAt(0) !== "/" ? DesktopEntries.byId(name) : null
    if (entry && entry.icon) name = entry.icon
    if (apps) return apps.iconSource(name)
    return name.charAt(0) === "/" ? Util.fileUrl(name) : Quickshell.iconPath(name, "application-x-executable")
  }
}
