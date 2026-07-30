#pragma once

#include "effect/effect.h"
#include <QPointF>
#include <QtTypes>
#include <QDBusConnectionInterface>

namespace KWin {

class PointerButtonEvent;

}

namespace fcitx5osk {

class Fcitx5OskEffect : public KWin::Effect {

    Q_OBJECT
    Q_PROPERTY(int interval READ interval)

public:
    Fcitx5OskEffect();
    ~Fcitx5OskEffect() override;

    int interval() const;

    void reconfigure(ReconfigureFlags) override;
    bool touchDown(qint32, const QPointF&, std::chrono::microseconds time) override;

private Q_SLOTS:
    void onKWinVirtualKeyboardActiveChanged();

private:
    void maybeTrigger();

    std::optional<std::chrono::microseconds> m_lastTouchDown;
    std::optional<std::chrono::microseconds> m_lastActive;
    uint32_t m_interval;

    std::unique_ptr<QDBusInterface> m_kwinVirtualKeyboard;
};

}
