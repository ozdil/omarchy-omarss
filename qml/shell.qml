import QtQuick
import Quickshell
import Quickshell.Io
import "theme"

ShellRoot {
    id: root

    FloatingWindow {
        id: win
        title: "OmaRSS - Zero-Trust Feed Reader"
        implicitWidth: 1100
        implicitHeight: 720
        color: Theme.bgBase

        MainWindow {
            id: mainWin
            anchors.fill: parent
        }
    }

    IpcHandler {
        target: "ozdil.omarss"

        function toggle(): bool {
            win.visible = !win.visible;
            return win.visible;
        }

        function show(): bool {
            win.visible = true;
            return true;
        }

        function hide(): bool {
            win.visible = false;
            return false;
        }

        function refresh(): string {
            mainWin.refresh();
            return "OK";
        }
    }
}
