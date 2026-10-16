#include "VoiceTab.hpp"

#include <QAudioDevice>
#include <QLabel>
#include <QMediaDevices>
#include <QVBoxLayout>

VoiceTab::VoiceTab(QWidget *parent)
    : QWidget(parent)
{
    auto *layout = new QVBoxLayout(this);

    microphoneLabel = new QLabel(this);
    microphoneList = new QComboBox(this);

    audioDeviceLabel = new QLabel(this);
    audioDeviceList = new QComboBox(this);

    microphoneLabel->setText("Microphone: ");
    audioDeviceLabel->setText("Audio device: ");

    layout->addWidget(microphoneLabel);
    layout->addWidget(microphoneList);

    layout->addWidget(audioDeviceLabel);
    layout->addWidget(audioDeviceList);

    layout->addStretch();

    refreshMicrophone();
    refreshAudioDevice();

    // Devices come and go (USB headset, Bluetooth) and the default moves with
    // them, so the label must not go stale. The change signals live on the
    // QMediaDevices instance; the getters below are static.
    auto *devices = new QMediaDevices(this);
    connect(devices, &QMediaDevices::audioInputsChanged, this, [this] { refreshMicrophone(); });
    connect(devices, &QMediaDevices::audioOutputsChanged, this, [this] { refreshAudioDevice(); });
}

void VoiceTab::refreshAudioDevice()
{
    // Gets all microphones connected to the OS
    auto audioDevices = QMediaDevices::audioOutputs();
    const QAudioDevice defaultAudioDevice = QMediaDevices::defaultAudioOutput();
    int idx = 0;
    bool found = false;
    for (auto audioDevice : audioDevices) {
        audioDeviceList->addItem(audioDevice.description());
        if(audioDevice.description() != defaultAudioDevice.description() && !found){
            idx++;
        }else{
            found = true;
        }
    }

    // Get OS default audio device
    audioDeviceList->activated(idx);
}


void VoiceTab::refreshMicrophone()
{
    // Gets all microphones connected to the OS
    auto microphones = QMediaDevices::audioInputs();
    const QAudioDevice defaultMicrophone = QMediaDevices::defaultAudioInput();
    int idx = 0;
    bool found = false;
    for (auto microphone : microphones) {
        microphoneList->addItem(microphone.description());
        if(microphone.description() != defaultMicrophone.description() && !found){
            idx++;
        }else{
            found = true;
        }
    }

    // Get OS default microphone
    microphoneList->activated(idx);
}
