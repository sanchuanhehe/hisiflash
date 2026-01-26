# HisiFlash 需求规格说明书

## 1. 项目概述

### 1.1 项目背景

HisiFlash 是一个用 Rust 编写的跨平台命令行烧录工具，用于海思（HiSilicon）系列芯片的固件烧写、调试和交互。本项目参考了以下项目的设计思路：

- **ws63flash** (C): 社区开源的 WS63 芯片烧录工具，通过逆向 BurnTool 实现，**作为优先参考实现**
- **esptool** (Python): Espressif 官方的串口烧录工具，功能完善，支持多种芯片
- **espflash** (Rust): Rust 实现的 ESP 芯片烧录工具，提供库和 CLI 两种使用方式
- **fbb_burntool** (Qt/C++): 海思官方的烧录工具，支持串口、USB、JLink、网口等多种方式

### 1.2 优先实现目标

**第一阶段优先实现 WS63 芯片的完整支持**，功能对标 ws63flash，包括：
- FWPKG 固件包解析和烧写
- YMODEM 协议传输
- 海思私有帧协议
- 多波特率支持 (115200 ~ 921600)
- 擦除功能
- 裸机二进制烧写

### 1.3 项目目标

1. **跨平台支持**: 支持 Windows、Linux、macOS
2. **命令行优先**: 提供强大的 CLI 工具，便于脚本自动化
3. **库支持**: 提供可复用的 Rust 库，方便集成到其他项目
4. **可扩展架构**: 支持新芯片、新协议的扩展
5. **现代化开发体验**: 友好的错误提示、进度显示、自动检测

### 1.4 目标用户

- 嵌入式开发者
- 自动化测试工程师
- 生产线烧录系统集成商
- 固件开发工程师

---

## 2. Phase 1: WS63 完整支持 (优先实现)

基于 ws63flash 项目，优先实现 WS63 芯片的完整烧录功能。

### 2.1 核心命令 (对标 ws63flash)

| 命令 | ws63flash | hisiflash | 描述 | 优先级 |
|------|-----------|-----------|------|--------|
| flash | `--flash PORT FWPKG [BIN...]` | `flash -p PORT FWPKG [--only BIN]` | 烧写 fwpkg 固件包 | P0 |
| write | `--write PORT LOADER BIN@ADDR...` | `write -p PORT LOADER BIN@ADDR...` | 烧写裸机二进制 | P0 |
| write-program | `--write-program PORT BIN` | `write -p PORT BIN --sign` | 签名并烧写程序 | P1 |
| erase | `--erase PORT FWPKG` | `erase -p PORT [FWPKG]` | 擦除 Flash | P0 |

### 2.2 命令行选项

| 选项 | ws63flash | hisiflash | 描述 |
|------|-----------|-----------|------|
| 波特率 | `-b BAUD` | `-b, --baud BAUD` | 设置波特率 (默认 115200) |
| 延迟切换波特率 | `--late-baud` | `--late-baud` | LoaderBoot 后切换波特率 |
| 详细输出 | `-v` | `-v, --verbose` | 详细输出 (可多次使用) |
| 帮助 | `--help` | `-h, --help` | 显示帮助 |
| 版本 | `--version` | `-V, --version` | 显示版本 |

### 2.3 协议实现

| 模块 | 描述 | 优先级 |
|------|------|--------|
| 海思帧协议 | `0xDEADBEEF + LEN + CMD + SCMD + DATA + CRC16` | P0 |
| YMODEM 协议 | 标准 YMODEM-1K 文件传输 | P0 |
| CRC16-XMODEM | 校验算法 | P0 |
| FWPKG 解析 | 固件包格式解析 | P0 |
| WS63 签名 | 裸机程序签名 (0x300 字节头) | P1 |

### 2.4 支持的命令码

