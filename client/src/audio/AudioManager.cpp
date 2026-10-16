#include "AudioManager.hpp"

AudioManager *audioManager = nullptr;

AudioManager::AudioManager(const QAudioDevice *microphone, const QAudioDevice *audioDevice)
    : microphone(microphone ? new QAudioDevice(*microphone) : nullptr)
    , audioDevice(audioDevice ? new QAudioDevice(*audioDevice) : nullptr)
    , audioSource(new QAudioSource)
{
}
void AudioManager::setMicVolume(double volume)
{
    audioSource->setVolume(qBound(0.0, volume, 1.0));
}
