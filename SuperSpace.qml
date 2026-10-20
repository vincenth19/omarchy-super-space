import QtQuick
import Quickshell
import Quickshell.Io
import Quickshell.Wayland
import qs.Commons
import qs.Commons as Commons  // Qt 6.12's QtQuick has a Color of its own: the theme's is Commons.Color
import qs.Ui
import "ui"

// Super Space: one search box over everything on this computer, as an Omarchy
// menu plugin. It talks to `superspace serve` over one persistent socket
// (DESIGN.md "Protocol"): each keystroke sends one search with a new id, and
// only responses carrying the latest id are shown, the instant phase as soon
// as it lands and the final phase in its place. While typing, responses only
// add: rows, the answer card and the selection stay until typing pauses, so
// typing is calm (DESIGN.md "UI"). Running a result is the daemon's job; this
// sends `activate`.
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
  property var shown: ({ rows: [] })  // what is on screen, as one response
  property var last: ({})       // the latest final phase, shown as it is once typing pauses
  property bool empty: false    // "Nothing on this computer matches": no local row and no answer
  property int sel: 0           // 0 is the answer card when there is one
  property string selKey: ""    // the key the selection stays on as responses arrive
  property int hit: 0           // the selected row's picked hit, an index into its hits
  // "text": ←/→ move the cursor; "rows": ↑/↓ chose a row, ←/→ step its hits; "sections": Tab listed them, ←/→ step them
  property string mode: "text"
  property var activated: null  // the last activate sent, for the test bench
  property var perf: []         // [phase, ms] per response since opening, for the bench
  property var indexing: null   // the latest response's {done, total, stage}, while the daemon is still indexing
  property var filter: null     // the kinds the query's own words limit it to ("golden retriever video"), as the daemon read them
  readonly property string filterLabel: (filter || []).map(function(k) {
    return ({ app: "Apps", setting: "Settings", file: "Text", image: "Photos", video: "Videos", audio: "Audio",
              clip: "Clipboard", emoji: "Emoji", history: "History", web: "Web" })[k] || k
  }).join(" + ")
  readonly property int offset: answer ? 1 : 0
  readonly property var current: sel >= offset ? rows[sel - offset] || null : null
  readonly property string selectedKey: (target(sel) || {}).key || ""
  // What ←/→ step in the selected row, for the key hints.
  readonly property string parts: {
    var h = current && current.hits || [], l = (h[0] || {}).locator || {}
    return h.length < 2 ? "" : current.kind === "emoji" ? "emoji" : l.t !== undefined ? "sections" : l.page ? "pages"
         : l.slide ? "slides" : l.sheet ? "sheets" : l.line ? "lines" : "matches"
  }
  readonly property string sans: Qt.application.font.family  // the system's UI sans

  function open(payloadJson) {
    perf = []
    opened = true
    Qt.callLater(function() { field.forceActiveFocus() })
  }

  function close() {
    opened = false
    pending = []
    field.text = ""
    if (!setup.running) progress = null
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
      path: Quickshell.env("SUPERSPACE_SOCK") || Quickshell.env("XDG_RUNTIME_DIR") + "/superspace.sock"
      connected: true
      parser: SplitParser { onRead: function(line) { root.receive(line) } }
      onConnectionStateChanged: {
        root.sock = connected ? socket : null
        root.pending = []  // keys waiting for results that will not come
        if (connected && field.text.trim()) root.search(field.text)
      }
    }
  }
  property QtObject sock: null  // the connected Socket
  readonly property bool connected: sock !== null

  Timer {
    running: root.opened && !root.connected
    interval: 1000
    repeat: true
    triggeredOnStart: true
    onTriggered: { link.active = false; link.active = true }
  }

  // The plugin's setup, with its progress shown in the box (DESIGN.md
  // "Setup"). It runs as a systemd unit of its own, so closing the box, a
  // plugin reload or a shell restart never kills it mid-download; this follows
  // the JSON lines it writes to a file: {step, message} or {step: "download",
  // file, done, total}; "ask-bind" means it left Super+Space alone, so the box asks.
  readonly property string pluginDir: decodeURIComponent(String(Qt.resolvedUrl(".")).replace(/^file:\/\//, ""))
  property var progress: null   // setup's last line
  property bool askBind: false

  Process {
    id: setup
    command: ["sh", "-c", `systemctl --user -q is-active superspace-setup.service || { : >"$1"
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

  // Setup never ran (no daemon binary): open by itself and start setup, so
  // `omarchy plugin add … --enable` is the only command. Checked once per
  // plugin load; the only place setup starts, never for a daemon that is
  // merely slow or down. It cannot loop:
  // setup's first step installs the binary, setup writes nothing in the
  // plugin folder (no reload), and the setup command never starts a second one.
  Process {
    running: true
    command: ["test", "-e", Quickshell.env("HOME") + "/.local/share/superspace/bin/superspace"]
    onExited: function(code) { if (code) { root.open("{}"); setup.running = true } }
  }

  function send(request) {
    if (!connected) return false
    sock.write(JSON.stringify(request) + "\n")
    sock.flush()
    return true
  }

  function search(q) {
    seq += 1
    pending = []
    typing.restart()
    if (!q.trim() || !send({ op: "search", id: seq, q: q }))
      show({ phase: "", rows: [] })
  }

  function receive(line) {
    var msg
    try { msg = JSON.parse(line) } catch (e) { return }
    if (msg.id !== seq || !Array.isArray(msg.rows)) return
    show(msg)
    if (msg.phase === "final") pending.splice(0).forEach(settle)
  }

  // Hits are listed best first; they are shown and stepped in document or
  // time order. The web row is always last, whatever arrives.
  function order(hits) {
    var at = function(h) { var l = h.locator || {}; return l.t ?? l.page ?? l.slide ?? l.row ?? l.line ?? 0 }
    return hits.map(function(h, i) { return i }).sort(function(a, b) { return at(hits[a]) - at(hits[b]) || a - b })
  }

  // Calm while typing: until typing pauses for 400 ms, a response only adds.
  // Its rows go on top of the rows on screen, its answer replaces the card (no
  // answer keeps it), and it never brings up the empty notice; shown so, a
  // final phase counts as "instant". Once typing pauses, the latest final
  // phase is shown as it is. An instant phase always only adds: its final follows.
  Timer {
    id: typing
    interval: 400
    onTriggered: if (root.last.id === root.seq && root.phase !== "final") root.show(root.last)
  }

  function show(msg) {
    var started = Date.now(), keep = selKey, before = JSON.stringify(answer)
    var local = function(r) { return r.kind !== "web" }
    if (msg.phase === "final") last = msg
    if (msg.phase === "instant" || (msg.phase && typing.running)) {
      var keys = msg.rows.map(function(r) { return r.key })
      var old = shown.rows.filter(function(r) { return local(r) && keys.indexOf(r.key) < 0 })
      var kept = old.length || (shown.answer && !msg.answer) || (msg.empty && !shown.empty)
      msg = { phase: kept ? "instant" : msg.phase, answer: msg.answer || shown.answer, empty: msg.empty && shown.empty,
              filter: msg.filter || shown.filter, progress: msg.progress, rows: msg.rows.filter(local).concat(old, msg.rows.filter(function(r) { return !local(r) })) }
    }
    shown = msg
    var all = msg.rows.filter(local)
    empty = msg.empty === true && !msg.answer && !all.length
    all = all.concat(msg.rows.filter(function(r) { return !local(r) }))
    // Consecutive emoji make one strip of up to 8: a row whose hits are the emoji.
    all = all.reduce(function(out, r) {
      var last = out[out.length - 1]
      if (r.kind !== "emoji") out.push(r)
      else if (!last || last.kind !== "emoji") out.push({ key: r.key, kind: "emoji", hits: [r] })
      else if (last.hits.length < 8) last.hits.push(r)
      return out
    }, [])
    all.forEach(function(r) { r._order = order(r.hits || []) })
    answer = msg.answer || null
    indexing = msg.progress || null
    filter = msg.filter || null
    rows = all
    phase = msg.phase
    // The selection is the top until ↑/↓, → or Tab chose one; a chosen one stays on its key, but a
    // new answer (or a new value) takes it, so Enter and Ctrl+C act on it.
    var found = keep === "answer" ? (answer ? 0 : -1) : rows.findIndex(function(r) { return r.key === keep })
    if (found < 0 || mode === "text" || (answer && JSON.stringify(answer) !== before)) { sel = 0; hit = 0; mode = "text" }
    else sel = keep === "answer" ? 0 : found + offset
    if (current && hit >= (current.hits || []).length) hit = 0
    selKey = current ? current.key : answer ? "answer" : ""
    list.forceLayout()
    glide.stop()
    if (current) list.positionViewAtIndex(sel - offset, ListView.Contain)
    if (msg.phase) perf = perf.concat([[msg.phase, Date.now() - started]])
  }

  // ------------------------------------------------------------ keys

  // Enter, Ctrl+1…9, Ctrl+C, Tab, ↑/↓ and → at the end act on the current text's rows, never on a row calm
  // typing kept from an earlier query: they end the calm at once, so its final phase is shown as
  // it is, and while that phase is still on the way they wait for it. Typing again drops them,
  // and so does losing or regaining the daemon or closing; without one they act at once, on what
  // is shown. `steers`: the mode the key may bring, which changes what later keys mean: "rows" (↑/↓,
  // Ctrl+N/P, → at the end: ←/→ step parts) or "sections" (Tab: ←/→ step them, Shift+Tab and Esc go back).
  property var pending: []
  function settle(act, steers) {
    typing.stop()
    if (last.id === seq && phase !== "final") show(last)
    if (!connected || last.id === seq || !field.text.trim()) act()
    else pending.push(Object.assign(act, { steers: steers }))
  }

  // The first ↑/↓ from the field stays on the highlighted row, giving ←/→ to its parts; the next ones move.
  function move(step) {
    var count = offset + rows.length
    if (!count) return
    if (mode !== "text") { sel = Math.max(0, Math.min(count - 1, sel + step)); hit = 0 }
    selKey = current ? current.key : "answer"
    mode = "rows"
    glide.stop()
    if (current) list.positionViewAtIndex(sel - offset, ListView.Contain)
  }

  // In rows and sections, ←/→ walk the selected row's hits in drawn order.
  function stepHit(step) {
    var o = current ? current._order : []
    if (o.length) hit = o[Math.max(0, Math.min(o.length - 1, o.indexOf(hit) + step))]
  }

  // Where Alt+←/→ go, as on macOS: the start of this or the previous word, the end of this or the next.
  function word(step) {
    var t = field.text, at = field.cursorPosition
    return step < 0 ? at - /\S*\s*$/.exec(t.slice(0, at))[0].length : at + /^\s*\S*/.exec(t.slice(at))[0].length
  }

  function copy(text) {
    if (text) Quickshell.execDetached(["wl-copy", "--", String(text)])
  }

  // What item i runs: the answer, a row, or in an emoji strip the emoji picked with ←/→.
  function target(i) {
    var row = i < offset ? { key: "answer" } : rows[i - offset]
    return row && row.kind === "emoji" ? row.hits[i === sel ? hit : 0] : row
  }

  function copyText() {
    var r = target(sel) || {}, a = r.action || {}
    return r.key === "answer" ? answer.copy : a.path || a.url || a.text || r.title
  }

  // A click on a timeline away from its sections: mpv from that time, as the daemon runs a section.
  function play(row, t) {
    if (!(row.action || {}).path) return
    Quickshell.execDetached(["uwsm-app", "--", "mpv", "--start=" + t.toFixed(1), row.action.path])
    close()
  }

  // The daemon runs it; alt (Ctrl+Enter) is the alternative: show in folder, or copy.
  function activate(i, alt) {
    var row = target(i)
    if (!row) return
    var request = { op: "activate", id: seq + 1, key: row.key, q: field.text, hit: (row.hits || []).length ? (i === sel ? hit : 0) : null, alt: !!alt }
    activated = request
    send(request)
    close()
  }

  // True when the key was ours. The text field does the rest itself: ←/→, Shift to select,
  // Ctrl+←/→ by words, Home/End, Ctrl+A, Ctrl+Backspace, Ctrl+X/C/V. Alt and Super (Cmd in
  // the VM) add the macOS meanings where Hyprland's own Super bindings let them through.
  function press(key, mods) {
    var ctrl = (mods & Qt.ControlModifier) !== 0, alt = (mods & Qt.AltModifier) !== 0
    var meta = (mods & Qt.MetaModifier) !== 0, shift = (mods & Qt.ShiftModifier) !== 0
    var enter = key === Qt.Key_Return || key === Qt.Key_Enter
    var side = key === Qt.Key_Left ? -1 : key === Qt.Key_Right ? 1 : 0
    var edge = meta ? side : ctrl ? (key === Qt.Key_Home ? -1 : key === Qt.Key_End ? 1 : 0) : 0  // to the start or the end
    var back = key === Qt.Key_Backtab || (key === Qt.Key_Tab && shift)
    // Plain ←/→ wait behind a waiting Tab, ↑/↓, Ctrl+N/P or →, and Shift+Tab and Esc behind a waiting Tab, as
    // those change what they mean. Otherwise, or with no daemon, they act at once: Esc closes, dropping the rest.
    var waits = pending.map(function(a) { return a.steers })
    if (connected && ((side && !mods && waits.some(Boolean)) || ((back || key === Qt.Key_Escape) && waits.indexOf("sections") >= 0))) {
      pending.push(function() { if (!press(key, mods)) field.cursorPosition += side })  // a ←/→ the field would have had
      return true
    }
    // Editing keys give ←/→ back to the cursor (typing and moving the cursor do too).
    if ([Qt.Key_Home, Qt.Key_End, Qt.Key_Backspace, Qt.Key_Delete].indexOf(key) >= 0 || ((ctrl || meta) && key === Qt.Key_A)) mode = "text"
    if (askBind && !field.text && (enter || key === Qt.Key_Escape)) { bind.running = enter; askBind = false }
    else if (mode === "sections" && (key === Qt.Key_Escape || back)) mode = "text"
    else if (key === Qt.Key_Escape) close()
    else if (key === Qt.Key_Down || (ctrl && key === Qt.Key_N)) settle(function() { move(1) }, "rows")
    else if (key === Qt.Key_Up || (ctrl && key === Qt.Key_P)) settle(function() { move(-1) }, "rows")
    else if (enter) settle(function() { activate(sel, ctrl) })
    else if (mode !== "text" && side && !mods) stepHit(side)
    // → where the cursor cannot go further steps the selected row's parts, so the top row needs no ↑/↓ first.
    else if (side > 0 && !mods && !field.selectedText && field.cursorPosition === field.length)
      settle(function() { if (parts) { mode = "rows"; stepHit(1) } }, "rows")
    // Tab or Shift+Tab with nothing to go into or back from: the keyboard stays in the box.
    else if (key === Qt.Key_Tab && !back) settle(function() { if (current && (current.hits || []).length) mode = "sections" }, "sections")
    else if (back) {}
    else if (ctrl && key >= Qt.Key_1 && key <= Qt.Key_9) settle(function() { activate(offset + key - Qt.Key_1) })
    else if (ctrl && key === Qt.Key_C && !field.selectedText) settle(function() { if (copyText()) { copy(copyText()); close() } })
    else if (edge || (alt && side)) {
      var to = edge ? (edge < 0 ? 0 : field.length) : word(side)
      if (shift) field.moveCursorSelection(to); else field.cursorPosition = to
    }
    else if (((alt || meta) && key === Qt.Key_Backspace) || (ctrl && key === Qt.Key_U)) field.remove(alt ? word(-1) : 0, field.cursorPosition)
    else if (meta && key === Qt.Key_A) field.selectAll()
    else return false
    return true
  }

  // ------------------------------------------------------------ test bench
  //   omarchy-shell superspace open 'golden retriever'   open if closed, set the query
  //   omarchy-shell superspace key Down                  Up Left Right Enter Escape Tab shift+Tab Backspace ctrl+Enter ctrl+2 alt+Backspace …
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
        mods |= ({ ctrl: Qt.ControlModifier, shift: Qt.ShiftModifier, alt: Qt.AltModifier, super: Qt.MetaModifier })[p.toLowerCase()] || 0
      })
      var code = ({ Down: Qt.Key_Down, Up: Qt.Key_Up, Left: Qt.Key_Left, Right: Qt.Key_Right, Enter: Qt.Key_Return,
                    Return: Qt.Key_Return, Escape: Qt.Key_Escape, Tab: Qt.Key_Tab, Backspace: Qt.Key_Backspace, Home: Qt.Key_Home, End: Qt.Key_End })[k]
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
        mode: root.mode, expanded: root.mode === "sections", empty: root.empty, answer: root.answer, filter: root.filter,
        rows: root.rows.map(function(r) { return { key: r.key, kind: r.kind, title: r.title, hits: (r.hits || []).length } }),
        activated: root.activated, perf: root.perf, setup: root.progress, askBind: root.askBind, progress: root.indexing,
        theme: { background: hex(Commons.Color.menu.background), text: hex(Commons.Color.menu.text) }
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

    Rectangle { anchors.fill: parent; color: Commons.Color.menu.scrim }
    MouseArea { anchors.fill: parent; onClicked: root.close() }

    BorderSurface {
      id: card
      width: Math.min(Style.space(680), panel.width - Style.gapsOut * 2)
      height: column.implicitHeight + contentTopInset + contentBottomInset
      x: Math.round((panel.width - width) / 2)
      y: Math.round(panel.height * 0.18)
      radius: Style.cornerRadius
      color: Commons.Color.menu.background
      borderSpec: Border.surfaceSpec("menu", "border", Commons.Color.menu.border, Math.max(1, Style.space(2)))
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
          leftPadding: Style.spacing.rowPaddingX + Style.space(36) + Style.spacing.xxl  // in line with the rows' text
          rightPadding: Style.spacing.rowPaddingX + (chip.visible ? chip.width + Style.spacing.lg : 0)
          focus: true
          clip: true
          color: Commons.Color.menu.text
          selectionColor: Style.selectionFill
          selectedTextColor: Commons.Color.menu.text
          font.family: root.sans
          font.pixelSize: Style.font.display
          onTextChanged: { root.mode = "text"; root.search(text) }
          onCursorPositionChanged: root.mode = "text"  // typing or moving the cursor takes ←/→ back from the rows
          // Like Spotlight: another window taking the keyboard closes the box.
          onActiveFocusChanged: if (!activeFocus && root.opened) root.close()
          Keys.onPressed: function(event) { if (root.press(event.key, event.modifiers)) event.accepted = true }

          Text {  // Omarchy's mark, where the rows have their icons; dim until something is typed
            x: Style.spacing.rowPaddingX
            width: Style.space(36)
            anchors.verticalCenter: parent.verticalCenter
            horizontalAlignment: Text.AlignHCenter
            visible: Qt.fontFamilies().indexOf("omarchy") >= 0
            text: "\ue900"
            color: Commons.Color.menu.text
            opacity: field.text ? 1 : 0.52
            font.family: "omarchy"  // theme-ok: the logo's own glyph font, not a text style
            font.pixelSize: Style.font.display
          }

          Text {
            x: field.leftPadding
            visible: !field.text
            text: "Search apps, files, photos, answers…"
            color: Commons.Color.menu.text
            opacity: 0.58
            font: field.font
          }

          Rectangle {  // the kinds the query is limited to, e.g. "Photos + Text"
            id: chip
            visible: root.filterLabel !== ""
            anchors.right: parent.right
            anchors.rightMargin: Style.spacing.rowPaddingX
            anchors.verticalCenter: parent.verticalCenter
            width: chipText.implicitWidth + Style.spacing.lg * 2
            height: chipText.implicitHeight + Style.spacing.xs * 2
            radius: Math.min(Style.cornerRadius, height / 2)
            color: Style.selectedAccentFill
            Text {
              id: chipText
              anchors.centerIn: parent
              text: root.filterLabel
              textFormat: Text.PlainText
              color: Commons.Color.menu.text
              font.family: root.sans
              font.pixelSize: Style.font.bodySmall
            }
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
          visible: setupShown || (field.text.trim() !== "" && (!root.connected || (root.empty && !root.rows.length)))
          width: parent.width
          leftPadding: Style.spacing.rowPaddingX
          rightPadding: Style.spacing.rowPaddingX
          text: setupShown ? (p.file ? "Downloading " + p.file + (p.total ? " · " + Math.floor(p.done * 100 / p.total) + "% of " + Math.round(p.total / 1048576) + " MB" : "") : p.message || "")
                : root.connected ? "Nothing on this computer matches" : "Waiting for Super Space (superspace.service)…"
          textFormat: Text.PlainText
          wrapMode: Text.Wrap
          color: p.step === "error" ? Commons.Color.urgent : Commons.Color.menu.text
          opacity: 0.7
          font.family: root.sans
          font.pixelSize: Style.font.title

          Rectangle {  // download progress
            visible: parent.setupShown && parent.p.total > 0
            anchors.top: parent.bottom
            x: parent.leftPadding
            width: (parent.width - parent.leftPadding * 2) * Math.min(1, parent.p.done / parent.p.total)
            height: Style.space(3)
            radius: height / 2
            color: Commons.Color.accent
          }
        }

        Text {
          visible: root.askBind && !field.text
          width: parent.width
          leftPadding: Style.spacing.rowPaddingX
          text: "Make Super+Space open Super Space?    ↵ yes    esc no"
          color: Commons.Color.menu.selectedText
          font.family: root.sans
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

          // Wheel and touchpad move the list as far as they turn, never fling it: a touchpad
          // (it sends scroll phases) by its own pixels, a wheel by the system's scroll lines per
          // notch. Whole notches ease over a few frames; a high-resolution wheel (the VM turns
          // the Mac's trackpad into one) and a touchpad are smooth already, so they move at once.
          WheelHandler {
            enabled: list.interactive
            acceptedDevices: PointerDevice.Mouse | PointerDevice.TouchPad  // Qt on Wayland calls every wheel a touchpad
            onWheel: function(e) {
              var pad = e.phase !== Qt.NoScrollPhase
              var by = pad ? e.pixelDelta.y : e.angleDelta.y / 120 * Qt.styleHints.wheelScrollLines * Style.space(24)
              var to = Math.max(list.originY, Math.min(list.originY + list.contentHeight - list.height, (glide.running ? glide.to : list.contentY) - by))
              glide.stop()
              if (pad || e.angleDelta.y % 120) list.contentY = to
              else { glide.to = to; glide.start() }
            }
          }
          NumberAnimation { id: glide; target: list; property: "contentY"; duration: 140; easing.type: Easing.OutCubic }

          delegate: ResultRow {
            required property var modelData
            required property int index
            width: ListView.view.width
            item: modelData
            // A video's icon from the daemon is its first frame, unreadable at icon size: it wears mpv's, as audio does.
            iconSource: root.iconSource(modelData.kind === "video" && String(modelData.icon).charAt(0) === "/" ? "mpv" : modelData.icon)
            selected: index + root.offset === root.sel
            hit: selected ? root.hit : 0
            expanded: selected && root.mode === "sections"
            notice: root.empty && modelData.kind === "web"
            onClicked: function(pick) {
              if (pick >= 0) { root.sel = index + root.offset; root.hit = pick }  // an emoji, page or section: pick it, then open it
              root.activate(index + root.offset)
            }
            onSeek: function(t) { root.play(modelData, t) }
          }
        }

        Text {  // still indexing: how far, and what is being embedded
          readonly property var p: root.indexing || ({})
          visible: !!root.indexing
          leftPadding: Style.spacing.rowPaddingX
          text: "Indexing · " + Number(p.done).toLocaleString(Qt.locale(), "f", 0) + " of " + Number(p.total).toLocaleString(Qt.locale(), "f", 0) + (p.stage ? " · " + p.stage : "")
          textFormat: Text.PlainText
          color: Commons.Color.menu.text
          opacity: 0.52
          font.family: root.sans
          font.pixelSize: Style.font.caption
        }

        Text {
          width: parent.width
          horizontalAlignment: Text.AlignRight
          rightPadding: Style.spacing.rowPaddingX
          visible: list.count > 0 || !!root.answer
          text: root.mode === "sections" ? "← → step    ↵ open there    shift tab or esc back to typing"
                : root.mode === "rows" && root.parts ? "← → " + root.parts + "    ↵ open there    tab list them    type, home or end to edit    esc close"
                : root.parts ? "↵ open    → " + root.parts + "    ↓ ↑ rows    tab list them    ctrl ↵ reveal    esc close"
                : (root.sel < root.offset ? "↵ copy" : "↵ open    ctrl ↵ reveal")  // an answer copies with or without ctrl
                  + (root.rows.some(function(r) { return (r.hits || []).length > 1 }) ? "    ↓ then ← → pages, sections, emoji    tab list them" : "")
                  + "    ctrl 1–9 row    esc close"
          color: Commons.Color.menu.text
          opacity: 0.52
          font.family: root.sans
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
