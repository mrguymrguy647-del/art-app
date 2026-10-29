import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import ArtApp

// Side panel showing the live readings from a PenTestArea.
Pane {
    id: panel

    // The area whose readings we show. "required" means whoever creates
    // this panel must set it.
    required property PenTestArea area

    // One "name: value" line. Defined once here, used many times below.
    component InfoRow: RowLayout {
        property alias name: nameLabel.text
        property alias value: valueLabel.text

        Layout.fillWidth: true

        Label {
            id: nameLabel
            opacity: 0.7
            Layout.fillWidth: true
        }
        Label {
            id: valueLabel
            font.bold: true
        }
    }

    function yesNo(flag) {
        return flag ? qsTr("Yes") : qsTr("No")
    }

    ColumnLayout {
        anchors.fill: parent
        spacing: 8

        Label {
            text: qsTr("Pen test")
            font.pixelSize: 18
            font.bold: true
        }

        InfoRow { name: qsTr("Input"); value: panel.area.inputName }
        InfoRow { name: qsTr("Touching / pressed"); value: panel.yesNo(panel.area.pressed) }
        InfoRow { name: qsTr("Events per second"); value: panel.area.eventsPerSecond.toString() }

        MenuSeparator { Layout.fillWidth: true }

        InfoRow { name: qsTr("Pressure"); value: panel.area.pressure.toFixed(3) }
        ProgressBar {
            Layout.fillWidth: true
            from: 0
            to: 1
            value: panel.area.pressure
        }

        MenuSeparator { Layout.fillWidth: true }

        InfoRow { name: qsTr("Tilt X"); value: panel.area.tiltX.toFixed(1) + "°" }
        InfoRow { name: qsTr("Tilt Y"); value: panel.area.tiltY.toFixed(1) + "°" }
        InfoRow { name: qsTr("Altitude (90° = upright)"); value: panel.area.altitude.toFixed(1) + "°" }
        InfoRow { name: qsTr("Lean direction"); value: panel.area.azimuth.toFixed(0) + "°" }
        InfoRow { name: qsTr("Barrel rotation"); value: panel.area.penRotation.toFixed(1) + "°" }

        // Picture of the tilt, seen from above: the line points the way
        // the pen leans and gets longer the further it leans.
        Item {
            id: tiltView

            readonly property real lean: (90 - panel.area.altitude) / 90

            Layout.alignment: Qt.AlignHCenter
            Layout.topMargin: 4
            implicitWidth: 120
            implicitHeight: 120

            Rectangle {
                anchors.fill: parent
                radius: width / 2
                color: panel.palette.base
                border.color: panel.palette.light
            }
            Rectangle {
                x: tiltView.width / 2
                y: tiltView.height / 2 - height / 2
                width: tiltView.lean * tiltView.width / 2
                height: 3
                color: panel.palette.highlight
                antialiasing: true
                transformOrigin: Item.Left
                rotation: panel.area.azimuth
            }
            Rectangle {
                anchors.centerIn: parent
                width: 8
                height: 8
                radius: 4
                color: panel.palette.text
            }
        }

        MenuSeparator { Layout.fillWidth: true }

        InfoRow { name: qsTr("Device reports pressure"); value: panel.yesNo(panel.area.reportsPressure) }
        InfoRow { name: qsTr("Device reports tilt"); value: panel.yesNo(panel.area.reportsTilt) }
        Label {
            Layout.fillWidth: true
            text: qsTr("Device: %1").arg(panel.area.deviceName || "-")
            elide: Text.ElideRight
            opacity: 0.7
        }

        Item { Layout.fillHeight: true }  // pushes the tip to the bottom

        Label {
            Layout.fillWidth: true
            wrapMode: Text.WordWrap
            opacity: 0.6
            text: qsTr("Try: press lightly then hard, tilt the pen in each direction, and flip it to the eraser end if it has one.")
        }
    }
}
