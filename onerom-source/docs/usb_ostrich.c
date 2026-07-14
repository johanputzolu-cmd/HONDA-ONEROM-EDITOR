// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
// Modifications OSTRICH/LED activity: YOSUPRA
//
// MIT License

#include "usb_plugin.h"
#include "usb_picobootx.h"
#include "usb_rom.h"
#include "picobootx.h"

#define OST_DATALOG_CDC_ITF 0u
#define OST_CDC_ITF 1u
#define OST_CMD_BUF_MAX 8u
#define OST_RX_BUDGET_PER_TASK 1024u
#define OST_TXQ_SIZE 16u
#define OST_DATALOG_PAUSE_HOLD_MS 60u

#define OST_PERSIST_IDLE_MS 1000u
#define OST_FLASH_SECTOR 4096u
#define OST_FLASH_PAGE 256u
#define OST_FLASH_BLOCK_SIZE 4096u
#define OST_FLASH_BLOCK_ERASE_CMD 0x20u
#define OST_FLASH_PROTECTED_END (RP2350_FLASH_BASE + 128u * 1024u)
#define OST_PERSIST_OFF_INVALID 0xFFFFFFFFu

#define OST_HW_VER 0x14u
#define OST_FW_VER 0x09u
#define OST_HW_CH  'O'
#define OST_VENDOR_ID 0x01u
#define OST_SERIAL_STR "YOSUPRA1e"

typedef enum {
    OST_MODE_CMD = 0,
    OST_MODE_WRITE_DATA,
} ost_mode_t;

typedef struct {
    ost_mode_t mode;
    uint8_t cmd_buf[OST_CMD_BUF_MAX];
    uint8_t cmd_len;

    uint32_t wr_addr;
    uint32_t wr_size;
    uint32_t wr_pos;
    uint8_t wr_sum;
    rom_pin_layout_t wr_layout;
    bool wr_layout_valid;
    bool wr_wrap_27256;

    uint8_t bank_emu;
    uint8_t bank_update;
    uint8_t bank_persist;

    uint8_t txq[OST_TXQ_SIZE];
    uint8_t txq_head;
    uint8_t txq_tail;
    uint8_t txq_count;
} ost_ctx_t;

static ost_ctx_t ost;
static bool ost_persist_dirty;
static uint32_t ost_persist_last_ms;
static uint32_t ost_persist_phys_off;
static uint32_t ost_datalog_pause_until_ms;

typedef void (*ost_connect_internal_flash_fn_t)(void);
typedef void (*ost_flash_exit_xip_fn_t)(void);
typedef void (*ost_flash_range_erase_fn_t)(uint32_t offs, uint32_t count, uint32_t block_size, uint8_t block_cmd);
typedef void (*ost_flash_flush_cache_fn_t)(void);
typedef void (*ost_flash_select_xip_read_mode_fn_t)(uint8_t mode, uint8_t clkdiv);
typedef void (*ost_flash_range_program_fn_t)(uint32_t offs, const uint8_t *data, uint32_t count);

static void __attribute__((section(".ramfunc"), noinline)) ost_flash_erase_critical(
    ost_flash_exit_xip_fn_t exit_xip,
    ost_flash_range_erase_fn_t range_erase,
    ost_flash_flush_cache_fn_t flush_cache,
    ost_flash_select_xip_read_mode_fn_t select_xip,
    uint32_t flash_offs,
    uint32_t size,
    uint8_t clkdiv
) {
    __asm volatile ("cpsid i");
    exit_xip();
    range_erase(flash_offs, size, OST_FLASH_BLOCK_SIZE, OST_FLASH_BLOCK_ERASE_CMD);
    select_xip(3u, clkdiv);
    flush_cache();
    __asm volatile ("cpsie i");
}

static inline void ost_persist_mark(void) {
    ost_persist_dirty = true;
    ost_persist_last_ms = context.timer_ms;
    // Restart any in-progress drain after a new successful write command.
    ost_persist_phys_off = OST_PERSIST_OFF_INVALID;
}

static inline void ost_persist_touch(void) {
    // Debounce touch used during uploads: keep re-arming the idle timer
    // while host is actively streaming write bytes.
    ost_persist_last_ms = context.timer_ms;
}

