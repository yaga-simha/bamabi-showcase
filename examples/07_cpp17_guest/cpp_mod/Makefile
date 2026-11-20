UNAME_S := $(shell uname -s)
CXX ?= c++

ifeq ($(UNAME_S),Darwin)
    TARGET = libcpp17_mod.dylib
    LDFLAGS = -shared -fPIC -fvisibility=hidden -fvisibility-inlines-hidden
else ifeq ($(OS),Windows_NT)
    TARGET = cpp17_mod.dll
    LDFLAGS = -shared
else
    TARGET = libcpp17_mod.so
    LDFLAGS = -shared -fPIC -fvisibility=hidden -fvisibility-inlines-hidden
endif

ifneq ($(strip $(BAM_INCLUDE_DIR)),)
    INC_DIR := $(BAM_INCLUDE_DIR)
else ifneq ($(strip $(BAM_WIRE_DIR)),)
    INC_DIR := $(BAM_WIRE_DIR)/include
else ifneq ($(wildcard ../../../../bamabi-wire/include/bamabi.h),)
    INC_DIR := ../../../../bamabi-wire/include
else
    INC_DIR := ../../../target/include
endif

# Strict ISO C++17 compile flags (override prevents environment CXXFLAGS clobbering)
override CXXFLAGS += -std=c++17 -Wall -Wextra -Wpedantic -Werror -I$(INC_DIR) -O3

all: $(TARGET)

ifeq ($(INC_DIR),../../../target/include)
$(INC_DIR)/bamabi.h:
	@echo "Staging $(INC_DIR)/bamabi.h via cargo xtask header (standalone checkout)..."
	@cargo run --manifest-path ../../../Cargo.toml -p xtask -- header
endif

$(TARGET): mod.cpp bam_cpp17_sdk.hpp example_components.h $(INC_DIR)/bamabi.h
	$(CXX) $(CXXFLAGS) $(LDFLAGS) -o $@ $<

clean:
	rm -f $(TARGET)

.PHONY: all clean