| 命令 | 值 | 描述 |
|------|-----|------|
| CMD_HANDSHAKE | 0xF0 | 握手建立连接 |
| CMD_SETBAUDR | 0x5A | 设置波特率 |
| CMD_DOWNLOADI | 0xD2 | 下载/擦除 |
| CMD_RST | 0x87 | 复位 |

### 2.5 hisiflash Phase 1 命令示例

```bash
# 烧写 fwpkg 固件包 (等同于 ws63flash --flash)
hisiflash flash -p /dev/ttyUSB0 firmware.fwpkg

# 使用高波特率烧写 (推荐)
hisiflash flash -p /dev/ttyUSB0 -b 921600 firmware.fwpkg

# 选择性烧写指定分区
hisiflash flash -p /dev/ttyUSB0 firmware.fwpkg --only ws63-liteos-app.bin

# 烧写裸机二进制文件 (等同于 ws63flash --write)
hisiflash write -p /dev/ttyUSB0 loader.bin app.bin@0x230000 flashboot.bin@0x220000

# 签名并烧写程序 (等同于 ws63flash --write-program)
hisiflash write -p /dev/ttyUSB0 app.bin --sign

# 擦除 Flash (等同于 ws63flash --erase)
hisiflash erase -p /dev/ttyUSB0

# 显示 fwpkg 信息
hisiflash info firmware.fwpkg

# 详细输出模式
hisiflash flash -p /dev/ttyUSB0 -vvv firmware.fwpkg
```

---

## 3. Phase 2+: 扩展功能 (后续实现)

### 3.1 通用烧录功能 (参考 esptool/espflash)

#### 3.1.1 烧录功能 (Flash)

| 功能 | 描述 | esptool | espflash | fbb_burntool | 优先级 |
|------|------|---------|----------|--------------|--------|
| write-flash | 烧写固件到指定地址 | ✅ | ✅ | ✅ | P0 |
| read-flash | 从芯片读取 Flash 内容 | ✅ | ✅ | ❌ | P1 |
| erase-flash | 擦除整个 Flash | ✅ | ✅ | ✅ | P0 |
| erase-region | 擦除指定区域 | ✅ | ✅ | ❌ | P1 |
| verify-flash | 校验烧写内容 | ✅ | ❌ | ❌ | P2 |

#### 3.1.2 设备信息功能

| 功能 | 描述 | esptool | espflash | fbb_burntool | 优先级 |
|------|------|---------|----------|--------------|--------|
| board-info | 显示设备信息 | ✅ | ✅ | ✅ | P0 |
| chip-id | 读取芯片 ID | ✅ | ✅ | ❌ | P1 |
| flash-id | 读取 Flash 芯片信息 | ✅ | ❌ | ❌ | P2 |
| read-mac | 读取 MAC 地址 | ✅ | ❌ | ❌ | P2 |
| checksum-md5 | 计算指定区域的 MD5 | ✅ | ✅ | ❌ | P2 |

#### 2.1.3 镜像处理功能

| 功能 | 描述 | esptool | espflash | fbb_burntool | 优先级 |
|------|------|---------|----------|--------------|--------|
| elf2image | ELF 转固件镜像 | ✅ | ✅ | ❌ | P1 |
| image-info | 显示镜像信息 | ✅ | ✅ | ❌ | P1 |
| merge-bin | 合并多个固件文件 | ✅ | ❌ | ✅ | P1 |
| save-image | 保存固件镜像到本地 | ❌ | ✅ | ❌ | P2 |
| partition-table | 分区表处理 | ❌ | ✅ | ❌ | P2 |

#### 2.1.4 设备控制功能

| 功能 | 描述 | esptool | espflash | fbb_burntool | 优先级 |
|------|------|---------|----------|--------------|--------|
| reset | 复位芯片 | ✅ | ✅ | ✅ | P0 |
| hold-in-reset | 保持复位状态 | ❌ | ✅ | ❌ | P2 |
| load-ram | 加载程序到 RAM 运行 | ✅ | ❌ | ❌ | P2 |
| run | 运行已加载的程序 | ✅ | ❌ | ❌ | P2 |

