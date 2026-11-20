UNAME_S := $(shell uname -s)

ifeq ($(UNAME_S),Darwin)
    TARGET := libmod_cuddle.dylib
    LDFLAGS = -shared -fPIC -lm
else ifeq ($(OS),Windows_NT)
    TARGET := mod_cuddle.dll
    LDFLAGS = -shared
else
    TARGET := libmod_cuddle.so
    LDFLAGS = -shared -fPIC -lm
endif

ifneq ($(strip $(BAM_INCLUDE_DIR)),)
    INC_DIR := $(BAM_INCLUDE_DIR)
else ifneq ($(strip $(BAM_WIRE_DIR)),)
    INC_DIR := $(BAM_WIRE_DIR)/include
else ifneq ($(wildcard ../../../../../bamabi-wire/include/bamabi.h),)
    INC_DIR := ../../../../../bamabi-wire/include
else
    INC_DIR := ../../../../target/include
endif

override CFLAGS += -std=c99 -Wall -Wextra -Wpedantic -Werror -I$(INC_DIR) -O3

all: $(TARGET)

ifeq ($(INC_DIR),../../../../target/include)
$(INC_DIR)/bamabi.h:
	@echo "Staging $(INC_DIR)/bamabi.h via cargo xtask header (standalone checkout)..."
	@cargo run --manifest-path ../../../../Cargo.toml -p xtask -- header
endif

$(TARGET): mod.c $(INC_DIR)/bamabi.h
	$(CC) $(CFLAGS) $(LDFLAGS) -o $@ $<

clean:
	rm -f $(TARGET)
	rm -rf *.dSYM

.PHONY: all clean
