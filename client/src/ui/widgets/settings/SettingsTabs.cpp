#include "SettingsTabs.hpp"
#include "tabs/GeneralTab.hpp"
#include "tabs/VoiceTab.hpp"

SettingsTabs::SettingsTabs(QWidget *parent)
    : QTabWidget(parent)
{
    addTab(new GeneralTab(this), "General");
    addTab(new VoiceTab(this), "Voice");
}
