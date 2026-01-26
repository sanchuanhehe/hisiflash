# HiSilicon SEBOOT Protocol Specification

This document describes the SEBOOT protocol used by HiSilicon chips (WS63, BS2X series) for firmware flashing.

## Overview

SEBOOT (Secure Boot) is the bootloader protocol used by HiSilicon chips. It enables:
- Firmware downloading via UART
- eFuse/OTP programming
- Flash memory management
- Device configuration

The protocol is based on a simple frame format with CRC16 verification.

## Frame Format

All SEBOOT commands use the following frame structure:

```
+------------+--------+------+-------+---------------+--------+
|   Magic    | Length | Type | ~Type |     Data      | CRC16  |
+------------+--------+------+-------+---------------+--------+
|  4 bytes   | 2 bytes| 1 B  |  1 B  |   variable    | 2 bytes|
+------------+--------+------+-------+---------------+--------+
```

### Field Descriptions

| Field | Size | Description |
|-------|------|-------------|
| Magic | 4 bytes | Always `0xDEADBEEF` (little-endian: `EF BE AD DE`) |
| Length | 2 bytes | Total frame length including all fields (little-endian) |
| Type | 1 byte | Command type code |
| ~Type | 1 byte | Bitwise complement of Type (for validation) |
| Data | variable | Command-specific payload |
| CRC16 | 2 bytes | CRC16-XMODEM of all preceding bytes (little-endian) |

### CRC Calculation

The CRC16 is calculated using the XMODEM polynomial:
- Polynomial: `0x1021`
- Initial value: `0x0000`
- No final XOR
- Input/output not reflected

```rust
fn crc16_xmodem(data: &[u8]) -> u16 {
    let mut crc: u16 = 0x0000;
    for byte in data {
        crc ^= (*byte as u16) << 8;
        for _ in 0..8 {
            if crc & 0x8000 != 0 {
                crc = (crc << 1) ^ 0x1021;
            } else {
                crc <<= 1;
            }
        }
    }
    crc
}
```

## Command Types

| Code | Name | Description |
|------|------|-------------|
| `0xF0` | Handshake | Establish connection and negotiate baud rate |
| `0xE1` | ACK | Response frame from device |
| `0xD2` | DownloadFlashImage | Download firmware to flash |
| `0xC3` | DownloadOtpEfuse | Write OTP/eFuse data |
| `0xB4` | UploadData | Read data from device |
| `0xA5` | ReadOtpEfuse | Read OTP/eFuse data |
| `0x96` | FlashLock | Lock flash regions |
| `0x87` | Reset | Reset the device |
| `0x78` | DownloadFactoryBin | Download factory calibration data |
| `0x69` | DownloadVersion | Download version information |
| `0x5A` | SetBaudRate | Change UART baud rate |
| `0x4B` | DownloadNv | Download NV (Non-Volatile) data |
| `0x1E` | SwitchDfu | Switch to DFU mode |

## Command Details

### Handshake (0xF0)

Establishes connection and negotiates communication parameters.

**Frame structure (18 bytes total):**
```
+--------+--------+------+-------+----------+----------+----------+----------+--------+
| Magic  | Length | Type | ~Type | BaudRate | DataBits | StopBits | Parity   | CRC16  |
+--------+--------+------+-------+----------+----------+----------+----------+--------+
| 4 B    | 2 B    | 0xF0 | 0x0F  | 4 B      | 1 B      | 1 B      | 1 B      | 2 B    |
+--------+--------+------+-------+----------+----------+----------+----------+--------+
```

**Parameters:**
- BaudRate: Target baud rate (e.g., 115200, 921600)
- DataBits: Usually 8
- StopBits: Usually 1
- Parity: 0 = None, 1 = Odd, 2 = Even

**Example (115200 baud):**
```
EF BE AD DE    // Magic
12 00          // Length = 18
F0 0F          // Type = Handshake
00 C2 01 00    // BaudRate = 115200
08             // DataBits = 8
01             // StopBits = 1
00             // Parity = None
00             // FlowCtrl = None
XX XX          // CRC16
```

### ACK Response (0xE1)

Device response to commands.

**Frame structure (12 bytes):**
```
+--------+--------+------+-------+--------+-----------+--------+
| Magic  | Length | Type | ~Type | Result | ErrorCode | CRC16  |
+--------+--------+------+-------+--------+-----------+--------+
| 4 B    | 2 B    | 0xE1 | 0x1E  | 1 B    | 1 B       | 2 B    |
+--------+--------+------+-------+--------+-----------+--------+
```

**Result codes:**
- `0x5A` - Success
- `0x00` - Failure

**Success ACK example:**
```
EF BE AD DE    // Magic
0C 00          // Length = 12
E1 1E          // Type = ACK
5A             // Result = Success
00             // ErrorCode = 0
XX XX          // CRC16
```

### Download Flash Image (0xD2)

Download binary data to flash memory.

**Frame structure (24 bytes):**
```
+--------+--------+------+-------+----------+---------+-----------+--------+--------+--------+
| Magic  | Length | Type | ~Type | FileAddr | FileLen | EraseSize | Formal | ~Formal| CRC16  |
+--------+--------+------+-------+----------+---------+-----------+--------+--------+--------+
| 4 B    | 2 B    | 0xD2 | 0x2D  | 4 B      | 4 B     | 4 B       | 1 B    | 1 B    | 2 B    |
+--------+--------+------+-------+----------+---------+-----------+--------+--------+--------+
```

