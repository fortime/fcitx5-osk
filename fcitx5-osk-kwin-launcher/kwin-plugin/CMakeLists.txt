enable_language(CXX)

set(CMAKE_CXX_STANDARD 23)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

find_package(ECM REQUIRED NO_MODULE)
find_package(KWin REQUIRED)
find_package(Qt6 CONFIG REQUIRED COMPONENTS
    DBus
    Widgets
)
find_package(KF6 REQUIRED COMPONENTS
    Config
    I18n
    KCMUtils
)
find_package(KWinDBusInterface REQUIRED)

include(KDEInstallDirs)
include(KDECMakeSettings)
include(KDECompilerSettings NO_POLICY_SCOPE)

include(ECMQtDeclareLoggingCategory)

set(fcitx5osk_SOURCES
    main.cpp
    fcitx5osk.cpp
    fcitx5osk.h
)

kconfig_add_kcfg_files(fcitx5osk_SOURCES
    fcitx5oskconfig.kcfgc
)

qt_add_dbus_interface(fcitx5osk_SOURCES "fyi.fortime.Fcitx5Osk.xml" fcitx5oskdbus)

add_library(fcitx5osk MODULE ${fcitx5osk_SOURCES})
target_link_libraries(fcitx5osk PRIVATE
    KWin::kwin

    KF6::ConfigGui
    KF6::I18n

    Qt6::DBus
)

ecm_qt_declare_logging_category(fcitx5osk
        HEADER fcitx5osklogging.h
        IDENTIFIER KWIN_FCITX5OSK
        CATEGORY_NAME kwin_effect_fcitx5osk
        DEFAULT_SEVERITY Warning
)

target_compile_definitions(fcitx5osk PRIVATE
                           KWIN_VERSION_MAJOR=${KWin_VERSION_MAJOR}
                           KWIN_VERSION_MINOR=${KWin_VERSION_MINOR}
                           KWIN_VERSION_PATCH=${KWin_VERSION_PATCH})
install(TARGETS
        fcitx5osk
    DESTINATION ${KDE_INSTALL_PLUGINDIR}/kwin/effects/plugins
    COMPONENT Fcitx5OskKwinLauncher
)

set(kwin_fcitx5osk_config_SRCS
    fcitx5osk_config.cpp
    fcitx5osk_config.h
)

ki18n_wrap_ui(kwin_fcitx5osk_config_SRCS fcitx5osk_config.ui)
kconfig_add_kcfg_files(kwin_fcitx5osk_config_SRCS fcitx5oskconfig.kcfgc)

qt_add_dbus_interface(kwin_fcitx5osk_config_SRCS ${KWIN_EFFECTS_INTERFACE} kwineffectsinterface)

add_library(kwin_fcitx5osk_config MODULE ${kwin_fcitx5osk_config_SRCS})
target_link_libraries(kwin_fcitx5osk_config
    KF6::ConfigGui
    KF6::I18n
    KF6::KCMUtils

    Qt6::DBus
)
install(TARGETS
        kwin_fcitx5osk_config
    DESTINATION ${KDE_INSTALL_PLUGINDIR}/kwin/effects/configs
    COMPONENT Fcitx5OskKwinLauncher
)