#### 2.1.5 调试监控功能

| 功能 | 描述 | esptool | espflash | fbb_burntool | 优先级 |
|------|------|---------|----------|--------------|--------|
| monitor | 串口监控终端 | ❌ | ✅ | ❌ | P1 |
| read-mem | 读取内存 | ✅ | ❌ | ❌ | P2 |
| write-mem | 写入内存 | ✅ | ❌ | ❌ | P2 |
| dump-mem | 导出内存内容 | ✅ | ❌ | ❌ | P2 |

#### 2.1.6 安全功能 (参考 espsecure/espefuse)

| 功能 | 描述 | esptool | espflash | fbb_burntool | 优先级 |
|------|------|---------|----------|--------------|--------|
| read-efuse | 读取 eFuse | ✅ | ✅ | ✅ | P1 |
| write-efuse | 写入 eFuse | ✅ | ❌ | ✅ | P2 |
| get-security-info | 获取安全信息 | ✅ | ❌ | ❌ | P2 |
| sign-data | 签名固件 | ✅ | ❌ | ❌ | P3 |
| encrypt-flash | 加密固件 | ✅ | ❌ | ✅ | P3 |

### 2.2 海思特有功能 (参考 fbb_burntool)

#### 2.2.1 多连接方式支持

| 连接方式 | 描述 | 优先级 |
|----------|------|--------|
| Serial (UART) | 串口连接 | P0 |
| TCP/IP | 网络连接 | P1 |
| USB (DFU) | USB DFU 模式 | P1 |
| JLink | JLink 调试器 | P2 |
| SLE | 星闪连接 | P3 |

#### 2.2.2 芯片支持

基于 fbb_burntool 的芯片定义，需要支持：

| 芯片类型 | 描述 | 优先级 |
|----------|------|--------|
| WIFI5GNB | WiFi 5G 芯片 | P0 |
| LUOFU (0x30005) | 罗浮芯片 | P1 |
| XILING (0x30006) | 西岭芯片 | P1 |
| EMEI (0x30007) | 峨眉芯片 | P1 |
| TG0/TG1/TG2 | TG 系列芯片 | P2 |
| MCU | MCU 芯片 | P2 |
| 其他扩展 | 预留扩展接口 | P3 |

#### 2.2.3 固件包格式支持

| 格式 | 描述 | 优先级 |
|------|------|--------|
| FWPKG | 海思固件包格式 | P0 |
| FWPKG_NEW | 新版固件包格式 | P0 |
| BIN | 单独 BIN 文件 | P0 |
| HEX | Intel HEX 格式 | P1 |
| ELF | ELF 格式 | P2 |

#### 2.2.4 烧录协议支持

| 协议 | 描述 | 优先级 |
|------|------|--------|
| YMODEM | YMODEM 协议 | P0 |
| XMODEM | XMODEM 协议 | P2 |
| 自定义协议 | 海思自定义协议 | P0 |

---

## 3. 非功能需求

### 3.1 性能需求

| 指标 | 要求 |
|------|------|
| 烧录速度 | 支持高达 921600 波特率 |
| 响应时间 | 命令执行响应 < 1s |
| 内存占用 | CLI 工具 < 50MB |
| 启动时间 | 冷启动 < 500ms |

### 3.2 可靠性需求

| 指标 | 要求 |
|------|------|
| 烧录失败恢复 | 支持断点续烧 |
| 数据校验 | 支持 CRC/MD5 校验 |
| 超时处理 | 可配置超时，自动重试 |
| 错误提示 | 友好的错误信息和建议 |

### 3.3 兼容性需求

| 平台 | 最低版本 |
|------|----------|
| Windows | Windows 10+ |
| Linux | Ubuntu 20.04+ / glibc 2.31+ |
| macOS | macOS 11+ (Big Sur) |
| Rust | MSRV 1.75+ |

### 3.4 安全需求