static bool ost_persist_commit_block(void) {
    if (context.ora_lookup_fn == NULL) {
        return false;
    }

    const sdrr_rom_set_t *rs = app_get_active_rom_set(&context);
    if (rs == NULL || rs->data == NULL || rs->size == 0u) {
        return false;
    }

    uint32_t flash_addr = (uint32_t)(uintptr_t)rs->data;
    uint32_t size = rs->size;

    // Never touch firmware/metadata/system plugin reserved area.
    if (flash_addr < OST_FLASH_PROTECTED_END) {
        return false;
    }
    if ((flash_addr % OST_FLASH_SECTOR) != 0u) {
        return false;
    }
    if ((size % OST_FLASH_PAGE) != 0u || size == 0u) {
        return false;
    }

    ora_get_active_ram_slot_fn_t get_active_slot = context.ora_lookup_fn(ORA_ID_GET_ACTIVE_RAM_SLOT);
    ora_get_ram_slot_info_fn_t get_ram_slot_info = context.ora_lookup_fn(ORA_ID_GET_RAM_SLOT_INFO);
    if (get_active_slot == NULL || get_ram_slot_info == NULL) {
        return false;
    }

    uint8_t ram_slot = 0u;
    if (get_active_slot(&ram_slot) != ORA_RESULT_OK) {
        return false;
    }

    uint32_t ram_addr = 0u;
    uint32_t ram_size = 0u;
    if (get_ram_slot_info(ram_slot, &ram_addr, &ram_size, NULL) != ORA_RESULT_OK) {
        return false;
    }
    if (ram_addr == 0u || ram_size < size) {
        return false;
    }

    uint32_t off = ost_persist_phys_off;
    if (off >= size) {
        return false;
    }

    uint32_t remaining = size - off;
    uint32_t prog_size = (remaining < OST_FLASH_SECTOR) ? remaining : OST_FLASH_SECTOR;

    const uint8_t *flash_blk = (const uint8_t *)(uintptr_t)(flash_addr + off);
    const uint8_t *ram_blk = (const uint8_t *)(uintptr_t)(ram_addr + off);

    // Skip unchanged blocks to reduce erase/program wear and latency.
    if (memcmp(flash_blk, ram_blk, prog_size) == 0) {
        ost_persist_phys_off = off + OST_FLASH_SECTOR;
        return (ost_persist_phys_off < size);
    }

    ost_connect_internal_flash_fn_t connect_internal_flash =
        (ost_connect_internal_flash_fn_t)picoboot_lookup_boot_fn('I', 'F');
    ost_flash_exit_xip_fn_t flash_exit_xip =
        (ost_flash_exit_xip_fn_t)picoboot_lookup_boot_fn('E', 'X');
    ost_flash_range_erase_fn_t flash_range_erase =
        (ost_flash_range_erase_fn_t)picoboot_lookup_boot_fn('R', 'E');
    ost_flash_flush_cache_fn_t flash_flush_cache =
        (ost_flash_flush_cache_fn_t)picoboot_lookup_boot_fn('F', 'C');
    ost_flash_select_xip_read_mode_fn_t flash_select_xip_read_mode =
        (ost_flash_select_xip_read_mode_fn_t)picoboot_lookup_boot_fn('X', 'M');
    ost_flash_range_program_fn_t flash_range_program =
        (ost_flash_range_program_fn_t)picoboot_lookup_boot_fn('R', 'P');
    if (connect_internal_flash == NULL || flash_exit_xip == NULL ||
        flash_range_erase == NULL || flash_flush_cache == NULL ||
        flash_select_xip_read_mode == NULL || flash_range_program == NULL) {
        return false;
    }

    ora_enter_exclusive_mode_fn_t enter_exclusive = context.ora_lookup_fn(ORA_ID_ENTER_EXCLUSIVE_MODE);
    ora_exit_exclusive_mode_fn_t exit_exclusive = context.ora_lookup_fn(ORA_ID_EXIT_EXCLUSIVE_MODE);
    if (enter_exclusive == NULL || exit_exclusive == NULL) {
        return false;
    }

    if (enter_exclusive() != ORA_RESULT_OK) {
        return false;
    }

    connect_internal_flash();

    uint32_t flash_offs = flash_addr - RP2350_FLASH_BASE;
    uint8_t clkdiv = (uint8_t)((XIP_QMI_M0_TIMING >> XIP_QMI_M0_CLKDIV_SHIFT) & XIP_QMI_M0_CLKDIV_MASK);

    ost_flash_erase_critical(
        flash_exit_xip,
        flash_range_erase,
        flash_flush_cache,
        flash_select_xip_read_mode,
        flash_offs + off,
        OST_FLASH_SECTOR,
        clkdiv
    );

    // Program the changed sub-range page-by-page to shorten individual
    // programming windows and reduce runtime stalls.
    for (uint32_t page_off = 0u; page_off < prog_size; page_off += OST_FLASH_PAGE) {
        uint32_t page_size = prog_size - page_off;
        if (page_size > OST_FLASH_PAGE) {
            page_size = OST_FLASH_PAGE;
        }

        flash_range_program(
            flash_offs + off + page_off,
            ram_blk + page_off,
            page_size
        );
    }

    exit_exclusive();

    ost_persist_phys_off = off + OST_FLASH_SECTOR;
    return (ost_persist_phys_off < size);
}