**Parameters:**
- FileAddr: Flash address to write to
- FileLen: Size of data to write
- EraseSize: Size to erase before writing (0xFFFFFFFF for full erase)
- Formal: 0x00 for normal write, 0x01 for ROM write

**Workflow:**
1. Send DownloadFlashImage command
2. Wait for ACK
3. Send data via YMODEM protocol
4. Wait for completion ACK

### Download NV (0x4B)

Download Non-Volatile configuration data.

**Frame structure (26 bytes):**
```
+--------+--------+------+-------+------+------+-----------+------------+------+--------+
| Magic  | Length | Type | ~Type | Addr | Len  | EraseSize | EncItemCnt | Flag | CRC16  |
+--------+--------+------+-------+------+------+-----------+------------+------+--------+
| 4 B    | 2 B    | 0x4B | 0xB4  | 4 B  | 4 B  | 4 B       | 2 B        | 2 B  | 2 B    |
+--------+--------+------+-------+------+------+-----------+------------+------+--------+
```

**Parameters:**
- Addr: NV storage address
- Len: Data length
- EraseSize: Size to erase
- EncItemCnt: Number of encrypted items
- Flag: Bit 0 = erase all flag

### Reset (0x87)

Reset the device after flashing.

**Frame structure (12 bytes):**
```
+--------+--------+------+-------+----------+--------+
| Magic  | Length | Type | ~Type | Reserved | CRC16  |
+--------+--------+------+-------+----------+--------+
| 4 B    | 2 B    | 0x87 | 0x78  | 2 B      | 2 B    |
+--------+--------+------+-------+----------+--------+
```

### Set Baud Rate (0x5A)

Change UART communication speed.

**Frame structure:**
```
+--------+--------+------+-------+----------+--------+--------+
| Magic  | Length | Type | ~Type | BaudRate | Magic2 | CRC16  |
+--------+--------+------+-------+----------+--------+--------+
| 4 B    | 2 B    | 0x5A | 0xA5  | 4 B      | 4 B    | 2 B    |
+--------+--------+------+-------+----------+--------+--------+
```

**Parameters:**
- BaudRate: New baud rate
- Magic2: Usually `0x0108`

## Image Types

When downloading images, the following types are supported:

| Value | Type | Description |
|-------|------|-------------|
| 0 | Loader | First stage bootloader |
| 1 | Normal | Normal firmware partition |
| 2 | KvNv | Key-Value NV storage |
| 3 | Efuse | eFuse data |
| 4 | Otp | OTP data |
| 5 | FlashBoot | Second stage loader |
| 6 | Factory | Factory calibration data |
| 7 | Version | Version information |
| 8-10 | Security A/B/C | Security partitions |
| 11 | ProtocolA | Protocol partition |
| 12 | AppsA | Application partition |
| 13 | RadioConfig | Radio configuration |

## Data Transfer (YMODEM)

After sending a download command and receiving ACK, data is transferred using YMODEM-1K protocol:

1. **Receiver sends `C`** to indicate CRC mode
2. **Sender sends SOH/STX block:**
   - `SOH` (0x01) for 128-byte blocks
   - `STX` (0x02) for 1024-byte blocks
   - Block number (1 byte)
   - Block number complement (1 byte)
   - Data (128 or 1024 bytes)
   - CRC16 (2 bytes, big-endian)
3. **Receiver sends ACK** (0x06) or NAK (0x15)
4. **End with EOT** (0x04)

## Typical Flashing Sequence

```
Host                              Device
  |                                  |
  |--- Handshake (0xF0) ------------>|
  |<-- ACK (0xE1, 0x5A) -------------|
  |                                  |
  |--- SetBaudRate (0x5A) ---------->|  (optional)
  |<-- ACK (0xE1, 0x5A) -------------|
  |    [Switch baud rate]            |
  |                                  |
  |--- DownloadFlashImage (0xD2) --->|
  |<-- ACK (0xE1, 0x5A) -------------|
  |                                  |
  |<-- 'C' (YMODEM ready) -----------|
  |--- YMODEM Data Blocks ---------->|
  |<-- ACK per block ----------------|
  |--- EOT ------------------------->|
  |<-- ACK (Transfer complete) ------|
  |                                  |
  |--- Reset (0x87) ---------------->|
  |<-- ACK (0xE1, 0x5A) -------------|
  |    [Device resets]               |
```

## Error Handling

Common error scenarios:
1. **No ACK received**: Retry handshake or reset device
2. **NAK received during YMODEM**: Resend current block
3. **CRC mismatch**: Frame corruption, resend command
4. **Timeout**: Device may be in wrong mode, power cycle required

## Chip-Specific Notes

### WS63
- Default baud: 115200
- High-speed baud: 921600
- Supports late baud rate switch (after LoaderBoot)

### BS2X (BS21, BS25)
- Default baud: 115200
- High-speed baud: up to 2000000
- Supports USB DFU mode

## References

- HiSilicon fbb_burntool source code (`WifiBurnCtrl.cpp`, `Channel.h`)
- ws63flash project
- HiSilicon SDK documentation
