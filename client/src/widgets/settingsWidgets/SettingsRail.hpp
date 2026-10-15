#pragma once

#include <QFrame>
#include <qobject.h>
#include <qwidget.h>
class SettingsRail : public QFrame {
    Q_OBJECT

signals:
    void CategoryChanged();

public:
    QString currentCategory;
    SettingsRail(QWidget *parent);
};
