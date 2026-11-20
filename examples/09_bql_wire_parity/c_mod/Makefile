CC ?= cc
UNAME_S := $(shell uname -s)

ifneq ($(strip $(BAM_INCLUDE_DIR)),)
    INC_DIR := $(BAM_INCLUDE_DIR)
else ifneq ($(strip $(BAM_WIRE_DIR)),)
    INC_DIR := $(BAM_WIRE_DIR)/include
else ifneq ($(wildcard ../../../../bamabi-wire/include/bamabi.h),)
    INC_DIR := ../../../../bamabi-wire/include
else
    INC_DIR := ../../../target/include
endif

CFLAGS := -std=c99 -Wall -Wextra -Wpedantic -Werror -O2 -fPIC -I$(INC_DIR)

ifeq ($(UNAME_S),Darwin)
    EXT := dylib
    LDFLAGS := -dynamiclib
else
    EXT := so
    LDFLAGS := -shared
endif

TARGET_V1 := libbql_parity_c_v1.$(EXT)
TARGET_V2 := libbql_parity_c_v2.$(EXT)

.PHONY: all clean

all: $(TARGET_V1) $(TARGET_V2)

ifeq ($(INC_DIR),../../../target/include)
$(INC_DIR)/bamabi.h:
	@echo "Staging $(INC_DIR)/bamabi.h via cargo xtask header (standalone checkout)..."
	@cargo run --manifest-path ../../../Cargo.toml -p xtask -- header
endif

$(TARGET_V1): bql_parity_mod.c $(INC_DIR)/bamabi.h
	$(CC) $(CFLAGS) $(LDFLAGS) bql_parity_mod.c -o $(TARGET_V1)

$(TARGET_V2): bql_parity_mod.c $(INC_DIR)/bamabi.h
	$(CC) $(CFLAGS) -DBQL_SCHEMA_V2=1 $(LDFLAGS) bql_parity_mod.c -o $(TARGET_V2)

clean:
	rm -f $(TARGET_V1) $(TARGET_V2)
