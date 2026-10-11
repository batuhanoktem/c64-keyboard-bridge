CL65 ?= cl65
CC65 ?= cc65
CA65 ?= ca65
HOST_CC ?= cc
BUILD ?= build
OBJECT_NAMES := main.o bridge.o uci.o scanner.o
KEY_OBJECTS := $(addprefix $(BUILD)/keyboard/,$(OBJECT_NAMES))
PROBE_OBJECTS := $(addprefix $(BUILD)/probe/,$(OBJECT_NAMES))
C64_FLAGS := -t c64 --cpu 6502 -Oirs --standard c99 -I include

.PHONY: all test clean
all: $(BUILD)/KEYBRIDGE.PRG $(BUILD)/NETWORKTEST.PRG

$(BUILD):
	mkdir -p $(BUILD)

$(BUILD)/keyboard $(BUILD)/probe: | $(BUILD)
	mkdir -p $@

$(BUILD)/keyboard/%.o: src/%.c include/bridge.h include/uci.h | $(BUILD)/keyboard
	$(CC65) $(C64_FLAGS) -o $(BUILD)/keyboard/$*.s $<
	$(CA65) -t c64 --cpu 6502 -o $@ $(BUILD)/keyboard/$*.s

$(BUILD)/probe/%.o: src/%.c include/bridge.h include/uci.h | $(BUILD)/probe
	$(CC65) $(C64_FLAGS) -DNETWORK_PROBE -o $(BUILD)/probe/$*.s $<
	$(CA65) -t c64 --cpu 6502 -o $@ $(BUILD)/probe/$*.s

$(BUILD)/keyboard/%.o: src/%.s | $(BUILD)/keyboard
	$(CA65) -t c64 --cpu 6502 -o $@ $<

$(BUILD)/probe/%.o: src/%.s | $(BUILD)/probe
	$(CA65) -t c64 --cpu 6502 -o $@ $<

$(BUILD)/KEYBRIDGE.PRG: $(KEY_OBJECTS)
	$(CL65) -t c64 -m $(BUILD)/KEYBRIDGE.map -o $@ $(KEY_OBJECTS)

$(BUILD)/NETWORKTEST.PRG: $(PROBE_OBJECTS)
	$(CL65) -t c64 -m $(BUILD)/NETWORKTEST.map -o $@ $(PROBE_OBJECTS)

$(BUILD)/test-driver: tests/test_driver.c src/bridge.c src/uci.c include/bridge.h include/uci.h | $(BUILD)
	$(HOST_CC) -std=c99 -Wall -Wextra -Werror -pedantic -I include tests/test_driver.c src/bridge.c src/uci.c -o $@

$(BUILD)/test-main-state: tests/test_main_state.c tests/fixtures/conio.h src/main.c src/bridge.c src/uci.c include/bridge.h include/uci.h | $(BUILD)
	$(HOST_CC) -std=c99 -Wall -Wextra -Werror -pedantic -I tests/fixtures -I include tests/test_main_state.c src/bridge.c src/uci.c -o $@

$(BUILD)/test-probe-state: tests/test_main_state.c tests/fixtures/conio.h src/main.c src/bridge.c src/uci.c include/bridge.h include/uci.h | $(BUILD)
	$(HOST_CC) -std=c99 -Wall -Wextra -Werror -pedantic -DNETWORK_PROBE -I tests/fixtures -I include tests/test_main_state.c src/bridge.c src/uci.c -o $@

test: $(BUILD)/test-driver $(BUILD)/test-main-state $(BUILD)/test-probe-state
	./$(BUILD)/test-driver
	./$(BUILD)/test-main-state
	./$(BUILD)/test-probe-state

clean:
	rm -rf $(BUILD)
	rm -f src/*.o
