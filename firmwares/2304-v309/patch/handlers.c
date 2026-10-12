/*
 * handlers.c — Standard RT & Extension patch for MonsGeek FUN60 Pro (dev_id 2304, fw v309)
 *
 * Provides two hooks:
 *   1. handle_rt_eval (filter on rt_eval_per_key @ 0x08001FE8):
 *      When g_rt_flavour == 0 (traditional / standard RT):
 *        Checks if current travel depth d has risen above the actuation threshold act.
 *        If d < act: disarms RT by clearing bit key_idx in [state_base + 0x834],
 *        resets cumulative displacement tracker, and returns 1 (skip original CRT logic).
 *        If d >= act: returns 0 (let original firmware handle downward firing).
 *      When g_rt_flavour == 1 (continuous RT / CRT):
 *        Returns 0 (passes through to original firmware CRT logic unmodified).
 *
 *   2. handle_vendor_cmd (filter on vendor_command_dispatch @ 0x08011BF8):
 *      Intercepts vendor commands before the stock switch table:
 *        - 0xE7: GET_PATCH_INFO — returns magic 0xCAFE, version 1, capability standard_rt
 *        - 0x1F: SET_RTMODE     — sets g_rt_flavour (0 = traditional, 1 = continuous)
 *        - 0x9F: GET_RTMODE     — returns current g_rt_flavour in byte 1 of response payload
 *      All other commands return 0 (passthrough to original firmware dispatch).
 */

#include <stdint.h>

/* ── Linker-provided BSS boundaries ─────────────────────────────────────── */

extern uint8_t __patch_bss_start[];
extern uint8_t __patch_bss_end[];

static void zero_patch_bss(void) {
    for (uint8_t *p = __patch_bss_start; p < __patch_bss_end; p++)
        *p = 0;
}

/* ── Patch State (stored in PATCH_SRAM .bss) ─────────────────────────────── */

static uint8_t patch_initialized;

/* Rapid Trigger flavor:
 *   0 = traditional / standard RT (disarm when travel rises above actuation) [DEFAULT]
 *   1 = continuous RT (CRT: stock behavior)
 */
static uint8_t g_rt_flavour;

/* ── Firmware Constants ─────────────────────────────────────────────────── */

/* RT state struct: ctx + 12 + key_idx * 44 (stride confirmed from 0x1FF4) */
#define RT_STATE_OFFSET   12u
#define RT_STATE_STRIDE   44u

#define RT_DEPTH_OFF      0x18u   /* u16: current travel depth d */
#define RT_ACTU_OFF       0x1Au   /* u16: actuation threshold */
#define RT_CUMUL_OFF      0x1Cu   /* u32: cumulative displacement */

/* Armed bitmask: state_base + 0x834 */
#define ARMED_MASK_OFF    0x834u

/* Vendor command buffer in SRAM (confirmed from literal pool at 0x08011FF8) */
#define VENDOR_CMD_BUF    ((volatile uint8_t *)0x20006d14)

/* ── Rapid Trigger Evaluation Hook ───────────────────────────────────────── */

static inline void clear_armed(uint32_t state_base, uint32_t key_idx) {
    volatile uint32_t *mask = (volatile uint32_t *)(state_base + ARMED_MASK_OFF);
    *mask &= ~(1u << key_idx);
}

int handle_rt_eval(uint32_t ctx, uint32_t key_idx) {
    if (!patch_initialized) {
        zero_patch_bss();
        patch_initialized = 1;
    }

    /* If user configured continuous RT, pass through to stock CRT */
    if (g_rt_flavour != 0) {
        return 0;
    }

    uint32_t state_base = *(volatile uint32_t *)ctx;
    uint8_t *s = (uint8_t *)(ctx + RT_STATE_OFFSET + key_idx * RT_STATE_STRIDE);

    uint16_t d   = *(uint16_t *)(s + RT_DEPTH_OFF);
    uint16_t act = *(uint16_t *)(s + RT_ACTU_OFF);

    /* Standard RT: dis-arm if key has risen above actuation point */
    if (d < act) {
        clear_armed(state_base, key_idx);
        *(uint32_t *)(s + RT_CUMUL_OFF) = 0;
        return 1;   /* intercepted — skip stock CRT re-fire */
    }

    return 0;   /* pass through to stock for normal downward movement */
}

/* Turbo Mode state:
 *   0 = Normal (housekeeping and RGB tick active) [DEFAULT]
 *   1 = Turbo / Zero-Jitter (main loop tick locked at 0)
 */
static uint8_t g_turbo_mode;

/* Priority Scan state:
 *   0 = Off (full matrix scan every pass) [DEFAULT]
 *   1 = FPS / WASD cluster preset (~38 kHz on movement keys)
 *   2 = MOBA cluster preset (QWER, DF, 1-4, B, Space)
 *   0xFF = Custom 64-bit priority mask
 */
static uint8_t g_priority_mode;
static uint64_t g_active_priority_mask;