static void ost_persist_task(void) {
    if (!ost_persist_dirty) {
        return;
    }

    // Never commit while a command header is being assembled.
    // This acts as a WRITE_PARAMS-style guard.
    if (ost.mode == OST_MODE_CMD && ost.cmd_len != 0u) {
        ost_persist_touch();
        return;
    }

    if (ost.mode == OST_MODE_WRITE_DATA) {
        ost_persist_touch();
        return;
    }

    if (tud_cdc_n_available(OST_DATALOG_CDC_ITF) > 0u ||
        tud_cdc_n_available(OST_CDC_ITF) > 0u) {
        ost_persist_last_ms = context.timer_ms;
        return;
    }

    if ((uint32_t)(context.timer_ms - ost_persist_last_ms) < OST_PERSIST_IDLE_MS) {
        return;
    }

    if (ost_persist_phys_off == OST_PERSIST_OFF_INVALID) {
        ost_persist_phys_off = 0u;
    }

    bool more = ost_persist_commit_block();
    if (!more) {
        ost_persist_dirty = false;
        ost_persist_phys_off = OST_PERSIST_OFF_INVALID;
    }
}

static inline uint8_t ost_sum_bytes(const uint8_t *buf, uint32_t len) {
    uint8_t sum = 0;
    for (uint32_t i = 0; i < len; i++) {
        sum = (uint8_t)(sum + buf[i]);
    }
    return sum;
}

static bool ost_send_bytes(const uint8_t *buf, uint32_t len) {
    if (len == 0u) {
        return true;
    }

    uint32_t sent = 0u;
    while (sent < len) {
        uint32_t avail = tud_cdc_n_write_available(OST_CDC_ITF);
        if (avail == 0u) {
            tud_cdc_n_write_flush(OST_CDC_ITF);
            tud_task();
            continue;
        }

        uint32_t chunk = len - sent;
        if (chunk > avail) {
            chunk = avail;
        }

        uint32_t wrote = tud_cdc_n_write(OST_CDC_ITF, &buf[sent], chunk);
        if (wrote == 0u) {
            tud_cdc_n_write_flush(OST_CDC_ITF);
            tud_task();
            continue;
        }
        sent += wrote;
    }

    tud_cdc_n_write_flush(OST_CDC_ITF);
    return true;
}

static bool ost_send_byte(uint8_t v) {
    return ost_send_bytes(&v, 1u);
}

static bool ost_txq_push(uint8_t v) {
    if (ost.txq_count >= OST_TXQ_SIZE) {
        return false;
    }

    ost.txq[ost.txq_tail] = v;
    ost.txq_tail = (uint8_t)((ost.txq_tail + 1u) % OST_TXQ_SIZE);
    ost.txq_count++;
    return true;
}

static void ost_txq_drain(void) {
    while (ost.txq_count > 0u) {
        uint32_t avail = tud_cdc_n_write_available(OST_CDC_ITF);
        if (avail == 0u) {
            break;
        }

        uint8_t v = ost.txq[ost.txq_head];
        if (tud_cdc_n_write(OST_CDC_ITF, &v, 1u) != 1u) {
            break;
        }

        ost.txq_head = (uint8_t)((ost.txq_head + 1u) % OST_TXQ_SIZE);
        ost.txq_count--;
    }

    if (ost.txq_count > 0u) {
        tud_cdc_n_write_flush(OST_CDC_ITF);
    }
}

static void ost_send_or_queue_byte(uint8_t v) {
    led_note_activity();
    ost_datalog_pause_until_ms = context.timer_ms + OST_DATALOG_PAUSE_HOLD_MS;

    if (tud_cdc_n_write_available(OST_CDC_ITF) > 0u) {
        if (tud_cdc_n_write(OST_CDC_ITF, &v, 1u) == 1u) {
            tud_cdc_n_write_flush(OST_CDC_ITF);
            return;
        }
    }

    (void)ost_txq_push(v);
}

