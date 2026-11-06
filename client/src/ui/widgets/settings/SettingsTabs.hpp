#pragma once

#include <QTabWidget>

// Settings content as tabs: one page per category, replacing the old button
// rail. Pages are QWidgets (GeneralTab, VoiceTab) owned by the tab widget.
class SettingsTabs final : public QTabWidget {
    Q_OBJECT

public:
    explicit SettingsTabs(QWidget *parent);
};
