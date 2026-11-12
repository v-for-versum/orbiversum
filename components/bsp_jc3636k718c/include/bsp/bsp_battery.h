/*
 * Battery voltage sense.
 *
 * THERE IS NO FUEL GAUGE ON THIS BOARD, AND NO CHARGE-STATUS LINE EITHER — and the
 * schematic settles that second half rather than assuming it: the charger's STAT pin
 * drives an LED on the board, and no net from it reaches the ESP32 at all.
 *
 * The cell reaches GPIO6 through a divider. That is the whole of what the board can say
 * about the battery: one voltage. Everything else — the percentage below, and any claim
 * about charging — is arithmetic laid on top of that number, and this module keeps that
 * arithmetic in one place so the application never invents its own.
 *
 * The divider ratio is a constant here, and it is still a fitted one — but no longer a
 * guess between two theories. Guition's own documentation set for this board does include
 * the schematic, and its text was read: the sense line leaves the power sheet on GPIO6,
 * beside the cell connector (a 2-pin HX1.25) and the charger, which is an ETA6003. What
 * that sheet does NOT give up is the resistor pair itself. See BSP_BATTERY_DIVIDER.
 *
 * A reading is CACHED for a few seconds on purpose, and then SMOOTHED over about a minute
 * and a half. The ESP32-S3's ADC jitters by tens of millivolts, and a percentage that
 * danced with it would be unreadable: measured on the instrument, the pin wandered between
 * 2104 and 2275 mV between reads five seconds apart, which near the top of a lithium curve
 * is eight percent of charge. The cell moves over hours, so the smoothing is still far
 * faster than the thing it measures. The cell also sags under load, so a percentage taken
 * while the instrument is playing is a little low, and that is honest rather than wrong.
 *
 * The functions that report the pin report the SMOOTHED value; the boot line is the one
 * place the raw burst is printed, and it is the number to calibrate the divider against.
 */
#pragma once

#include <stdbool.h>

#include "esp_err.h"

#ifdef __cplusplus
extern "C" {
#endif

/*
 * The divider, as a multiplier: pin millivolts times this is the cell's millivolts.
 *
 * It is the one number here that the firmware does not measure. It is 1.90 and not the 2.00
 * that was guessed first, and there are now two independent arguments rather than one.
 *
 *   . The arithmetic. On the first boot with the sense line working the pin read 2211 mV.
 *     Through 1:2 that is 4.42 V at the cell, and no lithium cell holds that: 4.20 V is the
 *     charge voltage and 4.35 V the highest chemistry that exists. The ADC's own one
 *     percent could not close a gap that wide. So the true multiplier is at most about
 *     1.92, and 1.90 is the value that makes a cell held at the charger's 4.20 V read full.
 *
 *   . The schematic, which kills the alternative theory rather than merely not supporting
 *     it. The worry was that the pin might be a fixed rail — 3.3 V through 1:1.5 is 2200 mV,
 *     within half a percent of what was read. It cannot be: 3V3 on this board comes from a
 *     TLV62569 buck fed by the 5 V that exists only while USB is attached, so with the cable
 *     out there is no 3V3 to read. The line nonetheless reported a cell — 47%, about
 *     3.82 V. A rail cannot do that, and the power sheet agrees: the sense line sits with
 *     the charger's BATT node and the cell connector, not with a regulator.
 *
 * What stays unmeasured is the resistor pair. The only candidates the schematic's text puts
 * near the cell connector are 10k and 10k, and those cannot be the divider for the same
 * reason as above — they would report 4.4 V at a full cell. So this is fitted, not read, and
 * the fitting is good to the ADC's error if the real pair is anywhere near 1.90. A
 * multimeter across the cell would make it exact: cell_mV / pin_mV.
 *
 * One consequence of the design belongs here, because it explains a reading that looks like
 * a fault and is not. The charger is an ETA6003, a power-path part, so while USB is attached
 * the cell node is held at the charger's regulated voltage rather than at whatever the cell
 * is doing. The percentage then reads full and is telling the truth — the cell really is at
 * 4.2 V. The instant the cable comes out it drops to the cell's loaded voltage: 3.82 V,
 * about 47%, in the one measurement available here. That step is the power path plus the
 * cell's internal resistance. It is not a broken sense line, and no software can remove it.
 */
#define BSP_BATTERY_DIVIDER     1.90f

/*
 * Bring up the ADC channel the cell is on.
 *
 * Returns ESP_ERR_NOT_FOUND if the sense pin is not an ADC pin on this chip. That is not
 * fatal and the rest of the board still boots; the percentage then reads as unavailable.
 */
esp_err_t bsp_battery_init(void);

/* True once bsp_battery_init() has found a usable channel. */
bool bsp_battery_is_available(void);

/*
 * The divider's own output, in millivolts: a burst average, then smoothed as above. This is
 * the number to compare with a multimeter when settling BSP_BATTERY_DIVIDER; the display
 * never shows it.
 */
int bsp_battery_pin_mv(void);

/* The cell's millivolts: the pin's, times the divider. 0 when nothing could be read. */
int bsp_battery_mv(void);

/*
 * State of charge, 0..100, or -1 when there is no reading to give.
 *
 * -1 is a real answer and the caller should show it as such rather than as an empty
 * battery: an unreadable sense line and a flat cell look nothing alike.
 */
int bsp_battery_percent(void);

/*
 * True when the cell node looks like it is being held up by the charger — that is, when the
 * instrument is on USB.
 *
 * This is INFERENCE and not a measurement, the only claim of its kind in this module, and it
 * exists because the board has no charge-status line (see the top of this header). It reads
 * the consequence instead: a power-path charger holds the cell node at its own regulated
 * voltage whenever the cable is attached, while a cell running the instrument alone sits at
 * its own state of charge. At the pin those are about 200 mV apart, which is three times the
 * line's own wander, so this decides on the RAW reading with two thresholds and flips within
 * one five-second reading. The percentage is smoothed separately and keeps its own, slower
 * pace — the two deliberately do not share a time constant.
 *
 * There is still no trend test behind it, and there cannot be: a trend tight enough to catch
 * the remaining ambiguity would fire on the wander instead. So a cell that is full, at rest
 * and off the cable reads as USB until the instrument's load pulls it down. Treat it as a
 * label on a readout, never as an input to anything that decides.
 */
bool bsp_battery_on_usb(void);

#ifdef __cplusplus
}
#endif
