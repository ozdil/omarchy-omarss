import QtQuick
import QtQuick.Layouts
import QtQuick.Controls
import Quickshell
import Quickshell.Io
import qs.Commons
import qs.Ui

Panel {
  id: root
  moduleName: "ozdil.omarss"
  ipcTarget: "ozdil.omarss"
  manageIpc: false

  implicitWidth: button.implicitWidth
  implicitHeight: button.implicitHeight

  property int totalArticles: 0
  property int unreadArticles: 0
  property int totalFeeds: 0
  property var articles: []
  property var feeds: []
  property int activeTab: 0 // 0: Unread, 1: All, 2: Feeds
  property string selectedRegion: "all" // "all", "tr", "global"
  property bool isRefreshing: false

  function resolveEnginePath() {
    return Qt.resolvedUrl("omarss-engine").toString().replace(/^file:\/\//, "")
  }

  function sendCmd(arg, param1, param2) {
    var eng = root.resolveEnginePath()
    if (param2 !== undefined && param2 !== "") {
      actionProc.command = [eng, arg, param1, param2]
    } else if (param1 !== undefined && param1 !== "") {
      actionProc.command = [eng, arg, param1]
    } else {
      actionProc.command = [eng, arg]
    }
    actionProc.running = true
  }

  function refreshFeeds() {
    root.isRefreshing = true
    root.sendCmd("--refresh")
  }

  function markAllAsRead() {
    root.sendCmd("--mark-all-read")
  }

  function isLinuxArticle(art) {
    if (!art) return false
    var cat = String(art.category || "")
    var name = String(art.feed_name || "")
    return cat.indexOf("Linux") !== -1 || cat.indexOf("Kernel") !== -1 || cat.indexOf("Distro") !== -1 || name === "Phoronix" || name === "LWN.net" || name === "It's FOSS" || name === "OMG! Linux" || name === "Linux Today" || name === "DistroWatch" || name === "Linux Magazine" || name === "Arch Linux"
  }

  function isGamingArticle(art) {
    if (!art) return false
    var cat = String(art.category || "")
    var name = String(art.feed_name || "")
    return cat.indexOf("Gaming") !== -1 || cat.indexOf("Steam") !== -1 || cat.indexOf("Deck") !== -1 || cat.indexOf("Epic") !== -1 || name === "GamingOnLinux" || name === "Steam Official" || name === "Steam Deck HQ" || name === "Boiling Steam" || name === "Epic Games PC"
  }

  function isTrArticle(art) {
    if (!art) return false
    var cat = String(art.category || "")
    var name = String(art.feed_name || "")
    return cat.indexOf("TR") !== -1 || cat.indexOf("Türkiye") !== -1 || name === "Webrazzi" || name === "ShiftDelete" || name === "DonanımHaber" || name === "LOG" || name === "Webtekno" || name === "Evrim Ağacı"
  }

  function isGlobalArticle(art) {
    return !root.isTrArticle(art) && !root.isLinuxArticle(art) && !root.isGamingArticle(art)
  }

  function getCountForRegion(reg) {
    if (!root.articles) return 0
    var list = root.articles
    if (root.activeTab === 0) {
      list = list.filter(function(a) { return !a.is_read })
    }
    if (reg === "linux") {
      return list.filter(function(a) { return root.isLinuxArticle(a) }).length
    } else if (reg === "gaming") {
      return list.filter(function(a) { return root.isGamingArticle(a) }).length
    } else if (reg === "tr") {
      return list.filter(function(a) { return root.isTrArticle(a) }).length
    } else if (reg === "global") {
      return list.filter(function(a) { return root.isGlobalArticle(a) }).length
    }
    return list.length
  }

  function getFilteredArticles() {
    if (!root.articles) return []
    var list = root.articles
    if (root.activeTab === 0) {
      list = list.filter(function(a) { return !a.is_read })
    }
    if (root.selectedRegion === "linux") {
      list = list.filter(function(a) { return root.isLinuxArticle(a) })
    } else if (root.selectedRegion === "gaming") {
      list = list.filter(function(a) { return root.isGamingArticle(a) })
    } else if (root.selectedRegion === "tr") {
      list = list.filter(function(a) { return root.isTrArticle(a) })
    } else if (root.selectedRegion === "global") {
      list = list.filter(function(a) { return root.isGlobalArticle(a) })
    }
    return list
  }

  IpcHandler {
    target: "ozdil.omarss"
    function open() { root.open() }
    function close() { root.close() }
    function toggle() { root.toggle() }
    function refresh() { root.refreshFeeds() }
    function markRead() { root.markAllAsRead() }
  }

  Process {
    id: engineProc
    command: [root.resolveEnginePath(), "--json"]
    stdout: StdioCollector {
      waitForEnd: true
      onStreamFinished: {
        try {
          var raw = String(text || "").slice(0, 524288)
          var data = JSON.parse(raw)
          root.totalArticles = data.total_articles || 0
          root.unreadArticles = data.unread_articles || 0
          root.totalFeeds = data.total_feeds || 0
          root.articles = data.articles || []
          root.feeds = data.feeds || []
          root.isRefreshing = false
        } catch (e) {
          root.isRefreshing = false
        }
      }
    }
  }

  Process {
    id: actionProc
    onExited: function(exitCode) {
      engineProc.running = true
    }
  }

  Timer {
    interval: 60000
    running: true
    repeat: true
    triggeredOnStart: true
    onTriggered: {
      if (!engineProc.running) engineProc.running = true
    }
  }

  Component.onCompleted: {
    if (!engineProc.running) engineProc.running = true
  }
  Component.onDestruction: {
    if (engineProc.running) engineProc.running = false
    if (actionProc.running) actionProc.running = false
  }

  BarIconButton {
    id: button
    anchors.fill: parent
    bar: root.bar
    text: ""
    useActiveColor: false
    foreground: root.unreadArticles > 0 ? "#22c55e" : (root.bar ? root.bar.foreground : Color.foreground)
    tooltipText: "OmaRSS Feed Reader" + (root.unreadArticles > 0 ? ("\n" + root.unreadArticles + " unread article" + (root.unreadArticles > 1 ? "s" : "")) : "\nAll feeds caught up")
    onPressed: function(b) {
      root.toggle()
    }
  }

  KeyboardPanel {
    id: panel
    anchorItem: button
    owner: root
    bar: root.bar
    open: root.opened
    contentWidth: panel.fittedContentWidth(Style.space(480))
    contentHeight: panel.fittedContentHeight(panelColumn.implicitHeight, Style.space(620))

    ScrollView {
      id: scrollArea
      anchors.fill: parent
      clip: true
      ScrollBar.horizontal.policy: ScrollBar.AlwaysOff
      ScrollBar.vertical.policy: panelColumn.implicitHeight > height ? ScrollBar.AsNeeded : ScrollBar.AlwaysOff

      Column {
        id: panelColumn
        width: scrollArea.availableWidth
        spacing: Style.space(12)

        // ---------- Hero Section ----------
        Item {
          width: parent.width
          implicitHeight: Math.max(heroIcon.implicitHeight, heroLabels.implicitHeight, heroActions.implicitHeight)

          Text {
            id: heroIcon
            textFormat: Text.PlainText
            text: ""
            color: root.unreadArticles > 0 ? "#22c55e" : (root.bar ? root.bar.foreground : Color.foreground)
            font.family: root.bar ? root.bar.fontFamily : Style.font.family
            font.pixelSize: Style.font.display
            anchors.left: parent.left
            anchors.verticalCenter: parent.verticalCenter
          }

          Column {
            id: heroLabels
            anchors.left: heroIcon.right
            anchors.leftMargin: Style.space(12)
            anchors.right: heroActions.left
            anchors.rightMargin: Style.space(8)
            anchors.verticalCenter: parent.verticalCenter
            spacing: Style.space(2)

            Text {
              textFormat: Text.PlainText
              text: "OmaRSS"
              color: root.bar ? root.bar.foreground : Color.foreground
              font.family: root.bar ? root.bar.fontFamily : Style.font.family
              font.pixelSize: Style.font.title
              font.bold: true
            }

            Text {
              textFormat: Text.PlainText
              text: (root.isRefreshing ? "REFRESHING FEEDS..." : (root.unreadArticles > 0 ? (root.unreadArticles + " UNREAD ARTICLES") : "ALL FEEDS CAUGHT UP")).toUpperCase()
              color: root.unreadArticles > 0 ? "#22c55e" : Qt.darker(root.bar ? root.bar.foreground : Color.foreground, 1.4)
              font.family: root.bar ? root.bar.fontFamily : Style.font.family
              font.pixelSize: Style.font.caption
              font.bold: true
              font.letterSpacing: 1.1
            }
          }

          Row {
            id: heroActions
            anchors.right: parent.right
            anchors.verticalCenter: parent.verticalCenter
            spacing: Style.space(6)

            PanelActionButton {
              iconText: ""
              tooltipText: "Refresh Feeds"
              onClicked: root.refreshFeeds()
            }

            PanelActionButton {
              iconText: "✓"
              tooltipText: "Mark All Read"
              onClicked: root.markAllAsRead()
            }
          }
        }

        // ---------- Tabs Navigation ----------
        RowLayout {
          width: parent.width
          spacing: Style.space(6)

          BorderSurface {
            Layout.fillWidth: true
            implicitHeight: Style.space(32)
            radius: Style.cornerRadius
            color: root.activeTab === 0 ? Color.accent : Style.controlFill(false, mouseTab0.containsMouse, Color.foreground, Color.accent)
            borderSpec: Border.controlSpec(root.activeTab === 0 ? "active" : "normal", Color.foreground, Color.accent)

            MouseArea {
              id: mouseTab0
              anchors.fill: parent
              hoverEnabled: true
              cursorShape: Qt.PointingHandCursor
              onClicked: root.activeTab = 0
            }

            Row {
              anchors.centerIn: parent
              spacing: Style.space(6)

              Text {
                textFormat: Text.PlainText
                text: "Unread"
                color: root.activeTab === 0 ? Color.popups.background : Color.foreground
                font.family: root.bar ? root.bar.fontFamily : Style.font.family
                font.pixelSize: Style.font.bodySmall
                font.bold: root.activeTab === 0
                anchors.verticalCenter: parent.verticalCenter
              }

              Rectangle {
                width: badgeText0.implicitWidth + Style.space(8)
                height: Style.space(16)
                radius: Style.cornerRadius
                color: root.activeTab === 0 ? Qt.rgba(0,0,0,0.2) : Style.selectedFillFor(root.bar ? root.bar.foreground : Color.foreground, Color.accent)
                border.color: root.activeTab === 0 ? "transparent" : Qt.darker(root.bar ? root.bar.foreground : Color.foreground, 1.4)
                border.width: 1
                anchors.verticalCenter: parent.verticalCenter

                Text {
                  id: badgeText0
                  anchors.centerIn: parent
                  textFormat: Text.PlainText
                  text: String(root.unreadArticles)
                  color: root.activeTab === 0 ? (root.bar ? root.bar.background : Color.background) : (root.bar ? root.bar.foreground : Color.foreground)
                  font.family: root.bar ? root.bar.fontFamily : Style.font.family
                  font.pixelSize: Style.font.caption - 1
                  font.bold: true
                }
              }
            }
          }

          BorderSurface {
            Layout.fillWidth: true
            implicitHeight: Style.space(32)
            radius: Style.cornerRadius
            color: root.activeTab === 1 ? Color.accent : Style.controlFill(false, mouseTab1.containsMouse, Color.foreground, Color.accent)
            borderSpec: Border.controlSpec(root.activeTab === 1 ? "active" : "normal", Color.foreground, Color.accent)

            MouseArea {
              id: mouseTab1
              anchors.fill: parent
              hoverEnabled: true
              cursorShape: Qt.PointingHandCursor
              onClicked: root.activeTab = 1
            }

            Row {
              anchors.centerIn: parent
              spacing: Style.space(6)

              Text {
                textFormat: Text.PlainText
                text: "All"
                color: root.activeTab === 1 ? Color.popups.background : Color.foreground
                font.family: root.bar ? root.bar.fontFamily : Style.font.family
                font.pixelSize: Style.font.bodySmall
                font.bold: root.activeTab === 1
                anchors.verticalCenter: parent.verticalCenter
              }

              Text {
                textFormat: Text.PlainText
                text: "(" + root.totalArticles + ")"
                color: root.activeTab === 1 ? Color.popups.background : Color.muted
                font.family: root.bar ? root.bar.fontFamily : Style.font.family
                font.pixelSize: Style.font.caption
                anchors.verticalCenter: parent.verticalCenter
              }
            }
          }

          BorderSurface {
            Layout.fillWidth: true
            implicitHeight: Style.space(32)
            radius: Style.cornerRadius
            color: root.activeTab === 2 ? Color.accent : Style.controlFill(false, mouseTab2.containsMouse, Color.foreground, Color.accent)
            borderSpec: Border.controlSpec(root.activeTab === 2 ? "active" : "normal", Color.foreground, Color.accent)

            MouseArea {
              id: mouseTab2
              anchors.fill: parent
              hoverEnabled: true
              cursorShape: Qt.PointingHandCursor
              onClicked: root.activeTab = 2
            }

            Row {
              anchors.centerIn: parent
              spacing: Style.space(6)

              Text {
                textFormat: Text.PlainText
                text: "Feeds"
                color: root.activeTab === 2 ? Color.popups.background : Color.foreground
                font.family: root.bar ? root.bar.fontFamily : Style.font.family
                font.pixelSize: Style.font.bodySmall
                font.bold: root.activeTab === 2
                anchors.verticalCenter: parent.verticalCenter
              }

              Text {
                textFormat: Text.PlainText
                text: "(" + root.totalFeeds + ")"
                color: root.activeTab === 2 ? Color.popups.background : Color.muted
                font.family: root.bar ? root.bar.fontFamily : Style.font.family
                font.pixelSize: Style.font.caption
                anchors.verticalCenter: parent.verticalCenter
              }
            }
          }
        }

        PanelSeparator {
          foreground: root.bar ? root.bar.foreground : Color.foreground
        }

        // ---------- Articles View (Tab 0 & 1) ----------
        Column {
          width: parent.width
          spacing: Style.space(8)
          visible: root.activeTab === 0 || root.activeTab === 1

          // Region Filter Pill Bar (Tümü | Linux | Gaming | Türkiye | Dünya)
          RowLayout {
            width: parent.width
            spacing: Style.space(4)

            Repeater {
              model: [
                { id: "all", label: "Tümü", count: root.getCountForRegion("all") },
                { id: "linux", label: "Linux", count: root.getCountForRegion("linux") },
                { id: "gaming", label: "Gaming", count: root.getCountForRegion("gaming") },
                { id: "tr", label: "Türkiye", count: root.getCountForRegion("tr") },
                { id: "global", label: "Dünya", count: root.getCountForRegion("global") }
              ]

              delegate: BorderSurface {
                id: pillSurface
                Layout.fillWidth: true
                implicitHeight: Style.space(26)
                radius: Style.cornerRadius
                readonly property bool active: root.selectedRegion === modelData.id
                color: active ? Style.selectedFillFor(root.bar ? root.bar.foreground : Color.foreground, Color.accent) : Style.controlFill(false, pillMouse.containsMouse, Color.foreground, Color.accent)
                borderSpec: Border.controlSpec(active ? "active" : "normal", Color.foreground, Color.accent)

                MouseArea {
                  id: pillMouse
                  anchors.fill: parent
                  hoverEnabled: true
                  cursorShape: Qt.PointingHandCursor
                  onClicked: root.selectedRegion = modelData.id
                }

                Row {
                  anchors.centerIn: parent
                  spacing: Style.space(4)

                  Text {
                    textFormat: Text.PlainText
                    text: modelData.label
                    color: pillSurface.active ? Color.accent : Color.foreground
                    font.family: root.bar ? root.bar.fontFamily : Style.font.family
                    font.pixelSize: Style.font.caption
                    font.bold: pillSurface.active
                    anchors.verticalCenter: parent.verticalCenter
                  }

                  Text {
                    textFormat: Text.PlainText
                    text: "(" + modelData.count + ")"
                    color: pillSurface.active ? Color.accent : Color.muted
                    font.family: root.bar ? root.bar.fontFamily : Style.font.family
                    font.pixelSize: Style.font.caption - 1
                    anchors.verticalCenter: parent.verticalCenter
                  }
                }
              }
            }
          }

          // Empty State
          BorderSurface {
            width: parent.width
            implicitHeight: Style.space(110)
            radius: Style.cornerRadius
            color: Style.controlFill(false, false, Color.foreground, Color.accent)
            borderSpec: Border.controlSpec("normal", Color.foreground, Color.accent)
            visible: root.getFilteredArticles().length === 0

            Column {
              anchors.centerIn: parent
              spacing: Style.space(6)

              Text {
                anchors.horizontalCenter: parent.horizontalCenter
                textFormat: Text.PlainText
                text: "✓"
                color: "#22c55e"
                font.pixelSize: Style.font.display
              }

              Text {
                anchors.horizontalCenter: parent.horizontalCenter
                textFormat: Text.PlainText
                text: root.activeTab === 0 ? "You're all caught up!" : "No articles found."
                color: Color.foreground
                font.family: root.bar ? root.bar.fontFamily : Style.font.family
                font.pixelSize: Style.font.body
                font.bold: true
              }

              Text {
                anchors.horizontalCenter: parent.horizontalCenter
                textFormat: Text.PlainText
                text: root.activeTab === 0 ? "All feeds have been read." : "Add RSS feeds in the Feeds tab."
                color: Color.muted
                font.family: root.bar ? root.bar.fontFamily : Style.font.family
                font.pixelSize: Style.font.caption
              }
            }
          }

          // Articles Repeater
          Repeater {
            model: root.getFilteredArticles().slice(0, 80)
            delegate: BorderSurface {
              id: artCard
              width: parent.width
              implicitHeight: cardContent.implicitHeight + Style.space(16)
              radius: Style.cornerRadius
              color: Style.controlFill(false, mouseArt.containsMouse, Color.foreground, Color.accent)
              borderSpec: Border.controlSpec(mouseArt.containsMouse ? "hover-cursor" : "normal", Color.foreground, Color.accent)

              readonly property var art: modelData

              MouseArea {
                id: mouseArt
                anchors.fill: parent
                hoverEnabled: true
                cursorShape: Qt.PointingHandCursor
                onClicked: {
                  if (art && art.link) {
                    root.sendCmd("--open-url", art.link, art.id)
                  }
                }
              }

              Column {
                id: cardContent
                anchors.left: parent.left
                anchors.right: parent.right
                anchors.top: parent.top
                anchors.margins: Style.space(8)
                spacing: Style.space(4)

                // Meta row: Feed badge, date, read button
                RowLayout {
                  width: parent.width
                  spacing: Style.space(6)

                  Rectangle {
                    implicitHeight: Style.space(18)
                    implicitWidth: feedBadgeText.implicitWidth + Style.space(10)
                    radius: Style.cornerRadius
                    color: Style.selectedFillFor(root.bar ? root.bar.foreground : Color.foreground, Color.accent)

                    Text {
                      id: feedBadgeText
                      anchors.centerIn: parent
                      textFormat: Text.PlainText
                      text: art ? String(art.feed_name) : ""
                      color: Color.accent
                      font.family: root.bar ? root.bar.fontFamily : Style.font.family
                      font.pixelSize: Style.font.caption - 1
                      font.bold: true
                    }
                  }

                  Rectangle {
                    implicitHeight: Style.space(18)
                    implicitWidth: artCatText.implicitWidth + Style.space(8)
                    radius: Style.cornerRadius
                    color: Qt.rgba(0, 0, 0, 0.08)
                    visible: art && art.category && art.category.length > 0

                    Text {
                      id: artCatText
                      anchors.centerIn: parent
                      textFormat: Text.PlainText
                      text: art ? String(art.category) : ""
                      color: Color.muted
                      font.family: root.bar ? root.bar.fontFamily : Style.font.family
                      font.pixelSize: Style.font.caption - 2
                    }
                  }

                  Text {
                    textFormat: Text.PlainText
                    text: art ? String(art.date) : ""
                    color: Color.muted
                    font.family: root.bar ? root.bar.fontFamily : Style.font.family
                    font.pixelSize: Style.font.caption
                  }

                  Item { Layout.fillWidth: true }

                  // Read toggle / badge button
                  Rectangle {
                    implicitHeight: Style.space(18)
                    implicitWidth: statusText.implicitWidth + Style.space(8)
                    radius: Style.cornerRadius
                    color: art && art.is_read ? Qt.rgba(0,0,0,0.1) : Style.selectedFillFor(root.bar ? root.bar.foreground : Color.foreground, Color.accent)
                    border.color: art && art.is_read ? "transparent" : Qt.darker(root.bar ? root.bar.foreground : Color.foreground, 1.4)
                    border.width: 1

                    Text {
                      id: statusText
                      anchors.centerIn: parent
                      textFormat: Text.PlainText
                      text: art && art.is_read ? "READ" : "● NEW"
                      color: art && art.is_read ? Color.muted : (root.bar ? root.bar.foreground : Color.foreground)
                      font.family: root.bar ? root.bar.fontFamily : Style.font.family
                      font.pixelSize: Style.font.caption - 1
                      font.bold: true
                    }

                    MouseArea {
                      anchors.fill: parent
                      cursorShape: Qt.PointingHandCursor
                      onClicked: {
                        if (art && art.id) {
                          root.sendCmd("--toggle-read", art.id)
                        }
                      }
                    }
                  }
                }

                // Article Title
                Text {
                  width: parent.width
                  textFormat: Text.PlainText
                  text: art ? String(art.title) : ""
                  color: Color.foreground
                  font.family: root.bar ? root.bar.fontFamily : Style.font.family
                  font.pixelSize: Style.font.bodySmall
                  font.bold: art ? !art.is_read : false
                  wrapMode: Text.Wrap
                  maximumLineCount: 2
                  elide: Text.ElideRight
                }

                // Excerpt
                Text {
                  width: parent.width
                  textFormat: Text.PlainText
                  text: art ? String(art.excerpt) : ""
                  color: Color.muted
                  font.family: root.bar ? root.bar.fontFamily : Style.font.family
                  font.pixelSize: Style.font.caption
                  wrapMode: Text.Wrap
                  maximumLineCount: 2
                  elide: Text.ElideRight
                  visible: text.length > 0
                }
              }
            }
          }
        }

        // ---------- Feeds View (Tab 2) ----------
        Column {
          width: parent.width
          spacing: Style.space(10)
          visible: root.activeTab === 2

          PanelSectionHeader {
            text: "ADD NEW FEED"
            foreground: root.bar ? root.bar.foreground : Color.foreground
            fontFamily: root.bar ? root.bar.fontFamily : Style.font.family
          }

          RowLayout {
            width: parent.width
            spacing: Style.space(8)

            TextField {
              id: newFeedInput
              Layout.fillWidth: true
              implicitHeight: Style.space(34)
              placeholderText: "https://example.com/feed.xml"
              onAccepted: {
                if (newFeedInput.text.trim()) {
                  root.sendCmd("--add-feed", newFeedInput.text.trim())
                  newFeedInput.text = ""
                }
              }
            }

            Button {
              implicitHeight: Style.space(34)
              implicitWidth: Style.space(60)
              text: "Add"
              onClicked: {
                if (newFeedInput.text.trim()) {
                  root.sendCmd("--add-feed", newFeedInput.text.trim())
                  newFeedInput.text = ""
                }
              }
            }
          }

          RowLayout {
            width: parent.width

            PanelSectionHeader {
              text: "SUBSCRIBED FEEDS"
              foreground: root.bar ? root.bar.foreground : Color.foreground
              fontFamily: root.bar ? root.bar.fontFamily : Style.font.family
              Layout.fillWidth: true
            }

            PanelActionButton {
              iconText: "↺"
              tooltipText: "Reset All Feeds to Defaults"
              onClicked: root.sendCmd("--reset-feeds")
            }
          }

          Repeater {
            model: root.feeds
            delegate: BorderSurface {
              width: parent.width
              implicitHeight: Style.space(52)
              radius: Style.cornerRadius
              color: Style.controlFill(false, mouseFeed.containsMouse, Color.foreground, Color.accent)
              borderSpec: Border.controlSpec(mouseFeed.containsMouse ? "hover-cursor" : "normal", Color.foreground, Color.accent)

              readonly property var feedItem: modelData

              MouseArea {
                id: mouseFeed
                anchors.fill: parent
                hoverEnabled: true
              }

              RowLayout {
                anchors.fill: parent
                anchors.margins: Style.space(8)
                spacing: Style.space(10)

                Text {
                  textFormat: Text.PlainText
                  text: feedItem ? String(feedItem.icon || "") : ""
                  font.family: root.bar ? root.bar.fontFamily : Style.font.family
                  font.pixelSize: Style.font.title
                  color: feedItem && feedItem.enabled ? Color.accent : Color.muted
                  Layout.preferredWidth: Style.space(24)
                  horizontalAlignment: Text.AlignHCenter
                }

                Column {
                  Layout.fillWidth: true
                  spacing: Style.space(2)

                  Row {
                    spacing: Style.space(6)
                    Text {
                      textFormat: Text.PlainText
                      text: feedItem ? String(feedItem.name) : ""
                      color: Color.foreground
                      font.family: root.bar ? root.bar.fontFamily : Style.font.family
                      font.pixelSize: Style.font.bodySmall
                      font.bold: true
                    }

                    Rectangle {
                      implicitHeight: Style.space(16)
                      implicitWidth: catText.implicitWidth + Style.space(6)
                      radius: Style.cornerRadius
                      color: Style.selectedFillFor(root.bar ? root.bar.foreground : Color.foreground, Color.accent)
                      anchors.verticalCenter: parent.verticalCenter

                      Text {
                        id: catText
                        anchors.centerIn: parent
                        textFormat: Text.PlainText
                        text: feedItem ? String(feedItem.category) : ""
                        color: Color.muted
                        font.family: root.bar ? root.bar.fontFamily : Style.font.family
                        font.pixelSize: Style.font.caption - 2
                        font.bold: true
                      }
                    }
                  }

                  Text {
                    width: parent.width
                    textFormat: Text.PlainText
                    text: feedItem ? String(feedItem.url) : ""
                    color: Color.muted
                    font.family: root.bar ? root.bar.fontFamily : Style.font.family
                    font.pixelSize: Style.font.caption - 1
                    elide: Text.ElideMiddle
                  }
                }

                ToggleSwitch {
                  checked: feedItem ? feedItem.enabled : false
                  accent: Color.accent
                  onToggled: {
                    if (feedItem && feedItem.url) {
                      root.sendCmd("--toggle-feed", feedItem.url)
                    }
                  }
                }

                PanelActionButton {
                  iconText: "✕"
                  tooltipText: "Unsubscribe Feed"
                  hoverColor: Color.urgent
                  onClicked: {
                    if (feedItem && feedItem.url) {
                      root.sendCmd("--remove-feed", feedItem.url)
                    }
                  }
                }
              }
            }
          }
        }
      }
    }
  }
}
