#pragma once

#include <array>

#include "defines.h"
#include "trackers/tracker.h"

class TrackerManager {
public:
    ~TrackerManager();
    void register_tracker(uint8_t index, TrackerKind kind, uint8_t address, bool required);
    void setup();
    void poll_tracker_status();

    const std::array<Tracker*, MAX_TRACKER_COUNT>& get_trackers() const { return m_trackers; }

private:
    std::array<Tracker*, MAX_TRACKER_COUNT> m_trackers;
    uint64_t m_last_status_poll_time = 0;
};