static bool ost_addr_in_bounds(uint32_t addr, uint32_t size, uint32_t limit) {
    return (addr <= limit) && (size <= (limit - addr));
}

static uint32_t ost_get_logical_rom_size(void) {
    uint32_t logical = app_get_active_rom_size(&context);
    if (logical != 0u) {
        return logical;
    }

    // Fallback when metadata isn't ready yet.
    if (context.runtime != NULL) {
        return context.runtime->rom_table_size;
    }

    return 0u;
}

static bool ost_normalize_addr(
    uint32_t host_addr,
    uint32_t size,
    uint32_t rom_size,
    uint32_t *rom_addr_out,
    bool *wrap_27256_out
) {
    *wrap_27256_out = false;

    // Default mapping: host address directly indexes the active ROM image.
    if (ost_addr_in_bounds(host_addr, size, rom_size)) {
        *rom_addr_out = host_addr;
        return true;
    }

    // 27256-compatible mirrored 64KB host window.
    // Accept any range fully contained in 0x0000..0xFFFF and map with wrap.
    if (rom_size == 0x8000u && host_addr <= 0xFFFFu && size <= (0x10000u - host_addr)) {
        *rom_addr_out = (host_addr & 0x7FFFu);
        *wrap_27256_out = true;
        return true;
    }

    return false;
}

static void ost_reset_cmd(void) {
    ost.cmd_len = 0;
}

static bool ost_try_handle_read_or_write_cmd(void) {
    // Header layouts:
    //   R size ah al cksum
    //   W size ah al
    //   Z R count ah al cksum
    //   Z W count ah al
    bool bulk = false;
    uint8_t cmd = 0;
    uint32_t hdr_len = 0;
    bool has_cmd_checksum = false;

    if (ost.cmd_len >= 1u && ost.cmd_buf[0] == 'Z') {
        bulk = true;
        if (ost.cmd_len < 2u) {
            return false;
        }
        cmd = ost.cmd_buf[1];
        hdr_len = (cmd == 'R') ? 6u : (cmd == 'W' ? 5u : 0u);
        has_cmd_checksum = (cmd == 'R');
    } else if (ost.cmd_len >= 1u) {
        cmd = ost.cmd_buf[0];
        hdr_len = (cmd == 'R') ? 5u : (cmd == 'W' ? 4u : 0u);
        has_cmd_checksum = (cmd == 'R');
    }

    if (hdr_len == 0u || ost.cmd_len < hdr_len) {
        return false;
    }

    if (has_cmd_checksum) {
        uint8_t exp = ost_sum_bytes(ost.cmd_buf, hdr_len - 1u);
        if (ost.cmd_buf[hdr_len - 1u] != exp) {
            ost_reset_cmd();
            return true;
        }
    }

    uint8_t size_or_count = bulk ? ost.cmd_buf[2] : ost.cmd_buf[1];
    uint8_t addr_hi = bulk ? ost.cmd_buf[3] : ost.cmd_buf[2];
    uint8_t addr_lo = bulk ? ost.cmd_buf[4] : ost.cmd_buf[3];

    uint32_t host_addr = ((uint32_t)addr_hi << 8) | (uint32_t)addr_lo;
    uint32_t size = (uint32_t)size_or_count;

    if (bulk) {
        if (size == 0u) {
            // Moates protocol uses 0 to represent 256 bulk blocks.
            size = 256u;
        }
        host_addr *= 256u;
        size *= 256u;
    } else if (size == 0u) {
        // Defensive compatibility for hosts using 0 as a 256-byte block.
        size = 256u;
    }

    uint32_t rom_size = ost_get_logical_rom_size();

    uint32_t addr = 0u;
    bool wrap_27256 = false;
    if (!ost_normalize_addr(host_addr, size, rom_size, &addr, &wrap_27256)) {
        ost_reset_cmd();
        return true;
    }

    if (cmd == 'R') {
        // Read response: data bytes then checksum.
        rom_pin_layout_t layout;
        if (app_retrieve_pin_layout(&context, &layout) != PB_STATUS_OK) {
            ost_reset_cmd();
            return true;
        }

        uint8_t sum = 0u;
        uint8_t chunk[64];
        uint32_t chunk_len = 0u;
        for (uint32_t i = 0u; i < size; i++) {
            uint32_t v = 0u;
            uint32_t logical_addr = wrap_27256 ? ((addr + i) & 0x7FFFu) : (addr + i);
            if (app_get_logical_byte_from_logical_addr(logical_addr, &v, &layout, &context) != PB_STATUS_OK) {
                ost_reset_cmd();
                return true;
            }

            uint8_t b = (uint8_t)(v & 0xFFu);
            sum = (uint8_t)(sum + b);
            chunk[chunk_len++] = b;
            if (chunk_len == sizeof(chunk)) {
                (void)ost_send_bytes(chunk, chunk_len);
                chunk_len = 0u;
            }
        }

        if (chunk_len != 0u) {
            (void)ost_send_bytes(chunk, chunk_len);
        }

        (void)ost_send_byte(sum);
        ost_reset_cmd();
        return true;
    }

    // Write response/flow: host sends payload bytes then one checksum byte,
    // device acknowledges with 'O' on success.
    ost.mode = OST_MODE_WRITE_DATA;
    ost_persist_touch();
    ost.wr_addr = addr;
    ost.wr_size = size;
    ost.wr_pos = 0;
    ost.wr_wrap_27256 = wrap_27256;
    // Moates write checksum includes command bytes plus payload bytes.
    ost.wr_sum = ost_sum_bytes(ost.cmd_buf, hdr_len);
    ost.wr_layout_valid = (app_retrieve_pin_layout(&context, &ost.wr_layout) == PB_STATUS_OK);
    ost_reset_cmd();
    return true;
}

