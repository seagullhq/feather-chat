#pragma once

#include <QWidget>
#include <QComboBox>

class QLabel;

// "Voice" settings page, one tab of SettingsTabs. Shows which microphone the
// system reports as the default input.
class VoiceTab final : public QWidget {
    Q_OBJECT

public:
    explicit VoiceTab(QWidget *parent);

private:
    void refreshMicrophone();
    void refreshAudioDevice();
    QLabel* microphoneLabel {nullptr};
    QLabel* audioDeviceLabel {nullptr};
    QComboBox *microphoneList { nullptr };
    QComboBox *audioDeviceList { nullptr };
};
