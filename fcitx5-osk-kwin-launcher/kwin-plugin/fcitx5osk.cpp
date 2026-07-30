#include "fcitx5osk.h"

#include <fcitx5oskconfig.h>
#include <fcitx5oskdbus.h>
#include <fcitx5osklogging.h>

#include <QDBusConnection>

namespace fcitx5osk {

Fcitx5OskEffect::Fcitx5OskEffect()
{
    Fcitx5OskConfig::instance(KWIN_CONFIG);

    m_kwinVirtualKeyboard = std::make_unique<QDBusInterface>(
        QStringLiteral("org.kde.keyboard"),
        QStringLiteral("/VirtualKeyboard"),
        QStringLiteral("org.kde.kwin.VirtualKeyboard"),
        QDBusConnection::sessionBus(),
        this);

    connect(
        m_kwinVirtualKeyboard.get(),
        SIGNAL(activeChanged()),
        this,
        SLOT(onKWinVirtualKeyboardActiveChanged()));

    reconfigure(ReconfigureAll);
}

Fcitx5OskEffect::~Fcitx5OskEffect()
{
}

void Fcitx5OskEffect::reconfigure(ReconfigureFlags)
{
    Fcitx5OskConfig::self()->read();
    m_interval = Fcitx5OskConfig::interval();

    qCDebug(KWIN_FCITX5OSK) << "interval=" << m_interval;
}

int Fcitx5OskEffect::interval() const
{
    return m_interval;
}

bool Fcitx5OskEffect::touchDown(qint32, const QPointF&, std::chrono::microseconds time)
{
    qCDebug(KWIN_FCITX5OSK) << "touch down";

    m_lastTouchDown = time;

    maybeTrigger();

    return false;
}

void Fcitx5OskEffect::onKWinVirtualKeyboardActiveChanged()
{
    bool active = m_kwinVirtualKeyboard->property("active").toBool();
    qCDebug(KWIN_FCITX5OSK) << "active is changed: " << active;

    if (!active) {
        m_lastActive.reset();
    } else {
        m_lastActive = std::chrono::duration_cast<std::chrono::microseconds>(
            std::chrono::steady_clock::now().time_since_epoch());
        maybeTrigger();
    }
}

void Fcitx5OskEffect::maybeTrigger()
{
    std::optional<std::chrono::microseconds> lastTouchDown = m_lastTouchDown;
    std::optional<std::chrono::microseconds> lastActive = m_lastActive;
    if (!lastTouchDown || !lastActive)
        return;

    auto delta = *lastTouchDown - *lastActive;

    if (std::chrono::abs(delta) <= std::chrono::milliseconds(m_interval)) {
        qCDebug(KWIN_FCITX5OSK) << "Show keyboard, delta: " << delta;
        // Show keyboard
        FyiFortimeFcitx5OskController1Interface interface(QStringLiteral("fyi.fortime.Fcitx5Osk"),
            QStringLiteral("/fyi/fortime/Fcitx5Osk/Controller"),
            QDBusConnection::sessionBus());
        interface.Show();
        m_lastTouchDown.reset();
        m_lastActive.reset();
    }
}

}

#include <moc_fcitx5osk.cpp>
