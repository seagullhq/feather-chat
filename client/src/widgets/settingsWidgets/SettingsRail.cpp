#include "SettingsRail.hpp"
#include "../../tools/Utils.hpp"
#include <qboxlayout.h>
#include <qpushbutton.h>

SettingsRail::SettingsRail(QWidget *parent)
    : currentCategory("General")
{

    QVBoxLayout *layout = new QVBoxLayout;

    QPushButton *generalCategory = new QPushButton;
    generalCategory->setText("General");
    QPushButton *audioCategory = new QPushButton;
    audioCategory->setText("Voice");

    layout->addWidget(generalCategory);
    layout->addWidget(audioCategory);
    setLayout(layout);

    QT_MANAGER::connectWithContext(this,
        QT_MANAGER::Conn { generalCategory, &QPushButton::clicked,
            [this]() { this->currentCategory = "General"; } },
        QT_MANAGER::Conn {
            audioCategory, &QPushButton::clicked, [this]() { this->currentCategory = "Voice"; } });
}
