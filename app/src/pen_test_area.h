#pragma once

#include <artengine/pen_sample.h>

#include <QPointF>
#include <QPointer>
#include <QQuickItem>
#include <QString>
#include <QTimer>
#include <QtQml/qqmlregistration.h>

class QMouseEvent;
class QPointingDevice;
class QTabletEvent;
class QTouchEvent;

/// A test area that shows what your pen, mouse or finger reports.
///
/// It watches the window's input events, keeps the ones that land on this
/// item, turns them into an engine art::PenSample and offers the values to
/// QML as properties. The canvas we build next will read input the same way.
///
/// Why watch the whole window? Qt Quick items have no tabletEvent() function
/// (unlike Qt Widgets), and QML input handlers offer pressure but not tilt.
/// Watching the window (an "event filter") lets us see the full tablet
/// event, including tilt, before Qt Quick processes it.
class PenTestArea : public QQuickItem
{
    Q_OBJECT
    QML_ELEMENT

    // Everything below updates together, so one signal (sampleChanged) is enough.
    Q_PROPERTY(bool active READ isActive NOTIFY sampleChanged)
    Q_PROPERTY(bool hovering READ isHovering NOTIFY sampleChanged)
    Q_PROPERTY(bool pressed READ isPressed NOTIFY sampleChanged)
    Q_PROPERTY(QString inputName READ inputName NOTIFY sampleChanged)
    Q_PROPERTY(QString deviceName READ deviceName NOTIFY sampleChanged)
    Q_PROPERTY(bool reportsPressure READ reportsPressure NOTIFY sampleChanged)
    Q_PROPERTY(bool reportsTilt READ reportsTilt NOTIFY sampleChanged)
    Q_PROPERTY(QPointF penPosition READ penPosition NOTIFY sampleChanged)
    Q_PROPERTY(double pressure READ pressure NOTIFY sampleChanged)
    Q_PROPERTY(double tiltX READ tiltX NOTIFY sampleChanged)
    Q_PROPERTY(double tiltY READ tiltY NOTIFY sampleChanged)
    Q_PROPERTY(double altitude READ altitude NOTIFY sampleChanged)
    Q_PROPERTY(double azimuth READ azimuth NOTIFY sampleChanged)
    Q_PROPERTY(double penRotation READ penRotation NOTIFY sampleChanged)

    // How many input events arrived during the last second.
    Q_PROPERTY(int eventsPerSecond READ eventsPerSecond NOTIFY eventsPerSecondChanged)

public:
    explicit PenTestArea(QQuickItem *parent = nullptr);
    ~PenTestArea() override;

    bool isActive() const { return m_active; }      ///< true once any input has arrived
    bool isHovering() const { return m_hovering; }  ///< pointer is over this area
    bool isPressed() const { return m_pressed; }    ///< pen/finger touching, or mouse button held
    QString inputName() const;
    QString deviceName() const { return m_deviceName; }
    bool reportsPressure() const { return m_reportsPressure; }
    bool reportsTilt() const { return m_reportsTilt; }
    QPointF penPosition() const { return {m_sample.x, m_sample.y}; }
    double pressure() const { return m_sample.pressure; }
    double tiltX() const { return m_sample.tiltX; }
    double tiltY() const { return m_sample.tiltY; }
    double altitude() const { return m_angles.altitude; }
    double azimuth() const { return m_angles.azimuth; }
    double penRotation() const { return m_sample.rotation; }
    int eventsPerSecond() const { return m_eventsPerSecond; }

signals:
    void sampleChanged();
    void eventsPerSecondChanged();

protected:
    bool eventFilter(QObject *watched, QEvent *event) override;
    void itemChange(ItemChange change, const ItemChangeData &value) override;

private:
    void handleTablet(const QTabletEvent *event);
    void handleMouse(const QMouseEvent *event);
    void handleTouch(const QTouchEvent *event);

    /// Stores a sample if scenePos is inside this item, and notifies QML.
    void record(art::PenSample sample, QPointF scenePos,
                const QPointingDevice *device, bool pressed);
    void setHovering(bool hovering);

    QPointer<QQuickWindow> m_window;  // the window we are watching (QPointer = becomes null if it is deleted)

    art::PenSample m_sample;
    art::PenAngles m_angles;
    QString m_deviceName;
    bool m_active = false;
    bool m_hovering = false;
    bool m_pressed = false;
    bool m_reportsPressure = false;
    bool m_reportsTilt = false;

    QTimer m_rateTimer;          // fires once per second to publish the event rate
    int m_eventsThisSecond = 0;
    int m_eventsPerSecond = 0;
};
