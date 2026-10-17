#pragma once

#include <QComboBox>
#include <QSlider>
#include <QWidget>

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
    QLabel *microphoneLabel { nullptr };
    QLabel *audioDeviceLabel { nullptr };
    QLabel *volumeLabel { nullptr };
    QComboBox *microphoneList { nullptr };
    QComboBox *audioDeviceList { nullptr };
    QSlider *volumeSlider { nullptr };
};
