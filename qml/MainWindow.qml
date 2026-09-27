import QtQuick
import QtQuick.Layouts
import QtQuick.Controls
import Quickshell
import Quickshell.Io
import "theme"

Item {
    id: root

    property int unreadCount: 0
    property var articles: []
    property var categories: ["All"]
    property string selectedCategory: "All"
    property bool unreadOnly: false
    property string searchQuery: ""

    readonly property string enginePath: {
        var base = Qt.resolvedUrl(".").toString().replace(/^file:\/\//, "");
        var parent = base.replace(/\/qml\/?$/, "");
        return parent + "/omarss-engine";
    }

    readonly property var displayArticles: {
        if (!root.articles || root.articles.length === 0) return [];
        var list = [];
        var query = root.searchQuery.toLowerCase().trim();

        for (var i = 0; i < root.articles.length; i++) {
            var a = root.articles[i];
            if (!a) continue;

            if (root.unreadOnly && a.is_read) continue;
            if (root.selectedCategory !== "All" && a.category !== root.selectedCategory) continue;

            if (query.length > 0) {
                var titleMatch = (a.title || "").toLowerCase().indexOf(query) !== -1;
                var feedMatch = (a.feed_name || "").toLowerCase().indexOf(query) !== -1;
                var excerptMatch = (a.excerpt || "").toLowerCase().indexOf(query) !== -1;
                if (!titleMatch && !feedMatch && !excerptMatch) continue;
            }

            list.push(a);
        }
        return list;
    }

    function refresh() {
        if (!engineProc.running) {
            engineProc.running = true;
        }
    }

    function markRead(id) {
        actionProc.command = [root.enginePath, "--mark-read", id];
        actionProc.running = true;
    }

    function markAllRead() {
        actionProc.command = [root.enginePath, "--mark-all-read"];
        actionProc.running = true;
    }

    function openUrl(url) {
        Qt.openUrlExternally(url);
    }

    Process {
        id: engineProc
        command: [root.enginePath, "--json"]
        stdout: StdioCollector {
            waitForEnd: true
            onStreamFinished: {
                try {
                    var list = JSON.parse(text || "[]");
                    root.articles = list;

                    var unread = 0;
                    var cats = { "All": true };
                    for (var i = 0; i < list.length; i++) {
                        if (!list[i].is_read) unread++;
                        if (list[i].category) cats[list[i].category] = true;
                    }
                    root.unreadCount = unread;
                    root.categories = Object.keys(cats);
                } catch(e) {
                    console.warn("Failed to parse omarss json:", e);
                }
            }
        }
    }

    Process {
        id: actionProc
        onExited: root.refresh()
    }

    Component.onCompleted: refresh()

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: 16
        spacing: 14

        // Header Bar
        Rectangle {
            Layout.fillWidth: true
            height: 68
            radius: Theme.radiusMd
            color: Theme.bgSurface
            border.color: Theme.border
            border.width: 1

            RowLayout {
                anchors.fill: parent
                anchors.leftMargin: 16
                anchors.rightMargin: 16
                spacing: 12

                Text {
                    text: Theme.iconRss
                    font.family: Theme.iconFont
                    font.pixelSize: 24
                    color: Theme.accent
                }

                ColumnLayout {
                    spacing: 2
                    Text {
                        text: "OMARSS SYNDICATION HUB"
                        font.family: Theme.fontFamily
                        font.pixelSize: 15
                        font.bold: true
                        color: Theme.textMain
                    }
                    Text {
                        text: "Zero-Trust Local Feed Aggregator • " + root.unreadCount + " unread articles"
                        font.family: Theme.fontFamily
                        font.pixelSize: 11
                        color: Theme.textMuted
                    }
                }

                Item { Layout.fillWidth: true }

                // Mark All Read Button
                Rectangle {
                    height: 34
                    implicitWidth: markAllRow.implicitWidth + 20
                    radius: Theme.radiusSm
                    color: markAllArea.containsMouse ? Theme.bgCardHover : Theme.bgCard
                    border.color: Theme.border
                    border.width: 1

                    RowLayout {
                        id: markAllRow
                        anchors.centerIn: parent
                        spacing: 6

                        Text {
                            text: Theme.iconCheckAll
                            font.family: Theme.iconFont
                            font.pixelSize: 12
                            color: Theme.textMain
                        }
                        Text {
                            text: "Mark All Read"
                            font.family: Theme.fontFamily
                            font.pixelSize: 11
                            color: Theme.textMain
                        }
                    }

                    MouseArea {
                        id: markAllArea
                        anchors.fill: parent
                        cursorShape: Qt.PointingHandCursor
                        onClicked: root.markAllRead()
                    }
                }

                // Refresh Button
                Rectangle {
                    width: 36
                    height: 36
                    radius: Theme.radiusSm
                    color: refreshArea.containsMouse ? Theme.bgCardHover : Theme.bgCard
                    border.color: Theme.border
                    border.width: 1

                    Text {
                        anchors.centerIn: parent
                        text: Theme.iconRefresh
                        font.family: Theme.iconFont
                        font.pixelSize: 14
                        color: Theme.textMain
                    }

                    MouseArea {
                        id: refreshArea
                        anchors.fill: parent
                        hoverEnabled: true
                        cursorShape: Qt.PointingHandCursor
                        onClicked: root.refresh()
                    }
                }
            }
        }

        // Main Layout: Category Sidebar + Feed River
        RowLayout {
            Layout.fillWidth: true
            Layout.fillHeight: true
            spacing: 14

            // Left Sidebar: Categories & Filters
            Rectangle {
                Layout.preferredWidth: 260
                Layout.fillHeight: true
                radius: Theme.radiusMd
                color: Theme.bgSurface
                border.color: Theme.border
                border.width: 1

                ColumnLayout {
                    anchors.fill: parent
                    anchors.margins: 14
                    spacing: 10

                    Text {
                        text: "FEED CHANNELS"
                        font.family: Theme.fontFamily
                        font.pixelSize: 11
                        font.bold: true
                        font.letterSpacing: 1.0
                        color: Theme.textMuted
                    }

                    // Unread Only Toggle
                    Rectangle {
                        Layout.fillWidth: true
                        height: 32
                        radius: Theme.radiusSm
                        color: root.unreadOnly ? Theme.bgCardHover : Theme.bgCard
                        border.color: root.unreadOnly ? Theme.accent : Theme.border
                        border.width: 1

                        RowLayout {
                            anchors.centerIn: parent
                            spacing: 8

                            Text {
                                text: Theme.iconFilter
                                font.family: Theme.iconFont
                                font.pixelSize: 11
                                color: root.unreadOnly ? Theme.accentWarning : Theme.textMuted
                            }
                            Text {
                                text: "Unread Only"
                                font.family: Theme.fontFamily
                                font.pixelSize: 11
                                font.bold: root.unreadOnly
                                color: Theme.textMain
                            }
                        }

                        MouseArea {
                            anchors.fill: parent
                            cursorShape: Qt.PointingHandCursor
                            onClicked: root.unreadOnly = !root.unreadOnly
                        }
                    }

                    // Categories List
                    ListView {
                        id: catListView
                        Layout.fillWidth: true
                        Layout.fillHeight: true
                        clip: true
                        spacing: 4
                        model: root.categories

                        delegate: Rectangle {
                            width: catListView.width
                            height: 32
                            radius: Theme.radiusSm
                            color: root.selectedCategory === modelData ? Theme.bgCardHover : "transparent"

                            RowLayout {
                                anchors.fill: parent
                                anchors.leftMargin: 10
                                anchors.rightMargin: 10
                                spacing: 8

                                Text {
                                    text: Theme.iconTag
                                    font.family: Theme.iconFont
                                    font.pixelSize: 10
                                    color: root.selectedCategory === modelData ? Theme.accent : Theme.textMuted
                                }

                                Text {
                                    Layout.fillWidth: true
                                    text: modelData
                                    font.family: Theme.fontFamily
                                    font.pixelSize: 11
                                    font.bold: root.selectedCategory === modelData
                                    color: root.selectedCategory === modelData ? Theme.textMain : Theme.textMuted
                                    elide: Text.ElideRight
                                }
                            }

                            MouseArea {
                                anchors.fill: parent
                                cursorShape: Qt.PointingHandCursor
                                onClicked: root.selectedCategory = modelData
                            }
                        }
                    }
                }
            }

            // Right Area: Search & Article River
            ColumnLayout {
                Layout.fillWidth: true
                Layout.fillHeight: true
                spacing: 10

                // Search Bar
                Rectangle {
                    Layout.fillWidth: true
                    height: 34
                    radius: Theme.radiusSm
                    color: Theme.bgSurface
                    border.color: rssSearchInput.activeFocus ? Theme.borderLight : Theme.border
                    border.width: 1

                    RowLayout {
                        anchors.fill: parent
                        anchors.leftMargin: 10
                        anchors.rightMargin: 10
                        spacing: 8

                        Text {
                            text: Theme.iconSearch
                            font.family: Theme.iconFont
                            font.pixelSize: 13
                            color: Theme.textMuted
                        }

                        TextInput {
                            id: rssSearchInput
                            Layout.fillWidth: true
                            font.family: Theme.fontFamily
                            font.pixelSize: 12
                            color: Theme.textMain
                            clip: true
                            onTextChanged: root.searchQuery = text

                            Text {
                                anchors.fill: parent
                                text: "Search articles across all subscribed channels..."
                                font.family: Theme.fontFamily
                                font.pixelSize: 12
                                color: Theme.textDim
                                visible: !rssSearchInput.text && !rssSearchInput.activeFocus
                            }
                        }
                    }
                }

                // Articles River
                Rectangle {
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    radius: Theme.radiusMd
                    color: Theme.bgSurface
                    border.color: Theme.border
                    border.width: 1
                    clip: true

                    ListView {
                        id: rssListView
                        anchors.fill: parent
                        anchors.margins: 10
                        spacing: 8
                        model: root.displayArticles

                        delegate: Rectangle {
                            width: rssListView.width
                            implicitHeight: articleBox.implicitHeight + 24
                            radius: Theme.radiusSm
                            color: modelData.is_read ? Theme.bgCard : Theme.bgCardHover
                            border.color: modelData.is_read ? Theme.border : Theme.borderLight
                            border.width: 1

                            ColumnLayout {
                                id: articleBox
                                anchors.fill: parent
                                anchors.margins: 12
                                spacing: 8

                                RowLayout {
                                    Layout.fillWidth: true
                                    spacing: 8

                                    // Feed Name Badge
                                    Rectangle {
                                        height: 20
                                        implicitWidth: feedBadgeText.implicitWidth + 12
                                        radius: 3
                                        color: Theme.bgDark
                                        border.color: Theme.border
                                        border.width: 1

                                        Text {
                                            id: feedBadgeText
                                            anchors.centerIn: parent
                                            text: modelData.feed_name || "Feed"
                                            font.family: Theme.monoFont
                                            font.pixelSize: 10
                                            color: Theme.accent
                                        }
                                    }

                                    // Title
                                    Text {
                                        Layout.fillWidth: true
                                        text: modelData.title || ""
                                        font.family: Theme.fontFamily
                                        font.pixelSize: 13
                                        font.bold: !modelData.is_read
                                        color: Theme.textMain
                                        elide: Text.ElideRight
                                    }

                                    // Date
                                    Text {
                                        text: modelData.date || ""
                                        font.family: Theme.monoFont
                                        font.pixelSize: 11
                                        color: Theme.textMuted
                                    }

                                    // Mark Read Action
                                    Rectangle {
                                        width: 28
                                        height: 28
                                        radius: Theme.radiusSm
                                        visible: !modelData.is_read
                                        color: markArea.containsMouse ? Theme.bgCardHover : Theme.bgDark
                                        border.color: Theme.border
                                        border.width: 1

                                        Text {
                                            anchors.centerIn: parent
                                            text: Theme.iconCheck
                                            font.family: Theme.iconFont
                                            font.pixelSize: 11
                                            color: Theme.accentSuccess
                                        }

                                        MouseArea {
                                            id: markArea
                                            anchors.fill: parent
                                            hoverEnabled: true
                                            cursorShape: Qt.PointingHandCursor
                                            onClicked: root.markRead(modelData.id)
                                        }
                                    }

                                    // Open in Browser Action
                                    Rectangle {
                                        width: 28
                                        height: 28
                                        radius: Theme.radiusSm
                                        color: openArea.containsMouse ? Theme.bgCardHover : Theme.bgDark
                                        border.color: Theme.border
                                        border.width: 1

                                        Text {
                                            anchors.centerIn: parent
                                            text: Theme.iconExternal
                                            font.family: Theme.iconFont
                                            font.pixelSize: 11
                                            color: Theme.textMain
                                        }

                                        MouseArea {
                                            id: openArea
                                            anchors.fill: parent
                                            hoverEnabled: true
                                            cursorShape: Qt.PointingHandCursor
                                            onClicked: {
                                                root.markRead(modelData.id);
                                                root.openUrl(modelData.link);
                                            }
                                        }
                                    }
                                }

                                // Excerpt
                                Text {
                                    Layout.fillWidth: true
                                    text: modelData.excerpt || ""
                                    font.family: Theme.fontFamily
                                    font.pixelSize: 11
                                    color: Theme.textMuted
                                    wrapMode: Text.WordWrap
                                    maximumLineCount: 2
                                    elide: Text.ElideRight
                                }
                            }
                        }

                        Text {
                            anchors.centerIn: parent
                            visible: root.displayArticles.length === 0
                            text: "No articles found in this view."
                            font.family: Theme.fontFamily
                            font.pixelSize: 13
                            color: Theme.textMuted
                        }
                    }
                }
            }
        }
    }
}