static bool ost_try_handle_simple_cmd(void) {
    // VV -> version triple
    if (ost.cmd_len >= 2u && ost.cmd_buf[0] == 'V' && ost.cmd_buf[1] == 'V') {
        const uint8_t resp[3] = { OST_HW_VER, OST_FW_VER, OST_HW_CH };
        (void)ost_send_bytes(resp, sizeof(resp));
        ost_reset_cmd();
        return true;
    }

    // NS + checksum -> vendor + serial bytes + checksum.
    if (ost.cmd_len >= 3u && ost.cmd_buf[0] == 'N' && ost.cmd_buf[1] == 'S') {
        uint8_t exp = (uint8_t)('N' + 'S');
        if (ost.cmd_buf[2] == exp) {
            uint8_t resp[1 + sizeof(OST_SERIAL_STR) - 1 + 1];
            resp[0] = OST_VENDOR_ID;
            memcpy(&resp[1], OST_SERIAL_STR, sizeof(OST_SERIAL_STR) - 1);
            resp[sizeof(resp) - 1] = ost_sum_bytes(resp, sizeof(resp) - 1);
            ost_send_bytes(resp, sizeof(resp));
        }
        ost_reset_cmd();
        return true;
    }

    // Bank command + checksum.
    // Set forms:
    //   B E <bank> + cksum  (set emu bank)
    //   B R <bank> + cksum  (set update bank)
    //   B S <bank> + cksum  (set persistent bank)
    // Get forms:
    //   B E R + cksum       (get emu bank)
    //   B R R + cksum       (get update bank)
    //   B E S + cksum       (get persistent bank)
    if (ost.cmd_len >= 4u && ost.cmd_buf[0] == 'B') {
        uint8_t exp = ost_sum_bytes(ost.cmd_buf, 3u);
        if (ost.cmd_buf[3] == exp) {
            uint8_t op = ost.cmd_buf[1];
            uint8_t arg = ost.cmd_buf[2];

            if (op == 'E' && arg == 'R') {
                ost_send_or_queue_byte(ost.bank_emu);
            } else if (op == 'R' && arg == 'R') {
                ost_send_or_queue_byte(ost.bank_update);
            } else if ((op == 'E' && arg == 'S') || (op == 'S' && arg == 'R')) {
                ost_send_or_queue_byte(ost.bank_persist);
            } else if (op == 'E') {
                // One ROM USB currently exposes a single active RAM bank.
                // Keep emulator-visible bank fixed to 0 for Ostrich compatibility.
                (void)arg;
                ost.bank_emu = 0u;
                ost_send_or_queue_byte((uint8_t)'O');
            } else if (op == 'R') {
                (void)arg;
                ost.bank_update = 0u;
                ost_send_or_queue_byte((uint8_t)'O');
            } else if (op == 'S') {
                (void)arg;
                ost.bank_persist = 0u;
                ost_send_or_queue_byte((uint8_t)'O');
            }
        }
        ost_reset_cmd();
        return true;
    }

    // S <div> + checksum -> acknowledge 'O'.
    if (ost.cmd_len >= 3u && ost.cmd_buf[0] == 'S') {
        uint8_t exp = (uint8_t)(ost.cmd_buf[0] + ost.cmd_buf[1]);
        if (ost.cmd_buf[2] == exp) {
            ost_send_or_queue_byte((uint8_t)'O');
        }
        ost_reset_cmd();
        return true;
    }

    return false;
}