- 敏感信息（密钥、证书）不明文存储
- 支持加密固件烧写
- 支持安全启动相关配置

---

## 4. 架构设计

### 4.1 整体架构

```
┌─────────────────────────────────────────────────────────────────┐
│                         CLI Layer                                │
│  ┌─────────┐ ┌─────────┐ ┌─────────┐ ┌─────────┐ ┌─────────┐   │
│  │  flash  │ │  read   │ │  erase  │ │ monitor │ │  efuse  │   │
│  └────┬────┘ └────┬────┘ └────┬────┘ └────┬────┘ └────┬────┘   │
└───────┼───────────┼───────────┼───────────┼───────────┼─────────┘
        │           │           │           │           │
┌───────┴───────────┴───────────┴───────────┴───────────┴─────────┐
│                        Library Layer                             │
│  ┌─────────────────────────────────────────────────────────────┐│
│  │                      Flasher Module                         ││
│  │  ┌─────────┐ ┌─────────┐ ┌─────────┐ ┌─────────┐           ││
│  │  │ Image   │ │ Flash   │ │ Verify  │ │ Progress│           ││
│  │  │ Parser  │ │ Writer  │ │ Handler │ │ Tracker │           ││
│  │  └─────────┘ └─────────┘ └─────────┘ └─────────┘           ││
│  └─────────────────────────────────────────────────────────────┘│
│  ┌─────────────────────────────────────────────────────────────┐│
│  │                     Target Module                           ││
│  │  ┌─────────┐ ┌─────────┐ ┌─────────┐ ┌─────────┐           ││
│  │  │ Chip    │ │ eFuse   │ │ Memory  │ │ Flash   │           ││
│  │  │ Detect  │ │ Handler │ │ Map     │ │ Layout  │           ││
│  │  └─────────┘ └─────────┘ └─────────┘ └─────────┘           ││
│  └─────────────────────────────────────────────────────────────┘│
│  ┌─────────────────────────────────────────────────────────────┐│
│  │                   Connection Module                         ││
│  │  ┌─────────┐ ┌─────────┐ ┌─────────┐ ┌─────────┐           ││
│  │  │ Serial  │ │  TCP    │ │  USB    │ │ JLink   │           ││
│  │  │ Port    │ │  Port   │ │  DFU    │ │ Driver  │           ││
│  │  └─────────┘ └─────────┘ └─────────┘ └─────────┘           ││
│  └─────────────────────────────────────────────────────────────┘│
│  ┌─────────────────────────────────────────────────────────────┐│
│  │                    Protocol Module                          ││
│  │  ┌─────────┐ ┌─────────┐ ┌─────────┐ ┌─────────┐           ││
│  │  │ YMODEM  │ │ XMODEM  │ │ Custom  │ │ Stub    │           ││
│  │  │ Protocol│ │ Protocol│ │ Protocol│ │ Loader  │           ││
│  │  └─────────┘ └─────────┘ └─────────┘ └─────────┘           ││
│  └─────────────────────────────────────────────────────────────┘│
└─────────────────────────────────────────────────────────────────┘
                              │
┌─────────────────────────────┴───────────────────────────────────┐
│                       Platform Layer                             │
│  ┌─────────────────┐ ┌─────────────────┐ ┌─────────────────┐    │
│  │    Windows      │ │     Linux       │ │     macOS       │    │
│  │   (COM ports)   │ │   (tty devices) │ │   (cu devices)  │    │
│  └─────────────────┘ └─────────────────┘ └─────────────────┘    │
└─────────────────────────────────────────────────────────────────┘
```

### 4.2 模块职责

#### 4.2.1 CLI Layer (`hisiflash-cli`)
- 命令行参数解析 (使用 clap)
- 用户交互界面
- 进度显示和日志输出
- 配置文件管理

#### 4.2.2 Library Layer (`hisiflash`)

**Flasher Module**
- 镜像解析和处理
- 烧写控制逻辑
- 进度回调机制

