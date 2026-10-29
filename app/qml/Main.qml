import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import ArtApp

ApplicationWindow {
    id: window

    width: 1280
    height: 800
    visible: true
    title: qsTr("Art App %1").arg(Qt.application.version)

    // A dark grey theme, like most painting apps, so the colours you paint
    // with stand out. The Fusion style (chosen in main.cpp) uses these.
    palette {
        window: "#2b2b2b"
        windowText: "#e0e0e0"
        base: "#1f1f1f"
        alternateBase: "#303030"
        text: "#e0e0e0"
        button: "#3a3a3a"
        buttonText: "#e0e0e0"
        highlight: "#3d7eff"
        highlightedText: "#ffffff"
        placeholderText: "#808080"
        light: "#505050"
        midlight: "#444444"
        mid: "#2f2f2f"
        dark: "#1a1a1a"
        shadow: "#000000"
    }

    header: ToolBar {
        RowLayout {
            anchors.fill: parent
            anchors.leftMargin: 12
            anchors.rightMargin: 12

            Label {
                text: qsTr("Art App")
                font.bold: true
            }
            Item { Layout.fillWidth: true }
            Label {
                text: qsTr("Step 1: pen test")
                opacity: 0.7
            }
        }
    }

    RowLayout {
        anchors.fill: parent
        spacing: 0

        // The big area where the canvas will go. For now it is a pen test area.
        PenTestArea {
            id: penArea

            Layout.fillWidth: true
            Layout.fillHeight: true
            clip: true

            Rectangle {
                anchors.fill: parent
                color: "#404040"
            }

            Label {
                anchors.centerIn: parent
                visible: !penArea.active
                text: qsTr("Move your pen, mouse or finger here")
                font.pixelSize: 20
                opacity: 0.6
            }

            // A ring that follows the pen. It grows when you press harder
            // and fills in while the pen is touching.
            Rectangle {
                visible: penArea.hovering
                width: 12 + penArea.pressure * 60
                height: width
                radius: width / 2
                x: penArea.penPosition.x - width / 2
                y: penArea.penPosition.y - height / 2
                color: penArea.pressed ? Qt.rgba(0.24, 0.49, 1.0, 0.5) : "transparent"
                border.color: "white"
                border.width: 2
            }
        }

        PenInfoPanel {
            area: penArea
            Layout.preferredWidth: 320
            Layout.fillHeight: true
        }
    }
}
