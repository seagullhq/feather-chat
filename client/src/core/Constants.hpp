#pragma once
#include <cstdint>
#ifndef __H__CONSTANT
#define __H__CONSTANT
constexpr auto DefaultHost = "127.0.0.1";
constexpr uint16_t ControlPort = 7700;
constexpr uint16_t MediaPort = 7701;
constexpr int TimeoutMs = 10000;
constexpr unsigned int MaxFrame = 1u << 20;
#endif