**Target Module**  
- 芯片特征定义
- eFuse 操作
- 内存映射和 Flash 布局

**Connection Module**
- 串口连接管理
- TCP/IP 连接
- USB DFU 连接
- JLink 连接 (可选)

**Protocol Module**
- YMODEM 协议实现
- 自定义烧写协议
- Stub Loader 管理

### 4.3 扩展机制

#### 4.3.1 芯片扩展

```rust
// 芯片特征 trait
pub trait ChipTarget {
    /// 芯片名称
    fn name(&self) -> &'static str;
    
    /// 芯片 ID
    fn chip_id(&self) -> u32;
    
    /// Flash 布局
    fn flash_layout(&self) -> FlashLayout;
    
    /// 内存映射
    fn memory_map(&self) -> MemoryMap;
    
    /// 默认波特率
    fn default_baud_rate(&self) -> u32;
    
    /// 连接握手
    fn handshake(&self, conn: &mut Connection) -> Result<()>;
    
    /// 特定芯片的烧写前置操作
    fn pre_flash(&self, conn: &mut Connection) -> Result<()>;
    
    /// 特定芯片的烧写后置操作
    fn post_flash(&self, conn: &mut Connection) -> Result<()>;
}
```

#### 4.3.2 连接方式扩展

```rust
// 连接 trait
pub trait ConnectionPort: Send {
    /// 打开连接
    fn open(&mut self) -> Result<()>;
    
    /// 关闭连接
    fn close(&mut self) -> Result<()>;
    
    /// 写入数据
    fn write(&mut self, data: &[u8]) -> Result<usize>;
    
    /// 读取数据
    fn read(&mut self, buf: &mut [u8]) -> Result<usize>;
    
    /// 设置超时
    fn set_timeout(&mut self, timeout: Duration) -> Result<()>;
    
    /// 清空缓冲区
    fn flush(&mut self) -> Result<()>;
}
```

#### 4.3.3 协议扩展

```rust
// 传输协议 trait
pub trait TransferProtocol {
    /// 协议名称
    fn name(&self) -> &'static str;
    
    /// 发送数据
    fn send(&mut self, conn: &mut dyn ConnectionPort, data: &[u8]) -> Result<()>;
    
    /// 接收数据
    fn receive(&mut self, conn: &mut dyn ConnectionPort) -> Result<Vec<u8>>;
    
    /// 发送文件
    fn send_file<P: AsRef<Path>>(&mut self, conn: &mut dyn ConnectionPort, path: P) -> Result<()>;
}
```

---

## 5. 配置文件设计

### 5.1 全局配置 (`~/.config/hisiflash/config.toml`)

```toml
# HisiFlash 全局配置文件

[connection]
# 默认串口
default_port = "/dev/ttyUSB0"
# 默认波特率
default_baud = 115200
# 超时时间 (毫秒)
timeout = 5000

[flash]
# 默认擦除模式: "normal", "all", "none"
erase_mode = "normal"
# 烧写后校验
verify_after_flash = true
# 烧写后复位
reset_after_flash = true

[monitor]
# 监控波特率
baud = 115200
# 日志格式: "plain", "defmt", "esp"
log_format = "plain"

[logging]
# 日志级别: "error", "warn", "info", "debug", "trace"
level = "info"
# 保存日志到文件
save_to_file = false
log_file = "~/.config/hisiflash/hisiflash.log"
```

### 5.2 项目配置 (`hisiflash.toml`)

```toml
# HisiFlash 项目配置文件

[chip]
# 芯片类型
type = "wifi5gnb"
# 芯片 ID (可选，用于验证)
chip_id = 0x30005

[connection]
# 连接方式: "serial", "tcp", "usb", "jlink"
type = "serial"
port = "/dev/ttyUSB0"
baud = 921600

[flash]
# Flash 配置
mode = "dio"
size = "4MB"
frequency = "40MHz"

[firmware]
# 固件包路径
package = "./firmware.fwpkg"
# 或者单独指定各分区
# [[firmware.partitions]]
# name = "bootloader"
# path = "./bootloader.bin"
# address = 0x0

[build]
# 构建配置 (可选)
elf_file = "./target/release/app.elf"
output_dir = "./build"
```

