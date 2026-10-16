#include "SettingsTabs.hpp"
#include "GeneralTab.hpp"
#include "VoiceTab.hpp"

SettingsTabs::SettingsTabs(QWidget *parent)
    : QTabWidget(parent)
{
    addTab(new GeneralTab(this), "General");
    addTab(new VoiceTab(this), "Voice");
}
