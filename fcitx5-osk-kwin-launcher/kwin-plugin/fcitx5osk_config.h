#pragma once

#include <kcmodule.h>

#include "ui_fcitx5osk_config.h"

namespace fcitx5osk
{

class Fcitx5OskEffectConfig : public KCModule
{
    Q_OBJECT

public:
    explicit Fcitx5OskEffectConfig(QObject *parent, const KPluginMetaData &data);

    void save() override;

private:
    Ui::Fcitx5OskEffectConfig m_ui;
};

} // namespace
