#pragma once
#include <QAudioDevice>
#include <QAudioSource>
#include <QMediaDevices>

class AudioManager {
public:
    AudioManager(const QAudioDevice *microphone, const QAudioDevice *audioDevice);
    QAudioDevice *getMicrophone() const
    {
        return microphone;
    };

    QAudioDevice *getAudioDevice() const
    {
        return audioDevice;
    };
    void setMicVolume(double volume);

private:
    QAudioDevice *microphone;
    QAudioDevice *audioDevice;
    QAudioSource *audioSource;
};

#ifndef __H__AUDIO_MANAGER
#define __H__AUDIO_MANAGER

extern AudioManager *audioManager;

#endif
