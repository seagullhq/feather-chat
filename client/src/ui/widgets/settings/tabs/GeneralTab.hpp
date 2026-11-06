#pragma once

#include <QWidget>

// "General" settings page, one tab of SettingsTabs. Must be a QWidget (it was
// a QLayout), because QTabWidget can only hold widgets.
class GeneralTab final : public QWidget {
    Q_OBJECT

public:
    GeneralTab(QWidget *parent);
};