### 5.3 串口配置 (`hisiflash_ports.toml`)

```toml
# 串口配置文件 (建议 .gitignore)

[serial]
port = "/dev/ttyUSB0"

[[usb_device]]
# USB 设备 VID/PID 自动识别
vid = "0403"
pid = "6001"

[[usb_device]]
vid = "10c4"
pid = "ea60"
```

---

## 6. 命令行接口设计

### 6.1 主命令

```bash
hisiflash [OPTIONS] <COMMAND>

COMMANDS:
  flash          烧写固件到设备
  read           从设备读取 Flash 内容
  erase          擦除 Flash
  info           显示设备/固件信息
  monitor        串口监控
  efuse          eFuse 操作
  reset          复位设备
  partition      分区表操作
  completions    生成 shell 补全脚本
  help           显示帮助信息

OPTIONS:
  -p, --port <PORT>      串口设备
  -b, --baud <BAUD>      波特率 [default: 115200]
  -c, --chip <CHIP>      芯片类型
  -C, --config <FILE>    配置文件路径
  -v, --verbose          详细输出
  -q, --quiet            安静模式
  -h, --help             显示帮助
  -V, --version          显示版本
```

### 6.2 子命令详情

#### flash - 烧写固件

```bash
hisiflash flash [OPTIONS] <FILE>

ARGUMENTS:
  <FILE>                    固件文件 (fwpkg/bin/hex/elf)

OPTIONS:
  -a, --address <ADDR>      烧写地址 [default: 0x0]
  -e, --erase <MODE>        擦除模式 [normal/all/none]
  -n, --no-verify           跳过校验
  -r, --no-reset            烧写后不复位
  --partition <NAME>        只烧写指定分区
  --progress                显示进度条
```

#### read - 读取 Flash

```bash
hisiflash read [OPTIONS] <OUTPUT>

ARGUMENTS:
  <OUTPUT>                  输出文件

OPTIONS:
  -a, --address <ADDR>      起始地址 [default: 0x0]
  -s, --size <SIZE>         读取大小
  --format <FMT>            输出格式 [bin/hex]
```

#### erase - 擦除 Flash

```bash
hisiflash erase [OPTIONS]

OPTIONS:
  -a, --address <ADDR>      起始地址
  -s, --size <SIZE>         擦除大小
  --all                     擦除全部
  --partition <NAME>        擦除指定分区
```

#### info - 显示信息

```bash
hisiflash info [SUBCOMMAND]

SUBCOMMANDS:
  board                     显示设备信息
  image <FILE>              显示固件信息
  flash                     显示 Flash 信息
```

#### monitor - 串口监控

```bash
hisiflash monitor [OPTIONS]

OPTIONS:
  --log-format <FMT>        日志格式 [plain/defmt/hex]
  --save <FILE>             保存日志到文件
  --timestamp               显示时间戳
  --no-color                禁用颜色
```

#### efuse - eFuse 操作

```bash
hisiflash efuse [SUBCOMMAND]

SUBCOMMANDS:
  read                      读取 eFuse
  write                     写入 eFuse (危险操作)
  dump                      导出 eFuse 内容
```

---

## 7. 错误处理设计

### 7.1 错误类型

