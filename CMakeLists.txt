# ---------------------------------------------------------------------------
# esp32-round-base — ESP-IDF scaffold for the Guition JC3636K718C
# (1.85" round knob screen): board support package + network component.
# ---------------------------------------------------------------------------
# Top-level ESP-IDF project file.
#
# The target is pinned to esp32s3 so that a fresh clone configures correctly
# without running `idf.py set-target esp32s3` first. This board only exists as
# an ESP32-S3 design, so pinning it removes a whole class of setup mistakes.

cmake_minimum_required(VERSION 3.16)

set(IDF_TARGET "esp32s3")

include($ENV{IDF_PATH}/tools/cmake/project.cmake)

# The release number is given here rather than left to `git describe`: the
# VERSION argument is read by ESP-IDF before it ever reaches for git, so this is
# the one place a release is named. It becomes PROJECT_VER, lands in the image
# header as esp_app_desc.version (printed in the boot log) and sets
# CMAKE_PROJECT_VERSION for anything that asks the build system.
project(esp32_round_base VERSION 0.1.0)
