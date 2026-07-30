#include "fcitx5osk_config.h"

#include <kwin/config-kwin.h>

// KConfigSkeleton
#include <fcitx5oskconfig.h>
#include <kwineffectsinterface.h>

#include <KPluginFactory>

#include <QWidget>


namespace fcitx5osk {

K_PLUGIN_CLASS(Fcitx5OskEffectConfig)

Fcitx5OskEffectConfig::Fcitx5OskEffectConfig(QObject* parent, const KPluginMetaData& data)
    : KCModule(parent, data)
{
    m_ui.setupUi(widget());

    Fcitx5OskConfig::instance(KWIN_CONFIG);
    addConfig(Fcitx5OskConfig::self(), widget());
}

void Fcitx5OskEffectConfig::save()
{
    KCModule::save();

    OrgKdeKwinEffectsInterface interface(QStringLiteral("org.kde.KWin"),
        QStringLiteral("/Effects"),
        QDBusConnection::sessionBus());
    interface.reconfigureEffect(QStringLiteral("fcitx5osk"));
}

} // namespace

#include <fcitx5osk_config.moc>

#include <moc_fcitx5osk_config.cpp>