#define MASK_TABLE_ADDR   0x2000283Eu
#define MASK_TABLE_SIZE   84u
#define MAIN_LOOP_TICK    ((volatile uint8_t *)0x20000468)

static uint8_t g_stock_mask[MASK_TABLE_SIZE];
static uint8_t g_priority_mask[MASK_TABLE_SIZE];
static uint8_t g_masks_initialized;
static uint8_t g_scan_pass;

/* FPS / WASD cluster:
 * 4: Shift_L, 5: Ctrl_L, 8: Q, 9: A, 14: W, 15: S, 20: E, 21: D, 26: R, 27: F, 41: Space
 */
#define PRIORITY_MASK_FPS  (\
    (1ULL << 4)  | (1ULL << 5)  | (1ULL << 8)  | (1ULL << 9)  | \
    (1ULL << 14) | (1ULL << 15) | (1ULL << 20) | (1ULL << 21) | \
    (1ULL << 26) | (1ULL << 27) | (1ULL << 41) )

/* MOBA cluster:
 * 2: 1, 7: 2, 13: 3, 19: 4, 8: Q, 14: W, 20: E, 26: R, 21: D, 27: F, 33: B, 41: Space
 */
#define PRIORITY_MASK_MOBA (\
    (1ULL << 2)  | (1ULL << 7)  | (1ULL << 13) | (1ULL << 19) | \
    (1ULL << 8)  | (1ULL << 14) | (1ULL << 20) | (1ULL << 26) | \
    (1ULL << 21) | (1ULL << 27) | (1ULL << 33) | (1ULL << 41) )

static void rebuild_priority_mask(uint64_t mask) {
    for (uint32_t i = 0; i < MASK_TABLE_SIZE; i++) {
        uint8_t m = g_stock_mask[i];
        if (m != 0) {
            /* Unpopulated switch position */
            g_priority_mask[i] = m;
        } else if (i < 64 && ((mask & (1ULL << i)) != 0)) {
            /* Priority key: sample on priority passes */
            g_priority_mask[i] = 0;
        } else {
            /* Non-priority key: skip on fast priority passes */
            g_priority_mask[i] = 1;
        }
    }
}

static void init_turbo_masks(void) {
    volatile uint8_t *hw_mask = (volatile uint8_t *)MASK_TABLE_ADDR;
    for (uint32_t i = 0; i < MASK_TABLE_SIZE; i++) {
        g_stock_mask[i] = hw_mask[i];
    }
    rebuild_priority_mask(PRIORITY_MASK_FPS);
    g_masks_initialized = 1;
}

/* ── Matrix Scan Hook ─────────────────────────────────────────────────────── */

int handle_matrix_scan(void) {
    if (!patch_initialized) {
        zero_patch_bss();
        patch_initialized = 1;
    }

    if (!g_masks_initialized) {
        init_turbo_masks();
    }

    if (g_turbo_mode != 0) {
        /* Keep main loop tick counter at 0 to suppress periodic RGB/housekeeping pauses */
        *MAIN_LOOP_TICK = 0;

        if (g_priority_mode != 0) {
            /* Interleaved scan: 3 priority passes (0, 1, 2) per 1 full pass (3) */
            volatile uint8_t *hw_mask = (volatile uint8_t *)MASK_TABLE_ADDR;
            g_scan_pass = (g_scan_pass + 1) & 3;

            const uint8_t *src = (g_scan_pass == 3) ? g_stock_mask : g_priority_mask;
            for (uint32_t i = 0; i < MASK_TABLE_SIZE; i++) {
                hw_mask[i] = src[i];
            }
        } else if (g_scan_pass != 0) {
            /* Restore full stock mask if leaving priority mode */
            volatile uint8_t *hw_mask = (volatile uint8_t *)MASK_TABLE_ADDR;
            for (uint32_t i = 0; i < MASK_TABLE_SIZE; i++) {
                hw_mask[i] = g_stock_mask[i];
            }
            g_scan_pass = 0;
        }
    } else {
        /* Turbo is off: priority scan cannot run without zero-jitter loop */
        if (g_scan_pass != 0) {
            volatile uint8_t *hw_mask = (volatile uint8_t *)MASK_TABLE_ADDR;
            for (uint32_t i = 0; i < MASK_TABLE_SIZE; i++) {
                hw_mask[i] = g_stock_mask[i];
            }
            g_scan_pass = 0;
        }
    }

    return 0;   /* Proceed with matrix_scan */
}

/* ── Vendor Command Hook ─────────────────────────────────────────────────── */

/* Capabilities:
 *   bit 7: standard_rt
 *   bit 8: fast_scan (raw 1-sample 0.25ms ADC scan)
 *   bit 9: turbo (zero-jitter tight loop)
 *   bit 10: priority_scan (dynamic multi-rate priority scanning)
 */