static void ost_process_cmd_byte(uint8_t b) {
    if (ost.cmd_len < OST_CMD_BUF_MAX) {
        ost.cmd_buf[ost.cmd_len++] = b;
    } else {
        // Overflow or desync: reset parser state to recover quickly.
        ost_reset_cmd();
        return;
    }

    if (ost_try_handle_simple_cmd()) {
        return;
    }

    (void)ost_try_handle_read_or_write_cmd();
}

static void ost_process_write_data_byte(uint8_t b) {
    ost_persist_touch();

    uint32_t rom_size = ost_get_logical_rom_size();
    if (!ost.wr_layout_valid) {
        ost.mode = OST_MODE_CMD;
        return;
    }

    if (ost.wr_wrap_27256) {
        if (rom_size != 0x8000u) {
            ost.mode = OST_MODE_CMD;
            return;
        }
    } else if (!ost_addr_in_bounds(ost.wr_addr, ost.wr_size, rom_size)) {
        ost.mode = OST_MODE_CMD;
        return;
    }

    if (ost.wr_pos < ost.wr_size) {
        uint32_t logical_addr = ost.wr_wrap_27256
                                ? ((ost.wr_addr + ost.wr_pos) & 0x7FFFu)
                                : (ost.wr_addr + ost.wr_pos);
        if (app_set_logical_byte_at_logical_addr(
                logical_addr,
                b,
                &ost.wr_layout,
                &context
            ) != PB_STATUS_OK) {
            ost.mode = OST_MODE_CMD;
            return;
        }
        ost.wr_sum = (uint8_t)(ost.wr_sum + b);
        ost.wr_pos++;
        return;
    }

    // Checksum byte.
    if (b == ost.wr_sum) {
        ost_send_or_queue_byte((uint8_t)'O');
        ost_persist_mark();
    }

    ost.mode = OST_MODE_CMD;
}

void usb_ostrich_init(void) {
    memset(&ost, 0, sizeof(ost));
    ost.mode = OST_MODE_CMD;
    ost.bank_emu = 0u;
    ost.bank_update = 0u;
    ost.bank_persist = 0u;
    ost.wr_layout_valid = false;
    ost_persist_dirty = false;
    ost_persist_last_ms = 0u;
    ost_persist_phys_off = OST_PERSIST_OFF_INVALID;
    ost_datalog_pause_until_ms = 0u;
}

bool usb_ostrich_pause_datalog(void) {
    if (ost.mode == OST_MODE_WRITE_DATA) {
        return true;
    }

    if (ost.txq_count > 0u) {
        return true;
    }

    if (tud_cdc_n_available(OST_CDC_ITF) > 0u) {
        return true;
    }

    return ((int32_t)(ost_datalog_pause_until_ms - context.timer_ms) > 0);
}

void usb_ostrich_task(void) {
    ost_txq_drain();

    if (tud_cdc_n_available(OST_CDC_ITF) > 0u) {
        led_note_activity();
        ost_datalog_pause_until_ms = context.timer_ms + OST_DATALOG_PAUSE_HOLD_MS;
    }

    // Avoid starving TinyUSB when host sends bursts of commands.
    // Process a bounded amount of RX per scheduler pass while in command mode,
    // but always finish an in-progress write frame (data + checksum) to avoid
    // desync when host sends successive updates quickly.
    uint32_t budget = OST_RX_BUDGET_PER_TASK;
    while (tud_cdc_n_available(OST_CDC_ITF)) {
        if (ost.mode == OST_MODE_CMD && budget == 0u) {
            break;
        }

        uint8_t b = (uint8_t)tud_cdc_n_read_char(OST_CDC_ITF);
        bool was_cmd_mode = (ost.mode == OST_MODE_CMD);

        if (ost.mode == OST_MODE_WRITE_DATA) {
            ost_process_write_data_byte(b);
        } else {
            ost_process_cmd_byte(b);
        }

        ost_datalog_pause_until_ms = context.timer_ms + OST_DATALOG_PAUSE_HOLD_MS;

        if (was_cmd_mode && budget > 0u) {
            budget--;
        }
    }

    ost_txq_drain();

    ost_persist_task();
}
