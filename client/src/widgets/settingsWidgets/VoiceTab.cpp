#include "VoiceTab.hpp"

#include <QVBoxLayout>

VoiceTab::VoiceTab(QWidget *parent)
    : QWidget(parent)
{
    auto *layout = new QVBoxLayout(this);
    // Settings rows go here; a trailing stretch keeps them at the top.
    layout->addStretch();
}
