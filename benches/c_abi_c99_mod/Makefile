UNAME_S := $(shell uname -s)

ifeq ($(UNAME_S),Darwin)
    TARGET_DEBUG   = libc_abi_c99_mod_debug.dylib
    TARGET_RELEASE = libc_abi_c99_mod_release.dylib
    LDFLAGS = -shared -fPIC -lm
else ifeq ($(OS),Windows_NT)
    TARGET_DEBUG   = c_abi_c99_mod_debug.dll
    TARGET_RELEASE = c_abi_c99_mod_release.dll
    LDFLAGS = -shared
else
    TARGET_DEBUG   = libc_abi_c99_mod_debug.so
    TARGET_RELEASE = libc_abi_c99_mod_release.so
    LDFLAGS = -shared -fPIC -lm
endif

ifneq ($(strip $(BAM_INCLUDE_DIR)),)
    INC_DIR := $(BAM_INCLUDE_DIR)
else ifneq ($(strip $(BAM_WIRE_DIR)),)
    INC_DIR := $(BAM_WIRE_DIR)/include
else ifneq ($(wildcard ../../../bamabi-wire/include/bamabi.h),)
    INC_DIR := ../../../bamabi-wire/include
else
    INC_DIR := ../../target/include
endif

BASE_CFLAGS = -std=c99 -Wall -Wextra -Wpedantic -Werror -I$(INC_DIR)

all: $(TARGET_DEBUG) $(TARGET_RELEASE)

ifeq ($(INC_DIR),../../target/include)
$(INC_DIR)/bamabi.h:
	@echo "Staging $(INC_DIR)/bamabi.h via cargo xtask header (standalone checkout)..."
	@cargo run --manifest-path ../../Cargo.toml -p xtask -- header
endif

$(TARGET_DEBUG): mod.c $(INC_DIR)/bamabi.h
	$(CC) $(BASE_CFLAGS) -O0 -g $(LDFLAGS) -o $@ $<

$(TARGET_RELEASE): mod.c $(INC_DIR)/bamabi.h
	$(CC) $(BASE_CFLAGS) -O3 $(LDFLAGS) -o $@ $<

clean:
	rm -f $(TARGET_DEBUG) $(TARGET_RELEASE)
	rm -rf *.dSYM

.PHONY: all clean
