#include "pen_test_area.h"

#include <QMouseEvent>
#include <QPointingDevice>
#include <QQuickWindow>
#include <QTabletEvent>
#include <QTouchEvent>

PenTestArea::PenTestArea(QQuickItem *parent)
    : QQuickItem(parent)
{
    // Once per second, publish how many events arrived and start counting again.
    m_rateTimer.setInterval(1000);
    connect(&m_rateTimer, &QTimer::timeout, this, [this] {
        if (m_eventsPerSecond != m_eventsThisSecond) {
            m_eventsPerSecond = m_eventsThisSecond;
            emit eventsPerSecondChanged();
        }
        m_eventsThisSecond = 0;
    });
    m_rateTimer.start();
}

PenTestArea::~PenTestArea()
{
    if (m_window)
        m_window->removeEventFilter(this);
}

QString PenTestArea::inputName() const
{
    if (!m_active)
        return QStringLiteral("-");

    switch (m_sample.kind) {
    case art::InputKind::Pen:       return tr("Pen tip");
    case art::InputKind::PenEraser: return tr("Pen eraser end");
    case art::InputKind::Mouse:     return tr("Mouse");
    case art::InputKind::Touch:     return tr("Touch");
    }
    return {};
}

void PenTestArea::itemChange(ItemChange change, const ItemChangeData &value)
{
    // Called when this item is put into a window (or taken out of one).
    // Move our event filter to the new window.
    if (change == ItemSceneChange) {
        if (m_window)
            m_window->removeEventFilter(this);
        m_window = value.window;
        if (m_window)
            m_window->installEventFilter(this);
    }
    QQuickItem::itemChange(change, value);
}

bool PenTestArea::eventFilter(QObject *watched, QEvent *event)
{
    switch (event->type()) {
    case QEvent::TabletPress:
    case QEvent::TabletMove:
    case QEvent::TabletRelease:
        handleTablet(static_cast<const QTabletEvent *>(event));
        break;

    case QEvent::MouseButtonPress:
    case QEvent::MouseMove:
    case QEvent::MouseButtonRelease:
        handleMouse(static_cast<const QMouseEvent *>(event));
        break;

    case QEvent::TouchBegin:
    case QEvent::TouchUpdate:
    case QEvent::TouchEnd:
        handleTouch(static_cast<const QTouchEvent *>(event));
        break;

    case QEvent::Leave:  // pointer left the window
        setHovering(false);
        break;

    default:
        break;
    }

    // We only look; we never "eat" the event. The rest of the window
    // (buttons, panels) still receives it as usual.
    return QQuickItem::eventFilter(watched, event);
}

void PenTestArea::handleTablet(const QTabletEvent *event)
{
    art::PenSample sample;
    sample.kind = event->pointerType() == QPointingDevice::PointerType::Eraser
                      ? art::InputKind::PenEraser
                      : art::InputKind::Pen;
    sample.pressure = event->pressure();
    sample.tiltX = event->xTilt();
    sample.tiltY = event->yTilt();
    sample.rotation = event->rotation();
    sample.timeMs = event->timestamp();

    // While the tip touches the tablet, Qt reports the left button as held.
    // A pen hovering just above the tablet sends moves with no buttons.
    const bool touching = event->buttons() != Qt::NoButton;

    record(sample, event->scenePosition(), event->pointingDevice(), touching);
}

void PenTestArea::handleMouse(const QMouseEvent *event)
{
    // When a pen or finger is not handled as a tablet/touch event, Qt also
    // sends a pretend ("synthesized") mouse event for it. Those come from
    // the pen or touch device, so we skip anything that isn't a real mouse.
    const QPointingDevice *device = event->pointingDevice();
    if (!device || device->type() != QInputDevice::DeviceType::Mouse)
        return;

    art::PenSample sample;
    sample.kind = art::InputKind::Mouse;
    const bool pressed = event->buttons() != Qt::NoButton;
    sample.pressure = pressed ? 1.0 : 0.0;
    sample.timeMs = event->timestamp();

    record(sample, event->scenePosition(), device, pressed);
}

void PenTestArea::handleTouch(const QTouchEvent *event)
{
    if (event->points().isEmpty())
        return;

    // Only the first finger is shown; later we will use two fingers for
    // zooming and rotating the canvas.
    const QEventPoint &point = event->points().first();

    art::PenSample sample;
    sample.kind = art::InputKind::Touch;
    const bool touching = point.state() != QEventPoint::State::Released;
    sample.pressure = touching ? point.pressure() : 0.0;
    sample.timeMs = event->timestamp();

    record(sample, point.scenePosition(), event->pointingDevice(), touching);
}

void PenTestArea::record(art::PenSample sample, QPointF scenePos,
                         const QPointingDevice *device, bool pressed)
{
    // Events arrive in window ("scene") coordinates. Convert to coordinates
    // inside this item, and ignore anything outside it.
    const QPointF local = mapFromScene(scenePos);
    if (!contains(local)) {
        setHovering(false);
        return;
    }

    sample.x = local.x();
    sample.y = local.y();
    m_sample = sample;
    m_angles = art::penAngles(sample.tiltX, sample.tiltY);

    // What the device says it can do (not every pen supports tilt).
    m_deviceName = device ? device->name() : QString();
    const auto caps = device ? device->capabilities() : QInputDevice::Capabilities();
    m_reportsPressure = caps.testFlag(QInputDevice::Capability::Pressure);
    m_reportsTilt = caps.testFlag(QInputDevice::Capability::XTilt)
                    || caps.testFlag(QInputDevice::Capability::YTilt);

    m_active = true;
    m_hovering = true;
    m_pressed = pressed;
    ++m_eventsThisSecond;

    emit sampleChanged();
}

void PenTestArea::setHovering(bool hovering)
{
    if (m_hovering == hovering)
        return;
    m_hovering = hovering;
    if (!hovering)
        m_pressed = false;
    emit sampleChanged();
}