#define CAP_STANDARD_RT   (1u << 7)
#define CAP_FAST_SCAN     (1u << 8)
#define CAP_TURBO         (1u << 9)
#define CAP_PRIORITY_SCAN (1u << 10)
#define CAPABILITIES      (CAP_STANDARD_RT | CAP_FAST_SCAN | CAP_TURBO | CAP_PRIORITY_SCAN)

int handle_vendor_cmd(void) {
    if (!patch_initialized) {
        zero_patch_bss();
        patch_initialized = 1;
    }

    volatile uint8_t *cmd_buf = VENDOR_CMD_BUF;

    /* cmd_buf[0] is set non-zero when a new command is received */
    if (cmd_buf[0] == 0)
        return 0;

    uint8_t cmd = cmd_buf[2];

    switch (cmd) {
    case 0xE7: {
        /* GET_PATCH_INFO
         * Response payload layout:
         *   cmd_buf[3] = 0xCA (MAGIC_HI)
         *   cmd_buf[4] = 0xFE (MAGIC_LO)
         *   cmd_buf[5] = 1    (patch version)
         *   cmd_buf[6] = 0x80 (capabilities lo: bit 7 = standard_rt)
         *   cmd_buf[7] = 0x07 (capabilities hi: bit 8 = fast_scan, bit 9 = turbo, bit 10 = priority_scan)
         *   cmd_buf[8..15] = "MONSMOD\0"
         */
        cmd_buf[3]  = 0xCA;
        cmd_buf[4]  = 0xFE;
        cmd_buf[5]  = 1;
        cmd_buf[6]  = (uint8_t)(CAPABILITIES & 0xFF);
        cmd_buf[7]  = (uint8_t)((CAPABILITIES >> 8) & 0xFF);
        cmd_buf[8]  = 'M';
        cmd_buf[9]  = 'O';
        cmd_buf[10] = 'N';
        cmd_buf[11] = 'S';
        cmd_buf[12] = 'M';
        cmd_buf[13] = 'O';
        cmd_buf[14] = 'D';
        cmd_buf[15] = '\0';
        cmd_buf[0]  = 0;   /* mark command handled */
        return 1;          /* intercepted */
    }

    case 0x1F: {
        /* SET_RTMODE
         * Request: cmd_buf[3] = mode (0 = traditional, 1 = continuous)
         */
        g_rt_flavour = cmd_buf[3] & 1;
        cmd_buf[0]   = 0;  /* mark command handled */
        return 1;          /* intercepted */
    }

    case 0x9F: {
        /* GET_RTMODE
         * Response: cmd_buf[3] = mode (0 = traditional, 1 = continuous)
         */
        cmd_buf[3] = g_rt_flavour;
        cmd_buf[0] = 0;   /* mark command handled */
        return 1;         /* intercepted */
    }

    case 0x20: {
        /* SET_TURBO (global zero-jitter toggle)
         * Request: cmd_buf[3] = mode (0 = off, 1 = on)
         */
        g_turbo_mode = cmd_buf[3] & 1;
        cmd_buf[0]   = 0;  /* mark command handled */
        return 1;          /* intercepted */
    }

    case 0xA0: {
        /* GET_TURBO
         * Response: cmd_buf[3] = mode (0 = off, 1 = on)
         */
        cmd_buf[3] = g_turbo_mode;
        cmd_buf[0] = 0;   /* mark command handled */
        return 1;         /* intercepted */
    }

    case 0x21: {
        /* SET_PRIORITY_MASK
         * Request:
         *   cmd_buf[3] = mode (0 = off, 1 = FPS, 2 = MOBA, 0xFF = custom)
         *   cmd_buf[4..11] = 64-bit custom bitmask (little-endian, when mode == 0xFF)
         */
        uint8_t mode = cmd_buf[3];
        uint64_t mask = 0;
        if (mode == 1) {
            mask = PRIORITY_MASK_FPS;
        } else if (mode == 2) {
            mask = PRIORITY_MASK_MOBA;
        } else if (mode == 0xFF) {
            for (uint32_t i = 0; i < 8; i++) {
                mask |= ((uint64_t)cmd_buf[4 + i]) << (i * 8);
            }
        }
        g_priority_mode = mode;
        g_active_priority_mask = mask;
        rebuild_priority_mask(mask);
        cmd_buf[0] = 0;   /* mark command handled */
        return 1;         /* intercepted */
    }

    case 0xA1: {
        /* GET_PRIORITY_MASK
         * Response:
         *   cmd_buf[3] = mode (0 = off, 1 = FPS, 2 = MOBA, 0xFF = custom)
         *   cmd_buf[4..11] = active 64-bit bitmask (little-endian)
         */
        cmd_buf[3] = g_priority_mode;
        for (uint32_t i = 0; i < 8; i++) {
            cmd_buf[4 + i] = (uint8_t)((g_active_priority_mask >> (i * 8)) & 0xFF);
        }
        cmd_buf[0] = 0;   /* mark command handled */
        return 1;         /* intercepted */
    }

    default:
        return 0;   /* passthrough to stock firmware */
    }
}