```rust
#[derive(Debug, thiserror::Error)]
pub enum Error {
    // 连接错误
    #[error("Failed to open port {port}: {source}")]
    PortOpen { port: String, source: std::io::Error },
    
    #[error("Connection timeout after {timeout}ms")]
    ConnectionTimeout { timeout: u64 },
    
    #[error("Device not responding")]
    DeviceNotResponding,
    
    // 芯片错误
    #[error("Unsupported chip: {chip}")]
    UnsupportedChip { chip: String },
    
    #[error("Chip ID mismatch: expected {expected:#x}, got {actual:#x}")]
    ChipIdMismatch { expected: u32, actual: u32 },
    
    // 烧写错误
    #[error("Flash write failed at address {address:#x}")]
    FlashWriteFailed { address: u32 },
    
    #[error("Flash verify failed at address {address:#x}")]
    FlashVerifyFailed { address: u32 },
    
    #[error("Invalid firmware format")]
    InvalidFirmwareFormat,
    
    // 文件错误
    #[error("File not found: {path}")]
    FileNotFound { path: String },
    
    #[error("Invalid file format: {details}")]
    InvalidFileFormat { details: String },
    
    // 协议错误
    #[error("Protocol error: {details}")]
    ProtocolError { details: String },
    
    #[error("CRC mismatch")]
    CrcMismatch,
}
```

### 7.2 错误恢复策略

| 错误类型 | 自动重试 | 用户提示 |
|----------|----------|----------|
| 连接超时 | 3次 | 检查设备连接 |
| 握手失败 | 3次 | 尝试手动复位设备 |
| 烧写失败 | 2次 | 检查 Flash 是否损坏 |
| 校验失败 | 1次 | 重新烧写 |
| 文件错误 | 不重试 | 检查文件路径 |

---

## 8. 测试计划

### 8.1 单元测试

- [ ] 协议解析测试
- [ ] 镜像格式解析测试
- [ ] CRC 计算测试
- [ ] 配置文件解析测试

### 8.2 集成测试

- [ ] 串口连接测试
- [ ] 固件烧写流程测试
- [ ] 多芯片支持测试
- [ ] 错误恢复测试

### 8.3 硬件测试 (需要实际设备)

- [ ] 各芯片型号烧写测试
- [ ] 多种连接方式测试
- [ ] 大文件烧写测试
- [ ] 压力测试

---

## 9. 开发计划

### Phase 1: 基础框架 (Week 1-2)

- [ ] 项目结构搭建
- [ ] 基础连接层实现 (Serial)
- [ ] 命令行框架
- [ ] 日志和错误处理

### Phase 2: 核心功能 (Week 3-4)

- [ ] YMODEM 协议实现
- [ ] 海思烧写协议实现
- [ ] 基础芯片支持
- [ ] 固件包解析

### Phase 3: 扩展功能 (Week 5-6)

- [ ] 多种连接方式
- [ ] 更多芯片支持
- [ ] Monitor 功能
- [ ] eFuse 操作

### Phase 4: 完善优化 (Week 7-8)

- [ ] 配置文件支持
- [ ] 完整测试覆盖
- [ ] 文档完善
- [ ] 性能优化

---

## 10. 附录

### 10.1 参考资料

- esptool 源码: https://github.com/espressif/esptool
- espflash 源码: https://github.com/esp-rs/espflash
- YMODEM 协议规范: https://en.wikipedia.org/wiki/YMODEM
- 海思芯片文档: (内部资料)

### 10.2 术语表

| 术语 | 解释 |
|------|------|
| FWPKG | 海思固件包格式 |
| eFuse | 一次性可编程存储器 |
| Stub Loader | 烧写加速程序 |
| YMODEM | 文件传输协议 |
| DFU | Device Firmware Update |

### 10.3 版本历史

| 版本 | 日期 | 描述 |
|------|------|------|
| 0.1 | 2026-01-27 | 初始需求文档 |

---

## 11. 待讨论问题

1. **芯片支持范围**: 需要确认具体支持哪些海思芯片型号？
2. **协议细节**: 海思自定义烧写协议的详细规范在哪里？
3. **固件包格式**: FWPKG 和 FWPKG_NEW 的详细格式定义？
4. **安全功能**: 加密和签名功能的具体需求？
5. **GUI 需求**: 是否需要后续开发 GUI 版本？
6. **兼容性**: 是否需要兼容 fbb_burntool 的配置文件格式？
